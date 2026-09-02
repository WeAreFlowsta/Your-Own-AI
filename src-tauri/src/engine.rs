//! Optional inference engines over the bundled llama.cpp core.
//!
//! The bundled sidecar (Vulkan on Linux/Windows, Metal on Apple Silicon)
//! is the universal zero-setup default. Heavier backends that only help
//! specific hardware - the CUDA build for NVIDIA GPUs first - are
//! downloaded on demand as optional components and resolved here, so the
//! base install stays lean.
//!
//! Only the CHAT server (and its device probes) follows the active
//! backend; the embedding and utility servers are CPU-only by design and
//! always run the bundled binary.

use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// The llama.cpp release tag this app is built against. MUST match
/// `RELEASE_TAG` in `.github/workflows/build-release.yml` - the
/// installer build fails when they disagree. Optional engine downloads
/// use the same tag, so the bundled engine and any downloaded backend
/// stay version-locked.
pub const LLAMA_ENGINE_TAG: &str = "llama-b10621";

/// Repo whose `llama-<tag>` releases hold the bundled binaries AND the
/// optional engine zips (built by build-llama-binaries.yml).
const ENGINE_RELEASE_REPO: &str = "WeAreFlowsta/Your-Own-AI";

/// Which backend powers the chat server.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Backend {
    Bundled,
    Cuda,
}

/// The version part of the tag ("llama-b10435" -> "b10435").
fn tag_version() -> &'static str {
    LLAMA_ENGINE_TAG
        .strip_prefix("llama-")
        .unwrap_or(LLAMA_ENGINE_TAG)
}

/// Target triple of this platform's CUDA engine build, if one exists
/// (Windows/Linux x64 only today).
fn cuda_triple() -> Option<&'static str> {
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    return Some("x86_64-unknown-linux-gnu");
    #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
    return Some("x86_64-pc-windows-msvc");
    #[allow(unreachable_code)]
    None
}

fn server_binary_name() -> &'static str {
    if cfg!(target_os = "windows") {
        "llama-server.exe"
    } else {
        "llama-server"
    }
}

/// Downloaded engines live under their own versioned dirs, NOT the models
/// dir (models are data; these are executables).
fn engines_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| format!("No app data dir: {}", e))?
        .join("engines"))
}

/// Install dir for the CUDA engine at the app's pinned tag. Versioned so
/// an app update cleanly offers a re-download instead of running a
/// mismatched binary.
pub fn cuda_engine_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(engines_dir(app)?.join(format!("cuda-{}", tag_version())))
}

/// Path to the installed CUDA llama-server for THIS app version, if present.
pub fn cuda_engine_binary(app: &AppHandle) -> Option<PathBuf> {
    let p = cuda_engine_dir(app).ok()?.join(server_binary_name());
    p.exists().then_some(p)
}

/// Is any OTHER version of the CUDA engine installed (stale after an app
/// update)? Drives the "update available" state on the engine card.
fn other_cuda_version_installed(app: &AppHandle) -> bool {
    let Ok(dir) = engines_dir(app) else { return false };
    let current = format!("cuda-{}", tag_version());
    let Ok(entries) = std::fs::read_dir(dir) else { return false };
    entries.flatten().any(|e| {
        e.file_name()
            .to_str()
            .map(|n| n.starts_with("cuda-") && n != current)
            .unwrap_or(false)
    })
}

/// Arch floor of our CUDA builds - CMAKE_CUDA_ARCHITECTURES "61;70;75;80;86;89"
/// in build-llama-binaries.yml and rebuild-llama-windows-cuda.yml (keep in
/// sync). Below this floor the engine contains no code the GPU can execute,
/// and every model load dies instantly with "no kernel image is available"
/// (seen in the field on a GTX 960M, Maxwell = 5.0). CUDA 12.2 could build
/// Maxwell (50;52) if a legacy engine is ever green-lit - the gate here
/// distinguishes "below OUR floor" from "no NVIDIA at all" for exactly that.
const CUDA_MIN_COMPUTE_CAP: f32 = 6.1;

/// The GPU's CUDA compute capability, queried once per session from
/// nvidia-smi (ships with every NVIDIA driver). None = no NVIDIA GPU, no
/// driver, or the query failed - callers must NOT gate on None (the
/// load-time device detection still protects them).
static COMPUTE_CAP: std::sync::OnceLock<Option<f32>> = std::sync::OnceLock::new();

