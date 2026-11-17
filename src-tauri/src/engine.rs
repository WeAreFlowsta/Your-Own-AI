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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

/// The llama.cpp release tag this app is built against. MUST match
/// `RELEASE_TAG` in `.github/workflows/build-release.yml` - the
/// installer build fails when they disagree. Optional engine downloads
/// use the same tag, so the bundled engine and any downloaded backend
/// stay version-locked.
pub const LLAMA_ENGINE_TAG: &str = "llama-b12359";

/// Repo whose `llama-<tag>` releases hold the bundled binaries AND the
/// optional engine zips (built by build-llama-binaries.yml).
const ENGINE_RELEASE_REPO: &str = "WeAreFlowsta/Your-Own-AI";

/// sha256 of each CUDA engine zip on the pinned release, by target triple
/// (GitHub's asset digests: `gh api repos/<repo>/releases/tags/<tag>
/// --jq '.assets[] | .name + " " + .digest'`). A downloaded zip that does
/// not match is deleted, never installed: the engine is an executable, and
/// the release page is the only thing standing between a download and a
/// swapped binary otherwise. Update WITH every LLAMA_ENGINE_TAG bump.
const CUDA_ZIP_SHA256: &[(&str, &str)] = &[
    ("x86_64-unknown-linux-gnu", "c533f1aba1011511df0fcfbc80f9af290aefb2db6303f5b8f2b75494aa37388d"),
    ("x86_64-pc-windows-msvc", "22349d6a142aec0546cd8a50c01d1448cd71c0848e598803f10e74e17b9cdcb3"),
];

/// Size in bytes of each CUDA engine zip on the pinned release (GitHub's
/// asset sizes: `gh release view <tag> --repo <repo> --json assets`). The
/// offer and Settings say this number - the tray counts the same bytes.
/// Update WITH every LLAMA_ENGINE_TAG bump, beside the checksums.
const CUDA_ZIP_BYTES: &[(&str, u64)] = &[
    ("x86_64-unknown-linux-gnu", 870_661_526),
    ("x86_64-pc-windows-msvc", 790_346_417),
];

fn cuda_zip_bytes() -> Option<u64> {
    let triple = cuda_triple()?;
    CUDA_ZIP_BYTES.iter().find(|(t, _)| *t == triple).map(|(_, b)| *b)
}

fn cuda_zip_expected_sha256() -> Option<&'static str> {
    let triple = cuda_triple()?;
    CUDA_ZIP_SHA256.iter().find(|(t, _)| *t == triple).map(|(_, h)| *h)
}

fn sha256_file(path: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut f = std::fs::File::open(path).map_err(|e| format!("cannot read the engine zip: {e}"))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut f, &mut hasher).map_err(|e| format!("cannot hash the engine zip: {e}"))?;
    Ok(hex::encode(hasher.finalize()))
}

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
    /// The pinned engine is being fetched by the app itself after an
    /// update: there is nothing to press.
    pub arriving: bool,
    /// What the NEXT chat-server spawn will use.
    pub active_backend: Backend,
    /// What the bundled engine runs models on, on THIS platform: "metal"
    /// (Apple Silicon), "processor" (Intel Macs - the bundled Intel build has
    /// no graphics path) or "vulkan" (Windows, Linux).
    pub bundled_runs_on: &'static str,
    /// What the RUNNING chat server was actually spawned with, if one runs -
    /// the card says "powering your chats" only when this says so.
    pub running_backend: Option<String>,
    pub tag: String,
    pub download_url: Option<String>,
    /// The CUDA download's size on this platform, in bytes (None where
    /// there is no CUDA build).
    pub download_bytes: Option<u64>,
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
        arriving: arriving(),
        active_backend: active_backend(&app),
        bundled_runs_on: if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
            "metal"
        } else if cfg!(target_os = "macos") {
            "processor"
        } else {
            "vulkan"
        },
        running_backend,
        tag: LLAMA_ENGINE_TAG.to_string(),
        download_url: cuda_download_url(),
        download_bytes: cuda_zip_bytes(),
    })
}

/// Download + install the CUDA engine. Reuses the model downloader's
/// transport (resume, retry, `model-download-progress` events keyed by the
/// zip filename), then extracts into the versioned engine dir and removes
/// the zip.
#[tauri::command]
pub async fn download_cuda_engine(
    app: AppHandle,
    state: tauri::State<'_, crate::llm::LLMState>,
) -> Result<(), String> {
    install_pinned(&app, state, false).await
}

