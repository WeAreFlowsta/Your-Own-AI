/**
 * LLM Module - llama.cpp Integration
 * 
 * This module uses llama.cpp Rust bindings to run local AI models.
 * No external binaries needed - everything is embedded in the app.
 * 
 * Features:
 * - Load and run GGUF models
 * - Model download from Hugging Face
 * - Model management (list, delete)
 * - Chat completion with streaming
 */

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::CommandChild;
use tokio::sync::Mutex;

/// Chat server port. The embedding (:8091, `EMBED_PORT`) and utility
/// (:8092, `UTIL_PORT`) servers have their own consts further down.
pub(crate) const CHAT_PORT: &str = "8080";

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct LocalModel {
    pub name: String,
    pub size: String,
    pub size_bytes: u64,
    pub parameter_size: String,
    pub quantization: String,
    pub modified_at: Option<String>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SystemInfo {
    pub total_memory_gb: f64,
    pub used_memory_gb: f64,
    pub cpu_count: usize,
    pub os_name: String,
    pub os_version: String,
    pub gpu_name: Option<String>,
    pub total_vram_gb: Option<f64>,
}

pub struct LLMState {
    pub models_dir: Mutex<Option<PathBuf>>,
    pub current_model: Mutex<Option<String>>, // Currently loaded model filename
    /// Multimodal projector (mmproj) filename the server was started with, if the
    /// current model had a paired projector downloaded — i.e. the server can see
    /// images this run. `None` = text-only.
    pub current_mmproj: Mutex<Option<String>>,
    pub server_process: Mutex<Option<CommandChild>>, // llama-server process
    pub is_server_running: Mutex<bool>,
    /// Which engine backend the RUNNING chat server was spawned with
    /// ("cuda" | "bundled") — the Engines card reports the truth of what's
    /// serving right now, not just what's installed.
    pub spawned_backend: Mutex<Option<String>>,
    /// Cancellation token for the active streaming request.
    /// Set to true to abort the current stream_chat_completion.
    pub cancel_stream: std::sync::atomic::AtomicBool,
    /// Second llama-server, run in `--embedding` mode on a separate port for
    /// memory retrieval. Same binary as the chat server (no extra download) —
    /// just a second process, started on demand. Runs CPU-only so it never
    /// contends with the chat model for VRAM.
    pub embed_process: Mutex<Option<CommandChild>>,
    pub embed_running: Mutex<bool>,
    pub embed_model: Mutex<Option<String>>, // embedding model filename it was started with
    /// Serializes embedding-server startup so overlapping embed calls during a
    /// cold start don't double-start and collide on the port.
    pub embed_startup: Mutex<()>,
    /// Third llama-server: a small GENERATIVE utility model on its own port, run
    /// CPU-only for on-device fact extraction + report/code classification. An
    /// OPTIONAL component (Settings) — when absent those features ride the
    /// chat/online model. Same bundled binary, separate process, never contends
    /// with the chat model for VRAM.
    pub util_process: Mutex<Option<CommandChild>>,
    pub util_running: Mutex<bool>,
    pub util_model: Mutex<Option<String>>, // utility model filename it was started with
    /// Serializes utility-server startup (same port-collision guard as embed).
    pub util_startup: Mutex<()>,
}

impl LLMState {
    pub fn new() -> Self {
        Self {
            models_dir: Mutex::new(None),
            current_model: Mutex::new(None),
            current_mmproj: Mutex::new(None),
            server_process: Mutex::new(None),
            is_server_running: Mutex::new(false),
            spawned_backend: Mutex::new(None),
            cancel_stream: std::sync::atomic::AtomicBool::new(false),
            embed_process: Mutex::new(None),
            embed_running: Mutex::new(false),
            embed_model: Mutex::new(None),
            embed_startup: Mutex::new(()),
            util_process: Mutex::new(None),
            util_running: Mutex::new(false),
            util_model: Mutex::new(None),
            util_startup: Mutex::new(()),
        }
    }
}

/**
 * Initialize models directory
 */
pub(crate) fn get_models_dir(app_handle: &AppHandle) -> Result<PathBuf, String> {
    let app_data_dir = app_handle
        .path()
        .app_data_dir()
        .map_err(|e| format!("Failed to get app data dir: {}", e))?;
    
    let models_dir = app_data_dir.join("models");
    
    // Create directory if it doesn't exist
    std::fs::create_dir_all(&models_dir)
        .map_err(|e| format!("Failed to create models directory: {}", e))?;
    
    Ok(models_dir)
}

/**
 * Check if llama-server is running
 */
#[tauri::command]
pub async fn is_llama_server_running() -> Result<bool, String> {
    match tokio::net::TcpStream::connect(format!("127.0.0.1:{}", CHAT_PORT)).await {
        Ok(_) => Ok(true),
        Err(_) => Ok(false)
    }
}

/**
 * Check if llama-server is ready (model fully loaded)
 * This checks the /health endpoint, not just the port
 */
#[tauri::command]
pub async fn is_llama_server_ready() -> Result<bool, String> {
    let client = reqwest::Client::new();
    match client.get(format!("http://localhost:{}/health", CHAT_PORT)).send().await {
        Ok(resp) if resp.status().is_success() => Ok(true),
        _ => Ok(false)
    }
}

/**
 * Force kill any process on port 8080
 * Useful for recovering from zombie processes after crashes/reboots
 */
#[tauri::command]
pub async fn kill_port_8080() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        use std::process::Command;
        
        // Try lsof first
        let lsof_result = Command::new("lsof")
            .args(&["-ti", &format!(":{}", CHAT_PORT)])
            .output();
        
        if let Ok(output) = lsof_result {
            if !output.stdout.is_empty() {
                let pid = String::from_utf8_lossy(&output.stdout).trim().to_string();
                
                // Kill the process
                let kill_result = Command::new("kill")
                    .args(&["-9", &pid])
                    .output();
                
                if kill_result.is_ok() {
                    return Ok(format!("Killed process {} on port 8080", pid));
                }
            }
        }
        
        // If lsof failed, try fuser as alternative
        let fuser_result = Command::new("fuser")
            .args(&["-k", "-9", &format!("{}/tcp", CHAT_PORT)])
            .output();
        
        if fuser_result.is_ok() {
            return Ok("Killed process on port 8080 using fuser".to_string());
        }
        
        // If both failed, port might be free already
        Ok("No process found on port 8080 (or unable to detect)".to_string())
    }
    
    #[cfg(not(target_os = "linux"))]
    {
        Ok("Port killing not implemented for this OS".to_string())
    }
}

/// A GPU as reported by `llama-server --list-devices`.
struct GpuDevice {
    id: String,        // e.g. "Vulkan1" / "CUDA0" — what the --device flag wants
    name: String,
    free_mib: u64,
    integrated: bool,  // uma: 1 (shares system RAM — iGPU / Apple unified)
}

/// Command for the CHAT-server binary, honoring the active engine backend:
/// the downloaded CUDA build when installed and not laddered out by GPU
/// safe mode, else the bundled sidecar. The embedding and utility servers
/// deliberately do NOT use this — they are CPU-only and always bundled.
fn chat_server_command(
    app_handle: &AppHandle,
) -> Result<tauri_plugin_shell::process::Command, String> {
    match crate::engine::active_backend(app_handle) {
        crate::engine::Backend::Cuda => {
            let bin = crate::engine::cuda_engine_binary(app_handle)
                .ok_or("CUDA engine binary missing")?;
            let dir = bin.parent().map(PathBuf::from).unwrap_or_default();
            // The CUDA runtime libraries sit beside the binary (exe-dir DLL
            // search on Windows, $ORIGIN rpath on Linux); current_dir is
            // belt-and-braces.
            Ok(app_handle.shell().command(bin).current_dir(dir))
        }
        crate::engine::Backend::Bundled => app_handle
            .shell()
            .sidecar("llama-server")
            .map_err(|e| format!("Failed to find llama-server binary: {}", e)),
    }
}

/// Heuristic: is this GPU integrated / shared-memory (iGPU, APU, or software
/// rasteriser)? Current llama.cpp `--list-devices` no longer prints the Vulkan
/// `uma:` flag, so we classify by name. Getting this right matters: pooling an
/// iGPU with a discrete GPU via `--tensor-split` runs slowly AND crashes the
/// older discrete card with a Vulkan `ErrorDeviceLost` mid-compute.
fn is_integrated_gpu(name: &str) -> bool {
    let n = name.to_lowercase();
    // Software rasterisers — never offload to these.
    if n.contains("llvmpipe") || n.contains("swiftshader") || n.contains("software") {
        return true;
    }
    // Intel integrated (UHD / Iris / HD Graphics). Intel Arc is discrete.
    if n.contains("intel") && !n.contains("arc") {
        return true;
    }
    // AMD APUs report "Radeon Graphics" with no model number; discrete cards are
    // "Radeon RX ..." / "Radeon Pro ...".
    if n.contains("radeon") && n.contains("graphics") && !n.contains(" rx") && !n.contains("pro") {
        return true;
    }
    // Apple Silicon unified memory.
    if n.contains("apple") {
        return true;
    }
    false
}

/// Parse `llama-server --list-devices` output. Lines look like:
///   `  Vulkan0: Intel(R) UHD Graphics 630 (CFL GT2) (11884 MiB, 8318 MiB free)`
///   `  CUDA0: NVIDIA GeForce RTX 4070 (12282 MiB, 11996 MiB free)`
/// (the prefix matches the backend the queried binary was built with). The
/// trailing `(NNNNN MiB, NNNN MiB free)` is the memory group; everything
/// before it (inner parens like `(R)` / `(CFL GT2)` included) is the name.
/// ⚠ Re-verify this parse on every engine bump — the format is not stable
/// (b9637 dropped the `uma:` flag this parser once used).
fn parse_gpu_devices(text: &str) -> Vec<GpuDevice> {
    let mut devices = Vec::new();
    for line in text.lines() {
        let l = line.trim();
        let (prefix, rest) = if let Some(r) = l.strip_prefix("Vulkan") {
            ("Vulkan", r)
        } else if let Some(r) = l.strip_prefix("CUDA") {
            ("CUDA", r)
        } else {
            continue;
        };
        let colon = match rest.find(':') {
            Some(c) => c,
            None => continue,
        };
        let index: u32 = match rest[..colon].trim().parse() {
            Ok(i) => i,
            Err(_) => continue,
        };
        let after = rest[colon + 1..].trim();
        let (name, mem) = match after.rfind('(') {
            Some(p) => (after[..p].trim().to_string(), &after[p..]),
            None => (after.to_string(), ""),
        };
        let free_mib = mem
            .find("MiB free")
            .and_then(|i| mem[..i].split_whitespace().last())
            .and_then(|n| n.parse::<u64>().ok())
            .unwrap_or(0);
        let integrated = is_integrated_gpu(&name);
        devices.push(GpuDevice {
            id: format!("{}{}", prefix, index),
            name,
            free_mib,
            integrated,
        });
    }
    devices
}

