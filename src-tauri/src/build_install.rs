//! Your Own AI Build - install on demand.
//!
//! The projects surface ships visible to everyone; the agent itself is a
//! free add-on downloaded from its GitHub release when the user asks.
//! Downloads ride the shared model-download transport (resume, retry,
//! `model-download-progress` events keyed by the archive name) and run in
//! the Rust runtime, so navigating the app - or closing the dropdown -
//! never interrupts them. Extraction is atomic: a `.partial` dir is
//! renamed into place only when complete.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use tauri::{AppHandle, Emitter, Manager};

/// Pinned release: the app version and the agent it installs move together
/// (same discipline as the engine downloads).
const BUILD_RELEASE_TAG: &str = "v0.1.0";
const BUILD_REPO: &str = "WeAreFlowsta/Your-Own-AI-Build";

static DOWNLOADING: AtomicBool = AtomicBool::new(false);
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

fn version() -> &'static str {
    BUILD_RELEASE_TAG.trim_start_matches('v')
}

/// This platform's release asset (archive filename), or None when the
/// platform has no build.
fn platform_asset() -> Option<String> {
    let label = match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x86_64",
        ("macos", "aarch64") => "macos-arm64",
        ("macos", "x86_64") => "macos-x86_64",
        ("windows", "x86_64") => "windows-x86_64",
        _ => return None,
    };
    let ext = if std::env::consts::OS == "windows" { "zip" } else { "tar.gz" };
    Some(format!("your-own-ai-build-{}-{}.{}", version(), label, ext))
}

fn binary_name() -> &'static str {
    if std::env::consts::OS == "windows" {
        "your-own-ai-build.exe"
    } else {
        "your-own-ai-build"
    }
}

/// Where the installed agent lives: `<app-data>/build/<binary>`.
fn install_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| format!("no app data dir: {}", e))?
        .join("build"))
}

pub fn installed_path(app: &AppHandle) -> Option<String> {
    let path = install_dir(app).ok()?.join(binary_name());
    path.is_file().then(|| path.display().to_string())
}

/// The installed agent's version, from the VERSION marker written at
/// install time. A binary with no marker predates versioning and can only
/// be v0.1.0 (the sole release shipped before the marker existed).
pub fn installed_version(app: &AppHandle) -> Option<String> {
    installed_path(app)?;
    let marker = install_dir(app).ok()?.join("VERSION");
    Some(
        std::fs::read_to_string(marker)
            .map(|s| s.trim().to_string())
            .unwrap_or_else(|_| "0.1.0".to_string()),
    )
}

#[derive(serde::Serialize)]
pub struct BuildInstallStatus {
    pub installed: bool,
    pub path: Option<String>,
    /// Version of the binary on disk (None when not installed).
    pub installed_version: Option<String>,
    /// The version this app build installs (BUILD_RELEASE_TAG).
    pub pinned_version: String,
    /// Installed, but an older version than this app ships - the UI must
    /// offer an UPDATE, never the first-install pitch.
    pub update_available: bool,
    pub downloading: bool,
    /// The archive name progress events are keyed by.
    pub archive: Option<String>,
    pub error: Option<String>,
}

#[tauri::command]
pub fn build_install_status(app: AppHandle) -> BuildInstallStatus {
    let path = installed_path(&app);
    let installed_version = installed_version(&app);
    let update_available = matches!(&installed_version, Some(v) if v != version());
    BuildInstallStatus {
        installed: path.is_some(),
        path,
        installed_version,
        pinned_version: version().to_string(),
        update_available,
        downloading: DOWNLOADING.load(Ordering::SeqCst),
        archive: platform_asset(),
        error: LAST_ERROR.lock().ok().and_then(|e| e.clone()),
    }
}

/// Download + install the agent. Runs to completion in the Rust runtime -
/// the caller may navigate away; progress rides `model-download-progress`
/// (keyed by the archive name) and completion emits `build-install-done`
/// with the installed path.
#[tauri::command]
pub async fn download_build_agent(
    app: AppHandle,
    bridge: tauri::State<'_, crate::agent_bridge::AgentBridgeState>,
) -> Result<String, String> {
    // Updating replaces the binary, which fails on Windows while the agent
    // runs (the exe is locked) and would orphan a live session anywhere -
    // same guard as uninstall, applied only when a binary already exists
    // (a first install has nothing running).
    if installed_path(&app).is_some() && bridge.has_open_folder().await {
        return Err("Close the open project first, then update.".to_string());
    }
    if DOWNLOADING.swap(true, Ordering::SeqCst) {
        return Err("Already downloading".to_string());
    }
    if let Ok(mut e) = LAST_ERROR.lock() {
        *e = None;
    }
    let result = download_and_install(&app).await;
    DOWNLOADING.store(false, Ordering::SeqCst);
    match &result {
        Ok(path) => {
            let _ = app.emit("build-install-done", serde_json::json!({ "path": path }));
        }
        Err(e) => {
            if let Ok(mut slot) = LAST_ERROR.lock() {
                *slot = Some(e.clone());
            }
            let _ = app.emit("build-install-failed", serde_json::json!({ "error": e }));
        }
    }
    result
}