/// Test builds only: the engine zip from another address, with its own
/// fingerprint (`e2e/engine-update-live.sh` serves this machine's installed
/// engine, so the test needs no 800 MB download). A release build has no
/// such door: its only source is the pinned release.
#[cfg(debug_assertions)]
fn test_source() -> Option<(String, String)> {
    let url = std::env::var("YOAI_TEST_CUDA_ZIP_URL").ok()?;
    let sha = std::env::var("YOAI_TEST_CUDA_ZIP_SHA256").ok()?;
    log::warn!("[Engine] TEST: the engine zip comes from {url}");
    Some((url, sha))
}

#[cfg(not(debug_assertions))]
fn test_source() -> Option<(String, String)> {
    None
}

/// The pinned engine's zip, as the download is named on disk.
fn cuda_zip_name() -> String {
    format!("llama-server-cuda-{}.zip", tag_version())
}

/// `quiet`: fetched by the app itself after an update (`fetch_due_update`).
/// A failure then raises no error; it is logged and the offer card stands.
async fn install_pinned(
    app: &AppHandle,
    state: tauri::State<'_, crate::llm::LLMState>,
    quiet: bool,
) -> Result<(), String> {
    // The URL is derived from the pin, never taken from the caller: an
    // engine is an executable, and the only source is the pinned release.
    let url = cuda_download_url().ok_or("unsupported_platform")?;
    let expected = cuda_zip_expected_sha256().ok_or("unsupported_platform")?.to_string();
    let (url, expected) = test_source().unwrap_or((url, expected));
    let zip_name = cuda_zip_name();
    crate::llm::download_model(app.clone(), url, zip_name.clone(), Some(quiet)).await?;
    let zip_path = crate::llm::get_models_dir(app)?.join(&zip_name);
    let result = match sha256_file(&zip_path) {
        Ok(actual) if actual == expected => install_cuda_zip(app, &zip_name),
        Ok(actual) => {
            log::warn!("[Engine] CUDA zip checksum mismatch: expected {expected}, got {actual} - not installed");
            Err("The engine download did not match its published checksum, so it was not installed. Try again later.".to_string())
        }
        Err(e) => Err(e),
    };
    // The zip is an installer artifact, not a model - clean it up either way.
    if let Ok(models) = crate::llm::get_models_dir(app) {
        let _ = std::fs::remove_file(models.join(&zip_name));
    }
    // The page that started this may be gone (the download outlives the
    // webview route); the outcome goes out as an event so any page can
    // reflect it - the offer card and Settings both listen.
    if let Err(e) = &result {
        if !quiet {
            let _ = app.emit("engine-install-failed", e.clone());
        }
    }
    result?;
    // No load waits on it any more, and the next one finds it in place.
    ARRIVING.store(false, Ordering::SeqCst);
    let _ = app.emit("engine-installed", LLAMA_ENGINE_TAG);
    // Older versions are superseded the moment the new one is in place.
    remove_stale_cuda_versions(app);
    log::info!("[Engine] CUDA engine installed ({})", LLAMA_ENGINE_TAG);
    // Applies now: the loaded model moves onto the new engine.
    if let Err(e) = crate::llm::reload_for_engine_change(app.clone(), state, None, None).await {
        log::warn!("[Engine] reload after CUDA install failed: {}", e);
    }
    Ok(())
}

// --- The engine as an update's last step ---------------------------------
//
// An app update that pins a newer engine leaves the installed NVIDIA engine
// behind: `cuda_engine_binary` looks only at the pinned version. Someone who
// chose the NVIDIA engine keeps it across updates with nothing to press: the
// pinned one is fetched as the app starts, and a model asked for meanwhile
// loads when it arrives (bounded: `hold_decision`), so the first replies
// after an update are not slower than the ones before it.

/// The pinned engine is on its way after an update.
static ARRIVING: AtomicBool = AtomicBool::new(false);
/// A load already stopped waiting for this arrival: later loads do not
/// wait again, and the model moves across when the engine lands.
static GAVE_UP: AtomicBool = AtomicBool::new(false);
/// The download so far: when it began, and the first and latest percent
/// seen (the first is not zero when it resumes a partial file).
static PROGRESS: Mutex<Option<Arrival>> = Mutex::new(None);

#[derive(Clone, Copy)]
struct Arrival {
    began: std::time::Instant,
    first: Option<(f64, f64)>, // (seconds since `began`, percent)
    latest: f64,
}