/// Decide which GPU(s) llama-server should offload to, returning the extra CLI
/// args to append. **Policy (Phase 2 device selection):**
/// - If one or more DISCRETE GPUs exist, use ONLY those — exclude integrated
///   iGPUs. (llama.cpp's default `--split-mode layer` otherwise spreads the
///   model across every Vulkan device, including a slow iGPU that often reports
///   *more* memory, which is a net performance loss.) With 2+ discrete GPUs we
///   add `--tensor-split` weighted by free VRAM to pool them.
/// - Otherwise — integrated-only, a single Metal GPU on macOS, or no GPU — we
///   return no args and let llama.cpp's default behaviour stand.
async fn select_gpu_device_args(app_handle: &AppHandle) -> Vec<String> {
    // Escape hatch: force CPU-only inference with FLOWSTA_CPU_ONLY=1. Some
    // setups (notably NVIDIA + Wayland + Vulkan compute) hard-hang the whole
    // system under GPU load; `-ngl 0` keeps every layer on the CPU. Slower, but
    // it takes the GPU out of the loop entirely (no Vulkan enumeration either).
    if std::env::var("FLOWSTA_CPU_ONLY").map(|v| v != "0").unwrap_or(false) {
        log::info!("[LLM] FLOWSTA_CPU_ONLY set — forcing CPU inference (-ngl 0)");
        return vec!["-ngl".to_string(), "0".to_string()];
    }

    // GPU crash-loop safety net: if recent GPU runs hard-crashed the system
    // (e.g. an NVIDIA+Wayland Vulkan hang), fall back to CPU automatically.
    if !crate::gpu_safety::gpu_allowed(app_handle) {
        log::warn!("[LLM] GPU safe mode active — running on CPU (-ngl 0)");
        return vec!["-ngl".to_string(), "0".to_string()];
    }

    let cmd = match chat_server_command(app_handle) {
        Ok(c) => c.args(["--list-devices"]),
        Err(_) => return Vec::new(),
    };
    let output = match cmd.output().await {
        Ok(o) => o,
        Err(_) => return Vec::new(),
    };
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));

    let devices = parse_gpu_devices(&combined);
    let discrete: Vec<&GpuDevice> = devices.iter().filter(|d| !d.integrated).collect();
    if discrete.is_empty() {
        return Vec::new(); // iGPU-only / Metal / CPU — leave the default alone
    }

    let device_list = discrete
        .iter()
        .map(|d| d.id.as_str())
        .collect::<Vec<_>>()
        .join(",");
    let mut args = vec!["--device".to_string(), device_list];
    if discrete.len() >= 2 {
        // Pool multiple discrete GPUs, weighted by free VRAM.
        let split = discrete
            .iter()
            .map(|d| d.free_mib.to_string())
            .collect::<Vec<_>>()
            .join(",");
        args.push("--tensor-split".to_string());
        args.push(split);
    }
    let names = discrete.iter().map(|d| d.name.as_str()).collect::<Vec<_>>().join(", ");
    log::info!("[LLM] GPU selection — using discrete only: {} ({:?})", names, args);
    args
}

/// Free VRAM (MiB) on the discrete GPU(s) the chat server uses, from
/// `--list-devices` — this reflects what's ACTUALLY free after the webview etc.
/// (unlike total heap size, which over-counts on small cards). `None` in CPU
/// mode → caller falls back to system RAM. Cached ~20s so the router can call it
/// per request without re-spawning the probe.
pub async fn available_vram_mib(app_handle: &AppHandle) -> Option<u64> {
    use std::time::{Duration, Instant};
    static CACHE: std::sync::OnceLock<tokio::sync::Mutex<Option<(Instant, Option<u64>)>>> =
        std::sync::OnceLock::new();
    let cache = CACHE.get_or_init(|| tokio::sync::Mutex::new(None));
    if let Some((t, v)) = cache.lock().await.as_ref() {
        if t.elapsed() < Duration::from_secs(20) {
            return *v;
        }
    }
    let fresh = compute_available_vram_mib(app_handle).await;
    *cache.lock().await = Some((Instant::now(), fresh));
    fresh
}

async fn compute_available_vram_mib(app_handle: &AppHandle) -> Option<u64> {
    if std::env::var("FLOWSTA_CPU_ONLY").map(|v| v != "0").unwrap_or(false) {
        return None;
    }
    if !crate::gpu_safety::gpu_allowed(app_handle) {
        return None;
    }
    let cmd = chat_server_command(app_handle).ok()?.args(["--list-devices"]);
    let output = cmd.output().await.ok()?;
    let mut combined = String::from_utf8_lossy(&output.stdout).into_owned();
    combined.push_str(&String::from_utf8_lossy(&output.stderr));
    let discrete: Vec<GpuDevice> = parse_gpu_devices(&combined)
        .into_iter()
        .filter(|d| !d.integrated)
        .collect();
    if discrete.is_empty() {
        return None; // iGPU-only / Metal / CPU → use system RAM instead
    }
    Some(discrete.iter().map(|d| d.free_mib).sum())
}

/// Find the multimodal projector (mmproj) paired with a chat model, if one is
/// downloaded. A projector pairs with a model family+variant (e.g. `gemma-4-E2B`),
/// not a specific quant — so we match by the projector's key (its name before
/// `-mmproj`) being a prefix of the model filename. That's variant-safe: an E2B
/// projector never pairs with an E4B model. Returns the projector path, or None
/// (the model then runs text-only).
pub(crate) fn find_projector_for(
    models_dir: &std::path::Path,
    model_filename: &str,
) -> Option<PathBuf> {
    let model_lc = model_filename.to_lowercase();
    for entry in std::fs::read_dir(models_dir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("gguf") {
            continue;
        }
        let Some(name_lc) = path
            .file_name()
            .and_then(|s| s.to_str())
            .map(|s| s.to_lowercase())
        else {
            continue;
        };
        if !name_lc.contains("mmproj") {
            continue;
        }
        // Key = projector name before "-mmproj" (e.g. "gemma-4-e2b"). Must prefix
        // the model filename for a valid pairing.
        if let Some(key) = name_lc.split("-mmproj").next() {
            if !key.is_empty() && model_lc.starts_with(key) {
                return Some(path);
            }
        }
    }
    None
}

/// Whether the chat server currently has a multimodal projector loaded — i.e. it
/// can accept images this run. The frontend checks this before sending an image
/// turn; if false, it reloads the model so the projector gets paired.
#[tauri::command]
pub async fn is_vision_ready(state: State<'_, LLMState>) -> Result<bool, String> {
    Ok(state.current_mmproj.lock().await.is_some())
}

/// Find a downloaded chat model that is vision-ready — i.e. it has its mmproj
/// projector downloaded too. Used to transparently route an image turn to a vision
/// model when the AI is in an Auto mode. Returns the model filename, or None.
#[tauri::command]
pub async fn find_vision_model(
    app_handle: AppHandle,
    query: Option<String>,
    query_vec: Option<Vec<f32>>,
) -> Result<Option<VisionPick>, String> {
    let models_dir = get_models_dir(&app_handle)?;
    // Every downloaded model with a paired projector is a candidate (skip any
    // already proven too large this session).
    let mut candidates: Vec<String> = Vec::new();
    for entry in std::fs::read_dir(&models_dir)
        .map_err(|e| e.to_string())?
        .flatten()
    {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("gguf") {
            continue;
        }
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if name.to_lowercase().contains("mmproj") {
            continue; // it's a projector, not a chat model
        }
        if is_model_too_big(name.to_string()) {
            continue;
        }
        if find_projector_for(&models_dir, name).is_some() {
            candidates.push(name.to_string());
        }
    }
    if candidates.is_empty() {
        return Ok(None);
    }

    // Health images (X-rays, skin photos, scans) prefer the medical specialist
    // - the same keep-local medical gate the router uses, on the same shared
    // turn embedding.
    let medical = match query.as_deref() {
        Some(q) if !q.trim().is_empty() => {
            crate::router::is_medical_turn(&app_handle, q, query_vec.as_deref()).await
        }
        _ => false,
    };

    // Rank: medical turns by medical capability, others by vision capability
    // (overall as the tiebreak in both cases).
    let axis = |name: &str| -> (u8, u8) {
        let c = crate::model_caps::caps_for(name);
        if medical {
            (c.medical, c.overall)
        } else {
            (c.vision, c.overall)
        }
    };
    let best = candidates
        .iter()
        .max_by_key(|n| axis(n))
        .cloned()
        .expect("non-empty");

    // Stickiness: keep the LOADED model unless the best beats it on the
    // ranking axis by the router's switch margin - same reload discipline as
    // text routing (an X-ray justifies swapping to MedGemma; a meme doesn't
    // justify swapping between two equal vision models).
    let current = app_handle
        .state::<LLMState>()
        .current_model
        .lock()
        .await
        .clone();
    let pick = match current.filter(|c| candidates.contains(c)) {
        Some(cur) if axis(&cur).0 + crate::router::SWITCH_MARGIN > axis(&best).0 => cur,
        _ => best,
    };

    let reason = if medical {
        if pick.to_lowercase().contains("medgemma") {
            "a health image — kept on your device, using your medical model"
        } else {
            "a health image — kept on your device"
        }
    } else {
        "an image — using your vision model"
    };
    Ok(Some(VisionPick {
        model: pick,
        reason: reason.to_string(),
    }))
}

/// The vision model chosen for an image turn, with the human reason the
/// routing receipt shows.
#[derive(serde::Serialize)]
pub struct VisionPick {
    pub model: String,
    pub reason: String,
}

/**
 * Start bundled llama-server
 */