fn query_compute_cap() -> Option<f32> {
    let mut cmd = std::process::Command::new("nvidia-smi");
    cmd.args(["--query-gpu=compute_cap", "--format=csv,noheader"]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000); // CREATE_NO_WINDOW - no console flash
    }
    let out = cmd.output().ok()?;
    if !out.status.success() {
        return None;
    }
    // One line per GPU; take the best card (matches our discrete-GPU pick).
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| l.trim().parse::<f32>().ok())
        .fold(None, |best: Option<f32>, c| Some(best.map_or(c, |b| b.max(c))))
}

/// Prime the cached compute-cap query off the startup path (called from
/// setup; the query is a subprocess and must not block a spawn decision).
pub fn prime_compute_cap() {
    let _ = COMPUTE_CAP.set(query_compute_cap());
}

/// Can this machine's NVIDIA GPU run our CUDA builds? True when unknown -
/// gating may only ever act on a POSITIVE "too old" reading.
pub fn cuda_gpu_supported() -> bool {
    match COMPUTE_CAP.get() {
        Some(Some(cap)) => *cap >= CUDA_MIN_COMPUTE_CAP,
        _ => true,
    }
}

/// Which backend the chat server should use right now: the downloaded CUDA
/// engine when installed at the current tag AND not laddered out by GPU
/// safe mode AND the GPU's generation can actually execute it; else the
/// bundled sidecar. The generation check protects people who installed the
/// CUDA engine before the gate existed (or pressed Update on a stale one).
pub fn active_backend(app: &AppHandle) -> Backend {
    if cuda_engine_binary(app).is_some()
        && crate::gpu_safety::cuda_allowed(app)
        && cuda_gpu_supported()
    {
        Backend::Cuda
    } else {
        Backend::Bundled
    }
}

/// Default download URL for this platform's CUDA engine zip.
fn cuda_download_url() -> Option<String> {
    Some(format!(
        "https://github.com/{}/releases/download/{}/llama-server-cuda-{}-{}.zip",
        ENGINE_RELEASE_REPO,
        LLAMA_ENGINE_TAG,
        tag_version(),
        cuda_triple()?
    ))
}

#[derive(Serialize)]
pub struct EngineStatus {
    /// A CUDA build exists for this platform.
    pub supported: bool,
    /// This machine's NVIDIA GPU generation can execute our CUDA build
    /// (true when unknown - only a positive too-old reading gates).
    pub gpu_supported: bool,
    /// The CUDA engine is installed at the app's pinned tag.
    pub installed: bool,
    /// An older CUDA engine version is on disk (update available).
    pub stale_version_installed: bool,
    /// What the NEXT chat-server spawn will use.
    pub active_backend: Backend,
    /// What the RUNNING chat server was actually spawned with, if one runs -
    /// the card says "powering your chats" only when this says so.
    pub running_backend: Option<String>,
    pub tag: String,
    pub download_url: Option<String>,
}

#[tauri::command]
pub async fn engine_status(
    app: AppHandle,
    state: tauri::State<'_, crate::llm::LLMState>,
) -> Result<EngineStatus, String> {
    let running_backend = if *state.is_server_running.lock().await {
        state.spawned_backend.lock().await.clone()
    } else {
        None
    };
    Ok(EngineStatus {
        supported: cuda_triple().is_some(),
        gpu_supported: cuda_gpu_supported(),
        installed: cuda_engine_binary(&app).is_some(),
        stale_version_installed: other_cuda_version_installed(&app),
        active_backend: active_backend(&app),
        running_backend,
        tag: LLAMA_ENGINE_TAG.to_string(),
        download_url: cuda_download_url(),
    })
}

/// Download + install the CUDA engine. Reuses the model downloader's
/// transport (resume, retry, `model-download-progress` events keyed by the
/// zip filename), then extracts into the versioned engine dir and removes
/// the zip. `url` override exists for dev testing against a locally served
/// zip (the release repo is private until launch).
#[tauri::command]
pub async fn download_cuda_engine(
    app: AppHandle,
    state: tauri::State<'_, crate::llm::LLMState>,
    url: Option<String>,
) -> Result<(), String> {
    let url = match url {
        Some(u) => u,
        None => cuda_download_url().ok_or("unsupported_platform")?,
    };
    let zip_name = format!("llama-server-cuda-{}.zip", tag_version());
    crate::llm::download_model(app.clone(), url, zip_name.clone()).await?;
    let result = install_cuda_zip(&app, &zip_name);
    // The zip is an installer artifact, not a model - clean it up either way.
    if let Ok(models) = crate::llm::get_models_dir(&app) {
        let _ = std::fs::remove_file(models.join(&zip_name));
    }
    result?;
    // Older versions are superseded the moment the new one is in place.
    remove_stale_cuda_versions(&app);
    log::info!("[Engine] CUDA engine installed ({})", LLAMA_ENGINE_TAG);
    // Applies now: the loaded model moves onto the new engine.
    if let Err(e) = crate::llm::reload_for_engine_change(app.clone(), state, None, None).await {
        log::warn!("[Engine] reload after CUDA install failed: {}", e);
    }
    Ok(())
}

