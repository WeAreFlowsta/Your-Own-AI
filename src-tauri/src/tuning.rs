//! Per-model fine-tune settings (FINE_TUNE_PANEL.md, layer 2).
//!
//! `model-tuning.json` in app data: machine-specific overrides per model
//! file - deliberately NOT a sidecar in the models folder, because "right
//! for this VRAM" must not travel to another computer with the file.
//! Every field unset = the automatics decide, byte-for-byte as before.

use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Default)]
pub struct ModelTuning {
    /// Pinned context size. Wins over the sizing AND over growth requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    /// MoE expert layers on the CPU. 0 = everything on the card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moe_cpu_layers: Option<u32>,
    /// Leave the registered speed-up draft out of the next loads.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft_off: Option<bool>,
}

impl ModelTuning {
    pub fn is_empty(&self) -> bool {
        self.context.is_none() && self.moe_cpu_layers.is_none() && self.draft_off.is_none()
    }
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("model-tuning.json"))
}

fn load_all(app: &AppHandle) -> HashMap<String, ModelTuning> {
    let Some(p) = path(app) else { return HashMap::new() };
    std::fs::read_to_string(p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// The overrides for one model file; all-None when nothing was set.
pub fn get(app: &AppHandle, model: &str) -> ModelTuning {
    load_all(app).get(model).copied().unwrap_or_default()
}

/// Machine-level worker-thread choice (settings.json `engineThreads`).
pub fn engine_threads(app: &AppHandle) -> Option<u32> {
    use tauri_plugin_store::StoreExt;
    let store = app.store("settings.json").ok()?;
    store
        .get("engineThreads")
        .and_then(|v| v.as_u64())
        .filter(|&t| t >= 1 && t <= 256)
        .map(|t| t as u32)
}

#[tauri::command]
pub async fn tuning_get(app: AppHandle, model: String) -> Result<ModelTuning, String> {
    Ok(get(&app, &model))
}

#[tauri::command]
pub async fn tuning_set(app: AppHandle, model: String, tuning: ModelTuning) -> Result<(), String> {
    crate::llm::forgive_too_big(&model);
    let mut all = load_all(&app);
    if tuning.is_empty() {
        all.remove(&model);
    } else {
        all.insert(model.clone(), tuning);
    }
    let p = path(&app).ok_or("cannot resolve app data dir")?;
    std::fs::write(&p, serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?)
        .map_err(|e| format!("cannot write {}: {e}", p.display()))
}

#[tauri::command]
pub async fn tuning_set_engine_threads(app: AppHandle, threads: Option<u32>) -> Result<(), String> {
    use tauri_plugin_store::StoreExt;
    let store = app.store("settings.json").map_err(|e| e.to_string())?;
    match threads.filter(|&t| t >= 1 && t <= 256) {
        Some(t) => store.set("engineThreads", serde_json::json!(t)),
        None => {
            store.delete("engineThreads");
        }
    }
    store.save().map_err(|e| e.to_string())
}

/// Apply a changed tuning immediately when THIS model is the loaded one:
/// the load_model short-circuit would otherwise no-op the reload. Returns
/// whether a reload actually ran (false = it applies at the next load).
#[tauri::command]
pub async fn tuning_apply_now(
    app: AppHandle,
    state: State<'_, crate::llm::LLMState>,
    model: String,
) -> Result<bool, String> {
    let current = state.current_model.lock().await.clone();
    if current.as_deref() != Some(model.as_str()) {
        return Ok(false);
    }
    crate::llm::FORCE_RELOAD_NEXT.store(true, std::sync::atomic::Ordering::SeqCst);
    let with_vision = state.current_mmproj.lock().await.is_some();
    crate::llm::load_model(app, state, model, with_vision, "fine-tune".into()).await?;
    Ok(true)
}

// ---------------------------------------------------------------------------
// The tune run (FINE_TUNE_PANEL build order 3): a consented bench that tries
// a handful of arms - context rungs, the expert split around the picker's
// choice, the draft on and off - on a separate port with the same binary and
// flags the app serves with, and stamps the server's own timings into
// `tune-profiles.json`. The Speed-Room slider reads that table; every number
// it shows was measured on this machine.
// ---------------------------------------------------------------------------

pub const BENCH_PORT: u16 = 18098;

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct TuneArm {
    pub ctx: u64,
    /// None = dense (no flag). Some(0) = everything on the card. Some(n) =
    /// that many expert layers' weights in main memory.
    pub moe_cpu_layers: Option<u32>,
    pub draft: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct TuneResult {
    pub ctx: u64,
    pub moe_cpu_layers: Option<u32>,
    pub draft: bool,
    pub load_secs: f32,
    pub pp_tps: f32,
    pub gen_tps: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed: Option<String>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Default)]
pub struct TuneProfile {
    pub measured_at: u64,
    pub results: Vec<TuneResult>,
}

fn profiles_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("tune-profiles.json"))
}