/// Set true when the chat server (`:8080`) exits during a load — i.e. it failed to
/// fit (out of GPU memory) or otherwise died. `start_llama_server` watches it to
/// turn a dead server into a clear "too large" error instead of a silent hang.
static CHAT_LOAD_FAILED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Whether the dying server's stderr showed MEMORY exhaustion. Distinguishes a
/// genuine too-large model from an engine crash (e.g. an access violation) -
/// beta 1 reported a crashing engine as "model too large", which sent the user
/// hunting for smaller models that would never help.
static CHAT_LOAD_OOM: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Memory-exhaustion markers in llama.cpp / ggml / driver output.
fn looks_like_oom(line: &str) -> bool {
    let l = line.to_ascii_lowercase();
    l.contains("out of memory")
        || l.contains("erroroutofdevicememory")
        || l.contains("erroroutofhostmemory")
        || l.contains("failed to allocate")
        || l.contains("cannot allocate")
        || l.contains("insufficient memory")
        || l.contains("not enough memory")
        || l.contains("oom")
}

async fn chat_server_health_ok() -> bool {
    let client = reqwest::Client::new();
    matches!(
        client.get(format!("http://127.0.0.1:{}/health", CHAT_PORT)).send().await,
        Ok(r) if r.status().is_success()
    )
}

/// Models that OOM'd the GPU this session. Once a model proves it won't fit, we
/// reject re-loading it instantly instead of burning ~30s for the same result.
/// Session-scoped on purpose: clears on restart, so freeing VRAM gives it another go.
fn too_big_set() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static S: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    S.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// True if `filename` already OOM'd the GPU this session (see `too_big_set`).
#[tauri::command]
pub fn is_model_too_big(filename: String) -> bool {
    too_big_set()
        .lock()
        .map(|s| s.contains(&filename))
        .unwrap_or(false)
}

#[tauri::command]
pub async fn start_llama_server(
    app_handle: AppHandle,
    state: State<'_, LLMState>,
    model_filename: Option<String>,
    with_vision: bool,
) -> Result<(), String> {
    let mut is_running = state.is_server_running.lock().await;
    
    if *is_running {
        println!("[LLM] Server already running");
        return Ok(());
    }
    
    // Check if already running externally (could be zombie process)
    if is_llama_server_running().await? {
        println!("[LLM] Port 8080 is in use - attempting to kill zombie process");
        // Try to kill any zombie process on port 8080
        let kill_result = kill_port_8080().await;
        println!("[LLM] Kill attempt result: {:?}", kill_result);
        
        // Wait a moment for port to be freed
        tokio::time::sleep(tokio::time::Duration::from_millis(1000)).await;
        
        // Check again after kill attempt
        if is_llama_server_running().await? {
            println!("[LLM] Port 8080 still in use after kill attempt - assuming it's our server");
            *is_running = true;
            return Ok(());
        } else {
            println!("[LLM] Port 8080 is now free, proceeding to start server");
        }
    }
    
    let models_dir = get_models_dir(&app_handle)?;
    
    // Determine safe context size based on available system RAM
    let sys = sysinfo::System::new_with_specifics(
        sysinfo::RefreshKind::nothing().with_memory(sysinfo::MemoryRefreshKind::everything()),
    );
    let total_ram_gb = sys.total_memory() as f64 / (1024.0 * 1024.0 * 1024.0);
    let ctx_size = if total_ram_gb >= 32.0 {
        "16384"  // 32GB+ — large context
    } else if total_ram_gb >= 12.0 {
        "8192"   // 12-32GB — moderate context
    } else {
        "4096"   // <12GB — minimum usable context
    };
    println!("[LLM] System RAM: {:.1}GB, using context size: {}", total_ram_gb, ctx_size);

    // Build args
    // --reasoning off: disables native thinking by default for all models.
    // Thinking models (Qwen 3.5, etc.) respond directly without internal reasoning.
    // For report mode, thinking is enabled per-request via chat_template_kwargs.
    // Non-thinking models (Phi-4, Gemma, etc.) ignore this flag entirely.
    let mut args = vec![
        "--port".to_string(),
        CHAT_PORT.to_string(),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "--no-webui".to_string(),
        "--reasoning".to_string(),
        "off".to_string(),
        "--ctx-size".to_string(),
        ctx_size.to_string(),
        // Disable the auto-fit-to-device-memory feature (new in recent llama.cpp).
        // When a model has to be split hard between a small GPU and CPU — common
        // when the GPU has limited free VRAM (e.g. the webview is using some) —
        // auto-fit can blow past GGML_SCHED_MAX_SPLIT_INPUTS and crash the server
        // (GGML_ASSERT in ggml-backend.cpp). `--fit off` uses the classic offload
        // path (what the previous engine did), which loads reliably.
        "--fit".to_string(),
        "off".to_string(),
    ];
    
    // If a model is specified, load it on startup
    *state.current_mmproj.lock().await = None;
    let model_specified = model_filename.is_some();
    let loading_name = model_filename.clone(); // remembered for the too-large cache
    let _ = with_vision; // pairing no longer depends on the turn (see below)
    if let Some(filename) = model_filename {
        let model_path = models_dir.join(&filename);
        if model_path.exists() {
            args.push("--model".to_string());
            args.push(model_path.to_string_lossy().to_string());
            log::info!("[LLM] Starting server with model: {}", filename);

            // Pair the multimodal projector whenever this model HAS one, so a
            // vision-capable model loads with its eyes ONCE and never needs a
            // mid-conversation reload to answer an image. The vision compute buffer is
            // small and idle on text turns; a model too tight to hold it just reports
            // "too large" like any other over-budget load.
            if let Some(projector) = find_projector_for(&models_dir, &filename) {
                let pname = projector
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                args.push("--mmproj".to_string());
                args.push(projector.to_string_lossy().to_string());
                args.push("--no-mmproj-offload".to_string()); // vision tower on CPU
                log::info!("[LLM] Vision: paired projector {} for {}", pname, filename);
                *state.current_mmproj.lock().await = Some(pname);
            }
        } else {
            println!("[LLM] Model not found, starting server without model");
        }
    }

    // Offload to the GPU whenever one is present. A model that doesn't fit will FAIL
    // to load (surfaced as "too large for your graphics card" by the load-failure
    // path) rather than crawling on the CPU — a slow CPU fallback was worse than an
    // honest stop, and small-GPU is the case we optimise for. A machine with no
    // discrete GPU gets `-ngl 0` from here and runs on the CPU (its only path).
    args.extend(select_gpu_device_args(&app_handle).await);

    // Start the chat server on the active engine backend (downloaded CUDA
    // build when installed, else the bundled sidecar). Clear the death-flag
    // first so the wait below reads THIS load's outcome (ready vs
    // out-of-memory), not a prior one's. Record the backend for the GPU
    // safety ladder — a crash steps down the right rung next launch.
    let backend = crate::engine::active_backend(&app_handle);
    crate::gpu_safety::note_backend(
        &app_handle,
        match backend {
            crate::engine::Backend::Cuda => "cuda",
            crate::engine::Backend::Bundled => "bundled",
        },
    );
    CHAT_LOAD_FAILED.store(false, std::sync::atomic::Ordering::SeqCst);
    CHAT_LOAD_OOM.store(false, std::sync::atomic::Ordering::SeqCst);
    let (mut rx, child) = chat_server_command(&app_handle)?
        .args(&args)
        .spawn()
        .map_err(|e| format!("Failed to start llama-server: {}", e))?;

    *state.server_process.lock().await = Some(child);
    *is_running = true;
    *state.spawned_backend.lock().await = Some(
        match backend {
            crate::engine::Backend::Cuda => "cuda",
            crate::engine::Backend::Bundled => "bundled",
        }
        .to_string(),
    );

    println!("[LLM] llama-server started on port {} ({:?} engine)", CHAT_PORT, backend);
    
    // Spawn a task to monitor server output
    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                tauri_plugin_shell::process::CommandEvent::Stdout(line) => {
                    log::info!("[llama-server] {}", String::from_utf8_lossy(&line).trim_end());
                }
                tauri_plugin_shell::process::CommandEvent::Stderr(line) => {
                    // llama.cpp logs most of its useful startup output (incl. the
                    // ggml_vulkan device lines + model load) to stderr.
                    let text = String::from_utf8_lossy(&line);
                    log::info!("[llama-server] {}", text.trim_end());
                    if looks_like_oom(&text) {
                        CHAT_LOAD_OOM.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                }
                tauri_plugin_shell::process::CommandEvent::Error(err) => {
                    log::error!("[llama-server error] {}", err);
                }
                tauri_plugin_shell::process::CommandEvent::Terminated(payload) => {
                    log::warn!("[llama-server] Process terminated with code: {:?}", payload.code);
                    // A NON-ZERO exit means the server died on its own — almost always
                    // out of GPU memory loading a too-large model. A `None` code means
                    // WE killed it (a model swap / shutdown), which is not a failure;
                    // flagging it would make a superseded load wrongly report "too large".
                    if matches!(payload.code, Some(code) if code != 0) {
                        CHAT_LOAD_FAILED.store(true, std::sync::atomic::Ordering::SeqCst);
                    }
                    break;
                }
                _ => {}
            }
        }
    });

    // Don't hold the running-state lock across the wait below (frontend readiness
    // polls need it).
    drop(is_running);

    // If we asked for a model, wait until it's actually serving — or until the server
    // dies trying (out of GPU memory on a too-large model). Surface that as a clear,
    // recognisable error rather than leaving a dead server for the next request to
    // hit. ~60s ceiling covers a slow but legitimate load.
    if model_specified {
        use std::sync::atomic::Ordering;
        for _ in 0..120 {
            if chat_server_health_ok().await {
                return Ok(());
            }
            if CHAT_LOAD_FAILED.load(Ordering::SeqCst) {
                // Remember the failure so we don't burn ~30s loading it again
                // this session — the next attempt rejects instantly. Clears on
                // restart (in case the user frees VRAM).
                if let Some(ref name) = loading_name {
                    if let Ok(mut set) = too_big_set().lock() {
                        set.insert(name.clone());
                    }
                }
                // Only claim "too large" when the engine's own output showed
                // memory exhaustion; a silent death is a crash, and telling the
                // user to pick a smaller model would send them down a dead end.
                if CHAT_LOAD_OOM.load(Ordering::SeqCst) {
                    return Err("MODEL_TOO_LARGE".to_string());
                }
                return Err("MODEL_LOAD_CRASHED".to_string());
            }
            tokio::time::sleep(std::time::Duration::from_millis(500)).await;
        }
        return Err("MODEL_LOAD_TIMEOUT".to_string());
    }

    Ok(())
}

/**
 * Stop llama-server
 */