/// Extract the downloaded zip (flat: binary + CUDA runtime libraries) into
/// the versioned engine dir and mark the server binary executable.
/// Extraction is ATOMIC: everything lands in a `.partial` sibling first and
/// only a complete, verified extract is renamed into place - so an app
/// killed mid-extract can never leave a half-engine that the presence check
/// would activate.
fn install_cuda_zip(app: &AppHandle, zip_name: &str) -> Result<(), String> {
    let zip_path = crate::llm::get_models_dir(app)?.join(zip_name);
    let file = std::fs::File::open(&zip_path)
        .map_err(|e| format!("engine zip missing after download: {}", e))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("bad engine zip: {}", e))?;

    let final_dir = cuda_engine_dir(app)?;
    let dir = final_dir
        .parent()
        .ok_or("engine dir has no parent")?
        .join(format!("cuda-{}.partial", tag_version()));
    let _ = std::fs::remove_dir_all(&dir); // stale partial from a killed run
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create engine dir: {}", e))?;

    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("bad zip entry: {}", e))?;
        if entry.is_dir() {
            continue;
        }
        // Flat archive: take the basename only - never join archive paths
        // (zip-slip). Our zips are built with `zip -j`.
        let Some(name) = std::path::Path::new(entry.name())
            .file_name()
            .and_then(|n| n.to_str())
            .map(String::from)
        else {
            continue;
        };
        let out_path = dir.join(&name);
        let mut out = std::fs::File::create(&out_path)
            .map_err(|e| format!("cannot write {}: {}", name, e))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| format!("extract failed: {}", e))?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bin = dir.join(server_binary_name());
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("cannot mark engine executable: {}", e))?;
    }

    if !dir.join(server_binary_name()).exists() {
        return Err("engine zip did not contain llama-server".into());
    }

    // Complete + verified: move into place in one step.
    let _ = std::fs::remove_dir_all(&final_dir);
    std::fs::rename(&dir, &final_dir)
        .map_err(|e| format!("could not finalize engine install: {}", e))?;
    Ok(())
}

fn remove_stale_cuda_versions(app: &AppHandle) {
    let Ok(dir) = engines_dir(app) else { return };
    let current = format!("cuda-{}", tag_version());
    let Ok(entries) = std::fs::read_dir(dir) else { return };
    for e in entries.flatten() {
        let name = e.file_name();
        let Some(n) = name.to_str() else { continue };
        if n.starts_with("cuda-") && n != current {
            let _ = std::fs::remove_dir_all(e.path());
            log::info!("[Engine] removed stale CUDA engine {}", n);
        }
    }
}

/// Remove the CUDA engine (all versions). Stops the chat server first so
/// Windows can delete the binary (no unlink-while-running), and so the next
/// start resolves back to the bundled engine.
#[tauri::command]
pub async fn remove_cuda_engine(
    app: AppHandle,
    state: tauri::State<'_, crate::llm::LLMState>,
) -> Result<(), String> {
    let _ = crate::llm::stop_llama_server(state).await;
    let Ok(dir) = engines_dir(&app) else { return Ok(()) };
    if let Ok(entries) = std::fs::read_dir(dir) {
        for e in entries.flatten() {
            if e.file_name().to_str().map(|n| n.starts_with("cuda-")).unwrap_or(false) {
                std::fs::remove_dir_all(e.path())
                    .map_err(|e| format!("could not remove engine: {}", e))?;
            }
        }
    }
    log::info!("[Engine] CUDA engine removed - back on the bundled engine");
    Ok(())
}

// ── External engine (connect-mode) ─────────────────────────────────────
// The user can point YOAI at any OpenAI-compatible server they run
// themselves (a llama.cpp on another box, a vLLM box, a Mac cluster).
// Chat requests for `external:<id>` models post straight to it - no auth,
// no billing, the user's own hardware. The URL is user-chosen by design;
// chat content goes wherever they point it.

