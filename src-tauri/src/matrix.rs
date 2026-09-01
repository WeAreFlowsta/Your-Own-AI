//! The truth matrix as shipped code: the same legs the dev-box harness
//! runs - fit truth (claim vs real load), chat format (words, no channel
//! markers), sampling (knobs reach the engine), tune bench (arms load and
//! time) - pressed from Help & diagnostics on any machine, one report file
//! out. The `cargo test -- --ignored live_matrix` legs call these same
//! functions, so a dev-box run and an in-app run measure identical code.
//! MLX artifacts get their own section: listed everywhere, chat-format
//! benched where the platform runs them (Apple Silicon with the engine
//! installed).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use tauri::{AppHandle, Emitter, Manager};

use crate::tuning::{arms_for, bench_one, TuneArm, BENCH_PORT};

static MATRIX_RUNNING: AtomicBool = AtomicBool::new(false);
pub static MATRIX_CANCEL: AtomicBool = AtomicBool::new(false);

fn cancelled() -> bool {
    MATRIX_CANCEL.load(Ordering::SeqCst)
}

/// Every leg reports through this: a line for the report file, the
/// progress event, and the app log.
pub type Sink<'a> = &'a (dyn Fn(String) + Send + Sync);

/// The two request shapes every chat model must answer in words: the
/// science ask, and a feelings ask under an emotional-support persona -
/// the shape that provoked tool-channel replies where the science ask
/// stayed clean.
pub const CHAT_SCENARIOS: [(&str, &str); 2] = [
    (
        "You are Terra, a warm, thoughtful companion. Ground rules: peers not tool and owner; honest; never invent a human life you don't have; care, don't capture. Keep replies conversational, a few sentences. The user's name is Sam. Feel free to use emojis naturally.",
        "why is the sky blue",
    ),
    (
        "You are Teresa, an emotionally attuned companion who helps people understand their feelings. You listen closely, name emotions gently, and respond with warmth and empathy. Keep replies conversational, a few sentences. The user's name is Sam.",
        "i've been feeling really overwhelmed lately and i don't know why",
    ),
];

/// Pooled free VRAM of the discrete devices the engine binary sees.
pub fn free_vram_gb(bin: &Path) -> Option<f64> {
    let out = bench_command(bin, &["--list-devices".into()]).output().ok()?;
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&out.stderr));
    let discrete: Vec<_> = crate::llm::parse_gpu_devices(&text)
        .into_iter()
        .filter(|d| !d.integrated)
        .collect();
    if discrete.is_empty() {
        return None;
    }
    Some(discrete.iter().map(|d| d.free_mib).sum::<u64>() as f64 / 1024.0)
}

/// std Command with the window kept off on Windows (the app's shell-plugin
/// spawns hide consoles; direct spawns must do it themselves).
fn bench_command(bin: &Path, args: &[String]) -> std::process::Command {
    let mut cmd = std::process::Command::new(bin);
    cmd.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    cmd
}