#[tauri::command]
pub async fn stop_llama_server(
    state: State<'_, LLMState>,
) -> Result<(), String> {
    let mut server_process = state.server_process.lock().await;
    let mut is_running = state.is_server_running.lock().await;
    
    if let Some(child) = server_process.take() {
        println!("[LLM] Stopping server...");
        child.kill().map_err(|e| format!("Failed to stop server: {}", e))?;
        *is_running = false;
        println!("[LLM] Server stopped");
    }

    Ok(())
}

// ── Embedding server (second llama-server, for memory retrieval) ────────────
// A separate llama-server process in `--embedding` mode on its own port. Same
// bundled binary as the chat server (no extra download), started on demand and
// CPU-only so it never competes with the chat model for GPU memory.

const EMBED_PORT: &str = "8091";

#[derive(Deserialize)]
struct EmbeddingResponse {
    data: Vec<EmbeddingDatum>,
}

#[derive(Deserialize)]
struct EmbeddingDatum {
    embedding: Vec<f32>,
    index: usize,
}

/// Best-effort: kill whatever process holds `port`. Used to clear an embedding
/// server orphaned by a dev hot-reload (cargo hard-kills the app, skipping the
/// clean-exit handler, so the old server keeps holding 8091 and the new one
/// can't bind it).
fn free_port(port: &str) {
    #[cfg(unix)]
    {
        use std::process::Command;
        if let Ok(out) = Command::new("lsof")
            .args(["-ti", &format!(":{}", port)])
            .output()
        {
            for pid in String::from_utf8_lossy(&out.stdout).split_whitespace() {
                let _ = Command::new("kill").args(["-9", pid]).output();
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        use std::process::Command;
        if let Ok(out) = Command::new("netstat").args(["-ano"]).output() {
            let s = String::from_utf8_lossy(&out.stdout);
            for line in s.lines().filter(|l| l.contains(&format!(":{}", port))) {
                if let Some(pid) = line.split_whitespace().last() {
                    let _ = Command::new("taskkill").args(["/F", "/PID", pid]).output();
                }
            }
        }
    }
}

async fn embed_server_ready() -> bool {
    let client = reqwest::Client::new();
    matches!(
        client.get(format!("http://localhost:{}/health", EMBED_PORT)).send().await,
        Ok(r) if r.status().is_success()
    )
}

async fn stop_embedding_server_inner(state: &State<'_, LLMState>) {
    if let Some(child) = state.embed_process.lock().await.take() {
        let _ = child.kill();
    }
    *state.embed_running.lock().await = false;
    *state.embed_model.lock().await = None;
}

/// Stop the embedding server (frees its RAM). Safe to call when not running.
#[tauri::command]
pub async fn stop_embedding_server(state: State<'_, LLMState>) -> Result<(), String> {
    stop_embedding_server_inner(&state).await;
    Ok(())
}

/// Ensure the embedding server is up and serving `model_filename`. Lazily
/// (re)starts it if it's down or running a different model, then waits for the
/// model to finish loading.
async fn ensure_embedding_server(
    app_handle: &AppHandle,
    state: &State<'_, LLMState>,
    model_filename: &str,
) -> Result<(), String> {
    // Serialize startup: overlapping embed calls (e.g. recall + indexing) must
    // not both try to start the server — that's what collided on the port.
    let _startup = state.embed_startup.lock().await;

    {
        let running = *state.embed_running.lock().await;
        let same_model = state.embed_model.lock().await.as_deref() == Some(model_filename);
        if running && same_model {
            // Already serving (or still loading) this model. If ready, done; if
            // still warming up, WAIT for it rather than restarting — a restart
            // races the old process for the port. Only fall through to a real
            // restart if it never becomes ready (genuinely stuck).
            for _ in 0..60 {
                if embed_server_ready().await {
                    return Ok(());
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        }
    }

    // Real (re)start: stop our tracked process, then FORCE-free the port in
    // case an embed server was orphaned (a dev hot-reload kills the app without
    // running the clean-exit handler, leaving the old one holding the port).
    // Then wait for the port to actually release before rebinding.
    stop_embedding_server_inner(state).await;
    free_port(EMBED_PORT);
    for _ in 0..20 {
        if tokio::net::TcpStream::connect(format!("127.0.0.1:{}", EMBED_PORT))
            .await
            .is_err()
        {
            break; // port released
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
    }

    let models_dir = get_models_dir(app_handle)?;
    let model_path = models_dir.join(model_filename);
    if !model_path.exists() {
        return Err(format!("Embedding model not downloaded: {}", model_filename));
    }

    // bge-small-en-v1.5: CLS pooling, 512-token context. CPU-only (-ngl 0) to
    // spare the chat GPU. NB: pooling + ctx are model-specific — swapping the
    // embedding model (see EMBEDDING_MODEL) may require changing these.
    let args = vec![
        "--model".to_string(),
        model_path.to_string_lossy().to_string(),
        "--embedding".to_string(),
        "--pooling".to_string(),
        "cls".to_string(),
        "--port".to_string(),
        EMBED_PORT.to_string(),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "--ctx-size".to_string(),
        "512".to_string(),
        "--no-webui".to_string(),
        "-ngl".to_string(),
        "0".to_string(),
    ];

    let (mut rx, child) = app_handle
        .shell()
        .sidecar("llama-server")
        .map_err(|e| format!("Failed to find llama-server binary: {}", e))?
        .args(&args)
        .spawn()
        .map_err(|e| format!("Failed to start embedding server: {}", e))?;

    *state.embed_process.lock().await = Some(child);
    *state.embed_running.lock().await = true;
    *state.embed_model.lock().await = Some(model_filename.to_string());
    println!("[LLM] embedding server started on port {}", EMBED_PORT);

    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                tauri_plugin_shell::process::CommandEvent::Stderr(line) => {
                    log::info!("[embed-server] {}", String::from_utf8_lossy(&line).trim_end());
                }
                tauri_plugin_shell::process::CommandEvent::Terminated(payload) => {
                    log::warn!("[embed-server] terminated: {:?}", payload.code);
                    break;
                }
                _ => {}
            }
        }
    });

    // Wait for the model to load (CPU load of a small embedder is a few seconds).
    for _ in 0..60 {
        if embed_server_ready().await {
            return Ok(());
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
    Err("Embedding server did not become ready in time".to_string())
}

/// Embed a batch of texts → one vector each (same order as input). Lazily
/// starts the embedding server using `model` (the embedding GGUF filename).
#[tauri::command]
pub async fn embed_texts(
    app_handle: AppHandle,
    state: State<'_, LLMState>,
    texts: Vec<String>,
    model: String,
) -> Result<Vec<Vec<f32>>, String> {
    if texts.is_empty() {
        return Ok(vec![]);
    }
    ensure_embedding_server(&app_handle, &state, &model).await?;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://localhost:{}/v1/embeddings", EMBED_PORT))
        .json(&serde_json::json!({ "model": model, "input": texts }))
        .send()
        .await
        .map_err(|e| format!("Embedding request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Embedding server returned {}", resp.status()));
    }

    let mut parsed: EmbeddingResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse embedding response: {}", e))?;
    parsed.data.sort_by_key(|d| d.index);
    Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
}

// ── Utility-model server ────────────────────────────────────────────────────
// A third llama-server running a small GENERATIVE model on its own port, for
// on-device fact extraction + report/code classification. Same bundled binary
// as the chat server (no extra download), started on demand and CPU-only so it
// never competes with the chat model for GPU memory. Optional: when this model
// isn't installed, the caller rides the chat/online model instead.

const UTIL_PORT: &str = "8092";

async fn util_server_ready() -> bool {
    let client = reqwest::Client::new();
    matches!(
        client.get(format!("http://localhost:{}/health", UTIL_PORT)).send().await,
        Ok(r) if r.status().is_success()
    )
}

async fn stop_utility_server_inner(state: &State<'_, LLMState>) {
    if let Some(child) = state.util_process.lock().await.take() {
        let _ = child.kill();
    }
    *state.util_running.lock().await = false;
    *state.util_model.lock().await = None;
}

/// Stop the utility server (frees its RAM). Safe to call when not running.
#[tauri::command]
pub async fn stop_utility_server(state: State<'_, LLMState>) -> Result<(), String> {
    stop_utility_server_inner(&state).await;
    Ok(())
}

/// Ensure the utility server is up and serving `model_filename`. Lazily
/// (re)starts it if it's down or running a different model, then waits for the
/// model to finish loading. Same startup serialization + orphaned-port guard as
/// the embedding server.
async fn ensure_utility_server(
    app_handle: &AppHandle,
    state: &State<'_, LLMState>,
    model_filename: &str,
) -> Result<(), String> {
    let _startup = state.util_startup.lock().await;

    {
        let running = *state.util_running.lock().await;
        let same_model = state.util_model.lock().await.as_deref() == Some(model_filename);
        if running && same_model {
            for _ in 0..120 {
                if util_server_ready().await {
                    return Ok(());
                }
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
            }
        }
    }

    stop_utility_server_inner(state).await;
    free_port(UTIL_PORT);
    for _ in 0..20 {
        if tokio::net::TcpStream::connect(format!("127.0.0.1:{}", UTIL_PORT))
            .await
            .is_err()
        {
            break;
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(250)).await;
    }

    let models_dir = get_models_dir(app_handle)?;
    let model_path = models_dir.join(model_filename);
    if !model_path.exists() {
        return Err(format!("Utility model not downloaded: {}", model_filename));
    }

    // Small generative model for extraction + classification. CPU-only (-ngl 0)
    // to spare the chat GPU; reasoning off (these tasks don't reason); modest
    // context fits the extraction prompt + one turn.
    let args = vec![
        "--model".to_string(),
        model_path.to_string_lossy().to_string(),
        "--port".to_string(),
        UTIL_PORT.to_string(),
        "--host".to_string(),
        "127.0.0.1".to_string(),
        "--ctx-size".to_string(),
        "4096".to_string(),
        "--reasoning".to_string(),
        "off".to_string(),
        "--no-webui".to_string(),
        "-ngl".to_string(),
        "0".to_string(),
    ];

    let (mut rx, child) = app_handle
        .shell()
        .sidecar("llama-server")
        .map_err(|e| format!("Failed to find llama-server binary: {}", e))?
        .args(&args)
        .spawn()
        .map_err(|e| format!("Failed to start utility server: {}", e))?;

    *state.util_process.lock().await = Some(child);
    *state.util_running.lock().await = true;
    *state.util_model.lock().await = Some(model_filename.to_string());
    println!("[LLM] utility server started on port {}", UTIL_PORT);

    tauri::async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            match event {
                tauri_plugin_shell::process::CommandEvent::Stderr(line) => {
                    log::info!("[util-server] {}", String::from_utf8_lossy(&line).trim_end());
                }
                tauri_plugin_shell::process::CommandEvent::Terminated(payload) => {
                    log::warn!("[util-server] terminated: {:?}", payload.code);
                    break;
                }
                _ => {}
            }
        }
    });

    // CPU load of a ~2 GB model is a few seconds; allow generous headroom.
    for _ in 0..120 {
        if util_server_ready().await {
            return Ok(());
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }
    Err("Utility server did not become ready in time".to_string())
}