/// The longest a model load waits for the engine.
const HOLD_MAX_SECS: f64 = 180.0;
/// How long the download is watched before its pace is judged.
const HOLD_JUDGE_AFTER_SECS: f64 = 15.0;

#[derive(Debug, PartialEq)]
enum Hold {
    Wait,
    GiveUp,
}

/// Should a model load keep waiting for the engine? It waits while the
/// download's pace says the engine lands inside `HOLD_MAX_SECS` of its
/// start, and gives up as soon as it says otherwise: nobody waits three
/// minutes for a download that needs ten.
fn hold_decision(elapsed: f64, first: Option<(f64, f64)>, latest: f64) -> Hold {
    if elapsed >= HOLD_MAX_SECS {
        return Hold::GiveUp;
    }
    if elapsed < HOLD_JUDGE_AFTER_SECS {
        return Hold::Wait;
    }
    let Some((t0, p0)) = first else { return Hold::GiveUp }; // nothing has moved
    let (dt, dp) = (elapsed - t0, latest - p0);
    if dt <= 0.0 || dp <= 0.0 {
        return Hold::GiveUp;
    }
    let left = (100.0 - latest).max(0.0) / (dp / dt);
    if elapsed + left <= HOLD_MAX_SECS { Hold::Wait } else { Hold::GiveUp }
}

/// Is an update of the installed NVIDIA engine due: another version is on
/// disk (the person chose it and did not remove it), the pinned one is
/// not, and this machine can use it?
pub fn update_due(app: &AppHandle) -> bool {
    cuda_triple().is_some()
        && cuda_engine_binary(app).is_none()
        && other_cuda_version_installed(app)
        && cuda_gpu_supported()
        && !crate::llm::downloads_blocked()
}

pub fn arriving() -> bool {
    ARRIVING.load(Ordering::SeqCst)
}

/// In setup, before the page can ask for a model: note that the engine is
/// on its way, so the first load waits for it. `fetch_due_update` follows
/// once the graphics card's generation has been read.
pub fn expect_due_update(app: &AppHandle) {
    if !update_due(app) || ARRIVING.swap(true, Ordering::SeqCst) {
        return;
    }
    GAVE_UP.store(false, Ordering::SeqCst);
    *PROGRESS.lock().unwrap() = Some(Arrival { began: std::time::Instant::now(), first: None, latest: 0.0 });
}

/// At start: fetch the pinned engine when an update of it is due. Returns
/// at once; the fetch runs on its own. A failure is quiet (the offer card
/// with its Update button stands, and the next start tries again).
pub fn fetch_due_update(app: &AppHandle) {
    // Marked in `expect_due_update`; the graphics card's generation is
    // known by now, and an engine it cannot run is not fetched.
    if !arriving() {
        return;
    }
    if !update_due(app) {
        ARRIVING.store(false, Ordering::SeqCst);
        return;
    }
    log::info!("[Engine] the installed NVIDIA engine is from before this update: fetching {} now", LLAMA_ENGINE_TAG);
    let _ = app.emit("engine-arriving", LLAMA_ENGINE_TAG);
    // The download's pace, for the loads that wait on it.
    {
        use tauri::Listener as _;
        let zip = cuda_zip_name();
        app.listen("model-download-progress", move |event| {
            if !arriving() {
                return;
            }
            let Ok(v) = serde_json::from_str::<serde_json::Value>(event.payload()) else { return };
            if v.get("filename").and_then(|f| f.as_str()) != Some(zip.as_str()) {
                return;
            }
            let Some(percent) = v.get("percent").and_then(|p| p.as_f64()) else { return };
            if let Some(a) = PROGRESS.lock().unwrap().as_mut() {
                if a.first.is_none() {
                    a.first = Some((a.began.elapsed().as_secs_f64(), percent));
                }
                a.latest = percent;
            }
        });
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<crate::llm::LLMState>();
        let result = install_pinned(&app, state, true).await;
        ARRIVING.store(false, Ordering::SeqCst);
        if let Err(e) = result {
            log::warn!("[Engine] the NVIDIA engine was not fetched (the Update button stands; the next start tries again): {}", e);
            let _ = app.emit("engine-install-put-off", ());
        }
    });
}