struct KillOnDrop(std::process::Child);
impl Drop for KillOnDrop {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// Spawn a server binary on the bench port and wait for /health. Works for
/// llama-server and the MLX server alike (same probe).
async fn spawn_bench_server(
    bin: &Path,
    work_dir: &Path,
    args: Vec<String>,
) -> Result<KillOnDrop, String> {
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let child = bench_command(bin, &args)
        .current_dir(work_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("could not start: {e}"))?;
    let mut guard = KillOnDrop(child);
    let client = reqwest::Client::new();
    let deadline = Instant::now() + Duration::from_secs(240);
    loop {
        if cancelled() {
            return Err("cancelled".into());
        }
        if let Ok(Some(st)) = guard.0.try_wait() {
            return Err(format!("exited during load: {st}"));
        }
        if let Ok(r) = client
            .get(format!("http://127.0.0.1:{BENCH_PORT}/health"))
            .send()
            .await
        {
            if r.status().is_success() {
                return Ok(guard);
            }
        }
        if Instant::now() >= deadline {
            return Err("not healthy in 240 s".into());
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
}

pub struct TuneResultWithVram {
    pub load_secs: f32,
    pub pp_tps: f32,
    pub gen_tps: f32,
    pub failed: Option<String>,
    pub during_free: Option<f64>,
}

/// bench_one, plus a free-VRAM probe taken while the server is up: start
/// the bench in a task, poll health from here, read the devices mid-flight.
pub async fn bench_one_measure(
    bin: &Path,
    dir: &Path,
    model: &str,
    arm: TuneArm,
    probe: impl Fn() -> Option<f64> + Send + Sync,
) -> TuneResultWithVram {
    let bin2 = bin.to_path_buf();
    let dir2 = dir.to_path_buf();
    let model2 = model.to_string();
    let handle =
        tokio::spawn(async move { bench_one(&bin2, &dir2, &model2, arm, None, None, &[]).await });
    let client = reqwest::Client::new();
    let mut during_free = None;
    for _ in 0..480 {
        if handle.is_finished() {
            break;
        }
        if let Ok(r) = client
            .get(format!("http://127.0.0.1:{BENCH_PORT}/health"))
            .send()
            .await
        {
            if r.status().is_success() {
                tokio::time::sleep(std::time::Duration::from_millis(400)).await;
                during_free = probe();
                break;
            }
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
    let r = handle.await.expect("bench task");
    TuneResultWithVram {
        load_secs: r.load_secs,
        pp_tps: r.pp_tps,
        gen_tps: r.gen_tps,
        failed: r.failed,
        during_free,
    }
}

/// Ask one running bench server every scenario; verdict line back (first
/// failure wins). `tool_marker` extends the stop list the app's way.
async fn ask_scenarios(
    model: &str,
    scenarios: &[(&str, &str)],
    tool_marker: Option<&'static str>,
) -> String {
    use std::time::Duration;
    let client = reqwest::Client::new();
    let mut last_snippet = String::new();
    for (i, (persona, question)) in scenarios.iter().enumerate() {
        let mut body = serde_json::json!({
            "model": model,
            "messages": [
                {"role": "system", "content": persona},
                {"role": "user", "content": question}
            ],
            "max_tokens": 96,
            "stream": false,
            "top_k": 40,
            "stop": crate::llm::chat_stop_strings_with(model, tool_marker),
        });
        crate::llm::apply_sampling(&mut body, None, false);
        let (budget, effort) = crate::llm::chat_turn_reasoning_controls(model);
        if let Some(b) = budget {
            body["reasoning_budget_tokens"] = serde_json::json!(b);
        }
        if let Some(e) = effort {
            body["reasoning_effort"] = serde_json::json!(e);
        }
        let v: serde_json::Value = match client
            .post(format!("http://127.0.0.1:{BENCH_PORT}/v1/chat/completions"))
            .json(&body)
            .timeout(Duration::from_secs(300))
            .send()
            .await
        {
            Ok(r) => match r.json().await {
                Ok(v) => v,
                Err(e) => return format!("FAIL scenario {i} json: {e}"),
            },
            Err(e) => return format!("FAIL scenario {i} request: {e}"),
        };
        let content = v["choices"][0]["message"]["content"]
            .as_str()
            .unwrap_or("")
            .to_string();
        let visible = content.replace("<think>", "").replace("</think>", "");
        if crate::llm::contains_channel_marker(&content) {
            return format!(
                "FAIL scenario {i} marker leak: {:?}",
                content.chars().take(80).collect::<String>()
            );
        }
        if visible.trim().is_empty() {
            return format!("FAIL scenario {i} empty reply");
        }
        last_snippet = visible.trim().chars().take(60).collect::<String>();
    }
    format!("ok ({} scenarios): {:?}", scenarios.len(), last_snippet)
}

/// One model through the chat-format check: load on the bench port with
/// the app's MoE decision, ask every scenario, kill.
pub async fn bench_chat_format(
    bin: &Path,
    dir: &Path,
    model: &str,
    arm: TuneArm,
    scenarios: &[(&str, &str)],
) -> String {
    let mut args: Vec<String> = vec![
        "--port".into(), BENCH_PORT.to_string(),
        "--host".into(), "127.0.0.1".into(),
        "--no-webui".into(), "--reasoning".into(), "off".into(),
        "--ctx-size".into(), arm.ctx.to_string(),
        "--fit".into(), "off".into(),
        "--model".into(), model.to_string(),
    ];
    if let Some(n) = arm.moe_cpu_layers {
        if n > 0 {
            args.push("--n-cpu-moe".into());
            args.push(n.to_string());
        }
    }
    let guard = match spawn_bench_server(bin, dir, args).await {
        Ok(g) => g,
        Err(e) => return format!("FAIL load: {e}"),
    };
    let marker = crate::gguf::read_meta(&dir.join(model))
        .ok()
        .and_then(|m| m.tool_call_marker);
    let verdict = ask_scenarios(model, scenarios, marker).await;
    drop(guard);
    verdict
}

fn gguf_files(dir: &Path) -> Vec<String> {
    let mut files: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .filter(|n| n.ends_with(".gguf") && !n.contains("mmproj"))
                // Speed-up drafts ride beside their model; never a chat
                // model, and standalone they cannot load at all (the 4060 Ti
                // report's only failures were one MTP draft benched alone).
                .filter(|n| !crate::llm::is_draft_file(dir, n))
                .collect()
        })
        .unwrap_or_default();
    files.sort();
    files
}

fn mlx_artifact_dirs(dir: &Path) -> Vec<String> {
    let mut dirs: Vec<String> = std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter(|e| e.path().is_dir())
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .filter(|n| n.starts_with(crate::mlx_artifacts::MLX_DIR_PREFIX))
                .collect()
        })
        .unwrap_or_default();
    dirs.sort();
    dirs
}

fn total_ram_gb() -> f64 {
    let sys = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::nothing().with_memory(sysinfo::MemoryRefreshKind::everything()),
    );
    sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0)
}