const EXTERNAL_ENGINE_FILE: &str = "external-engine.json";

/// Trim a pasted base URL to scheme://host[:port][/path], dropping a
/// trailing slash and a trailing /v1 (we add /v1/... ourselves).
fn normalize_base_url(url: &str) -> String {
    let mut u = url.trim().trim_end_matches('/').to_string();
    if u.to_lowercase().ends_with("/v1") {
        u.truncate(u.len() - 3);
        u = u.trim_end_matches('/').to_string();
    }
    u
}

fn external_engine_path(app: &AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join(EXTERNAL_ENGINE_FILE))
}

/// The configured external engine's base URL, if any.
pub fn external_engine_url(app: &AppHandle) -> Option<String> {
    let raw = std::fs::read_to_string(external_engine_path(app)?).ok()?;
    serde_json::from_str::<serde_json::Value>(&raw).ok()?["url"]
        .as_str()
        .map(String::from)
}

/// Probe an OpenAI-compatible server: GET /v1/models, return its model ids.
async fn probe_external(base: &str) -> Result<Vec<String>, String> {
    let resp = reqwest::Client::new()
        .get(format!("{}/v1/models", base))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .map_err(|e| format!("unreachable: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|_| "not an OpenAI-compatible /v1/models response".to_string())?;
    let models = v["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|m| m["id"].as_str().map(String::from))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    Ok(models)
}

/// What we can learn about one external model without running it: whether
/// its id matches a family in the capability registry (llama.cpp reports
/// GGUF filenames; vLLM/Ollama ids usually carry the family name). Unknown
/// families rank conservatively when routing ever considers them.
#[derive(Serialize, Clone)]
pub struct ExternalModelInfo {
    pub id: String,
    /// Overall capability score when the family is recognized; null = unknown.
    pub overall: Option<u8>,
}

fn scan_models(ids: &[String]) -> Vec<ExternalModelInfo> {
    ids.iter()
        .map(|id| ExternalModelInfo {
            id: id.clone(),
            overall: crate::model_caps::known_caps(id).map(|c| c.overall),
        })
        .collect()
}

/// One timed mini-generation against the server → tokens/sec. llama.cpp
/// responses carry exact `timings`; otherwise wall-clock over the usage
/// counts. `None` when the server can't generate (still connectable - the
/// speed just stays unknown).
async fn bench_external(base: &str, model_id: &str) -> Option<f64> {
    let started = std::time::Instant::now();
    let resp = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", base))
        .timeout(std::time::Duration::from_secs(60))
        .json(&serde_json::json!({
            "model": model_id,
            "messages": [{"role": "user", "content": "Reply with the single word: ready"}],
            "max_tokens": 24,
            "stream": false,
        }))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: serde_json::Value = resp.json().await.ok()?;
    if let Some(tps) = v["timings"]["predicted_per_second"].as_f64() {
        return Some(tps);
    }
    let toks = v["usage"]["completion_tokens"].as_f64()?;
    let secs = started.elapsed().as_secs_f64();
    (toks > 0.0 && secs > 0.0).then(|| toks / secs)
}

#[derive(Serialize)]
pub struct ExternalEngineInfo {
    pub url: Option<String>,
    pub healthy: bool,
    pub models: Vec<String>,
    /// Per-model capability match from the registry (same order as `models`).
    pub models_info: Vec<ExternalModelInfo>,
    /// Measured generation speed from the connect-time bench, if any.
    pub tps: Option<f64>,
    pub error: Option<String>,
}

/// Connect an external engine: probe it, run the one-shot capability scan
/// (registry match per model + a timed mini-generation for tokens/sec),
/// and only save on success. The bench runs ONCE here, not on every status
/// read - its result is cached in the config file.
#[tauri::command]
pub async fn set_external_engine(app: AppHandle, url: String) -> Result<ExternalEngineInfo, String> {
    let base = normalize_base_url(&url);
    if !base.starts_with("http://") && !base.starts_with("https://") {
        return Err("URL must start with http:// or https://".into());
    }
    let models = probe_external(&base).await?;
    let tps = match models.first() {
        Some(first) => bench_external(&base, first).await,
        None => None,
    };
    let path = external_engine_path(&app).ok_or("no app data dir")?;
    // Models are cached alongside the url so ROUTING can consider the
    // server without a network probe per request; the status read
    // refreshes this cache whenever the server answers.
    std::fs::write(
        &path,
        serde_json::json!({ "url": base, "tps": tps, "models": models }).to_string(),
    )
    .map_err(|e| format!("could not save: {}", e))?;
    log::info!(
        "[Engine] external engine connected: {} ({} models{})",
        base,
        models.len(),
        tps.map(|t| format!(", ~{:.0} tok/s", t)).unwrap_or_default()
    );
    let models_info = scan_models(&models);
    Ok(ExternalEngineInfo { url: Some(base), healthy: true, models, models_info, tps, error: None })
}

/// Cached bench result from the connect-time scan.
fn stored_external_tps(app: &AppHandle) -> Option<f64> {
    let raw = std::fs::read_to_string(external_engine_path(app)?).ok()?;
    serde_json::from_str::<serde_json::Value>(&raw).ok()?["tps"].as_f64()
}

/// Status of the configured external engine: fresh model probe + registry
/// scan each call, bench speed from the connect-time cache (no generation
/// cost on settings opens).
#[tauri::command]
pub async fn external_engine_info(app: AppHandle) -> ExternalEngineInfo {
    let Some(url) = external_engine_url(&app) else {
        return ExternalEngineInfo {
            url: None, healthy: false, models: vec![], models_info: vec![], tps: None, error: None,
        };
    };
    let tps = stored_external_tps(&app);
    match probe_external(&url).await {
        Ok(models) => {
            // Keep the routing cache current with what the server reports.
            if let Some(p) = external_engine_path(&app) {
                let _ = std::fs::write(
                    &p,
                    serde_json::json!({ "url": &url, "tps": tps, "models": &models }).to_string(),
                );
            }
            let models_info = scan_models(&models);
            ExternalEngineInfo { url: Some(url), healthy: true, models, models_info, tps, error: None }
        }
        Err(e) => ExternalEngineInfo {
            url: Some(url), healthy: false, models: vec![], models_info: vec![], tps, error: Some(e),
        },
    }
}

/// Quick reachability check on the connected external server - a short
/// probe so routing can fall back to a local model instead of handing a
/// chat to a server that's off. Two-second budget: routing latency matters.
pub async fn external_reachable(app: &AppHandle) -> bool {
    let Some(url) = external_engine_url(app) else { return false };
    reqwest::Client::new()
        .get(format!("{}/v1/models", url))
        .timeout(std::time::Duration::from_secs(2))
        .send()
        .await
        .map(|r| r.status().is_success())
        .unwrap_or(false)
}

/// The cached external-engine state for ROUTING: (model ids, measured tps).
/// Reads the connect-time cache - no network. Empty when nothing connected.
pub fn external_models_cached(app: &AppHandle) -> (Vec<String>, Option<f64>) {
    let Some(p) = external_engine_path(app) else { return (vec![], None) };
    let Ok(raw) = std::fs::read_to_string(p) else { return (vec![], None) };
    let Ok(v) = serde_json::from_str::<serde_json::Value>(&raw) else { return (vec![], None) };
    let models = v["models"]
        .as_array()
        .map(|a| a.iter().filter_map(|m| m.as_str().map(String::from)).collect())
        .unwrap_or_default();
    (models, v["tps"].as_f64())
}

#[tauri::command]
pub fn remove_external_engine(app: AppHandle) {
    if let Some(p) = external_engine_path(&app) {
        let _ = std::fs::remove_file(p);
    }
    log::info!("[Engine] external engine removed");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_version_strips_prefix() {
        assert_eq!(tag_version(), "b10435");
    }

    #[test]
    fn base_url_normalization() {
        assert_eq!(normalize_base_url("http://192.168.1.20:8000/v1/"), "http://192.168.1.20:8000");
        assert_eq!(normalize_base_url("http://host:8000/v1"), "http://host:8000");
        assert_eq!(normalize_base_url("https://my.box/llm/"), "https://my.box/llm");
        assert_eq!(normalize_base_url("  http://host:11434  "), "http://host:11434");
    }

    #[test]
    fn download_url_is_release_asset_shaped() {
        if let Some(url) = cuda_download_url() {
            assert!(url.starts_with(
                "https://github.com/WeAreFlowsta/Your-Own-AI/releases/download/llama-b10435/llama-server-cuda-b10435-"
            ));
            assert!(url.ends_with(".zip"));
        }
    }
}