/// A model load waits here while the engine is on its way, so it starts on
/// the engine the person chose. Bounded by `hold_decision`. Not when safe
/// mode holds models off the graphics card: the engine is still fetched,
/// but no load would use it.
pub async fn wait_while_arriving(app: &AppHandle) {
    if !arriving() || GAVE_UP.load(Ordering::SeqCst) || !crate::gpu_safety::cuda_allowed(app) {
        return;
    }
    log::info!("[Engine] the model loads when the NVIDIA engine is in place (it is downloading)");
    loop {
        if !arriving() {
            log::info!("[Engine] the wait for the NVIDIA engine is over");
            return;
        }
        let snapshot = *PROGRESS.lock().unwrap();
        let Some(a) = snapshot else { return };
        let elapsed = a.began.elapsed().as_secs_f64();
        if hold_decision(elapsed, a.first, a.latest) == Hold::GiveUp {
            GAVE_UP.store(true, Ordering::SeqCst);
            log::info!(
                "[Engine] the NVIDIA engine is not close enough ({:.0}% after {:.0} s): the model loads on the standard engine and moves across when it lands",
                a.latest, elapsed
            );
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    }
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
        let Some(name) = entry
            .name()
            .ok()
            .and_then(|n| std::path::Path::new(n.as_ref()).file_name().and_then(|f| f.to_str()).map(String::from))
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
    /// The server's context window per slot, when it reports one
    /// (llama.cpp `/props`); None for engines that do not say.
    pub ctx: Option<u64>,
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
    let ctx = probe_external_ctx(&base).await;
    let path = external_engine_path(&app).ok_or("no app data dir")?;
    // Models are cached alongside the url so ROUTING can consider the
    // server without a network probe per request; the status read
    // refreshes this cache whenever the server answers.
    std::fs::write(
        &path,
        serde_json::json!({ "url": base, "tps": tps, "models": models, "ctx": ctx }).to_string(),
    )
    .map_err(|e| format!("could not save: {}", e))?;
    log::info!(
        "[Engine] external engine connected: {} ({} models{}{})",
        base,
        models.len(),
        tps.map(|t| format!(", ~{:.0} tok/s", t)).unwrap_or_default(),
        ctx.map(|c| format!(", {}k window", c / 1024)).unwrap_or_default()
    );
    let models_info = scan_models(&models);
    Ok(ExternalEngineInfo { url: Some(base), healthy: true, ctx, models, models_info, tps, error: None })
}

/// The server's context window per slot, from llama.cpp's `/props`
/// (`default_generation_settings.n_ctx`). Other engines say nothing.
async fn probe_external_ctx(base: &str) -> Option<u64> {
    let resp = reqwest::Client::new()
        .get(format!("{}/props", base))
        .timeout(std::time::Duration::from_secs(5))
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: serde_json::Value = resp.json().await.ok()?;
    v["default_generation_settings"]["n_ctx"].as_u64().filter(|c| *c > 0)
}

/// The cached window of the connected server (routing reads this; no network).
pub fn external_ctx_cached(app: &AppHandle) -> Option<u64> {
    let raw = std::fs::read_to_string(external_engine_path(app)?).ok()?;
    serde_json::from_str::<serde_json::Value>(&raw).ok()?["ctx"].as_u64().filter(|c| *c > 0)
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
            url: None, healthy: false, ctx: None, models: vec![], models_info: vec![], tps: None, error: None,
        };
    };
    let tps = stored_external_tps(&app);
    match probe_external(&url).await {
        Ok(models) => {
            let ctx = probe_external_ctx(&url).await.or_else(|| external_ctx_cached(&app));
            // Keep the routing cache current with what the server reports.
            if let Some(p) = external_engine_path(&app) {
                let _ = std::fs::write(
                    &p,
                    serde_json::json!({ "url": &url, "tps": tps, "models": &models, "ctx": ctx }).to_string(),
                );
            }
            let models_info = scan_models(&models);
            ExternalEngineInfo { url: Some(url), healthy: true, ctx, models, models_info, tps, error: None }
        }
        Err(e) => ExternalEngineInfo {
            url: Some(url), healthy: false, ctx: external_ctx_cached(&app), models: vec![], models_info: vec![], tps, error: Some(e),
        },
    }
}

/// Quick reachability check on the connected external server - a short
/// probe so routing can fall back to a local model instead of handing a
/// chat to a server that's off. Two-second budget: routing latency matters.
/// The window a session is told for a server-served model. Not every engine
/// reports its own; a llama.cpp overflow answer is caught by the harness's
/// compact-and-resubmit path, so a generous default is safe.
pub const EXTERNAL_CTX_DEFAULT: u64 = 32_768;