/// One-shot generation on the utility server → the full text (non-streamed).
/// Lazily starts the server with `model` (the utility GGUF filename). An optional
/// GBNF `grammar` constrains the output (extraction JSON / classifier one-word).
#[tauri::command]
pub async fn utility_chat(
    app_handle: AppHandle,
    state: State<'_, LLMState>,
    model: String,
    system: String,
    user: String,
    grammar: Option<String>,
    max_tokens: u32,
) -> Result<String, String> {
    ensure_utility_server(&app_handle, &state, &model).await?;

    let mut body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ],
        "max_tokens": max_tokens,
        "temperature": 0.7,
        "stream": false,
        "cache_prompt": true
    });
    if let Some(g) = grammar {
        body["grammar"] = serde_json::Value::String(g);
    }

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("http://localhost:{}/v1/chat/completions", UTIL_PORT))
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Utility request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Utility server returned {}", resp.status()));
    }

    let parsed: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("Failed to parse utility response: {}", e))?;
    Ok(parsed["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("")
        .to_string())
}

/// The Apple Silicon chip name ("Apple M2"), for the system-info GPU field.
#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
fn mac_chip_name() -> String {
    std::process::Command::new("sysctl")
        .args(["-n", "machdep.cpu.brand_string"])
        .output()
        .ok()
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
        .unwrap_or_else(|| "Apple Silicon".to_string())
}

/**
 * Get GPU information using Vulkan
 */
#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
fn get_gpu_info() -> (Option<String>, Option<f64>) {
    use ash::{vk, Entry};
    use std::ffi::CStr;

    // Try to initialize Vulkan
    let entry = match unsafe { Entry::load() } {
        Ok(entry) => entry,
        Err(_) => return (None, None),
    };

    // Create Vulkan instance
    let app_info = vk::ApplicationInfo::default()
        .api_version(vk::make_api_version(0, 1, 0, 0));

    let create_info = vk::InstanceCreateInfo::default()
        .application_info(&app_info);

    let instance = match unsafe { entry.create_instance(&create_info, None) } {
        Ok(instance) => instance,
        Err(_) => return (None, None),
    };

    // Enumerate physical devices
    let physical_devices = match unsafe { instance.enumerate_physical_devices() } {
        Ok(devices) => devices,
        Err(_) => {
            unsafe { instance.destroy_instance(None) };
            return (None, None);
        }
    };

    if physical_devices.is_empty() {
        unsafe { instance.destroy_instance(None) };
        return (None, None);
    }

    // Gather every device: (type, name, device-local VRAM bytes)
    let mut all: Vec<(vk::PhysicalDeviceType, String, u64)> = Vec::new();
    for &device in &physical_devices {
        let props = unsafe { instance.get_physical_device_properties(device) };
        let mem = unsafe { instance.get_physical_device_memory_properties(device) };
        let name = unsafe {
            CStr::from_ptr(props.device_name.as_ptr())
                .to_string_lossy()
                .into_owned()
        };
        let mut vram = 0u64;
        for i in 0..mem.memory_heap_count as usize {
            let heap = mem.memory_heaps[i];
            if heap.flags.contains(vk::MemoryHeapFlags::DEVICE_LOCAL) {
                vram += heap.size;
            }
        }
        all.push((props.device_type, name, vram));
    }
    unsafe { instance.destroy_instance(None) };

    // Report the DISCRETE pool when present — this matches the offload
    // device-selection policy (integrated GPUs are excluded when a discrete one
    // exists), and VRAM is SUMMED so multi-GPU sizing reflects the pooled
    // capacity we actually use. Falls back to the best single device otherwise.
    let discrete: Vec<&(vk::PhysicalDeviceType, String, u64)> = all
        .iter()
        .filter(|(t, _, _)| *t == vk::PhysicalDeviceType::DISCRETE_GPU)
        .collect();

    let selected: Vec<&(vk::PhysicalDeviceType, String, u64)> = if !discrete.is_empty() {
        discrete
    } else {
        let priority = |t: vk::PhysicalDeviceType| match t {
            vk::PhysicalDeviceType::VIRTUAL_GPU => 3,
            vk::PhysicalDeviceType::INTEGRATED_GPU => 2,
            vk::PhysicalDeviceType::CPU => 0,
            _ => 1,
        };
        match all.iter().max_by_key(|(t, _, _)| priority(*t)) {
            Some(d) => vec![d],
            None => return (None, None),
        }
    };

    let total_vram_gb =
        selected.iter().map(|(_, _, v)| *v).sum::<u64>() as f64 / 1024_f64.powi(3);
    let name = if selected.len() == 1 {
        selected[0].1.clone()
    } else {
        format!("{}× {}", selected.len(), selected[0].1)
    };

    log::info!(
        "[LLM] GPU detected: {} ({:.1}GB usable VRAM across {} device(s))",
        name,
        total_vram_gb,
        selected.len()
    );

    (Some(name), Some(total_vram_gb))
}

/**
 * Get system information (RAM, CPU, OS, GPU, VRAM)
 */
#[tauri::command]
pub fn get_system_info() -> Result<SystemInfo, String> {
    use sysinfo::System;
    
    let mut sys = System::new_all();
    sys.refresh_all();
    
    let total_memory = sys.total_memory();
    let used_memory = sys.used_memory();
    let total_memory_gb = total_memory as f64 / (1024_f64.powi(3));
    let used_memory_gb = used_memory as f64 / (1024_f64.powi(3));
    
    // Get GPU info via Vulkan. Apple Silicon has NO Vulkan (get_gpu_info
    // returns nothing there), but its GPU shares unified memory - so report
    // the chip as the GPU with a conservative slice of RAM as its budget,
    // mirroring Metal's recommendedMaxWorkingSetSize (~65% of RAM on 8GB
    // machines, ~75% above). Without this, an M-series Mac looks CPU-only,
    // the 7GB desktop RAM reserve marks every model too big, and the welcome
    // screen falls through to a wild fallback pick.
    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    let (gpu_name, total_vram_gb) = (
        Some(mac_chip_name()),
        Some(if total_memory_gb <= 8.5 {
            total_memory_gb * 0.65
        } else {
            total_memory_gb * 0.75
        }),
    );
    #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
    let (gpu_name, total_vram_gb) = get_gpu_info();
    
    Ok(SystemInfo {
        total_memory_gb,
        used_memory_gb,
        cpu_count: sys.cpus().len(),
        os_name: System::name().unwrap_or_else(|| "Unknown".to_string()),
        os_version: System::os_version().unwrap_or_else(|| "Unknown".to_string()),
        gpu_name,
        total_vram_gb,
    })
}

/**
 * List downloaded models in the models directory
 */
#[tauri::command]
pub async fn list_local_models(
    app_handle: AppHandle,
) -> Result<Vec<LocalModel>, String> {
    let models_dir = get_models_dir(&app_handle)?;
    
    let mut models = Vec::new();
    
    // Read directory
    let entries = std::fs::read_dir(&models_dir)
        .map_err(|e| format!("Failed to read models directory: {}", e))?;
    
    for entry in entries {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        
        // Only include .gguf files
        if path.extension().and_then(|s| s.to_str()) == Some("gguf") {
            let metadata = std::fs::metadata(&path)
                .map_err(|e| format!("Failed to get file metadata: {}", e))?;
            
            let size_bytes = metadata.len();
            let size_gb = size_bytes as f64 / (1024_f64.powi(3));
            
            let filename = path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            // Skip capability dependencies that live in the same dir but aren't
            // chat models: vision projectors (mmproj) and embedding models (bge).
            // They must never appear in the model picker or as a routing candidate.
            if filename.to_lowercase().contains("mmproj") {
                continue;
            }
            if crate::gguf::read_meta(&path)
                .map(|m| m.is_embedding())
                .unwrap_or(false)
            {
                continue;
            }

            // Quant/param labels: start from the filename, then upgrade with the
            // GGUF header where it's better — the exact quant from `file_type`,
            // and `size_label` as a fallback when the filename has no numeric
            // param tag (e.g. "E2B"/"E4B" names the filename parser can't read).
            let (mut param_size, mut quant) = parse_model_info(&filename);
            if let Ok(meta) = crate::gguf::read_meta(&path) {
                let gq = meta.quant_label();
                if gq != "Unknown" {
                    quant = gq.to_string();
                }
                if param_size == "Unknown" {
                    let s = meta.size_label.trim();
                    if s.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                        param_size = s.to_uppercase();
                    }
                }
            }

            models.push(LocalModel {
                name: filename.clone(),
                size: format!("{:.1}GB", size_gb),
                size_bytes,
                parameter_size: param_size,
                quantization: quant,
                modified_at: metadata.modified().ok().map(|t| format!("{:?}", t)),
            });
        }
    }
    
    // Sort by size (largest first)
    models.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes));
    
    Ok(models)
}

/**
 * Parse model information from filename using regex.
 * Extracts parameter size (e.g. "3.8B", "0.8B", "27B") and quantization (e.g. "Q4_K_M", "Q5_0").
 * Examples:
 *   "Phi-4-mini-instruct-Q4_K_M.gguf"           → ("3.8B", "Q4_K_M")  — via known model lookup
 *   "Qwen3.5-2B-Q4_K_M.gguf"                    → ("2B", "Q4_K_M")
 *   "DeepSeek-R1-0528-Qwen3-8B-Q4_K_M.gguf"     → ("8B", "Q4_K_M")
 */