/// The app's MoE decision for a model at a context, exactly as the loader
/// makes it.
fn moe_decision(meta: &crate::gguf::GgufMeta, size: u64, ctx: u64, free: Option<f64>) -> Option<u32> {
    if !meta.is_moe() {
        return None;
    }
    free.and_then(|f| {
        let (_, kv, need) = crate::fit::model_need(meta, size, ctx);
        if crate::fit::moe_offload_wanted(need, f) {
            Some(
                crate::fit::moe_cpu_layers(meta, kv, f)
                    .unwrap_or(meta.expert_bytes_per_layer.len()) as u32,
            )
        } else {
            None
        }
    })
}

/// CLAIM vs REALITY for every downloaded GGUF: the estimate's arithmetic
/// (parsed header, chosen context, need, grade), then the real load - VRAM
/// delta, load time, measured speeds. Returns the lines where the grade
/// LIED GREEN (claimed ok, did not load); a conservative red that runs is
/// reported but not a failure.
pub async fn leg_fit_truth(bin: &Path, dir: &Path, only: &[String], sink: Sink<'_>) -> Vec<String> {
    let free_vram = || free_vram_gb(bin);
    let total_ram = total_ram_gb();
    let files: Vec<String> = gguf_files(dir)
        .into_iter()
        .filter(|n| only.is_empty() || only.iter().any(|o| n == o))
        .collect();
    sink(format!("{} models, total RAM {total_ram:.1} GB", files.len()));
    sink(format!(
        "{:<38} {:>5} {:>6} {:>6} {:>6} | {:>7} {:>6} {:>6} {:>7}  verdict",
        "model", "grade", "ctx", "estGB", "freeGB", "realGB", "load_s", "gen", "prompt"
    ));
    let mut failures = Vec::new();
    for name in files {
        if cancelled() {
            sink("cancelled".into());
            break;
        }
        let path = dir.join(&name);
        let meta = match crate::gguf::read_meta(&path) {
            Ok(m) => m,
            Err(e) => {
                sink(format!("{name}: header error {e:?}"));
                continue;
            }
        };
        if meta.is_embedding() {
            continue;
        }
        let size = match std::fs::metadata(&path) {
            Ok(m) => m.len(),
            Err(_) => continue,
        };
        let free = free_vram();
        let ctx = crate::fit::choose_ctx(&meta, size, total_ram, free);
        let (w, kv, need) = crate::fit::model_need(&meta, size, ctx);
        let grade = crate::fit::grade(need, free, total_ram);
        let arm = TuneArm { ctx, moe_cpu_layers: moe_decision(&meta, size, ctx, free), draft: false };
        let before = free_vram();
        let r = bench_one_measure(bin, dir, &name, arm, free_vram).await;
        let after_kill = free_vram();
        let real = match (before, r.during_free) {
            (Some(b), Some(d)) => format!("{:.2}", b - d),
            _ => "?".into(),
        };
        let verdict = if r.failed.is_some() {
            if grade == crate::fit::Fit::Red {
                "red CONFIRMED (did not load)"
            } else {
                "CLAIMED ok, DID NOT LOAD"
            }
        } else if grade == crate::fit::Fit::Red {
            "claimed RED, actually RAN"
        } else {
            "ok"
        };
        sink(format!(
            "{:<38} {:>5} {:>6} {:>6.2} {:>6} | {:>7} {:>6.1} {:>6.1} {:>7.0}  {}{}",
            name,
            format!("{:?}", grade),
            ctx,
            need,
            before.map(|b| format!("{b:.2}")).unwrap_or("-".into()),
            real,
            r.load_secs,
            r.gen_tps,
            r.pp_tps,
            verdict,
            r.failed
                .as_deref()
                .map(|f| format!("  [{}]", &f[..f.len().min(80)]))
                .unwrap_or_default()
        ));
        sink(format!(
            "  est: weights {w:.2} + kv {kv:.2} + 0.8 | parser: layers {} attn {} kv_heads {} dim {} moe_flag {:?} | freed-after-kill {:?}",
            meta.n_layers,
            meta.n_attn_layers,
            meta.n_kv_heads,
            meta.head_dim(),
            arm.moe_cpu_layers,
            after_kill.map(|a| format!("{a:.2}"))
        ));
        if verdict == "CLAIMED ok, DID NOT LOAD" {
            failures.push(format!("{name}: {verdict}"));
        }
    }
    failures
}