static EXTERNAL_FAILED_AT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// A request to the server failed: it is set aside for a minute, so the
/// next routes use this computer without waiting on a probe.
pub fn mark_external_failed() {
    EXTERNAL_FAILED_AT.store(now_secs(), std::sync::atomic::Ordering::SeqCst);
    log::warn!("[Engine] your server did not answer - set aside for a minute");
}

fn external_recently_failed() -> bool {
    let t = EXTERNAL_FAILED_AT.load(std::sync::atomic::Ordering::SeqCst);
    t > 0 && now_secs().saturating_sub(t) < 60
}

pub async fn external_reachable(app: &AppHandle) -> bool {
    let Some(url) = external_engine_url(app) else { return false };
    if external_recently_failed() {
        return false;
    }
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
    #[test]
    fn every_pinned_cuda_zip_has_a_size() {
        for (triple, _) in super::CUDA_ZIP_SHA256 {
            assert!(super::CUDA_ZIP_BYTES.iter().any(|(t, b)| t == triple && *b > 100_000_000), "no size pinned for {triple}");
        }
    }

    use super::*;

    #[test]
    fn a_load_waits_for_the_engine_only_while_it_is_close() {
        // The first seconds: wait, whatever the download says yet.
        assert_eq!(hold_decision(3.0, None, 0.0), Hold::Wait);
        assert_eq!(hold_decision(14.0, Some((1.0, 0.0)), 1.0), Hold::Wait);
        // Nothing has moved by the time it is judged.
        assert_eq!(hold_decision(15.0, None, 0.0), Hold::GiveUp);
        assert_eq!(hold_decision(20.0, Some((2.0, 40.0)), 40.0), Hold::GiveUp);
        // 2% a second: lands about a minute in. Wait.
        assert_eq!(hold_decision(20.0, Some((0.0, 0.0)), 40.0), Hold::Wait);
        // 0.2% a second: eight minutes. Not waited for.
        assert_eq!(hold_decision(20.0, Some((0.0, 0.0)), 4.0), Hold::GiveUp);
        // A resumed download is judged on what moved since, not on where
        // it started: 60% already there, 1% in 19 s is slow.
        assert_eq!(hold_decision(20.0, Some((1.0, 60.0)), 61.0), Hold::GiveUp);
        // ...and quick when it moves quickly.
        assert_eq!(hold_decision(20.0, Some((1.0, 60.0)), 90.0), Hold::Wait);
        // Never past the cap.
        assert_eq!(hold_decision(180.0, Some((0.0, 0.0)), 99.0), Hold::GiveUp);
    }

    #[test]
    fn tag_version_strips_prefix() {
        // Derived from the pin, never a literal: a bump must not trip this.
        assert_eq!(tag_version(), LLAMA_ENGINE_TAG.trim_start_matches("llama-"));
        assert!(tag_version().starts_with('b'), "engine tags are llama.cpp build tags");
        assert!(!tag_version().contains("llama-"));
    }

    #[test]
    fn base_url_normalization() {
        assert_eq!(normalize_base_url("http://192.168.1.20:8000/v1/"), "http://192.168.1.20:8000");
        assert_eq!(normalize_base_url("http://host:8000/v1"), "http://host:8000");
        assert_eq!(normalize_base_url("https://my.box/llm/"), "https://my.box/llm");
        assert_eq!(normalize_base_url("  http://host:11434  "), "http://host:11434");
    }

    #[test]
    fn cuda_zip_checksums_cover_every_cuda_platform() {
        for triple in ["x86_64-unknown-linux-gnu", "x86_64-pc-windows-msvc"] {
            let h = CUDA_ZIP_SHA256.iter().find(|(t, _)| *t == triple).map(|(_, h)| *h).expect(triple);
            assert_eq!(h.len(), 64, "{triple}: not a sha256 hex");
            assert!(h.chars().all(|c| c.is_ascii_hexdigit()), "{triple}: not hex");
        }
    }

    #[test]
    fn download_url_is_release_asset_shaped() {
        if let Some(url) = cuda_download_url() {
            // Shape only, derived from the pin - a bump must not trip this.
            let expected = format!(
                "https://github.com/WeAreFlowsta/Your-Own-AI/releases/download/{}/llama-server-cuda-{}-",
                LLAMA_ENGINE_TAG,
                tag_version()
            );
            assert!(url.starts_with(&expected), "{url} should start with {expected}");
            assert!(url.ends_with(".zip"));
        }
    }
}
