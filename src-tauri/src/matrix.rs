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
    let mut thought_chars = 0usize;
    for (i, (persona, question)) in scenarios.iter().enumerate() {
        // The app's own chat ceiling: some thinking distills ignore the
        // no-thinking request and reason at length (variable, thousands
        // of chars) before the first visible word - anything tighter than
        // what the app allows flips verdicts on sampling luck. A model
        // that produces no words in THIS much room has genuinely failed
        // the user.
        let mut body = serde_json::json!({
            "model": model,
            "messages": [
                {"role": "system", "content": persona},
                {"role": "user", "content": question}
            ],
            "max_tokens": 8192,
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
        // Read the reply the app's way: thought markers become <think>
        // blocks (Gemma family under --special), and the thinking - in
        // the body or in the server's reasoning field - counts as thought,
        // not as words.
        let content = crate::llm::translate_gemma_thought_markers(&content);
        let (visible, inline_thought) = split_think_blocks(&content);
        let reasoning_chars = v["choices"][0]["message"]["reasoning_content"]
            .as_str()
            .map(|r| r.chars().count())
            .unwrap_or(0)
            + inline_thought;
        thought_chars = thought_chars.max(reasoning_chars);
        if crate::llm::contains_channel_marker(&content) {
            return format!(
                "FAIL scenario {i} marker leak: {:?}",
                content.chars().take(80).collect::<String>()
            );
        }
        if visible.trim().is_empty() {
            return if reasoning_chars > 0 {
                format!(
                    "FAIL scenario {i} all reasoning, no words at the app's own ceiling ({reasoning_chars} thinking chars - the no-thinking request was not honored)"
                )
            } else {
                format!("FAIL scenario {i} empty reply")
            };
        }
        last_snippet = visible.trim().chars().take(60).collect::<String>();
    }
    format!(
        "ok ({} scenarios{}): {:?}",
        scenarios.len(),
        if thought_chars > 0 {
            format!(", thought first ({thought_chars} chars) despite the no-thinking ask")
        } else {
            String::new()
        },
        last_snippet
    )
}

/// Split `<think>…</think>` blocks out of a reply: (the words that remain,
/// the thinking's length). An unterminated block counts as thinking to
/// the end - the same reading the app's stream parser gives it.
fn split_think_blocks(text: &str) -> (String, usize) {
    let mut out = String::new();
    let mut thought = 0usize;
    let mut rest = text;
    while let Some(i) = rest.find("<think>") {
        out.push_str(&rest[..i]);
        let after = &rest[i + "<think>".len()..];
        match after.find("</think>") {
            Some(j) => {
                thought += after[..j].chars().count();
                rest = &after[j + "</think>".len()..];
            }
            None => {
                thought += after.chars().count();
                rest = "";
            }
        }
    }
    out.push_str(rest);
    (out, thought)
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
    let meta = crate::gguf::read_meta(&dir.join(model)).ok();
    // The app's own flags for this family (thought markers surfaced so
    // they can be translated) - the bench must load what the app loads.
    if meta.as_ref().is_some_and(|m| m.surfaces_special_tokens()) {
        args.push("--special".into());
    }
    let guard = match spawn_bench_server(bin, dir, args).await {
        Ok(g) => g,
        Err(e) => return format!("FAIL load: {e}"),
    };
    let marker = meta.as_ref().and_then(|m| m.tool_call_marker);
    let mut verdict = ask_scenarios(model, scenarios, marker).await;
    // The think switch (routing's P2 output) is honored: on a template with
    // a switch, thinking on must produce reasoning and thinking off must
    // not. Same server, two short asks.
    if let Some(m) = meta.as_ref().filter(|m| m.template_enable_thinking || m.template_reasoning_strength) {
        if !verdict.starts_with("FAIL") {
            let strength = m.template_reasoning_strength;
            let on = ask_think(model, true, strength, marker).await;
            let off = ask_think(model, false, strength, marker).await;
            verdict.push_str(&match (on, off) {
                (Ok((on_r, _)), Ok((off_r, off_v))) if on_r > 0 && off_r == 0 && off_v > 0 => {
                    format!(" | think switch ok (on {on_r} chars, off 0)")
                }
                (Ok((on_r, _)), Ok((off_r, off_v))) => {
                    format!(" | FAIL think switch: on {on_r} reasoning chars, off {off_r} reasoning chars / {off_v} visible")
                }
                (Err(e), _) | (_, Err(e)) => format!(" | FAIL think switch: {e}"),
            });
        }
    } else {
        verdict.push_str(" | no think switch in the template");
    }
    drop(guard);
    verdict
}

/// One ask with thinking on (enable_thinking / reasoning_strength high) or
/// off (the app's chat-turn controls). Returns (reasoning chars, visible chars).
async fn ask_think(model: &str, on: bool, strength: bool, marker: Option<&'static str>) -> Result<(usize, usize), String> {
    use std::time::Duration;
    let client = reqwest::Client::new();
    let mut body = serde_json::json!({
        "model": model,
        "messages": [
            {"role": "system", "content": "You are a careful assistant."},
            {"role": "user", "content": "Is 391 a prime number? Think it through step by step before you answer, then give the answer in one sentence."}
        ],
        "max_tokens": 2048,
        "stream": false,
        // Greedy: the verdict must not turn on sampling luck (gemma-4-E4B
        // answered without thinking once at default temperature).
        "temperature": 0,
        "stop": crate::llm::chat_stop_strings_with(model, marker),
    });
    if on {
        let mut kw = serde_json::Map::new();
        kw.insert("enable_thinking".into(), serde_json::json!(true));
        if strength {
            kw.insert("reasoning_strength".into(), serde_json::json!("high"));
        }
        body["chat_template_kwargs"] = serde_json::Value::Object(kw);
    } else {
        let (budget, effort) = crate::llm::chat_turn_reasoning_controls(model);
        if let Some(b) = budget {
            body["reasoning_budget_tokens"] = serde_json::json!(b);
        }
        if let Some(e) = effort {
            body["reasoning_effort"] = serde_json::json!(e);
        }
    }
    let v: serde_json::Value = client
        .post(format!("http://127.0.0.1:{BENCH_PORT}/v1/chat/completions"))
        .json(&body)
        .timeout(Duration::from_secs(300))
        .send()
        .await
        .map_err(|e| format!("request: {e}"))?
        .json()
        .await
        .map_err(|e| format!("json: {e}"))?;
    let content = crate::llm::translate_gemma_thought_markers(v["choices"][0]["message"]["content"].as_str().unwrap_or(""));
    let (visible, inline_thought) = split_think_blocks(&content);
    let reasoning = v["choices"][0]["message"]["reasoning_content"].as_str().map(|r| r.chars().count()).unwrap_or(0) + inline_thought;
    Ok((reasoning, visible.trim().chars().count()))
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
pub async fn leg_fit_truth(app: Option<&AppHandle>, bin: &Path, dir: &Path, only: &[String], sink: Sink<'_>) -> Vec<String> {
    let free_vram = || free_vram_gb(bin);
    // The grade column is the APP's grade (assess: MoE split, tune pins,
    // measured evidence) - the plain dense grade printed "claimed RED,
    // actually RAN" for every split MoE and read as a regression. The
    // headless harness (no app) keeps the plain grade.
    let app_grades: std::collections::HashMap<String, crate::fit::Fit> = match app {
        Some(app) => crate::fit::assess(app).await.into_iter().map(|f| (f.name, f.fit)).collect(),
        None => Default::default(),
    };
    // The same figures assess() grades with - the badge and the matrix
    // must read one machine (the Air's MedGemma said "Too large" in the
    // app and Green in the matrix until they did).
    let figures = crate::fit::MachineFigures::with_vram(free_vram());
    let total_ram = figures.total_ram_gb;
    let avail_ram = figures.avail_ram_gb;
    let files: Vec<String> = gguf_files(dir)
        .into_iter()
        .filter(|n| only.is_empty() || only.iter().any(|o| n == o))
        .collect();
    sink(format!(
        "{} models, RAM {total_ram:.1} GB total / {avail_ram:.1} GB available now (graded against available, as the app does)",
        files.len()
    ));
    if cfg!(windows) {
        // WDDM virtualizes GPU memory per process: a probe from a second
        // process cannot see the bench server's allocations, so the delta
        // column reads near zero there. 4060 Ti runs proved it; loads and
        // speeds are the evidence on Windows.
        sink("note: Windows virtualizes GPU memory per process - the realGB column is unreliable here; load success and speeds are the proof".into());
    }
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
        let grade = app_grades
            .get(&name)
            .copied()
            .unwrap_or_else(|| crate::fit::grade(need, free, avail_ram));
        let arm = TuneArm { ctx, moe_cpu_layers: moe_decision(&meta, size, ctx, free), draft: false };
        let before = free_vram();
        let r = bench_one(bin, dir, &name, arm, None, None, &[], Some(&free_vram)).await;
        let after_kill = free_vram();
        let real = match (before, r.during_free) {
            (Some(b), Some(d)) => format!("{:.2}", b - d),
            _ => "?".into(),
        };
        // A non-red grade that loads and then generates at a crawl on a
        // discrete card is the Windows CUDA sysmem-fallback shape: the
        // driver spills to system RAM instead of failing, so the model
        // "fits" and runs an order of magnitude slow (4060 Ti report:
        // Ornith 2.5 tok/s where its size class does 26). A split MoE is
        // exempt - CPU experts are slow honestly.
        let crawled = r.failed.is_none()
            && grade != crate::fit::Fit::Red
            && arm.moe_cpu_layers.is_none()
            && before.is_some()
            && r.gen_tps > 0.0
            && r.gen_tps < 6.0;
        let verdict = if r.failed.is_some() {
            if grade == crate::fit::Fit::Red {
                "red CONFIRMED (did not load)"
            } else {
                "CLAIMED ok, DID NOT LOAD"
            }
        } else if crawled {
            "ran but CRAWLED - likely spilled off the card"
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
        } else if crawled {
            failures.push(format!(
                "{name}: graded {:?} but generated at {:.1} tok/s - likely spilled off the card",
                grade, r.gen_tps
            ));
        }
    }
    failures
}

/// Every downloaded chat model answers every scenario with WORDS -
/// non-empty, no channel markers of any format. Returns the failures.
/// `YOAI_CHAT_FORMAT_MODELS=a.gguf,b.gguf` filters (repro runs).
pub async fn leg_chat_format(bin: &Path, dir: &Path, sink: Sink<'_>) -> Vec<String> {
    let only: Vec<String> = std::env::var("YOAI_CHAT_FORMAT_MODELS")
        .ok()
        .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
        .unwrap_or_default();
    let mut failures = Vec::new();
    for name in gguf_files(dir)
        .into_iter()
        .filter(|n| only.is_empty() || only.iter().any(|o| n == o))
    {
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
        let r = bench_one(bin, dir, model, arm, None, None, &[], None).await;
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
        // Bench only what the app would serve: a half-downloaded artifact
        // (disk filled mid-download, say) is not a finding about the model.
        if !crate::mlx_artifacts::artifact_complete(&model_dir) {
            sink(format!(
                "{:<38} skipped - download incomplete (the app will not serve it either)",
                name
            ));
            continue;
        }
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
/// The helper (utility) model file - the Settings > Components download.
/// Mirrors UTILITY_MODEL.filename in src/data/recommended-models.ts.
const HELPER_FILE: &str = "Ministral-3-3B-Instruct-2512-Q4_K_M.gguf";

/// The small CPU servers start and answer: the embedding server (memory
/// recall, freshness, the semantic health gate) and, when installed, the
/// helper model with the routing classifier's own grammar. These die
/// silently in the field (the chat server hides them behind a working
/// engine), so the matrix proves them on every machine.
pub async fn leg_components(app: &AppHandle, dir: &Path, sink: Sink<'_>) -> Vec<String> {
    let mut failures = Vec::new();
    let st = app.state::<crate::llm::LLMState>();
    if dir.join(crate::router::EMBED_MODEL).exists() {
        let texts = vec!["hello world".to_string(), "a second phrase".to_string(), "and a third".to_string()];
        match crate::llm::embed_texts(app.clone(), st.clone(), texts, crate::router::EMBED_MODEL.to_string()).await {
            Ok(v) if v.len() == 3 && v.iter().all(|x| !x.is_empty()) => {
                sink(format!("embedding server: ok (3 vectors, dim {})", v[0].len()));
            }
            Ok(v) => {
                sink(format!("embedding server: FAIL - {} vectors back for 3 inputs", v.len()));
                failures.push("components: embedding server returned the wrong count".into());
            }
            Err(e) => {
                sink(format!("embedding server: FAIL - {e}"));
                failures.push(format!("components: embedding server - {e}"));
            }
        }
    } else {
        sink("embedding model: not installed (memory recall, freshness and the semantic health check are off; Frontier-first stays on the device by design)".into());
    }
    if dir.join(HELPER_FILE).exists() {
        const BASE: &str = "Classify the user's message for model routing. Output exactly TWO words: the TASK then the DIFFICULTY.\nTASK = CODE | MATH | REASONING | GENERAL.\nDIFFICULTY = HARD if it needs a powerful model (complex, multi-step, deep, expert-level); EASY otherwise.";
        const GRAMMAR: &str = "root ::= (\"CODE\" | \"MATH\" | \"REASONING\" | \"GENERAL\") \" \" (\"EASY\" | \"HARD\")";
        let probes = [
            ("prove that there are infinitely many primes", "HARD"),
            ("what is the capital of France", "EASY"),
            ("write a python function that parses a csv file", "CODE"),
        ];
        let mut lines = Vec::new();
        for (q, want) in probes {
            if cancelled() {
                break;
            }
            match crate::llm::utility_chat(app.clone(), st.clone(), HELPER_FILE.to_string(), BASE.to_string(), q.to_string(), Some(GRAMMAR.to_string()), 12).await {
                Ok(out) => {
                    let verdict = out.trim().to_uppercase();
                    let parsed = verdict.split_whitespace().count() == 2;
                    let hit = verdict.contains(want);
                    lines.push(format!("{q:?} -> {verdict}{}", if hit { "" } else { " (expected it to say " } .to_string() + if hit { "" } else { want } + if hit { "" } else { ")" }));
                    if !parsed {
                        failures.push(format!("components: helper verdict unparseable: {verdict:?}"));
                    }
                }
                Err(e) => {
                    sink(format!("helper model: FAIL - {e}"));
                    failures.push(format!("components: helper model - {e}"));
                    break;
                }
            }
        }
        if !lines.is_empty() {
            sink(format!("helper model: answers ({})", lines.join("; ")));
        }
    } else {
        sink("helper model: not installed (routing difficulty stays unknown; memory extraction rides the chat model)".into());
    }
    failures
}

/// Routing, decide-only, on this machine's models: the battery's query
/// table (src-tauri/route-battery.json, generated from tools/route-battery.mjs)
/// through `route_dry` for every mode and dial. Never loads a model, never
/// generates, never spends credits: the online catalog list is a free
/// call and the query embeds run locally. Asserts the promises and prints
/// the per-bucket online shares.
pub async fn leg_routing(app: &AppHandle, dir: &Path, sink: Sink<'_>) -> Vec<String> {
    let mut failures = Vec::new();
    let table: serde_json::Value = match serde_json::from_str(include_str!("../route-battery.json")) {
        Ok(v) => v,
        Err(e) => {
            sink(format!("FAIL: battery table unreadable: {e}"));
            return vec![format!("routing: battery table unreadable: {e}")];
        }
    };
    let queries = table["queries"].as_array().cloned().unwrap_or_default();
    // Environment: what the numbers below can and cannot exercise.
    let share = crate::router::current_share(app);
    let not_entitled = crate::router::known_not_entitled(app);
    let catalog = crate::flowsta::list_online_models().await.ok();
    let everyday_listed = catalog.as_ref().is_some_and(|m| {
        m.iter().any(|x| x.id.strip_prefix("online:").unwrap_or(&x.id) == crate::router::DEFAULT_EVERYDAY.strip_prefix("online:").unwrap_or(crate::router::DEFAULT_EVERYDAY))
    });
    let embeddings = dir.join(crate::router::EMBED_MODEL).exists();
    let helper = dir.join(HELPER_FILE).exists();
    sink(format!(
        "dial: {share} | online catalog: {} | everyday default listed: {} | entitled: {} | embedding model: {} | helper model: {}",
        catalog.as_ref().map(|m| format!("{} models", m.len())).unwrap_or_else(|| "unreachable (online rungs not exercised)".into()),
        if everyday_listed { "yes" } else { "no" },
        if not_entitled { "no" } else { "yes or unknown" },
        if embeddings { "yes" } else { "no (semantic gates off - ordinary questions stay on the device)" },
        if helper { "yes" } else { "no (difficulty unknown)" },
    ));
    let online_possible = catalog.is_some() && !not_entitled;
    let picks = crate::router::OnlinePicks::from_store(app);
    let side_of = |model: &str| if model.starts_with("online:") { "online" } else if model.starts_with("external:") { "server" } else { "device" };
    // tallies: (mode, bucket, dial) -> (online, n, fails)
    let mut tally: std::collections::BTreeMap<(String, String, String), (u32, u32, u32)> = std::collections::BTreeMap::new();
    let mut listed_fails = 0usize;
    let mut think_ok = 0u32;
    let mut think_bad = 0u32;
    let mut decided = 0u32;
    let started = std::time::Instant::now();
    // Progress every 100 decisions, and a ceiling: a machine where every
    // embed stalls must still hand back a (partial) table.
    const ROUTING_LEG_CAP: std::time::Duration = std::time::Duration::from_secs(25 * 60);
    let mut capped = false;
    'sweep: for mode in ["offline", "online-offline"] {
        for dial in ["frontier", "balanced", "local"] {
            for q in &queries {
                if cancelled() {
                    capped = true;
                    break 'sweep;
                }
                let id = q["id"].as_str().unwrap_or("?");
                let bucket = q["bucket"].as_str().unwrap_or("?");
                let text = q["q"].as_str().unwrap_or("");
                let task = q["task"].as_str().unwrap_or("general");
                let difficulty = q["difficulty"].as_str().unwrap_or("easy");
                let expected = q["expect"][mode][dial].as_str().unwrap_or("either");
                let turn_tokens = if bucket == "long_turn" { Some(200_000u32) } else { None };
                let first = crate::router::route_dry(app, mode, text, dial, task, difficulty, "balanced", &picks, false, false, turn_tokens, &crate::router::PrevTurn::default()).await;
                // Follow-up pairs: the second turn must inherit the first's side.
                let (result, expected_side): (Result<crate::router::RouteResult, String>, String) = if let Some(follow) = q["follow"].as_str() {
                    match &first {
                        Ok(r1) => {
                            let prev = crate::router::PrevTurn { side: Some(side_of(&r1.model).to_string()), task: Some(task.to_string()), model: Some(r1.model.clone()), vec: None };
                            let r2 = crate::router::route_dry(app, mode, follow, dial, "general", "easy", "balanced", &picks, false, false, None, &prev).await;
                            let want = if q["firstBucket"].as_str() == Some("health") { "device".to_string() } else { side_of(&r1.model).to_string() };
                            (r2, want)
                        }
                        Err(e) => (Err(e.clone()), "either".into()),
                    }
                } else {
                    (first, expected.to_string())
                };
                decided += 1;
                if decided % 100 == 0 {
                    sink(format!("  ... {decided} decisions, {:.0} s", started.elapsed().as_secs_f64()));
                }
                if started.elapsed() > ROUTING_LEG_CAP {
                    capped = true;
                    break 'sweep;
                }
                let entry = tally.entry((mode.into(), bucket.into(), dial.into())).or_insert((0, 0, 0));
                entry.1 += 1;
                let (side, model, reason, think) = match &result {
                    Ok(r) => (side_of(&r.model), r.model.clone(), r.reason.clone(), r.think),
                    Err(e) => ("refused", String::new(), e.clone(), None),
                };
                if side == "online" {
                    entry.0 += 1;
                }
                // Judge: the offline promise, the health promise, the dial.
                let fail: Option<String> = if side == "refused" {
                    if mode == "offline" || bucket == "health" { None } else { Some("refused (no model)".into()) }
                } else if mode == "offline" && (side == "online" || reason.to_lowercase().contains("online")) {
                    Some("OFFLINE PROMISE BROKEN".into())
                } else if expected_side == "either" {
                    None
                } else if expected_side == "online" && !online_possible {
                    None // cannot be exercised here; the environment line says so
                } else if expected_side == "online" && !embeddings && bucket != "fresh_keyword" && bucket != "long_turn" {
                    None // the semantic gates are off on this machine; the environment line says so
                } else if expected_side != side {
                    // A device answer that says why (health check could not run,
                    // catalog unavailable) is the router being honest, not wrong.
                    // "As good for this question" is the strictly-better rule:
                    // it fires only when the registry scores the model that
                    // answers here above the Everyday model on that task (a
                    // coding specialist on the 4060 Ti box kept code home under
                    // Frontier-first - correct by Eric's rule). The dev battery
                    // stays strict; the field leg accepts it.
                    if side == "device" && (reason.contains("could not run") || reason.contains("unavailable") || reason.contains("as good for this question")) { None } else { Some(format!("expected {expected_side}, got {side}")) }
                } else if bucket == "long_turn" && side == "online" && !reason.contains("too long") {
                    Some("went online without the 'too long' reason".into())
                } else {
                    None
                };
                if let Some(f) = fail {
                    entry.2 += 1;
                    if listed_fails < 25 {
                        sink(format!("  FAIL {id} | {mode} | {dial} | {f} | {model} | {reason}"));
                    }
                    listed_fails += 1;
                    failures.push(format!("routing: {id} {mode}/{dial}: {f}"));
                }
                if let (Some(want), Some(got)) = (q["think"].as_bool(), think) {
                    if want == got { think_ok += 1 } else { think_bad += 1 }
                }
            }
        }
    }
    sink(format!("{:<15} {:<17} {:<9} {:<9} {:>7} {:>5}", "mode", "bucket", "dial", "", "online", "fails"));
    for ((mode, bucket, dial), (online, n, fails)) in &tally {
        sink(format!("{:<15} {:<17} {:<9} {:<9} {:>3}/{:<3} {:>5}", mode, bucket, dial, "", online, n, fails));
    }
    if capped {
        if cancelled() {
            sink(format!("cancelled at {decided} decisions - the table above is partial"));
        } else {
            sink(format!("STOPPED after 25 min at {decided} decisions - the table above is partial (a slow or stalled embed server? see the app log)"));
            failures.push(format!("routing: stopped after 25 min at {decided} decisions"));
        }
    }
    sink(format!("think verdicts: {think_ok} as expected, {think_bad} not ({decided} decisions in {:.0} s)", started.elapsed().as_secs_f64()));
    if think_bad > think_ok {
        failures.push(format!("routing: think verdicts off ({think_bad} of {})", think_ok + think_bad));
    }
    if listed_fails > 25 {
        sink(format!("  ... {} more failures not listed", listed_fails - 25));
    }
    failures
}

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
    legs: Option<Vec<String>>,
) -> Result<String, String> {
    // A subset of legs (dev trigger / a targeted rerun); None = all.
    let legs: Vec<String> = legs.unwrap_or_default();
    let want = |name: &str| legs.is_empty() || legs.iter().any(|l| l == name);
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
    // Every line is scrubbed of the home directory before it is written:
    // the report is made to be sent to other people, and an engine error
    // echoing an absolute path must not carry the OS account name.
    let home_prefix = std::env::var("USERPROFILE")
        .or_else(|_| std::env::var("HOME"))
        .unwrap_or_default();
    let sink = move |line: String| {
        let line = if home_prefix.len() > 3 {
            line.replace(&home_prefix, "~")
        } else {
            line
        };
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
    // The report is made to be sent to other people: the models path is
    // shown with the home directory folded to ~ so the OS account name
    // never rides along.
    let dir_shown = {
        let home = std::env::var("USERPROFILE")
            .or_else(|_| std::env::var("HOME"))
            .unwrap_or_default();
        let d = dir.display().to_string();
        if !home.is_empty() && d.starts_with(&home) {
            format!("~{}", &d[home.len()..])
        } else {
            d
        }
    };
    sink(format!(
        "RAM: {:.1} GB | models: {} ({} GGUF, {} MLX)",
        total_ram_gb(),
        dir_shown,
        ggufs.len(),
        mlxs.len()
    ));
    sink(String::new());

    let mut failures: Vec<String> = Vec::new();

    if want("components") {
        sink("== Components (the small servers answer) ==".into());
        failures.extend(leg_components(&app, &dir, sink).await);
        sink(String::new());
    }

    if want("fit") {
        sink("== Fit truth (claim vs real load) ==".into());
        failures.extend(leg_fit_truth(Some(&app), &bin, &dir, &[], sink).await);
        sink(String::new());
    }

    if !cancelled() && want("chat") {
        sink("== Chat format (words, no channel markers; think switch) ==".into());
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
    if !cancelled() && want("sampling") {
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
    if !cancelled() && want("tune") {
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

    if !cancelled() && want("mlx") {
        sink("== MLX ==".into());
        failures.extend(leg_mlx(&app, &dir, sink).await);
        sink(String::new());
    }

    if !cancelled() && want("routing") {
        sink("== Routing (decide-only, no credits; the battery on this machine's models) ==".into());
        failures.extend(leg_routing(&app, &dir, sink).await);
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
