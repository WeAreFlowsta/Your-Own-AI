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
    /// KV cache precision: "f16" (standard) or "q8_0" (compact, half the
    /// context memory). None = Auto: compact only where this machine's
    /// tune profile measured it clean (see `kv_choice`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kv_cache: Option<KvCache>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Debug, PartialEq)]
pub enum KvCache {
    #[serde(rename = "f16")]
    F16,
    #[serde(rename = "q8_0")]
    Q8_0,
}

impl ModelTuning {
    pub fn is_empty(&self) -> bool {
        self.context.is_none() && self.moe_cpu_layers.is_none() && self.draft_off.is_none() && self.kv_cache.is_none()
    }
}

/// What the loader does about the KV cache for one model.
#[derive(Clone, Debug, PartialEq)]
pub struct KvChoice {
    pub q8: bool,
    pub reason: &'static str,
}

/// Compact (q8_0) KV cache is worth half the context memory - a bigger
/// reading room, or a model that now fits the card whole - and costs
/// nothing at batch 1 where the engine has the path. It is never assumed:
/// Auto picks it only when this machine's tune profile holds a compact arm
/// that loaded and generated within 5% of the standard arm at the same
/// settings. The person's explicit setting always wins.
pub fn kv_choice(app: &AppHandle, model: &str) -> KvChoice {
    match get(app, model).kv_cache {
        Some(KvCache::Q8_0) => return KvChoice { q8: true, reason: "your fine-tune setting" },
        Some(KvCache::F16) => return KvChoice { q8: false, reason: "your fine-tune setting" },
        None => {}
    }
    let profile = profiles_load(app).remove(model);
    kv_choice_from_profile(profile.as_ref())
}

pub fn kv_choice_from_profile(profile: Option<&TuneProfile>) -> KvChoice {
    let Some(p) = profile else { return KvChoice { q8: false, reason: "not measured on this computer yet" } };
    let compact: Vec<&TuneResult> = p.results.iter().filter(|r| r.kv_q8 && r.failed.is_none() && r.gen_tps > 0.0).collect();
    if compact.is_empty() {
        return KvChoice { q8: false, reason: "the compact cache did not measure clean here" };
    }
    for c in compact {
        let twin = p.results.iter().find(|r| {
            !r.kv_q8 && r.failed.is_none() && r.ctx == c.ctx && r.moe_cpu_layers == c.moe_cpu_layers && r.draft == c.draft
        });
        match twin {
            Some(t) if c.gen_tps >= 0.95 * t.gen_tps => return KvChoice { q8: true, reason: "measured within 5% of the standard cache here" },
            Some(_) => return KvChoice { q8: false, reason: "the compact cache measured slower here" },
            None => {}
        }
    }
    KvChoice { q8: false, reason: "no standard arm to compare the compact cache against" }
}

/// The expert split the bench proved faster on this machine, if any: at
/// this context, an arm with FEWER expert layers in main memory than the
/// picker's that loaded and generated at least 5% faster. The picker
/// estimates from the file's tensor table; the bench measured the card.
/// (4060 Ti, 09-06: gpt-oss 20B at 16K, 13 layers instead of 16, 39.8 vs
/// 34.5 tok/s.) Only ever fewer layers, never more than the picker allows.
pub fn measured_moe_split(profile: Option<&TuneProfile>, ctx: u64, picker_n: usize) -> Option<usize> {
    let p = profile?;
    let at_ctx: Vec<&TuneResult> = p.results.iter().filter(|r| r.ctx == ctx && r.failed.is_none() && r.gen_tps > 0.0).collect();
    let picker = at_ctx.iter().find(|r| r.moe_cpu_layers.map(|n| n as usize) == Some(picker_n))?;
    at_ctx
        .iter()
        .filter(|r| r.moe_cpu_layers.map(|n| (n as usize) < picker_n).unwrap_or(false))
        .filter(|r| r.gen_tps >= 1.05 * picker.gen_tps)
        .max_by(|a, b| a.gen_tps.partial_cmp(&b.gen_tps).unwrap_or(std::cmp::Ordering::Equal))
        .and_then(|r| r.moe_cpu_layers.map(|n| n as usize))
}

pub fn measured_moe_split_for(app: &AppHandle, model: &str, ctx: u64, picker_n: usize) -> Option<usize> {
    let profile = profiles_load(app).remove(model);
    measured_moe_split(profile.as_ref(), ctx, picker_n)
}