fn parse_model_info(filename: &str) -> (String, String) {
    let lower = filename.to_lowercase();

    // Extract parameter size — look for patterns like "0.8b", "3.8b", "27b"
    // Use regex-like matching: find a number (with optional decimal) followed by 'b'
    // but NOT preceded by another letter (to avoid matching "q4_k_m" as "4b")
    let param_size = {
        let mut found = String::from("Unknown");
        let chars: Vec<char> = lower.chars().collect();
        for i in 0..chars.len() {
            // Must start with a digit or be preceded by a non-alphanumeric char
            if !chars[i].is_ascii_digit() { continue; }
            if i > 0 && chars[i - 1].is_ascii_alphabetic() { continue; }

            // Read the number (with optional decimal)
            let mut j = i;
            while j < chars.len() && (chars[j].is_ascii_digit() || chars[j] == '.') {
                j += 1;
            }

            // Must be followed by 'b' (case-insensitive)
            if j < chars.len() && chars[j] == 'b' {
                let num_str: String = chars[i..j].iter().collect();
                // Verify it's a valid number
                if num_str.parse::<f64>().is_ok() {
                    found = format!("{}B", num_str.to_uppercase());
                    // Prefer larger matches later in the string (parameter size
                    // usually comes after version numbers like "3.5" in "Qwen3.5-9B")
                }
            }
        }
        found
    };

    // Extract quantization — look for Q followed by a number and optional suffix
    let quant = {
        let mut found = String::from("Unknown");
        // Common patterns: Q4_K_M, Q4_K_S, Q5_0, Q6_K, Q8_0, etc.
        let parts: Vec<&str> = lower.split(|c: char| c == '-' || c == '.' || c == '_').collect();
        for (i, part) in parts.iter().enumerate() {
            if part.starts_with('q') && part.len() >= 2 && part.chars().nth(1).map_or(false, |c| c.is_ascii_digit()) {
                // Collect this part and any following underscore-connected parts (e.g., q4_k_m)
                let mut quant_parts = vec![*part];
                // Check if the original filename has underscore-connected parts after this
                let re_lower = &lower;
                if let Some(pos) = re_lower.find(part) {
                    let after = &re_lower[pos..];
                    // Find the full quantization string (e.g., "q4_k_m")
                    let end = after.find(|c: char| c == '-' || c == '.').unwrap_or(after.len());
                    found = after[..end].to_uppercase();
                }
                break;
            }
        }
        found
    };

    (param_size, quant)
}

/// Filenames with an in-flight `download_model` — prevents two concurrent
/// downloads writing the same `.part` (corruption), and lets the UI reattach to a
/// download in progress after the user navigates away.
fn downloading_set() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static S: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    S.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// RAII: clears the in-flight mark on every return path of `download_model`.
struct DownloadGuard(String);
impl Drop for DownloadGuard {
    fn drop(&mut self) {
        if let Ok(mut s) = downloading_set().lock() {
            s.remove(&self.0);
        }
    }
}

#[derive(serde::Serialize)]
pub struct DownloadStatus {
    pub downloading: bool,  // a download_model call is running for this file
    pub has_partial: bool,  // a resumable `.part` exists (e.g. after an app restart)
}

/// Lets the UI reattach to / resume a download started elsewhere — the parity fix
/// so Settings → Components survives navigation like the other download surfaces.
#[tauri::command]
pub async fn download_status(
    app_handle: AppHandle,
    filename: String,
) -> Result<DownloadStatus, String> {
    let downloading = downloading_set()
        .lock()
        .map(|s| s.contains(&filename))
        .unwrap_or(false);
    let models_dir = get_models_dir(&app_handle)?;
    let has_partial = models_dir.join(format!("{}.part", filename)).exists();
    Ok(DownloadStatus { downloading, has_partial })
}

/**
 * Download a model from Hugging Face with progress tracking
 */
#[tauri::command]
pub async fn download_model(
    app_handle: AppHandle,
    url: String,
    filename: String,
) -> Result<(), String> {
    use futures::StreamExt;
    use std::fs::File;
    use std::io::Write;
    
    println!("[LLM] Downloading model from: {}", url);
    println!("[LLM] Saving as: {}", filename);
    
    let models_dir = get_models_dir(&app_handle)?;
    let file_path = models_dir.join(&filename);
    // Download into a `.part` file and only rename to the final name once the
    // download completes AND its size checks out. A partial/interrupted download
    // therefore never appears as a usable model (list_local_models only lists
    // `.gguf`), which is what made an interrupted download look "corrupted".
    let part_path = models_dir.join(format!("{}.part", filename));

    // Check if file already exists (fully downloaded)
    if file_path.exists() {
        return Err("Model already downloaded".to_string());
    }
    // NOTE: we deliberately do NOT delete an existing `.part` — a partial from a
    // dropped connection is RESUMED via an HTTP Range request below. HF model
    // files are immutable per URL, so resuming is safe.

    // Refuse a second concurrent download of the same file — two writers would
    // corrupt the `.part`. A caller that finds it already running should reattach
    // (download_status), not restart. The guard clears the mark on every exit.
    {
        let mut set = downloading_set()
            .lock()
            .map_err(|_| "download registry lock poisoned".to_string())?;
        if set.contains(&filename) {
            return Err("Download already in progress".to_string());
        }
        set.insert(filename.clone());
    }
    let _guard = DownloadGuard(filename.clone());

    let client = reqwest::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))?;

    // Resumable download with auto-retry: if the connection drops mid-stream we
    // wait (backoff) and resume from the bytes already on disk, so a flaky or
    // briefly-dropped connection no longer kills a multi-GB download.
    const MAX_RETRIES: u32 = 10;
    let mut total_size: u64 = 0;
    let mut last_progress_percent = 0u32;
    let mut attempt: u32 = 0;

    loop {
        let resume_from = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);

        let mut req = client.get(&url);
        if resume_from > 0 {
            req = req.header(reqwest::header::RANGE, format!("bytes={}-", resume_from));
        }

        // One attempt; `break 'attempt Some(err)` on any failure so we retry.
        let attempt_err: Option<String> = 'attempt: {
            let response = match req.send().await {
                Ok(r) => r,
                Err(e) => break 'attempt Some(format!("connection error: {}", e)),
            };
            let status = response.status();
            // 416 = our resume offset is at/past EOF → the .part is already whole.
            if status == reqwest::StatusCode::RANGE_NOT_SATISFIABLE {
                break 'attempt None;
            }
            if !status.is_success() {
                break 'attempt Some(format!("server returned {}", status));
            }
            let is_partial = status == reqwest::StatusCode::PARTIAL_CONTENT;
            if total_size == 0 {
                let body_len = response.content_length().unwrap_or(0);
                total_size = if is_partial { resume_from + body_len } else { body_len };
                log::info!("[LLM] Downloading {} ({:.1}GB)", filename, total_size as f64 / 1024_f64.powi(3));
            }
            // Append when resuming (206); fresh file otherwise (200 = full restart).
            let mut file = match if resume_from > 0 && is_partial {
                std::fs::OpenOptions::new().append(true).open(&part_path)
            } else {
                File::create(&part_path)
            } {
                Ok(f) => f,
                Err(e) => break 'attempt Some(format!("file open failed: {}", e)),
            };
            let mut downloaded = if is_partial { resume_from } else { 0 };
            let mut stream = response.bytes_stream();
            loop {
                // Per-chunk read timeout: a dropped connection often *stalls*
                // rather than erroring, so bound the wait and let the retry loop
                // resume instead of hanging forever.
                let next = tokio::time::timeout(std::time::Duration::from_secs(60), stream.next()).await;
                match next {
                    Ok(Some(Ok(chunk))) => {
                        if let Err(e) = file.write_all(&chunk) {
                            break 'attempt Some(format!("write failed: {}", e));
                        }
                        downloaded += chunk.len() as u64;
                        let pct = if total_size > 0 {
                            ((downloaded as f64 / total_size as f64) * 100.0) as u32
                        } else { 0 };
                        if pct != last_progress_percent {
                            let _ = app_handle.emit("model-download-progress", serde_json::json!({
                                "filename": filename, "downloaded": downloaded, "total": total_size, "percent": pct
                            }));
                            last_progress_percent = pct;
                        }
                    }
                    Ok(Some(Err(e))) => break 'attempt Some(format!("stream error: {}", e)),
                    Ok(None) => break, // stream finished cleanly
                    Err(_) => break 'attempt Some("read timed out (connection stalled)".to_string()),
                }
            }
            file.flush().ok();
            None
        };

        let part_len = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);
        let complete = attempt_err.is_none() && (total_size == 0 || part_len >= total_size);
        if complete {
            break;
        }
        attempt += 1;
        if attempt > MAX_RETRIES {
            return Err(format!(
                "Download failed after {} retries ({:.1} of {:.1} GB). Your connection may be down — the partial download is saved, so trying again resumes it.",
                MAX_RETRIES, part_len as f64 / 1024_f64.powi(3), total_size as f64 / 1024_f64.powi(3)
            ));
        }
        if let Some(e) = &attempt_err {
            log::warn!("[LLM] Download attempt {} interrupted: {} — resuming in a moment", attempt, e);
        }
        let backoff = std::cmp::min(2u64.saturating_pow(attempt), 30);
        tokio::time::sleep(std::time::Duration::from_secs(backoff)).await;
    }

    // Final size check before publishing.
    let downloaded = std::fs::metadata(&part_path).map(|m| m.len()).unwrap_or(0);
    if total_size > 0 && downloaded > total_size {
        let _ = std::fs::remove_file(&part_path);
        return Err("Download corrupted (size mismatch). Please try again.".to_string());
    }

    // Atomically publish: only now does the model appear in list_local_models.
    std::fs::rename(&part_path, &file_path)
        .map_err(|e| format!("Failed to finalize download: {}", e))?;

    log::info!("[LLM] Download complete: {} ({} bytes)", filename, downloaded);

    // Emit completion event
    let _ = app_handle.emit("model-download-complete", serde_json::json!({
        "filename": filename,
        "path": file_path.to_string_lossy().to_string()
    }));

    Ok(())
}

/**
 * Delete a downloaded model
 */
#[tauri::command]
pub async fn delete_model(
    app_handle: AppHandle,
    filename: String,
) -> Result<(), String> {
    let models_dir = get_models_dir(&app_handle)?;
    let file_path = models_dir.join(&filename);
    
    if !file_path.exists() {
        return Err("Model file not found".to_string());
    }
    
    std::fs::remove_file(&file_path)
        .map_err(|e| format!("Failed to delete model: {}", e))?;
    
    println!("[LLM] Deleted model: {}", filename);
    Ok(())
}

/**
 * Get the path to the models directory
 */
#[tauri::command]
pub async fn get_models_directory(app_handle: AppHandle) -> Result<String, String> {
    let models_dir = get_models_dir(&app_handle)?;
    Ok(models_dir.to_string_lossy().to_string())
}