fn profiles_load(app: &AppHandle) -> HashMap<String, TuneProfile> {
    let Some(p) = profiles_path(app) else { return HashMap::new() };
    std::fs::read_to_string(p).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default()
}

#[tauri::command]
pub async fn tune_profiles_get(app: AppHandle, model: String) -> Result<Option<TuneProfile>, String> {
    Ok(profiles_load(&app).get(&model).cloned())
}

/// The arms worth measuring for this model on this machine: the automatic
/// context rung plus one below and one above (inside the trained limit),
/// each with the automatic expert split; at the automatic rung also the
/// draft switched off (when one is registered) and a leaner split. Small on
/// purpose - five loads at most.
pub fn arms_for(
    meta: &crate::gguf::GgufMeta,
    size_bytes: u64,
    total_ram_gb: f64,
    free_vram_gb: Option<f64>,
    has_draft: bool,
) -> Vec<TuneArm> {
    const LADDER: [u64; 6] = [4096, 8192, 16384, 32768, 65536, 131072];
    let auto_ctx = crate::fit::choose_ctx(meta, size_bytes, total_ram_gb, free_vram_gb);
    let cap = if meta.context_length > 0 { meta.context_length } else { u64::MAX };
    let i = LADDER
        .iter()
        .position(|&c| c >= auto_ctx)
        .unwrap_or(LADDER.len() - 1);
    let mut rungs: Vec<u64> = Vec::new();
    if i > 0 {
        rungs.push(LADDER[i - 1]);
    }
    rungs.push(LADDER[i]);
    if i + 1 < LADDER.len() && LADDER[i + 1] <= cap {
        rungs.push(LADDER[i + 1]);
    }
    let auto_n = |ctx: u64| -> Option<u32> {
        if !meta.is_moe() {
            return None;
        }
        let free = free_vram_gb?;
        let (_, kv_gb, need_gb) = crate::fit::model_need(meta, size_bytes, ctx);
        if !crate::fit::moe_offload_wanted(need_gb, free) {
            return Some(0);
        }
        crate::fit::moe_cpu_layers(meta, kv_gb, free)
            .map(|n| n as u32)
            .or(Some(meta.expert_bytes_per_layer.len() as u32))
    };
    let mut arms: Vec<TuneArm> = Vec::new();
    for &r in &rungs {
        arms.push(TuneArm { ctx: r, moe_cpu_layers: auto_n(r), draft: has_draft });
    }
    let auto_rung = LADDER[i];
    if has_draft {
        arms.push(TuneArm { ctx: auto_rung, moe_cpu_layers: auto_n(auto_rung), draft: false });
    }
    if let Some(n) = auto_n(auto_rung) {
        if n > 0 {
            let step = ((meta.expert_bytes_per_layer.len() as u32) / 8).max(2);
            arms.push(TuneArm { ctx: auto_rung, moe_cpu_layers: Some(n.saturating_sub(step)), draft: has_draft });
        }
    }
    let mut seen = std::collections::HashSet::new();
    arms.retain(|a| seen.insert((a.ctx, a.moe_cpu_layers, a.draft)));
    arms
}

static TUNE_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

#[tauri::command]
pub async fn tune_cancel() -> Result<(), String> {
    TUNE_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst);
    Ok(())
}