/// How much of the f16 KV estimate this model's cache really takes.
pub fn kv_scale_for(app: &AppHandle, model: &str) -> f64 {
    if kv_choice(app, model).q8 { 0.5 } else { 1.0 }
}

/// The engine flags for a compact cache. Quantized V needs flash
/// attention on; the bench arm proves the pair loads here before Auto
/// ever sends them.
pub fn kv_q8_args() -> [&'static str; 6] {
    ["-ctk", "q8_0", "-ctv", "q8_0", "-fa", "on"]
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
    /// Compact (q8_0) KV cache with flash attention on.
    #[serde(default)]
    pub kv_q8: bool,
    /// Bigger micro-batch (`-ub 2048 -b 2048`): fewer, larger passes over
    /// expert weights in main memory. 0 = the engine default (512).
    #[serde(default)]
    pub ubatch: u32,
    /// On the processor instead of the graphics device the engine would
    /// pick. Tried on machines whose only graphics is integrated: an Intel
    /// UHD 630 read at 29 tok/s and wrote at 5.6 where the same machine's
    /// processor did 52 and 9.1 (2026-09-22) - and a modern integrated GPU
    /// may well win; measured, never assumed.
    #[serde(default)]
    pub cpu: bool,
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub struct TuneResult {
    pub ctx: u64,
    pub moe_cpu_layers: Option<u32>,
    pub draft: bool,
    #[serde(default)]
    pub kv_q8: bool,
    #[serde(default)]
    pub ubatch: u32,
    #[serde(default)]
    pub cpu: bool,
    pub load_secs: f32,
    pub pp_tps: f32,
    pub gen_tps: f32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub failed: Option<String>,
    /// Free VRAM read by the caller's probe right AFTER the measured
    /// completion, server still up - the honest moment (CUDA allocates
    /// lazily; a probe racing the load reads 0). Matrix-only; never stored.
    #[serde(skip, default)]
    pub during_free: Option<f64>,
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
///
/// `runs_at` is the context the app reports for this model (the models
/// page's "runs at", fit's `context_runtime`, a pin included). When given
/// it is the automatic rung: the table then always holds the setup the
/// model actually starts with, so the Fine-tune dialog can say what
/// Automatic runs at. Without it the planner sizes the rung itself.
pub fn arms_for(
    meta: &crate::gguf::GgufMeta,
    size_bytes: u64,
    total_ram_gb: f64,
    free_vram_gb: Option<f64>,
    has_draft: bool,
    runs_at: Option<u64>,
) -> Vec<TuneArm> {
    const LADDER: [u64; 6] = [4096, 8192, 16384, 32768, 65536, 131072];
    let cap = if meta.context_length > 0 { meta.context_length } else { u64::MAX };
    let auto_ctx = runs_at
        .filter(|&c| c >= 4096 && c <= cap)
        .unwrap_or_else(|| crate::fit::choose_ctx(meta, size_bytes, total_ram_gb, free_vram_gb));
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
        arms.push(TuneArm { ctx: r, moe_cpu_layers: auto_n(r), draft: has_draft, kv_q8: false, ubatch: 0, cpu: false });
    }
    let auto_rung = LADDER[i];
    if has_draft {
        arms.push(TuneArm { ctx: auto_rung, moe_cpu_layers: auto_n(auto_rung), draft: false, kv_q8: false, ubatch: 0, cpu: false });
    }
    if let Some(n) = auto_n(auto_rung) {
        if n > 0 {
            let step = ((meta.expert_bytes_per_layer.len() as u32) / 8).max(2);
            arms.push(TuneArm { ctx: auto_rung, moe_cpu_layers: Some(n.saturating_sub(step)), draft: has_draft, kv_q8: false, ubatch: 0, cpu: false });
        }
    }
    // The compact-cache arm: the automatic rung with everything else the
    // same, so Auto has a like-for-like twin to judge it against.
    arms.push(TuneArm { ctx: auto_rung, moe_cpu_layers: auto_n(auto_rung), draft: has_draft, kv_q8: true, ubatch: 0, cpu: false });
    // The micro-batch arm: the automatic rung again with `-ub 2048`, only
    // when expert layers sit in main memory - that is where it pays
    // (measured 2026-09-22, 8k prompt: a split MoE 501 -> 699 tok/s, a
    // model whole on the card 745 -> 729, nothing). Costs ~200 MB of card.
    if auto_n(auto_rung).map(|n| n > 0).unwrap_or(false) {
        arms.push(TuneArm { ctx: auto_rung, moe_cpu_layers: auto_n(auto_rung), draft: has_draft, kv_q8: false, ubatch: 2048, cpu: false });
    }
    let mut seen = std::collections::HashSet::new();
    arms.retain(|a| seen.insert((a.ctx, a.moe_cpu_layers, a.draft, a.kv_q8, a.ubatch, a.cpu)));
    arms
}

/// The micro-batch the bench proved faster here, if any: the arm with
/// `-ub 2048` at the automatic rung that loaded and READ at least 15%
/// faster than its like-for-like twin without writing more than 10% slower
/// (the gain is in prompt reading; the card cost is real, so a small gain
/// does not earn it).
pub fn ubatch_choice_from_profile(profile: Option<&TuneProfile>) -> u32 {
    let Some(p) = profile else { return 0 };
    for big in p.results.iter().filter(|r| r.ubatch > 0 && r.failed.is_none() && r.pp_tps > 0.0) {
        let twin = p.results.iter().find(|r| {
            r.ubatch == 0 && r.failed.is_none() && r.ctx == big.ctx && r.moe_cpu_layers == big.moe_cpu_layers && r.draft == big.draft && r.kv_q8 == big.kv_q8
        });
        if let Some(t) = twin {
            // reading at least 15% faster, writing within 10% (the measured
            // split case: reading +40%, writing -12% one run, -2% the next)
            if big.pp_tps >= 1.15 * t.pp_tps && big.gen_tps >= 0.88 * t.gen_tps {
                return big.ubatch;
            }
        }
    }
    0
}

/// Arms that depend on the MACHINE, not the model. Both are twins of the
/// automatic rung (the compact-cache arm always sits there) and are kept
/// only when they measure faster:
///  - bigger batches on the bundled graphics engine with a discrete card
///    (Vulkan: Gemma E2B read +29% with them, a dense 2B −13% - so measured
///    per model, never a blanket flag);
///  - the processor on a machine whose only graphics is integrated.
pub fn machine_arms(arms: &[TuneArm], bundled_engine: bool, discrete_card: bool, only_integrated: bool) -> Vec<TuneArm> {
    let Some(auto) = arms.iter().find(|a| a.kv_q8).map(|a| TuneArm { kv_q8: false, ..a.clone() }) else { return Vec::new() };
    let mut out = Vec::new();
    if bundled_engine && discrete_card && !arms.iter().any(|a| a.ubatch > 0) {
        out.push(TuneArm { ubatch: 2048, ..auto.clone() });
    }
    if only_integrated {
        out.push(TuneArm { cpu: true, ..auto });
    }
    out
}

/// Did the processor beat the integrated graphics here? Both reading and
/// writing at least 10% faster than the like-for-like twin.
pub fn processor_choice_from_profile(profile: Option<&TuneProfile>) -> bool {
    let Some(p) = profile else { return false };
    for c in p.results.iter().filter(|r| r.cpu && r.failed.is_none() && r.gen_tps > 0.0) {
        let twin = p.results.iter().find(|r| {
            !r.cpu && r.failed.is_none() && r.ctx == c.ctx && r.moe_cpu_layers == c.moe_cpu_layers && r.draft == c.draft && r.kv_q8 == c.kv_q8 && r.ubatch == c.ubatch
        });
        if let Some(t) = twin {
            if c.pp_tps >= 1.1 * t.pp_tps && c.gen_tps >= 1.1 * t.gen_tps {
                return true;
            }
        }
    }
    false
}

/// This model runs on the processor here because the bench proved it faster
/// than the integrated graphics.
pub fn processor_choice(app: &AppHandle, model: &str) -> bool {
    processor_choice_from_profile(profiles_load(app).get(model))
}

/// The engine flags for a micro-batch the bench proved (0 = none).
pub fn ubatch_args(ubatch: u32) -> Vec<String> {
    if ubatch == 0 { return Vec::new() }
    vec!["-ub".into(), ubatch.to_string(), "-b".into(), ubatch.to_string()]
}

/// This model's proven micro-batch on this machine (0 = the default).
pub fn ubatch_choice(app: &AppHandle, model: &str) -> u32 {
    ubatch_choice_from_profile(profiles_load(app).get(model))
}

static TUNE_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The model a bench server (tune arm, matrix leg) holds on the card right
/// now, with its context - so the grader can hand that footprint back the
/// way it does for the chat model. Without it, every grade taken during a
/// tune read an empty card that was full (dev box 09-03: "Too large" on
/// models running at full speed).
static MAINTENANCE_LOAD: std::sync::Mutex<Option<(String, u64)>> = std::sync::Mutex::new(None);

pub(crate) fn maintenance_load() -> Option<(String, u64)> {
    MAINTENANCE_LOAD.lock().ok().and_then(|g| g.clone())
}

fn set_maintenance_load(v: Option<(String, u64)>) {
    if let Ok(mut g) = MAINTENANCE_LOAD.lock() {
        *g = v;
    }
}

/// The matrix runner shares the bench's cancel switch (bench_one is the
/// only place that can abort a load in flight).
pub(crate) fn tune_cancel_flag_set(v: bool) {
    TUNE_CANCEL.store(v, std::sync::atomic::Ordering::SeqCst);
}

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
    vram_probe: Option<&(dyn Fn() -> Option<f64> + Send + Sync)>,
    // This arm is the like-for-like twin of a micro-batch arm: it reads
    // the same long prompt so the two compare.
    arm_has_ubatch_twin: bool,
) -> TuneResult {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};
    let mut result = TuneResult {
        ctx: arm.ctx,
        moe_cpu_layers: arm.moe_cpu_layers,
        draft: arm.draft,
        kv_q8: arm.kv_q8,
        ubatch: arm.ubatch,
        cpu: arm.cpu,
        load_secs: 0.0,
        pp_tps: 0.0,
        gen_tps: 0.0,
        failed: None,
        during_free: None,
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
    if arm.cpu {
        args.extend(["--device", "none", "-ngl", "0"].iter().map(|s| s.to_string()));
    } else {
        args.extend(gpu_args.iter().cloned());
    }
    if let Some(n) = arm.moe_cpu_layers {
        if n > 0 {
            args.push("--n-cpu-moe".into());
            args.push(n.to_string());
        }
    }
    // The bench loads the way the app will: weights that stay in main
    // memory are read into it, not mapped (see the chat load in llm.rs).
    let force_cpu = arm.cpu || gpu_args.windows(2).any(|w| (w[0] == "-ngl" && w[1] == "0") || (w[0] == "--device" && w[1] == "none"));
    if force_cpu || arm.moe_cpu_layers.map(|n| n > 0).unwrap_or(false) || cfg!(target_os = "macos") {
        args.push("--no-mmap".into());
    }
    args.extend(ubatch_args(arm.ubatch));
    if arm.draft {
        if let Some((dt, df)) = &draft_file {
            args.push("--spec-type".into());
            args.push(dt.clone());
            args.push("--spec-draft-model".into());
            args.push(df.clone());
        }
    }
    if arm.kv_q8 {
        args.extend(kv_q8_args().iter().map(|s| s.to_string()));
    }
    let mut cmd = Command::new(bin);
    cmd.args(&args)
        .current_dir(models_dir)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        // The app's shell-plugin spawns hide consoles; this direct spawn
        // must do it itself or every bench arm flashes a window.
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let child = cmd.spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => {
            result.failed = Some(format!("could not start: {e}"));
            return result;
        }
    };
    set_maintenance_load(Some((model.to_string(), arm.ctx)));
    struct KillOnDrop<'a>(&'a mut std::process::Child);
    impl Drop for KillOnDrop<'_> {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
            set_maintenance_load(None);
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
    // The micro-batch arm and its twin read a LONG prompt (~5k tokens):
    // a 700-token prompt is over in a quarter of a second, start-up cost
    // dominates, and the difference the arm exists to measure is invisible.
    let reps = if arm.ubatch > 0 || arm_has_ubatch_twin { 170 } else { 24 };
    let prompt = format!("{}\nIn one short sentence, what is this text for?", sentence.repeat(reps));
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
    // The server is still up and has just computed a full turn - every
    // buffer it will ever allocate is allocated NOW. This is the only
    // honest moment for a free-VRAM reading.
    if let Some(probe) = vram_probe {
        result.during_free = probe();
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
    auto_ctx: Option<u64>,
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
    // The arms are chosen for the card EMPTY: stop the chat server first,
    // then read free VRAM. Reading it first decided the arm count on a
    // full card (dev box 09-03: "Qwen 2B: 2 arms, free VRAM 0.03"). The
    // figure is cached for 20 s, so a read taken while the model still
    // held the card must be forgotten, and the driver needs a moment to
    // give the memory back after the process goes.
    if crate::llm::stop_chat_server_for_maintenance(&state).await {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
    crate::llm::invalidate_vram_cache().await;
    let free_vram_gb = crate::llm::available_vram_mib(&app).await.map(|m| m as f64 / 1024.0);
    let draft_file = crate::llm::model_draft_for(&models_dir, &model).map(|d| (d.draft_type, d.draft));
    let mut arms = arms_for(&meta, size, total_ram_gb, free_vram_gb, draft_file.is_some(), auto_ctx);
    arms.extend(machine_arms(&arms, crate::engine::active_backend(&app) == crate::engine::Backend::Bundled, !gpu_args.is_empty(), crate::llm::only_integrated_gpu(&app).await));
    let total = arms.len();
    log::info!("[tune] {model}: {total} arms, free VRAM {free_vram_gb:?}, runs at {auto_ctx:?}");
    let mut results = Vec::new();
    for (i, arm) in arms.clone().into_iter().enumerate() {
        if TUNE_CANCEL.load(std::sync::atomic::Ordering::SeqCst) {
            break;
        }
        let desc = format!(
            "{} context{}{}{}{}{}",
            arm.ctx,
            match arm.moe_cpu_layers { Some(0) => " - all on the card".into(), Some(n) => format!(" - {n} expert layers in RAM"), None => String::new() },
            if draft_file.is_some() { if arm.draft { " - speed-up on" } else { " - speed-up off" } } else { "" },
            if arm.kv_q8 { " - compact cache" } else { "" },
            if arm.ubatch > 0 { " - bigger batches" } else { "" },
            if arm.cpu { " - on the processor" } else { "" }
        );
        let _ = app.emit("tune-run", serde_json::json!({ "model": model, "done": i, "total": total, "current": desc }));
        // A twin is any arm a micro-batch arm will be compared with.
        let twin = arm.ubatch == 0 && !arm.cpu && arms.iter().any(|b| b.ubatch > 0 && b.ctx == arm.ctx && b.moe_cpu_layers == arm.moe_cpu_layers && b.draft == arm.draft && b.kv_q8 == arm.kv_q8);
        let r = bench_one(&bin, &models_dir, &model, arm, draft_file.clone(), engine_threads(&app), &gpu_args, None, twin).await;
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

    fn r(ctx: u64, kv_q8: bool, gen: f32, failed: Option<&str>) -> TuneResult {
        TuneResult { ctx, moe_cpu_layers: None, draft: false, kv_q8, ubatch: 0, cpu: false, load_secs: 1.0, pp_tps: 50.0, gen_tps: gen, failed: failed.map(String::from), during_free: None }
    }

    #[test]
    fn kv_auto_is_standard_until_measured_clean() {
        assert!(!kv_choice_from_profile(None).q8);
        let p = TuneProfile { measured_at: 0, results: vec![r(8192, false, 20.0, None)] };
        assert!(!kv_choice_from_profile(Some(&p)).q8, "no compact arm");
        let p = TuneProfile { measured_at: 0, results: vec![r(8192, false, 20.0, None), r(8192, true, 0.0, Some("did not load"))] };
        assert!(!kv_choice_from_profile(Some(&p)).q8, "compact arm failed");
        let p = TuneProfile { measured_at: 0, results: vec![r(8192, false, 20.0, None), r(8192, true, 17.0, None)] };
        assert!(!kv_choice_from_profile(Some(&p)).q8, "compact arm 15% slower");
        let p = TuneProfile { measured_at: 0, results: vec![r(8192, false, 20.0, None), r(8192, true, 19.5, None)] };
        assert!(kv_choice_from_profile(Some(&p)).q8, "within 5%");
        let p = TuneProfile { measured_at: 0, results: vec![r(4096, false, 20.0, None), r(8192, true, 19.5, None)] };
        assert!(!kv_choice_from_profile(Some(&p)).q8, "no like-for-like twin");
    }

    #[test]
    fn measured_split_takes_a_faster_smaller_split_only() {
        let arm = |ctx: u64, n: u32, gen: f32| TuneResult { ctx, moe_cpu_layers: Some(n), draft: false, kv_q8: false, ubatch: 0, cpu: false, load_secs: 3.0, pp_tps: 300.0, gen_tps: gen, failed: None, during_free: None };
        let p = TuneProfile { measured_at: 0, results: vec![arm(16384, 16, 34.5), arm(16384, 13, 39.8), arm(8192, 15, 36.5)] };
        assert_eq!(measured_moe_split(Some(&p), 16384, 16), Some(13));
        assert_eq!(measured_moe_split(Some(&p), 16384, 13), None, "nothing smaller measured");
        assert_eq!(measured_moe_split(Some(&p), 32768, 17), None, "no arm at that context");
        let p2 = TuneProfile { measured_at: 0, results: vec![arm(16384, 35, 33.4), arm(16384, 30, 22.7)] };
        assert_eq!(measured_moe_split(Some(&p2), 16384, 35), None, "the smaller split was slower");
        let mut failed = arm(16384, 13, 45.0); failed.failed = Some("oom".into());
        let p3 = TuneProfile { measured_at: 0, results: vec![arm(16384, 16, 34.5), failed] };
        assert_eq!(measured_moe_split(Some(&p3), 16384, 16), None, "a failed arm never counts");
        assert_eq!(measured_moe_split(None, 16384, 16), None);
    }

    #[test]
    fn arms_include_one_compact_cache_twin() {
        let meta = crate::gguf::GgufMeta::default();
        let arms = arms_for(&meta, 3 * 1024 * 1024 * 1024, 16.0, Some(4.0), false, None);
        let compact: Vec<_> = arms.iter().filter(|a| a.kv_q8).collect();
        assert_eq!(compact.len(), 1);
        let c = compact[0];
        assert!(arms.iter().any(|a| !a.kv_q8 && a.ctx == c.ctx && a.moe_cpu_layers == c.moe_cpu_layers && a.draft == c.draft), "the twin exists");
    }

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
        let arms = arms_for(&meta, size, 31.0, free_vram_gb, false, None);
        assert!(!arms.is_empty());
        // The compact-cache arm and its standard twin: every matrix run
        // measures the pair, so the Auto rule's evidence is never stale.
        let compact = arms.iter().copied().find(|a| a.kv_q8).expect("a compact arm");
        let twin = arms
            .iter()
            .copied()
            .find(|a| !a.kv_q8 && a.ctx == compact.ctx && a.moe_cpu_layers == compact.moe_cpu_layers && a.draft == compact.draft)
            .expect("its standard twin");
        let mut results = Vec::new();
        for arm in [twin, compact] {
            let r = bench_one(&bin, &dir, &model, arm, None, None, &[], None, false).await;
            results.push(r.clone());
            eprintln!(
                "[matrix] tune arm ctx {} moe {:?} draft {} compact {}: load {:.1} s, prompt {:.0} tok/s, gen {:.1} tok/s, failed {:?}",
                r.ctx, r.moe_cpu_layers, r.draft, r.kv_q8, r.load_secs, r.pp_tps, r.gen_tps, r.failed
            );
            assert!(r.failed.is_none(), "arm failed: {:?}", r.failed);
            assert!(r.gen_tps > 0.0, "no generation timing");
        }
        let profile = TuneProfile { measured_at: 0, results };
        let choice = kv_choice_from_profile(Some(&profile));
        eprintln!("[matrix] compact cache verdict on this box: q8={} - {}", choice.q8, choice.reason);
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

    /// Headless matrix leg: CLAIM vs REALITY for every downloaded model.
    /// Prints the estimate's arithmetic (parsed header, chosen context,
    /// need, grade), then loads the model the app's way and records what
    /// actually happened: on-card memory delta, load time, measured
    /// speeds. The baseline table for the fit-truth fixes - run it before
    /// and after each one. `YOAI_FIT_TRUTH_MODELS=a.gguf,b.gguf` filters.
    #[tokio::test]
    #[ignore]
    async fn live_matrix_fit_truth() {
        let _one_at_a_time = crate::llm::LIVE_MATRIX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = std::env::var("HOME").unwrap_or_default();
        let dir = std::env::var("YOAI_MODELS_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::Path::new(&home).join(".local/share/com.solar.yourowai/models"));
        let triple = if cfg!(target_os = "windows") { "x86_64-pc-windows-msvc.exe" }
            else if cfg!(target_os = "macos") { if cfg!(target_arch = "aarch64") { "aarch64-apple-darwin" } else { "x86_64-apple-darwin" } }
            else { "x86_64-unknown-linux-gnu" };
        let bin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("bin").join(format!("llama-server-{triple}"));
        assert!(bin.exists());
        let only: Vec<String> = std::env::var("YOAI_FIT_TRUTH_MODELS").ok()
            .map(|v| v.split(',').map(|s| s.trim().to_string()).filter(|s| !s.is_empty()).collect())
            .unwrap_or_default();
        let sink = |l: String| eprintln!("[fit-truth] {l}");
        let failures = crate::matrix::leg_fit_truth(None, &bin, &dir, &only, &sink).await;
        assert!(failures.is_empty(), "grades lied green:\n{}", failures.join("\n"));
    }

    #[test]
    fn channel_markers_strip_and_detect() {
        use crate::llm::{contains_channel_marker, strip_channel_markers};
        assert!(contains_channel_marker("<|tool_call_start|>[code_blocks()]<|tool_call_end|>"));
        assert_eq!(strip_channel_markers("hi <|tool_call_start|>[x()]<|tool_call_end|> there"), "hi  there");
        assert_eq!(strip_channel_markers("lead <tool_call>{}"), "lead ");
        assert_eq!(strip_channel_markers("plain words"), "plain words");
        assert!(!contains_channel_marker("plain words"));
    }

    /// Headless matrix leg: every downloaded chat model answers every
    /// app-shaped, persona-carrying scenario with WORDS - non-empty, no
    /// channel markers of any format. The chat-format truth table.
    #[tokio::test]
    #[ignore]
    async fn live_matrix_chat_format() {
        let _one_at_a_time = crate::llm::LIVE_MATRIX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let home = std::env::var("HOME").unwrap_or_default();
        let dir = std::env::var("YOAI_MODELS_DIR")
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|_| std::path::Path::new(&home).join(".local/share/com.solar.yourowai/models"));
        let triple = if cfg!(target_os = "windows") { "x86_64-pc-windows-msvc.exe" }
            else if cfg!(target_os = "macos") { if cfg!(target_arch = "aarch64") { "aarch64-apple-darwin" } else { "x86_64-apple-darwin" } }
            else { "x86_64-unknown-linux-gnu" };
        let bin = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("bin").join(format!("llama-server-{triple}"));
        assert!(bin.exists());
        let sink = |l: String| eprintln!("[chat-format] {l}");
        let failures = crate::matrix::leg_chat_format(&bin, &dir, &sink).await;
        assert!(failures.is_empty(), "chat-format failures:\n{}", failures.join("\n"));
    }

    /// Headless matrix leg: the sampling knobs REACH the engine. One server,
    /// three completions through `apply_sampling`: temperature 0 twice must
    /// answer identically (greedy is deterministic); high temperature with
    /// two seeds must not both reproduce the greedy text.
    #[tokio::test]
    #[ignore]
    async fn live_matrix_sampling() {
        let _one_at_a_time = crate::llm::LIVE_MATRIX_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
        let sink = |l: String| eprintln!("[matrix] sampling {l}");
        crate::matrix::leg_sampling(&bin, &dir, &model, &sink)
            .await
            .expect("sampling leg");
    }

    #[test]
    fn a_bigger_batch_is_kept_only_when_it_reads_clearly_faster() {
        let r = |ubatch: u32, pp: f32, gen: f32| TuneResult { ctx: 32768, moe_cpu_layers: Some(15), draft: false, kv_q8: false, ubatch, cpu: false, load_secs: 3.0, pp_tps: pp, gen_tps: gen, failed: None, during_free: None };
        let p = |results: Vec<TuneResult>| TuneProfile { results, ..Default::default() };
        assert_eq!(ubatch_choice_from_profile(None), 0);
        // measured 501 -> 699 tok/s reading: kept
        assert_eq!(ubatch_choice_from_profile(Some(&p(vec![r(0, 501.0, 18.5), r(2048, 699.0, 16.3)]))), 2048);
        // 10% is not enough for ~200 MB of card
        assert_eq!(ubatch_choice_from_profile(Some(&p(vec![r(0, 500.0, 18.0), r(2048, 550.0, 18.0)]))), 0);
        // reads faster but writes much slower: not kept
        assert_eq!(ubatch_choice_from_profile(Some(&p(vec![r(0, 500.0, 18.0), r(2048, 700.0, 14.0)]))), 0);
        // no twin to compare with
        assert_eq!(ubatch_choice_from_profile(Some(&p(vec![r(2048, 700.0, 18.0)]))), 0);
        assert_eq!(ubatch_args(0).len(), 0);
        assert_eq!(ubatch_args(2048), vec!["-ub", "2048", "-b", "2048"]);
    }

    #[test]
    fn machine_arms_are_twins_of_the_automatic_rung() {
        let base = vec![
            TuneArm { ctx: 16384, moe_cpu_layers: None, draft: false, kv_q8: false, ubatch: 0, cpu: false },
            TuneArm { ctx: 32768, moe_cpu_layers: None, draft: false, kv_q8: false, ubatch: 0, cpu: false },
            TuneArm { ctx: 32768, moe_cpu_layers: None, draft: false, kv_q8: true, ubatch: 0, cpu: false },
        ];
        let m = machine_arms(&base, true, true, false);
        assert_eq!(m.len(), 1);
        assert_eq!((m[0].ctx, m[0].ubatch, m[0].cpu, m[0].kv_q8), (32768, 2048, false, false));
        let m = machine_arms(&base, false, true, true);
        assert_eq!(m.len(), 1);
        assert!(m[0].cpu && m[0].ubatch == 0 && m[0].ctx == 32768);
        assert!(machine_arms(&base, false, true, false).is_empty(), "CUDA with a card: nothing to add");
        assert!(machine_arms(&[], true, true, true).is_empty(), "no automatic rung to twin");
    }

    #[test]
    fn the_processor_is_chosen_only_when_it_wins_on_both_counts() {
        let r = |cpu: bool, pp: f32, gen: f32| TuneResult { ctx: 32768, moe_cpu_layers: None, draft: false, kv_q8: false, ubatch: 0, cpu, load_secs: 3.0, pp_tps: pp, gen_tps: gen, failed: None, during_free: None };
        let p = |results: Vec<TuneResult>| TuneProfile { results, ..Default::default() };
        assert!(!processor_choice_from_profile(None));
        // the UHD 630 case: 52 / 9.1 on the processor vs 29 / 5.6 on the iGPU
        assert!(processor_choice_from_profile(Some(&p(vec![r(false, 29.0, 5.6), r(true, 52.0, 9.1)]))));
        // reads faster, writes slower: the graphics device keeps the model
        assert!(!processor_choice_from_profile(Some(&p(vec![r(false, 29.0, 12.0), r(true, 52.0, 9.1)]))));
        // no twin
        assert!(!processor_choice_from_profile(Some(&p(vec![r(true, 52.0, 9.1)]))));
    }

    #[test]
    fn arms_cover_rungs_draft_and_leaner_split() {
        let meta = moe_meta();
        let arms = arms_for(&meta, 4_800_000_000, 31.0, Some(2.0), true, None);
        assert!(arms.len() <= 6, "small on purpose: {arms:?}");
        assert!(arms.iter().any(|a| !a.draft), "a draft-off arm exists");
        // experts in main memory: the bigger-batch arm exists, with a twin
        let big = arms.iter().find(|a| a.ubatch > 0).expect("a micro-batch arm");
        assert!(arms.iter().any(|a| a.ubatch == 0 && a.ctx == big.ctx && a.moe_cpu_layers == big.moe_cpu_layers && a.draft == big.draft && !a.kv_q8), "its twin");
        let ctxs: std::collections::HashSet<u64> = arms.iter().map(|a| a.ctx).collect();
        assert!(ctxs.len() >= 2, "more than one context rung: {ctxs:?}");
        assert!(arms.iter().all(|a| a.ctx <= 32768), "inside the trained limit");
    }

    #[test]
    fn arms_center_on_the_context_the_app_reports() {
        let mut meta = crate::gguf::GgufMeta::default();
        meta.context_length = 131072;
        // A 5 GB dense model on a 31 GB box with a small free-VRAM figure:
        // the planner alone would size the rung low, but the app says it
        // runs at 128K, so 128K is the automatic rung (with one below).
        let arms = arms_for(&meta, 5_000_000_000, 31.0, Some(2.0), false, Some(131072));
        let ctxs: std::collections::HashSet<u64> = arms.iter().map(|a| a.ctx).collect();
        assert!(ctxs.contains(&131072), "the reported context is measured: {ctxs:?}");
        assert!(ctxs.contains(&65536), "and the rung below: {ctxs:?}");
        assert!(arms.iter().any(|a| a.kv_q8 && a.ctx == 131072), "the compact twin sits at the automatic rung");
        // Past the trained limit or below the floor the figure is ignored.
        meta.context_length = 32768;
        let arms = arms_for(&meta, 5_000_000_000, 31.0, Some(2.0), false, Some(131072));
        assert!(arms.iter().all(|a| a.ctx <= 32768), "inside the trained limit: {arms:?}");
        let arms = arms_for(&meta, 5_000_000_000, 31.0, Some(2.0), false, Some(1024));
        assert!(arms.iter().all(|a| a.ctx >= 4096), "never below the floor: {arms:?}");
    }

    #[test]
    fn arms_for_dense_have_no_moe_field() {
        let meta = crate::gguf::GgufMeta { n_layers: 32, context_length: 16384, ..Default::default() };
        let arms = arms_for(&meta, 5_000_000_000, 31.0, Some(8.0), false, None);
        assert!(arms.iter().all(|a| a.moe_cpu_layers.is_none()));
        assert!(arms.iter().all(|a| !a.draft));
    }
}