/**
 * Check if a specific model is downloaded
 */
#[tauri::command]
pub async fn is_model_downloaded(
    app_handle: AppHandle,
    filename: String,
) -> Result<bool, String> {
    let models_dir = get_models_dir(&app_handle)?;
    let file_path = models_dir.join(&filename);
    Ok(file_path.exists())
}

/**
 * Load a model for inference
 * Restarts llama-server with the specified model
 */
#[tauri::command]
pub async fn load_model(
    app_handle: AppHandle,
    state: State<'_, LLMState>,
    filename: String,
    with_vision: bool,
    reason: String,
) -> Result<(), String> {
    // TRACE: who asked for this chat-model (re)load, and with what flags. Lets us
    // see model thrash (A→B→A) and pin the trigger that fires an unwanted reload.
    log::info!(
        "[LLM] load_model '{}' — reason: {} (with_vision={})",
        filename, reason, with_vision
    );
    let models_dir = get_models_dir(&app_handle)?;
    let model_path = models_dir.join(&filename);

    if !model_path.exists() {
        return Err("Model file not found".to_string());
    }

    // Never load an embedding/encoder model (bge etc.) into the chat server —
    // it can't do causal generation, so completions 500 with "context does not
    // [support] logits computation". The embedding server (port 8091) loads it
    // separately. This is a safety net behind the router, which already excludes
    // these from its candidates.
    if let Ok(meta) = crate::gguf::read_meta(&model_path) {
        if meta.is_embedding() {
            return Err(format!(
                "'{}' is an embedding model and can't be used for chat",
                filename
            ));
        }
    }

    // Already proven too large for the GPU this session — reject instantly instead of
    // tearing down the running server to spend ~30s re-confirming the OOM. Returns
    // BEFORE the stop below, so a working model stays loaded. Clears on restart.
    if is_model_too_big(filename.clone()) {
        log::info!("[LLM] '{}' is known too-large this session — skipping load", filename);
        return Err("MODEL_TOO_LARGE".to_string());
    }

    println!("[LLM] Loading model: {}", filename);

    // Stop existing server if running
    stop_llama_server(state.clone()).await.ok();

    // Wait for port 8080 to be fully released
    for attempt in 1..=10 {
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        match is_llama_server_running().await {
            Ok(false) => {
                println!("[LLM] Port 8080 freed after {}ms", attempt * 500);
                break;
            }
            _ => {
                if attempt == 10 {
                    // Force kill anything on port 8080
                    println!("[LLM] Port 8080 still in use after 5s, force killing");
                    kill_port_8080().await.ok();
                    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                }
            }
        }
    }

    // Reset running flag so start_llama_server doesn't skip
    *state.is_server_running.lock().await = false;

    // Start server with the model
    start_llama_server(app_handle, state.clone(), Some(filename.clone()), with_vision).await?;
    
    // Store the current model filename
    *state.current_model.lock().await = Some(filename.clone());
    
    println!("[LLM] Model loaded and server restarted: {}", filename);
    Ok(())
}

/// Make the loaded model's vision state match what this turn needs, reloading only
/// when it must change — so text turns stay fast and image turns get the projector.
/// No-op when nothing needs to change (e.g. a vision model that fits the GPU stays
/// put). The frontend calls this each turn with `want_vision = turn has an image`.
#[tauri::command]
pub async fn ensure_vision(
    app_handle: AppHandle,
    state: State<'_, LLMState>,
    want_vision: bool,
) -> Result<(), String> {
    let current = state.current_model.lock().await.clone();
    let Some(model) = current else { return Ok(()); };
    let vision_loaded = state.current_mmproj.lock().await.is_some();
    // Reload only when an image turn needs the projector and it isn't loaded yet.
    // (No CPU fallback now: a model either fits the GPU or is reported too-large, so
    // there's no "reclaim the GPU for text" case to handle.)
    let need_reload = want_vision && !vision_loaded;
    if need_reload {
        load_model(app_handle, state, model, want_vision, "ensure-vision".to_string()).await?;
    }
    Ok(())
}

/**
 * Get currently loaded model
 */