/// One arm: spawn on the bench port, wait for health, one measured
/// completion (the server's own timings), kill. Failures are results too.
pub async fn bench_one(
    bin: &std::path::Path,
    models_dir: &std::path::Path,
    model: &str,
    arm: TuneArm,
    draft_file: Option<(String, String)>,
    threads: Option<u32>,
    gpu_args: &[String],
) -> TuneResult {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut result = TuneResult {
        ctx: arm.ctx,
        moe_cpu_layers: arm.moe_cpu_layers,
        draft: arm.draft,
        load_secs: 0.0,
        pp_tps: 0.0,
        gen_tps: 0.0,
        failed: None,
    };
    let mut args: Vec<String> = vec![
        "--port".into(), BENCH_PORT.to_string(),
        "--host".into(), "127.0.0.1".into(),
        "--no-webui".into(), "--reasoning".into(), "off".into(),
        "--ctx-size".into(), arm.ctx.to_string(),
        "--fit".into(), "off".into(),
        "--model".into(), model.to_string(),
    ];
    if let Some(t) = threads {
        args.push("--threads".into());
        args.push(t.to_string());
    }
    args.extend(gpu_args.iter().cloned());
    if let Some(n) = arm.moe_cpu_layers {
        if n > 0 {
            args.push("--n-cpu-moe".into());
            args.push(n.to_string());
        }
    }
    if arm.draft {
        if let Some((dt, df)) = &draft_file {
            args.push("--spec-type".into());
            args.push(dt.clone());
            args.push("--spec-draft-model".into());
            args.push(df.clone());
        }
    }
    let child = Command::new(bin)
        .args(&args)
        .current_dir(models_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            result.failed = Some(format!("could not start: {e}"));
            return result;
        }
    };
    struct KillOnDrop<'a>(&'a mut std::process::Child);
    impl Drop for KillOnDrop<'_> {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let stderr = child.stderr.take();
    let tail = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    if let Some(err) = stderr {
        let sink = tail.clone();
        std::thread::spawn(move || {
            use std::io::BufRead;
            for line in std::io::BufReader::new(err).lines().map_while(Result::ok) {
                let mut t = sink.lock().unwrap();
                t.push(line);
                let over = t.len().saturating_sub(20);
                if over > 0 {
                    t.drain(0..over);
                }
            }
        });
    }
    let guard = KillOnDrop(&mut child);
    let client = reqwest::Client::new();
    let t0 = Instant::now();
    let deadline = t0 + Duration::from_secs(240);
    loop {
        if TUNE_CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
            result.failed = Some("cancelled".into());
            return result;
        }
        if let Ok(Some(status)) = guard.0.try_wait() {
            let t = tail.lock().unwrap().join("\n");
            result.failed = Some(format!("did not load ({status}): {}", t.chars().rev().take(300).collect::<String>().chars().rev().collect::<String>()));
            return result;
        }
        if let Ok(r) = client.get(format!("http://127.0.0.1:{BENCH_PORT}/health")).send().await {
            if r.status().is_success() {
                break;
            }
        }
        if Instant::now() >= deadline {
            result.failed = Some("did not become ready within 240 s".into());
            return result;
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
    result.load_secs = t0.elapsed().as_secs_f32();
    // A fixed reading-heavy prompt (~700 tokens) and a short answer: prompt
    // speed and generation speed from the server's own timing report.
    let sentence = "The measurement paragraph describes the same simple scene again so that every arm reads an identical stretch of text before it answers the one small question at the end. ";
    let prompt = format!("{}\nIn one short sentence, what is this text for?", sentence.repeat(24));
    let mut body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}],
        "max_tokens": 96,
        "temperature": 0,
        "stream": false,
        "stop": crate::llm::chat_stop_strings(model),
    });
    let (budget, effort) = crate::llm::chat_turn_reasoning_controls(model);
    if let Some(b) = budget {
        body["reasoning_budget_tokens"] = serde_json::json!(b);
    }
    if let Some(e) = effort {
        body["reasoning_effort"] = serde_json::json!(e);
    }
    match client
        .post(format!("http://127.0.0.1:{BENCH_PORT}/v1/chat/completions"))
        .json(&body)
        .timeout(Duration::from_secs(300))
        .send()
        .await
    {
        Ok(resp) => match resp.json::<serde_json::Value>().await {
            Ok(v) => {
                result.pp_tps = v["timings"]["prompt_per_second"].as_f64().unwrap_or(0.0) as f32;
                result.gen_tps = v["timings"]["predicted_per_second"].as_f64().unwrap_or(0.0) as f32;
                if result.gen_tps <= 0.0 {
                    result.failed = Some("no timing in the reply".into());
                }
            }
            Err(e) => result.failed = Some(format!("reply unreadable: {e}")),
        },
        Err(e) => result.failed = Some(format!("request failed: {e}")),
    }
    result
}