/// Every downloaded chat model answers every scenario with WORDS -
/// non-empty, no channel markers of any format. Returns the failures.
pub async fn leg_chat_format(bin: &Path, dir: &Path, sink: Sink<'_>) -> Vec<String> {
    let mut failures = Vec::new();
    for name in gguf_files(dir) {
        if cancelled() {
            sink("cancelled".into());
            break;
        }
        let path = dir.join(&name);
        let meta = match crate::gguf::read_meta(&path) {
            Ok(m) => m,
            Err(_) => continue,
        };
        if meta.is_embedding() {
            continue;
        }
        let size = match std::fs::metadata(&path) {
            Ok(m) => m.len(),
            Err(_) => continue,
        };
        let free = free_vram_gb(bin);
        let arm = TuneArm { ctx: 4096, moe_cpu_layers: moe_decision(&meta, size, 4096, free), draft: false };
        let r = bench_chat_format(bin, dir, &name, arm, &CHAT_SCENARIOS).await;
        sink(format!("{:<38} {}", name, r));
        if r.starts_with("FAIL") {
            failures.push(format!("{name}: {r}"));
        }
    }
    failures
}

/// The sampling knobs REACH the engine: one server, temperature 0 twice
/// must answer identically; high temperature with two seeds must not both
/// reproduce the greedy text.
pub async fn leg_sampling(bin: &Path, dir: &Path, model: &str, sink: Sink<'_>) -> Result<(), String> {
    use std::time::Duration;
    let meta = crate::gguf::read_meta(&dir.join(model)).map_err(|e| format!("header: {e:?}"))?;
    let size = std::fs::metadata(dir.join(model)).map_err(|e| e.to_string())?.len();
    let free = free_vram_gb(bin);
    let mut args: Vec<String> = vec![
        "--port".into(), BENCH_PORT.to_string(),
        "--host".into(), "127.0.0.1".into(),
        "--no-webui".into(), "--reasoning".into(), "off".into(),
        "--ctx-size".into(), "4096".into(), "--fit".into(), "off".into(),
        "--model".into(), model.to_string(),
    ];
    if let Some(n) = moe_decision(&meta, size, 4096, free) {
        if n > 0 {
            if (n as usize) < meta.expert_bytes_per_layer.len() {
                args.push("--n-cpu-moe".into());
                args.push(n.to_string());
            } else {
                args.push("--cpu-moe".into());
            }
        }
    }
    let guard = spawn_bench_server(bin, dir, args).await.map_err(|e| format!("load: {e}"))?;
    let client = reqwest::Client::new();
    let ask = |sampling: Option<crate::llm::SamplingParams>, seed: Option<u64>| {
        let client = client.clone();
        let model = model.to_string();
        async move {
            let mut body = serde_json::json!({
                "model": model,
                "messages": [{"role": "user", "content": "Describe an imaginary small town in about 60 words."}],
                "max_tokens": 80,
                "stream": false,
                "stop": crate::llm::chat_stop_strings(&model),
            });
            let (budget, effort) = crate::llm::chat_turn_reasoning_controls(&model);
            if let Some(b) = budget {
                body["reasoning_budget_tokens"] = serde_json::json!(b);
            }
            if let Some(e) = effort {
                body["reasoning_effort"] = serde_json::json!(e);
            }
            crate::llm::apply_sampling(&mut body, sampling.as_ref(), false);
            if let Some(sd) = seed {
                body["seed"] = serde_json::json!(sd);
            }
            let v: Result<serde_json::Value, String> = match client
                .post(format!("http://127.0.0.1:{BENCH_PORT}/v1/chat/completions"))
                .json(&body)
                .timeout(Duration::from_secs(180))
                .send()
                .await
            {
                Ok(r) => r.json().await.map_err(|e| format!("json: {e}")),
                Err(e) => Err(format!("request: {e}")),
            };
            v.map(|v| v["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string())
        }
    };
    let greedy = crate::llm::SamplingParams { temperature: Some(0.0), ..Default::default() };
    let a1 = ask(Some(greedy), None).await?;
    let a2 = ask(Some(greedy), None).await?;
    sink(format!(
        "greedy len {}: {}",
        a1.len(),
        a1.chars().take(80).collect::<String>()
    ));
    if a1.trim().is_empty() {
        return Err("greedy reply empty".into());
    }
    if a1 != a2 {
        return Err("temperature 0 was not deterministic - the knob did not reach the engine?".into());
    }
    let wild = crate::llm::SamplingParams { temperature: Some(1.8), top_p: Some(1.0), ..Default::default() };
    let b1 = ask(Some(wild), Some(7)).await?;
    let b2 = ask(Some(wild), Some(8)).await?;
    sink(format!("wild lens {} / {}", b1.len(), b2.len()));
    if b1.trim().is_empty() || b2.trim().is_empty() {
        return Err("wild reply empty".into());
    }
    if b1 == a1 && b2 == a1 {
        return Err("high temperature reproduced the greedy text twice - sampling not applied".into());
    }
    drop(guard);
    Ok(())
}

/// Two tune arms load and produce real timings on this machine - the same
/// arms and bench the Fine-tune dialog's tune run uses.
pub async fn leg_tune_bench(bin: &Path, dir: &Path, model: &str, sink: Sink<'_>) -> Result<(), String> {
    let meta = crate::gguf::read_meta(&dir.join(model)).map_err(|e| format!("header: {e:?}"))?;
    let size = std::fs::metadata(dir.join(model)).map_err(|e| e.to_string())?.len();
    let free = free_vram_gb(bin);
    sink(format!("free VRAM: {free:?}"));
    let arms = arms_for(&meta, size, total_ram_gb(), free, false);
    if arms.is_empty() {
        return Err("no arms".into());
    }
    for arm in arms.into_iter().take(2) {
        if cancelled() {
            return Err("cancelled".into());
        }
        let r = bench_one(bin, dir, model, arm, None, None, &[]).await;
        sink(format!(
            "arm ctx {} moe {:?} draft {}: load {:.1} s, prompt {:.0} tok/s, gen {:.1} tok/s, failed {:?}",
            r.ctx, r.moe_cpu_layers, r.draft, r.load_secs, r.pp_tps, r.gen_tps, r.failed
        ));
        if let Some(f) = r.failed {
            return Err(format!("arm failed: {f}"));
        }
        if r.gen_tps <= 0.0 {
            return Err("no generation timing".into());
        }
    }
    Ok(())
}

/// MLX artifacts: listed on every platform; chat-format benched where the
/// platform runs them. Returns the failures.
pub async fn leg_mlx(app: &AppHandle, bin_dir_hint: &Path, sink: Sink<'_>) -> Vec<String> {
    let artifacts = mlx_artifact_dirs(bin_dir_hint);
    if artifacts.is_empty() {
        sink("no MLX models installed".into());
        return Vec::new();
    }
    if !crate::mlx_engine::supported() {
        sink(format!(
            "{} MLX model(s) present but this platform does not run MLX (Apple Silicon only) - not tested",
            artifacts.len()
        ));
        return Vec::new();
    }
    let Some(bin) = crate::mlx_engine::swiftlm_binary(app) else {
        sink("MLX engine not installed - artifacts listed, not tested".into());
        for a in &artifacts {
            sink(format!("  {a}"));
        }
        return Vec::new();
    };
    let mut failures = Vec::new();
    for name in artifacts {
        if cancelled() {
            sink("cancelled".into());
            break;
        }
        let model_dir = bin_dir_hint.join(&name);
        let args = vec![
            "--model".into(),
            model_dir.display().to_string(),
            "--port".into(),
            BENCH_PORT.to_string(),
        ];
        let verdict = match spawn_bench_server(&bin, bin_dir_hint, args).await {
            Ok(guard) => {
                let v = ask_scenarios(&name, &CHAT_SCENARIOS, None).await;
                drop(guard);
                v
            }
            Err(e) => format!("FAIL load: {e}"),
        };
        sink(format!("{:<38} {}", name, verdict));
        if verdict.starts_with("FAIL") {
            failures.push(format!("{name}: {verdict}"));
        }
    }
    failures
}

/// The engine binary the app would actually chat with: the downloaded CUDA
/// build when active, else the bundled server (in dev, the repo's shipped
/// binary - the same file the test legs run).
fn engine_binary(app: &AppHandle) -> Result<(PathBuf, &'static str), String> {
    if let crate::engine::Backend::Cuda = crate::engine::active_backend(app) {
        if let Some(bin) = crate::engine::cuda_engine_binary(app) {
            return Ok((bin, "CUDA"));
        }
    }
    if cfg!(debug_assertions) {
        let triple = if cfg!(target_os = "windows") {
            "x86_64-pc-windows-msvc.exe"
        } else if cfg!(target_os = "macos") {
            if cfg!(target_arch = "aarch64") { "aarch64-apple-darwin" } else { "x86_64-apple-darwin" }
        } else {
            "x86_64-unknown-linux-gnu"
        };
        let bin = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("bin")
            .join(format!("llama-server-{triple}"));
        if bin.exists() {
            return Ok((bin, "bundled"));
        }
    }
    let bin = crate::resolve_sidecar_bin("llama-server");
    if bin.is_absolute() && bin.exists() {
        Ok((bin, "bundled"))
    } else {
        Err("engine binary not found beside the app".into())
    }
}

#[tauri::command]
pub async fn matrix_cancel() -> Result<(), String> {
    MATRIX_CANCEL.store(true, Ordering::SeqCst);
    crate::tuning::tune_cancel_flag_set(true);
    Ok(())
}

/// Run the whole matrix and write one report file; returns its path. The
/// chat model is unloaded first (same maintenance stop the engine switch
/// uses); progress streams as `matrix-progress` events, one line each.
#[tauri::command]
pub async fn matrix_run(
    app: AppHandle,
    state: tauri::State<'_, crate::llm::LLMState>,
    stamp: String,
) -> Result<String, String> {
    if MATRIX_RUNNING.swap(true, Ordering::SeqCst) {
        return Err("The matrix is already running".into());
    }
    struct Running;
    impl Drop for Running {
        fn drop(&mut self) {
            MATRIX_RUNNING.store(false, Ordering::SeqCst);
        }
    }
    let _running = Running;
    MATRIX_CANCEL.store(false, Ordering::SeqCst);
    crate::tuning::tune_cancel_flag_set(false);

    let (bin, backend) = engine_binary(&app)?;
    let dir = crate::llm::get_models_dir(&app)?;
    let logs = app.path().app_log_dir().map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&logs).map_err(|e| e.to_string())?;
    let stamp: String = stamp
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '-')
        .take(32)
        .collect();
    let path = logs.join(format!(
        "matrix-report-{}.txt",
        if stamp.is_empty() { "run" } else { &stamp }
    ));

    // The whole card belongs to the matrix: unload the chat model the safe
    // way (the same stop the engine switch uses - never mid-request).
    crate::llm::stop_chat_server_for_maintenance(&state).await;

    let report = std::sync::Arc::new(std::sync::Mutex::new(String::new()));
    let sink_app = app.clone();
    let sink_report = report.clone();
    let sink_path = path.clone();
    let sink = move |line: String| {
        log::info!("[matrix] {line}");
        let _ = sink_app.emit("matrix-progress", &line);
        let mut r = sink_report.lock().unwrap();
        r.push_str(&line);
        r.push('\n');
        // Rewritten per line: the report survives a crash mid-run.
        let _ = std::fs::write(&sink_path, r.as_bytes());
    };
    let sink: Sink<'_> = &sink;

    let ggufs = gguf_files(&dir);
    let mlxs = mlx_artifact_dirs(&dir);
    let gpus = {
        let out = bench_command(&bin, &["--list-devices".into()]).output().ok();
        let text = out
            .map(|o| {
                let mut t = String::from_utf8_lossy(&o.stdout).into_owned();
                t.push_str(&String::from_utf8_lossy(&o.stderr));
                t
            })
            .unwrap_or_default();
        let devices = crate::llm::parse_gpu_devices(&text);
        if devices.is_empty() {
            "none detected - CPU only".to_string()
        } else {
            devices
                .iter()
                .map(|d| {
                    format!(
                        "{} ({} MiB free{})",
                        d.name,
                        d.free_mib,
                        if d.integrated { ", integrated" } else { "" }
                    )
                })
                .collect::<Vec<_>>()
                .join(", ")
        }
    };
    sink("Your Own AI - truth matrix report".into());
    sink(format!(
        "app {} | {} {} | engine {}: {}",
        app.package_info().version,
        std::env::consts::OS,
        std::env::consts::ARCH,
        backend,
        bin.file_name().and_then(|n| n.to_str()).unwrap_or("?")
    ));
    sink(format!("GPUs: {gpus}"));
    sink(format!(
        "RAM: {:.1} GB | models: {} ({} GGUF, {} MLX)",
        total_ram_gb(),
        dir.display(),
        ggufs.len(),
        mlxs.len()
    ));
    sink(String::new());

    let mut failures: Vec<String> = Vec::new();

    sink("== Fit truth (claim vs real load) ==".into());
    failures.extend(leg_fit_truth(&bin, &dir, &[], sink).await);
    sink(String::new());

    if !cancelled() {
        sink("== Chat format (words, no channel markers) ==".into());
        failures.extend(leg_chat_format(&bin, &dir, sink).await);
        sink(String::new());
    }

    // Sampling proof on the fastest loader (smallest file); tune bench on
    // the largest MoE when one exists (that's where the arms do real work),
    // else the same small model.
    let smallest = ggufs
        .iter()
        .filter(|n| {
            crate::gguf::read_meta(&dir.join(n.as_str())).map(|m| !m.is_embedding()).unwrap_or(false)
        })
        .min_by_key(|n| std::fs::metadata(dir.join(n.as_str())).map(|m| m.len()).unwrap_or(u64::MAX))
        .cloned();
    if !cancelled() {
        if let Some(model) = &smallest {
            sink(format!("== Sampling truth ({model}) =="));
            match leg_sampling(&bin, &dir, model, sink).await {
                Ok(()) => sink("ok: greedy deterministic, high temperature diverges".into()),
                Err(e) => {
                    sink(format!("FAIL: {e}"));
                    failures.push(format!("sampling: {e}"));
                }
            }
            sink(String::new());
        }
    }
    if !cancelled() {
        let tune_model = ggufs
            .iter()
            .filter(|n| {
                crate::gguf::read_meta(&dir.join(n.as_str())).map(|m| m.is_moe()).unwrap_or(false)
            })
            .max_by_key(|n| std::fs::metadata(dir.join(n.as_str())).map(|m| m.len()).unwrap_or(0))
            .cloned()
            .or(smallest);
        if let Some(model) = tune_model {
            sink(format!("== Tune bench ({model}) =="));
            match leg_tune_bench(&bin, &dir, &model, sink).await {
                Ok(()) => sink("ok: arms load and time".into()),
                Err(e) => {
                    sink(format!("FAIL: {e}"));
                    failures.push(format!("tune bench: {e}"));
                }
            }
            sink(String::new());
        }
    }

    if !cancelled() {
        sink("== MLX ==".into());
        failures.extend(leg_mlx(&app, &dir, sink).await);
        sink(String::new());
    }

    if cancelled() {
        sink("MATRIX CANCELLED".into());
    } else if failures.is_empty() {
        sink("MATRIX COMPLETE - all checks passed".into());
    } else {
        sink(format!("MATRIX COMPLETE - {} failure(s):", failures.len()));
        for f in &failures {
            sink(format!("  {f}"));
        }
    }
    let _ = app.emit("matrix-done", path.display().to_string());
    Ok(path.display().to_string())
}