#[tauri::command]
pub async fn get_current_model(
    state: State<'_, LLMState>,
) -> Result<Option<String>, String> {
    let current = state.current_model.lock().await;
    Ok(current.clone())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ChatMessage {
    pub role: String,
    /// A plain string for text turns, or an OpenAI multimodal content array
    /// (`[{ "type": "text", ... }, { "type": "image_url", ... }]`) for image
    /// turns. Kept as a raw `Value` so it forwards to llama-server unchanged.
    pub content: serde_json::Value,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct StreamChunkData {
    chunk: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct StreamErrorData {
    error: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct StreamUsageData {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

/// A web source returned by search-grounded online models (Perplexity/Sonar).
#[derive(Debug, Serialize, Deserialize, Clone)]
struct SourceItem {
    url: String,
    title: String,
}

/// Stream chat completion from llama-server
/// This bypasses CORS by making the HTTP request from Rust
/// and forwarding chunks to the frontend via Tauri events
#[tauri::command]
pub async fn stream_chat_completion(
    app: AppHandle,
    state: State<'_, LLMState>,
    request_id: String,
    messages: Vec<ChatMessage>,
    system_prompt: Option<String>,
    max_tokens: Option<u32>,
    capture_thinking: Option<bool>,
    model: Option<String>,
    grammar: Option<String>,
) -> Result<(), String> {
    // Online models ("online:<id>") route to the YOAI proxy with the
    // Flowsta (Vault-grant) token; external models ("external:<id>") post to
    // the user's own connected OpenAI-compatible server (no auth, their
    // hardware); everything else is local llama.cpp.
    let online_model: Option<String> = model
        .as_deref()
        .filter(|m| m.starts_with("online:"))
        .map(|m| m["online:".len()..].to_string());
    let external_model: Option<String> = model
        .as_deref()
        .filter(|m| m.starts_with("external:"))
        .map(|m| m["external:".len()..].to_string());
    // Reset cancellation flag at the start of each new request
    state.cancel_stream.store(false, std::sync::atomic::Ordering::Relaxed);

    println!("[LLM] Starting stream_chat_completion for request: {}", request_id);

    // Build messages with system prompt if provided
    let mut all_messages = Vec::new();
    if let Some(prompt) = system_prompt {
        all_messages.push(serde_json::json!({
            "role": "system",
            "content": prompt
        }));
    }
    for msg in messages {
        all_messages.push(serde_json::json!({
            "role": msg.role,
            "content": msg.content
        }));
    }

    // Build request body
    // Server starts with --reasoning off (thinking disabled by default).
    // For report mode, we enable thinking per-request via chat_template_kwargs.
    // This works universally: thinking models use it, others ignore it.
    let should_think = capture_thinking.unwrap_or(false);

    // Get model name: prefer Rust state, fall back to querying llama-server
    let model_name = {
        let cached = state.current_model.lock().await.clone();
        if let Some(name) = cached {
            name
        } else {
            // Query the running server for its model name
            let client_tmp = reqwest::Client::new();
            match client_tmp.get(format!("http://localhost:{}/v1/models", CHAT_PORT)).send().await {
                Ok(resp) if resp.status().is_success() => {
                    if let Ok(body) = resp.json::<serde_json::Value>().await {
                        body["data"][0]["id"].as_str()
                            .unwrap_or("default")
                            .to_string()
                    } else {
                        "default".to_string()
                    }
                }
                _ => "default".to_string(),
            }
        }
    };
    let mut body = serde_json::json!({
        "model": model_name,
        "messages": all_messages,
        "stream": true,
        "stream_options": { "include_usage": true },
        "max_tokens": max_tokens.unwrap_or(4096),
        "temperature": 0.7,
        "repeat_penalty": 1.1,
        "top_p": 0.9,
        "top_k": 40,
        // Explicit stop sequences — ensures generation stops even when
        // llama-server applies -inf logit bias to EOS tokens (which
        // prevents models like Phi-4 from ever stopping on their own).
        "stop": [
            "<|end|>",           // Phi-4, Phi-3
            "<|endoftext|>",     // Phi-4, Phi-3
            "<|im_end|>",        // Qwen, DeepSeek
            "<end_of_turn>",     // Gemma
            "</s>",              // Mistral, Llama
            "<|eot_id|>",        // Llama 3
        ],
    });
    if should_think {
        body["chat_template_kwargs"] = serde_json::json!({
            "enable_thinking": true
        });
    }
    // Optional GBNF grammar to constrain output (local llama.cpp only — used by
    // memory extraction to force schema-valid JSON). Online providers don't take
    // it; their minimal body below omits it.
    if let Some(g) = grammar.filter(|g| !g.is_empty()) {
        body["grammar"] = serde_json::Value::String(g);
    }
    let request_body = if let Some(remote_id) = online_model.as_ref().or(external_model.as_ref()) {
        // Minimal standard body — provider-specific extras (stop lists,
        // chat_template_kwargs, top_k/repeat_penalty) stay local-only.
        serde_json::json!({
            "model": remote_id,
            "messages": all_messages,
            "stream": true,
            "stream_options": { "include_usage": true },
            "max_tokens": max_tokens.unwrap_or(4096),
            "temperature": 0.7,
        })
    } else {
        body
    };

    // Log a COMPACT summary to the log file — never the full body, whose base64
    // image data floods the console and buries everything useful.
    let summary: Vec<String> = all_messages
        .iter()
        .map(|m| {
            let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("?");
            match &m["content"] {
                serde_json::Value::Array(parts) => {
                    let kinds: Vec<&str> = parts
                        .iter()
                        .map(|p| match p.get("type").and_then(|t| t.as_str()) {
                            Some("image_url") => "image",
                            Some("text") => "text",
                            _ => "?",
                        })
                        .collect();
                    format!("{}:[{}]", role, kinds.join(","))
                }
                serde_json::Value::String(s) => format!("{}:text({}c)", role, s.len()),
                _ => format!("{}:?", role),
            }
        })
        .collect();
    log::info!(
        "[LLM] request → model={:?} vision_ready={:?} messages=[{}]",
        model,
        *state.current_mmproj.lock().await,
        summary.join(" ")
    );

    // Wait for model to be fully loaded by polling health endpoint
    let client = reqwest::Client::new();
    // Remote models (online proxy / external server) have no local model to wait for.
    let max_health_checks = if online_model.is_some() || external_model.is_some() { 0 } else { 60 };
    
    println!("[LLM] Waiting for model to be fully loaded...");
    for attempt in 1..=max_health_checks {
        match client.get(format!("http://localhost:{}/health", CHAT_PORT)).send().await {
            Ok(health_resp) if health_resp.status().is_success() => {
                println!("[LLM] Model is fully loaded and ready!");
                break;
            }
            _ => {
                if attempt % 5 == 0 {
                    println!("[LLM] Still waiting for model... ({}/{})", attempt, max_health_checks);
                }
                if attempt >= max_health_checks {
                    return Err(format!("Model took too long to load (>2 minutes)"));
                }
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
            }
        }
    }
    
    // Now send the actual chat completion request
    let response = if online_model.is_some() {
        let token = crate::flowsta::get_access_token(&app).await.map_err(|_| {
            // Structured error so the UI can raise the sign-in modal.
            let _ = app.emit(
                &format!("chat-stream-error-{}", request_id),
                StreamErrorData {
                    error: r#"{"code":"auth_required","message":"Sign in with Flowsta to use online models"}"#.to_string(),
                },
            );
            "auth_required".to_string()
        })?;
        client
            .post(format!("{}/v1/chat/completions", crate::flowsta::proxy_url()))
            .header("Content-Type", "application/json")
            .bearer_auth(token)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("Failed to reach the online-model service: {}", e))?
    } else if external_model.is_some() {
        let base = crate::engine::external_engine_url(&app)
            .ok_or("external_engine_not_configured")?;
        client
            .post(format!("{}/v1/chat/completions", base))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("Failed to reach your external engine: {}", e))?
    } else {
        client
            .post(format!("http://localhost:{}/v1/chat/completions", CHAT_PORT))
            .header("Content-Type", "application/json")
            .json(&request_body)
            .send()
            .await
            .map_err(|e| format!("Failed to connect to llama-server: {}", e))?
    };
    
    // Check for errors
    if !response.status().is_success() {
        let status = response.status();
        let error_text = response.text().await.unwrap_or_default();
        println!("[LLM] Error response status: {}", status);
        println!("[LLM] Error response body: {}", error_text);
        if online_model.is_some() {
            // Relay the proxy's structured error (code: entitlement_required /
            // allowance_exceeded / overage_settlement_failed / auth) so the
            // frontend can show the right modal instead of a raw string.
            let payload = serde_json::from_str::<serde_json::Value>(&error_text)
                .ok()
                .and_then(|v| v.get("error").cloned())
                .map(|e| e.to_string())
                .unwrap_or_else(|| {
                    format!(r#"{{"code":"online_error","message":"Online model error ({})"}}"#, status)
                });
            let _ = app.emit(
                &format!("chat-stream-error-{}", request_id),
                StreamErrorData { error: payload },
            );
            return Ok(());
        }
        return Err(format!("llama-server error ({}): {}", status, error_text));
    }
    
    // Stream response with line-by-line buffering
    use futures::StreamExt;
    let mut stream = response.bytes_stream();
    let mut sse_buffer = String::new();
    let mut content_buffer = String::new();
    let mut usage_data: Option<StreamUsageData> = None;
    let mut sources: Vec<SourceItem> = Vec::new();
    let mut in_reasoning = false;

    while let Some(chunk_result) = stream.next().await {
        // Check for cancellation
        if state.cancel_stream.load(std::sync::atomic::Ordering::Relaxed) {
            println!("[LLM] Stream cancelled by user for request: {}", request_id);
            let _ = app.emit(&format!("chat-stream-{}", request_id), StreamChunkData {
                chunk: "[DONE]".to_string(),
            });
            return Ok(());
        }

        match chunk_result {
            Ok(chunk) => {
                // Convert bytes to string
                let chunk_str = String::from_utf8_lossy(&chunk);
                sse_buffer.push_str(&chunk_str);

                // Process complete SSE lines
                while let Some(newline_pos) = sse_buffer.find('\n') {
                    let line = sse_buffer[..newline_pos].to_string();
                    sse_buffer = sse_buffer[newline_pos + 1..].to_string();

                    if line.trim().is_empty() {
                        continue;
                    }

                    if let Some(data) = line.strip_prefix("data: ") {
                        if data.trim() == "[DONE]" {
                            // Emit any remaining buffered content
                            if !content_buffer.is_empty() {
                                let _ = app.emit(&format!("chat-stream-{}", request_id), StreamChunkData {
                                    chunk: content_buffer.clone(),
                                });
                                content_buffer.clear();
                            }

                            // Emit usage data if captured
                            if let Some(ref usage) = usage_data {
                                println!("[LLM] Emitting usage: prompt={}, completion={}, total={}",
                                    usage.prompt_tokens, usage.completion_tokens, usage.total_tokens);
                                let _ = app.emit(&format!("chat-stream-usage-{}", request_id), usage.clone());
                            }

                            // Emit web sources if captured
                            if !sources.is_empty() {
                                let _ = app.emit(&format!("chat-stream-sources-{}", request_id), sources.clone());
                            }

                            println!("[LLM] Stream complete for request: {}", request_id);
                            let _ = app.emit(&format!("chat-stream-{}", request_id), StreamChunkData {
                                chunk: "[DONE]".to_string(),
                            });
                            return Ok(());
                        }

                        // Parse JSON chunk
                        if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(data) {
                            // If the server streams reasoning_content (native thinking from
                            // reasoning models like Grok), always wrap it in <think> tags and
                            // forward it. The UI decides how to present it — expanded in report
                            // mode, collapsed-but-live in chat. Online reasoning models bill for
                            // these tokens whether shown or not, so we never discard them.
                            // (Local models don't emit reasoning_content in chat, so they stay
                            // direct/snappy; in report mode they reason via enable_thinking.)
                            if let Some(reasoning) = parsed["choices"][0]["delta"]["reasoning_content"].as_str() {
                                if !reasoning.is_empty() {
                                    if !in_reasoning {
                                        in_reasoning = true;
                                        let _ = app.emit(&format!("chat-stream-{}", request_id), StreamChunkData {
                                            chunk: "<think>\n".to_string(),
                                        });
                                    }
                                    content_buffer.push_str(reasoning);
                                    while let Some(line_end) = content_buffer.find('\n') {
                                        let complete_line = content_buffer[..=line_end].to_string();
                                        content_buffer = content_buffer[line_end + 1..].to_string();
                                        let _ = app.emit(&format!("chat-stream-{}", request_id), StreamChunkData {
                                            chunk: complete_line,
                                        });
                                    }
                                }
                            }

                            // Extract regular content
                            if let Some(content) = parsed["choices"][0]["delta"]["content"].as_str() {
                                // Close thinking tag when transitioning from reasoning to content
                                if in_reasoning {
                                    in_reasoning = false;
                                    if !content_buffer.is_empty() {
                                        let remaining = format!("{}\n", content_buffer);
                                        content_buffer.clear();
                                        let _ = app.emit(&format!("chat-stream-{}", request_id), StreamChunkData {
                                            chunk: remaining,
                                        });
                                    }
                                    let _ = app.emit(&format!("chat-stream-{}", request_id), StreamChunkData {
                                        chunk: "</think>\n".to_string(),
                                    });
                                }
                                content_buffer.push_str(content);

                                // Check for complete lines (ending with \n)
                                while let Some(line_end) = content_buffer.find('\n') {
                                    // Extract complete line INCLUDING the newline
                                    let complete_line = content_buffer[..=line_end].to_string();
                                    content_buffer = content_buffer[line_end + 1..].to_string();

                                    // Emit complete line to frontend
                                    let _ = app.emit(&format!("chat-stream-{}", request_id), StreamChunkData {
                                        chunk: complete_line,
                                    });
                                }
                            }

                            // Capture usage data if present (typically in the final chunk)
                            if let Some(usage) = parsed.get("usage") {
                                if let (Some(prompt), Some(completion), Some(total)) = (
                                    usage["prompt_tokens"].as_u64(),
                                    usage["completion_tokens"].as_u64(),
                                    usage["total_tokens"].as_u64(),
                                ) {
                                    usage_data = Some(StreamUsageData {
                                        prompt_tokens: prompt as u32,
                                        completion_tokens: completion as u32,
                                        total_tokens: total as u32,
                                    });
                                }
                            }

                            // Capture web sources from search-grounded models
                            // (Perplexity/Sonar send `search_results` with
                            // title + url). Latest non-empty set wins.
                            if let Some(results) = parsed.get("search_results").and_then(|v| v.as_array()) {
                                let found: Vec<SourceItem> = results
                                    .iter()
                                    .filter_map(|r| {
                                        let url = r["url"].as_str()?.to_string();
                                        let title = r["title"].as_str().unwrap_or(&url).to_string();
                                        Some(SourceItem { url, title })
                                    })
                                    .collect();
                                if !found.is_empty() {
                                    sources = found;
                                }
                            }
                        }
                    }
                }
            }
            Err(e) => {
                let error_msg = format!("Stream error: {}", e);
                println!("[LLM] {}", error_msg);
                let _ = app.emit(&format!("chat-stream-error-{}", request_id), StreamErrorData {
                    error: error_msg.clone(),
                });
                return Err(error_msg);
            }
        }
    }

    // Emit any remaining buffered content at the end
    if !content_buffer.is_empty() {
        let _ = app.emit(&format!("chat-stream-{}", request_id), StreamChunkData {
            chunk: content_buffer,
        });
    }

    // Emit usage data if captured (fallback for streams that end without [DONE])
    if let Some(ref usage) = usage_data {
        let _ = app.emit(&format!("chat-stream-usage-{}", request_id), usage.clone());
    }

    // Emit web sources if captured (fallback path)
    if !sources.is_empty() {
        let _ = app.emit(&format!("chat-stream-sources-{}", request_id), sources.clone());
    }

    // Some providers (notably Perplexity/Sonar) close the stream without a
    // `data: [DONE]` sentinel, so the handler above never fires. Always emit
    // the completion event here so the UI leaves the "thinking" state.
    let _ = app.emit(&format!("chat-stream-{}", request_id), StreamChunkData {
        chunk: "[DONE]".to_string(),
    });

    Ok(())
}

/// Cancel the active streaming chat completion.
/// Called from the frontend Stop button.
#[tauri::command]
pub async fn cancel_chat_completion(
    state: State<'_, LLMState>,
) -> Result<(), String> {
    println!("[LLM] Cancelling active stream");
    state.cancel_stream.store(true, std::sync::atomic::Ordering::Relaxed);
    Ok(())
}