/// The consented tune run: stops the chat server (the model holds the card),
/// measures each arm on the bench port, stores the table. Partial results
/// are stored too - a cancelled run keeps what it learned.
#[tauri::command]
pub async fn tune_run(
    app: AppHandle,
    state: State<'_, crate::llm::LLMState>,
    model: String,
) -> Result<TuneProfile, String> {
    use tauri::Emitter;
    TUNE_CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
    let models_dir = crate::llm::get_models_dir(&app)?;
    let path = models_dir.join(&model);
    if !path.is_file() {
        return Err(format!("{model} is not in the models folder"));
    }
    let meta = crate::gguf::read_meta(&path).map_err(|e| format!("cannot read the model header: {e}"))?;
    let size = std::fs::metadata(&path).map_err(|e| e.to_string())?.len();
    let sys = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::nothing().with_memory(sysinfo::MemoryRefreshKind::everything()),
    );
    let total_ram_gb = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
    let bin = match crate::engine::active_backend(&app) {
        crate::engine::Backend::Cuda => crate::engine::cuda_engine_binary(&app).ok_or("CUDA engine binary missing")?,
        crate::engine::Backend::Bundled => crate::resolve_sidecar_bin("llama-server"),
    };
    if !bin.is_file() {
        return Err(format!("engine binary missing at {}", bin.display()));
    }
    let gpu_args = crate::llm::select_gpu_device_args(&app).await;
    let free_vram_gb = crate::llm::available_vram_mib(&app).await.map(|m| m as f64 / 1024.0);
    let draft_file = crate::llm::model_draft_for(&models_dir, &model).map(|d| (d.draft_type, d.draft));
    let arms = arms_for(&meta, size, total_ram_gb, free_vram_gb, draft_file.is_some());
    let total = arms.len();
    log::info!("[tune] {model}: {total} arms, free VRAM {free_vram_gb:?}");
    crate::llm::stop_chat_server_for_maintenance(&state).await;
    let mut results = Vec::new();
    for (i, arm) in arms.into_iter().enumerate() {
        if TUNE_CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        let desc = format!(
            "{} context{}{}",
            arm.ctx,
            match arm.moe_cpu_layers { Some(0) => " - all on the card".into(), Some(n) => format!(" - {n} expert layers in RAM"), None => String::new() },
            if draft_file.is_some() { if arm.draft { " - speed-up on" } else { " - speed-up off" } } else { "" }
        );
        let _ = app.emit("tune-run", serde_json::json!({ "model": model, "done": i, "total": total, "current": desc }));
        let r = bench_one(&bin, &models_dir, &model, arm, draft_file.clone(), engine_threads(&app), &gpu_args).await;
        log::info!(
            "[tune] {model} arm {desc}: load {:.1} s, prompt {:.0} tok/s, gen {:.1} tok/s{}",
            r.load_secs, r.pp_tps, r.gen_tps,
            r.failed.as_deref().map(|f| format!(" FAILED: {f}")).unwrap_or_default()
        );
        results.push(r);
    }
    let _ = app.emit("tune-run", serde_json::json!({ "model": model, "done": total, "total": total, "current": "" }));
    let profile = TuneProfile {
        measured_at: std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0),
        results,
    };
    let mut all = profiles_load(&app);
    all.insert(model.clone(), profile.clone());
    if let Some(p) = profiles_path(&app) {
        let _ = std::fs::write(&p, serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?);
    }
    Ok(profile)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Headless matrix leg for the tune bench: two arms on the shipped
    /// binary and the matrix model, real timings. Same runner as the MoE
    /// matrix: `cargo test --lib -- --ignored live_matrix --nocapture`.
    #[tokio::test]
    #[ignore]
    async fn live_matrix_tune_bench() {
        let _one_at_a_time = crate::llm::LIVE_MATRIX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = std::env::var("HOME").unwrap_or_default();
        let dir = std::env::var("YOAI_MODELS_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::Path::new(&home).join(".local/share/com.solar.yourowai/models"));
        let model = std::env::var("YOAI_MATRIX_MODEL").unwrap_or_else(|_| "LFM2.5-8B-A1B-Q4_K_M.gguf".into());
        let path = dir.join(&model);
        if !path.exists() {
            eprintln!("[matrix] SKIP: {} not present", path.display());
            return;
        }
        let triple = if cfg!(target_os = "windows") { "x86_64-pc-windows-msvc.exe" }
            else if cfg!(target_os = "macos") { if cfg!(target_arch = "aarch64") { "aarch64-apple-darwin" } else { "x86_64-apple-darwin" } }
            else { "x86_64-unknown-linux-gnu" };
        let bin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("bin").join(format!("llama-server-{triple}"));
        assert!(bin.exists(), "shipped engine binary missing at {}", bin.display());
        let meta = crate::gguf::read_meta(&path).expect("model header reads");
        let size = std::fs::metadata(&path).unwrap().len();
        // Free VRAM the way the app sees it, so the arms carry the real MoE
        // decision - without it a bigger-than-the-card model has no split
        // and the engine's default full offload fails to load.
        let free_vram_gb = std::process::Command::new(&bin)
            .arg("--list-devices")
            .output()
            .ok()
            .and_then(|out| {
                let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
                text.push_str(&String::from_utf8_lossy(&out.stderr));
                let discrete: Vec<_> = crate::llm::parse_gpu_devices(&text).into_iter().filter(|d| !d.integrated).collect();
                if discrete.is_empty() { None } else { Some(discrete.iter().map(|d| d.free_mib).sum::<u64>() as f64 / 1024.0) }
            });
        eprintln!("[matrix] tune bench free VRAM: {free_vram_gb:?}");
        let arms = arms_for(&meta, size, 31.0, free_vram_gb, false);
        assert!(!arms.is_empty());
        for arm in arms.into_iter().take(2) {
            let r = bench_one(&bin, &dir, &model, arm, None, None, &[]).await;
            eprintln!(
                "[matrix] tune arm ctx {} moe {:?} draft {}: load {:.1} s, prompt {:.0} tok/s, gen {:.1} tok/s, failed {:?}",
                r.ctx, r.moe_cpu_layers, r.draft, r.load_secs, r.pp_tps, r.gen_tps, r.failed
            );
            assert!(r.failed.is_none(), "arm failed: {:?}", r.failed);
            assert!(r.gen_tps > 0.0, "no generation timing");
        }
    }

    fn moe_meta() -> crate::gguf::GgufMeta {
        crate::gguf::GgufMeta {
            n_layers: 24,
            context_length: 32768,
            expert_bytes_per_layer: vec![150_000_000; 24],
            n_experts: 32,
            n_experts_used: 4,
            ..Default::default()
        }
    }

    #[test]
    fn sampling_reaches_the_body_with_the_right_precedence() {
        use crate::llm::{apply_sampling, SamplingParams};
        // No overrides: the app's constants, min_p absent (engine default rules).
        let mut b = serde_json::json!({});
        apply_sampling(&mut b, None, false);
        assert_eq!(b["temperature"], serde_json::json!(0.7));
        assert_eq!(b["top_p"], serde_json::json!(0.9));
        assert_eq!(b["repeat_penalty"], serde_json::json!(1.1));
        assert!(b.get("min_p").is_none());
        // Overrides win field by field; min_p appears only when chosen.
        let s = SamplingParams { temperature: Some(0.2), min_p: Some(0.1), ..Default::default() };
        let mut b = serde_json::json!({});
        apply_sampling(&mut b, Some(&s), false);
        assert_eq!(b["temperature"], serde_json::json!(0.2));
        assert_eq!(b["top_p"], serde_json::json!(0.9));
        assert_eq!(b["min_p"], serde_json::json!(0.1));
        // Remote: minimal standard body - temperature always, top_p only when chosen.
        let mut b = serde_json::json!({});
        apply_sampling(&mut b, Some(&s), true);
        assert_eq!(b["temperature"], serde_json::json!(0.2));
        assert!(b.get("top_p").is_none());
        assert!(b.get("min_p").is_none());
        let mut b = serde_json::json!({});
        apply_sampling(&mut b, Some(&SamplingParams { top_p: Some(0.5), ..Default::default() }), true);
        assert_eq!(b["top_p"], serde_json::json!(0.5));
    }

    /// Headless matrix leg: the sampling knobs REACH the engine. One server,
    /// three completions through `apply_sampling`: temperature 0 twice must
    /// answer identically (greedy is deterministic); high temperature with
    /// two seeds must not both reproduce the greedy text.
    #[tokio::test]
    #[ignore]
    async fn live_matrix_sampling() {
        use std::process::{Command, Stdio};
        use std::time::{Duration, Instant};
        let _one_at_a_time = crate::llm::LIVE_MATRIX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        const PORT: u16 = 18097;
        let home = std::env::var("HOME").unwrap_or_default();
        let dir = std::env::var("YOAI_MODELS_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::Path::new(&home).join(".local/share/com.solar.yourowai/models"));
        let model = std::env::var("YOAI_MATRIX_MODEL").unwrap_or_else(|_| "LFM2.5-8B-A1B-Q4_K_M.gguf".into());
        if !dir.join(&model).exists() {
            eprintln!("[matrix] SKIP: {} not present", dir.join(&model).display());
            return;
        }
        let triple = if cfg!(target_os = "windows") { "x86_64-pc-windows-msvc.exe" }
            else if cfg!(target_os = "macos") { if cfg!(target_arch = "aarch64") { "aarch64-apple-darwin" } else { "x86_64-apple-darwin" } }
            else { "x86_64-unknown-linux-gnu" };
        let bin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("bin").join(format!("llama-server-{triple}"));
        assert!(bin.exists());
        let meta = crate::gguf::read_meta(&dir.join(&model)).expect("header");
        let size = std::fs::metadata(dir.join(&model)).unwrap().len();
        let free = Command::new(&bin).arg("--list-devices").output().ok().and_then(|out| {
            let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
            text.push_str(&String::from_utf8_lossy(&out.stderr));
            let d: Vec<_> = crate::llm::parse_gpu_devices(&text).into_iter().filter(|d| !d.integrated).collect();
            if d.is_empty() { None } else { Some(d.iter().map(|x| x.free_mib).sum::<u64>() as f64 / 1024.0) }
        });
        let mut args: Vec<String> = vec![
            "--port".into(), PORT.to_string(), "--host".into(), "127.0.0.1".into(),
            "--no-webui".into(), "--reasoning".into(), "off".into(),
            "--ctx-size".into(), "4096".into(), "--fit".into(), "off".into(),
            "--model".into(), model.clone(),
        ];
        if meta.is_moe() {
            if let Some(f) = free {
                let (_, kv, need) = crate::fit::model_need(&meta, size, 4096);
                if crate::fit::moe_offload_wanted(need, f) {
                    match crate::fit::moe_cpu_layers(&meta, kv, f) {
                        Some(n) if n < meta.expert_bytes_per_layer.len() => {
                            args.push("--n-cpu-moe".into()); args.push(n.to_string());
                        }
                        _ => args.push("--cpu-moe".into()),
                    }
                }
            }
        }
        let mut child = Command::new(&bin).args(&args).current_dir(&dir).stdout(Stdio::null()).stderr(Stdio::null()).spawn().expect("spawn");
        struct Kill<'a>(&'a mut std::process::Child);
        impl Drop for Kill<'_> { fn drop(&mut self) { let _ = self.0.kill(); let _ = self.0.wait(); } }
        let guard = Kill(&mut child);
        let client = reqwest::Client::new();
        let deadline = Instant::now() + Duration::from_secs(240);
        loop {
            if let Ok(Some(st)) = guard.0.try_wait() { panic!("server exited during load: {st}"); }
            if let Ok(r) = client.get(format!("http://127.0.0.1:{PORT}/health")).send().await {
                if r.status().is_success() { break; }
            }
            assert!(Instant::now() < deadline, "not healthy in 240 s");
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let ask = |sampling: Option<crate::llm::SamplingParams>, seed: Option<u64>| {
            let client = client.clone();
            let model = model.clone();
            async move {
                let mut body = serde_json::json!({
                    "model": model,
                    "messages": [{"role": "user", "content": "Describe an imaginary small town in about 60 words."}],
                    "max_tokens": 80,
                    "stream": false,
                    "stop": crate::llm::chat_stop_strings(&model),
                });
                let (budget, effort) = crate::llm::chat_turn_reasoning_controls(&model);
                if let Some(b) = budget { body["reasoning_budget_tokens"] = serde_json::json!(b); }
                if let Some(e) = effort { body["reasoning_effort"] = serde_json::json!(e); }
                crate::llm::apply_sampling(&mut body, sampling.as_ref(), false);
                if let Some(sd) = seed { body["seed"] = serde_json::json!(sd); }
                let v: serde_json::Value = client
                    .post(format!("http://127.0.0.1:{PORT}/v1/chat/completions"))
                    .json(&body).timeout(Duration::from_secs(180))
                    .send().await.expect("request").json().await.expect("json");
                v["choices"][0]["message"]["content"].as_str().unwrap_or("").to_string()
            }
        };
        let greedy = crate::llm::SamplingParams { temperature: Some(0.0), ..Default::default() };
        let a1 = ask(Some(greedy), None).await;
        let a2 = ask(Some(greedy), None).await;
        eprintln!("[matrix] sampling greedy len {}: {}", a1.len(), a1.chars().take(80).collect::<String>());
        assert!(!a1.trim().is_empty());
        assert_eq!(a1, a2, "temperature 0 must be deterministic - the knob did not reach the engine?");
        let wild = crate::llm::SamplingParams { temperature: Some(1.8), top_p: Some(1.0), ..Default::default() };
        let b1 = ask(Some(wild), Some(7)).await;
        let b2 = ask(Some(wild), Some(8)).await;
        eprintln!("[matrix] sampling wild lens {} / {}", b1.len(), b2.len());
        assert!(!b1.trim().is_empty() && !b2.trim().is_empty());
        assert!(
            !(b1 == a1 && b2 == a1),
            "high temperature reproduced the greedy text twice - sampling not applied"
        );
    }

    #[test]
    fn arms_cover_rungs_draft_and_leaner_split() {
        let meta = moe_meta();
        let arms = arms_for(&meta, 4_800_000_000, 31.0, Some(2.0), true);
        assert!(arms.len() <= 5, "small on purpose: {arms:?}");
        assert!(arms.iter().any(|a| !a.draft), "a draft-off arm exists");
        let ctxs: std::collections::HashSet<u64> = arms.iter().map(|a| a.ctx).collect();
        assert!(ctxs.len() >= 2, "more than one context rung: {ctxs:?}");
        assert!(arms.iter().all(|a| a.ctx <= 32768), "inside the trained limit");
    }

    #[test]
    fn arms_for_dense_have_no_moe_field() {
        let meta = crate::gguf::GgufMeta { n_layers: 32, context_length: 16384, ..Default::default() };
        let arms = arms_for(&meta, 5_000_000_000, 31.0, Some(8.0), false);
        assert!(arms.iter().all(|a| a.moe_cpu_layers.is_none()));
        assert!(arms.iter().all(|a| !a.draft));
    }
}