async fn download_and_install(app: &AppHandle) -> Result<String, String> {
    let archive = platform_asset().ok_or("No build is available for this platform yet")?;
    let url = format!(
        "https://github.com/{}/releases/download/{}/{}",
        BUILD_REPO, BUILD_RELEASE_TAG, archive
    );
    crate::llm::download_model(app.clone(), url, archive.clone()).await?;

    let archive_path = crate::llm::get_models_dir(app)?.join(&archive);
    let final_dir = install_dir(app)?;
    let partial = final_dir.with_extension("partial");
    let _ = std::fs::remove_dir_all(&partial);
    std::fs::create_dir_all(&partial).map_err(|e| format!("cannot create install dir: {}", e))?;

    let extract_result = extract_archive(&archive_path, &partial);
    // The archive is an installer artifact - clean it up either way.
    let _ = std::fs::remove_file(&archive_path);
    extract_result?;

    let binary = partial.join(binary_name());
    if !binary.is_file() {
        let _ = std::fs::remove_dir_all(&partial);
        return Err("archive did not contain the agent".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("cannot mark executable: {}", e))?;
    }
    let _ = std::fs::remove_dir_all(&final_dir);
    std::fs::rename(&partial, &final_dir).map_err(|e| format!("cannot finalize install: {}", e))?;
    // Version marker - what update detection reads. A failed write is
    // tolerated (the install still works); the version then reads as the
    // pre-marker 0.1.0 and the next update offer simply re-downloads.
    if let Err(e) = std::fs::write(final_dir.join("VERSION"), version()) {
        log::warn!("[build] could not write version marker: {}", e);
    }
    let path = final_dir.join(binary_name()).display().to_string();
    log::info!("[build] installed Your Own AI Build {} at {}", version(), path);
    Ok(path)
}

/// Flatten-extract: entries land by BASENAME (the archives wrap everything
/// in a versioned directory; basenames avoid both nesting and zip-slip).
fn extract_archive(archive: &std::path::Path, dest: &std::path::Path) -> Result<(), String> {
    if archive.extension().and_then(|e| e.to_str()) == Some("zip") {
        let file = std::fs::File::open(archive).map_err(|e| format!("archive missing: {}", e))?;
        let mut zip = zip::ZipArchive::new(file).map_err(|e| format!("bad archive: {}", e))?;
        for i in 0..zip.len() {
            let mut entry = zip.by_index(i).map_err(|e| format!("bad entry: {}", e))?;
            if entry.is_dir() {
                continue;
            }
            let Some(name) = std::path::Path::new(entry.name())
                .file_name()
                .and_then(|n| n.to_str())
                .map(String::from)
            else {
                continue;
            };
            let mut out = std::fs::File::create(dest.join(&name))
                .map_err(|e| format!("cannot write {}: {}", name, e))?;
            std::io::copy(&mut entry, &mut out).map_err(|e| format!("extract failed: {}", e))?;
        }
        Ok(())
    } else {
        // tar.gz - tar ships on every macOS/Linux this app supports.
        let status = std::process::Command::new("tar")
            .arg("-xzf")
            .arg(archive)
            .arg("-C")
            .arg(dest)
            .arg("--strip-components=1")
            .status()
            .map_err(|e| format!("tar failed to start: {}", e))?;
        if !status.success() {
            return Err(format!("tar exited with {}", status));
        }
        Ok(())
    }
}

/// Remove the installed agent. Refused while a project is open (closing it
/// is one click; yanking the binary from a live session is not).
#[tauri::command]
pub async fn uninstall_build_agent(
    app: AppHandle,
    bridge: tauri::State<'_, crate::agent_bridge::AgentBridgeState>,
) -> Result<(), String> {
    if bridge.has_open_folder().await {
        return Err("Close the open project first, then uninstall.".to_string());
    }
    if DOWNLOADING.load(Ordering::SeqCst) {
        return Err("The download is still running - let it finish first.".to_string());
    }
    let dir = install_dir(&app)?;
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| format!("could not remove: {}", e))?;
    }
    let _ = app.emit("build-uninstalled", serde_json::json!({}));
    log::info!("[build] uninstalled Your Own AI Build");
    Ok(())
}
