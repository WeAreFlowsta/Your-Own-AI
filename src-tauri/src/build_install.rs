//! Your Own AI Build - the Projects helper, one of the app's parts.
//!
//! The agent is downloaded from its GitHub release: by itself once the
//! person has a model (`ensure_build_agent`, asked by the page's parts job),
//! replaced by itself when this app pins another version, and by a button
//! as before. Someone who removed it is not given it again uninvited.
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
const BUILD_RELEASE_TAG: &str = "v0.4.4";
/// The SHA-256 of each platform's archive at that release, by its label.
/// The download is checked against it before anything is unpacked, so a
/// file that is not the one this app was built for is never installed.
/// They move with the tag.
const BUILD_ARCHIVE_SHA256: &[(&str, &str)] = &[
    ("linux-x86_64", "4b1f7230e59fb85d8a288ae19ff01ada9ce7a8dad28f54729a091013f7a56ec8"),
    ("macos-arm64", "72a83bc262463c1a41aafbf56caefe358798bf774081a87adce1c7b86945f887"),
    ("macos-x86_64", "de8a7d7be96613990beaa92f84f2b5c386a561fa27a81c898d1590a9289491fd"),
    ("windows-x86_64", "c3e0bca601a57c32c748007e7405f21df366557cca7b35bdf24a8953f23cb37b"),
];
/// Whether the pinned release's Windows and macOS builds are signed. It
/// moves with the tag. Until it is true, the app does not install the
/// helper by itself for the first time on those two systems (a program
/// fetched and started for everyone should be a signed one); a helper the
/// person installed is still replaced by itself, and the buttons work.
/// v0.4.4 is the first signed release (Authenticode; Developer ID with
/// notarization), checked in its build's log.
const BUILD_IS_SIGNED: bool = true;

/// May the app install the helper for the first time with nobody asking?
fn may_install_first(os: &str, signed: bool) -> bool {
    signed || !matches!(os, "windows" | "macos")
}

const BUILD_REPO: &str = "WeAreFlowsta/Your-Own-AI-Build";

static DOWNLOADING: AtomicBool = AtomicBool::new(false);
static LAST_ERROR: Mutex<Option<String>> = Mutex::new(None);

pub(crate) fn pinned_version() -> &'static str {
    version()
}

fn version() -> &'static str {
    BUILD_RELEASE_TAG.trim_start_matches('v')
}

/// This platform's release asset (archive filename), or None when the
/// platform has no build.
fn platform_label() -> Option<&'static str> {
    Some(match (std::env::consts::OS, std::env::consts::ARCH) {
        ("linux", "x86_64") => "linux-x86_64",
        ("macos", "aarch64") => "macos-arm64",
        ("macos", "x86_64") => "macos-x86_64",
        ("windows", "x86_64") => "windows-x86_64",
        _ => return None,
    })
}

/// The pinned SHA-256 for a platform label.
fn pinned_sha256(label: &str) -> Option<&'static str> {
    BUILD_ARCHIVE_SHA256.iter().find(|(l, _)| *l == label).map(|(_, h)| *h)
}

/// Whether the downloaded archive is the pinned one.
fn check_archive(path: &std::path::Path, expected: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).map_err(|e| format!("archive missing: {}", e))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| format!("cannot read the download: {}", e))?;
    let got = hex::encode(hasher.finalize());
    if got == expected {
        Ok(())
    } else {
        log::warn!("[build] the downloaded archive's SHA-256 is {got}, the pinned one is {expected}");
        Err("The download is not the file this version of Your Own AI installs, so nothing was installed. Try again.".to_string())
    }
}

fn platform_asset() -> Option<String> {
    asset_for(version())
}

fn binary_name() -> &'static str {
    if std::env::consts::OS == "windows" {
        "your-own-ai-build.exe"
    } else {
        "your-own-ai-build"
    }
}

/// The note that the person removed the helper: `<app-data>/build-removed`.
/// While it is there the helper is fetched only when they ask for it.
fn removed_marker(app: &AppHandle) -> Option<std::path::PathBuf> {
    Some(app.path().app_data_dir().ok()?.join("build-removed"))
}

fn was_removed(app: &AppHandle) -> bool {
    removed_marker(app).map(|p| p.exists()).unwrap_or(false)
}

/// What the background check does about the helper.
#[derive(Debug, PartialEq, Clone, Copy)]
enum Ensure {
    /// No build exists for this platform.
    Unavailable,
    /// The person removed it: it comes back only when they ask.
    Removed,
    /// The pinned version is installed.
    Current,
    /// A download is running, or an agent is running on the program file.
    Busy,
    /// Missing, or another version than this app pins: fetch it.
    Fetch,
    /// Missing, and a first install by itself waits for a signed build on
    /// this system (`BUILD_IS_SIGNED`).
    NotYet,
}

fn what_to_do(has_build: bool, removed: bool, installed: Option<&str>, pin: &str, agent_running: bool, downloading: bool, first_allowed: bool) -> Ensure {
    if !has_build {
        Ensure::Unavailable
    } else if removed {
        Ensure::Removed
    } else if installed == Some(pin) {
        Ensure::Current
    } else if downloading || (installed.is_some() && agent_running) {
        Ensure::Busy
    } else if installed.is_none() && !first_allowed {
        Ensure::NotYet
    } else {
        Ensure::Fetch
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
    /// The person removed it; it is not fetched again uninvited.
    pub removed: bool,
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
        removed: was_removed(&app),
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
    // Asked for: a removal no longer stands.
    if let Some(m) = removed_marker(&app) {
        let _ = std::fs::remove_file(m);
    }
    let result = download_and_install(&app, false).await;
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

/// The background check: fetch the helper when it is missing or is not the
/// version this app pins, unless the person removed it, a download is
/// running, or an agent is running on it (then the next check does it).
/// Quiet: a failure is logged and tried again at a later check, with no
/// error shown, since nobody pressed anything. Returns what happened:
/// `current`, `removed`, `busy`, `unavailable`, `off`, `not-yet`, `installed`,
/// `updated`.
#[tauri::command]
pub async fn ensure_build_agent(
    app: AppHandle,
    bridge: tauri::State<'_, crate::agent_bridge::AgentBridgeState>,
) -> Result<String, String> {
    // An install with downloads switched off fetches nothing by itself.
    if crate::llm::downloads_blocked() {
        return Ok("off".to_string());
    }
    let installed = installed_version(&app);
    let decision = what_to_do(
        platform_asset().is_some(),
        was_removed(&app),
        installed.as_deref(),
        version(),
        bridge.agent_running().await,
        DOWNLOADING.load(Ordering::SeqCst),
        may_install_first(std::env::consts::OS, BUILD_IS_SIGNED),
    );
    let name = match decision {
        Ensure::Unavailable => "unavailable",
        Ensure::Removed => "removed",
        Ensure::Current => "current",
        Ensure::Busy => "busy",
        Ensure::NotYet => "not-yet",
        Ensure::Fetch => "",
    };
    if decision != Ensure::Fetch {
        return Ok(name.to_string());
    }
    if DOWNLOADING.swap(true, Ordering::SeqCst) {
        return Ok("busy".to_string());
    }
    let updated = installed.is_some();
    log::info!(
        "[build] fetching Your Own AI Build {} in the background ({})",
        version(),
        match &installed { Some(v) => format!("replacing {v}"), None => "not installed yet".to_string() }
    );
    let result = download_and_install(&app, true).await;
    DOWNLOADING.store(false, Ordering::SeqCst);
    match result {
        Ok(path) => {
            let _ = app.emit("build-install-done", serde_json::json!({ "path": path, "background": true, "updated": updated }));
            Ok(if updated { "updated" } else { "installed" }.to_string())
        }
        Err(e) => {
            log::warn!("[build] the background fetch did not finish (tried again at a later check): {}", e);
            // The tray's row for it goes; nothing is said.
            let _ = app.emit("build-install-put-off", serde_json::json!({}));
            Err(e)
        }
    }
}

/// `quiet`: a background fetch, whose failure raises no error row.
async fn download_and_install(app: &AppHandle, quiet: bool) -> Result<String, String> {
    let expected = platform_label().and_then(pinned_sha256).ok_or("No build is available for this platform yet")?;
    let final_dir = install_dir(app)?;
    let partial = final_dir.with_extension("partial");
    fetch_and_unpack(app, version(), expected, &partial, quiet).await?;
    let _ = std::fs::remove_dir_all(&final_dir);
    std::fs::rename(&partial, &final_dir).map_err(|e| format!("cannot finalize install: {}", e))?;
    let path = final_dir.join(binary_name()).display().to_string();
    log::info!("[build] installed Your Own AI Build {} at {}", version(), path);
    Ok(path)
}

/// This platform's archive name at a version.
fn asset_for(version: &str) -> Option<String> {
    let label = platform_label()?;
    let ext = if std::env::consts::OS == "windows" { "zip" } else { "tar.gz" };
    Some(format!("your-own-ai-build-{}-{}.{}", version, label, ext))
}

/// Download a version's archive, check it against `expected` (its SHA-256)
/// and unpack it into `dest` with its VERSION marker. `dest` is left absent
/// on any failure.
async fn fetch_and_unpack(app: &AppHandle, version: &str, expected: &str, dest: &std::path::Path, quiet: bool) -> Result<(), String> {
    let archive = asset_for(version).ok_or("No build is available for this platform yet")?;
    let url = format!("https://github.com/{}/releases/download/v{}/{}", BUILD_REPO, version, archive);
    if quiet {
        crate::llm::download_model_inner(app.clone(), url, archive.clone()).await?;
    } else {
        crate::llm::download_model(app.clone(), url, archive.clone(), None).await?;
    }
    let archive_path = crate::llm::get_models_dir(app)?.join(&archive);
    let _ = std::fs::remove_dir_all(dest);
    std::fs::create_dir_all(dest).map_err(|e| format!("cannot create install dir: {}", e))?;
    if let Err(e) = check_archive(&archive_path, expected) {
        let _ = std::fs::remove_file(&archive_path);
        let _ = std::fs::remove_dir_all(dest);
        return Err(e);
    }
    let extract_result = extract_archive(&archive_path, dest);
    // The archive is an installer artifact - clean it up either way.
    let _ = std::fs::remove_file(&archive_path);
    if let Err(e) = extract_result {
        let _ = std::fs::remove_dir_all(dest);
        return Err(e);
    }
    let binary = dest.join(binary_name());
    if !binary.is_file() {
        let _ = std::fs::remove_dir_all(dest);
        return Err("archive did not contain the agent".to_string());
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("cannot mark executable: {}", e))?;
    }
    // Version marker - what update detection reads. A failed write is
    // tolerated (the install still works); the version then reads as the
    // pre-marker 0.1.0 and the next check simply fetches it again.
    if let Err(e) = std::fs::write(dest.join("VERSION"), version) {
        log::warn!("[build] could not write version marker: {}", e);
    }
    Ok(())
}

// --- Fetched ahead of an update ------------------------------------------
//
// A release's signed manifest names the helper the new version needs
// (`parts`). The version doing the updating fetches it before it offers the
// restart and keeps it beside the installed one (`build.next`); the new
// version puts it in place at its first start, before any project opens.

/// The part's name in a release manifest.
pub(crate) const PART_NAME: &str = "projects-helper";

fn staged_dir(app: &AppHandle) -> Result<std::path::PathBuf, String> {
    Ok(install_dir(app)?.with_extension("next"))
}

fn staged_version(app: &AppHandle) -> Option<String> {
    let dir = staged_dir(app).ok()?;
    dir.join(binary_name()).is_file().then(|| std::fs::read_to_string(dir.join("VERSION")).ok().map(|s| s.trim().to_string())).flatten()
}

/// Should this install fetch the named helper ahead of the update? Only
/// for someone who has the helper and did not remove it, when it is another
/// version than the installed one and not staged already.
pub(crate) fn wants_ahead(app: &AppHandle, target: &str, version: &str) -> bool {
    platform_label() == Some(target)
        && !was_removed(app)
        && matches!(installed_version(app), Some(v) if v != version)
        && staged_version(app).as_deref() != Some(version)
}

/// Fetch the named helper version, check it against the hash the signed
/// manifest gives, and keep it for the new version. Quiet.
pub(crate) async fn fetch_ahead(app: &AppHandle, version: &str, sha256: &str) -> Result<(), String> {
    if DOWNLOADING.swap(true, Ordering::SeqCst) {
        return Err("a helper download is already running".to_string());
    }
    let result = async {
        let staged = staged_dir(app)?;
        let partial = staged.with_extension("next-partial");
        fetch_and_unpack(app, version, sha256, &partial, true).await?;
        let _ = std::fs::remove_dir_all(&staged);
        std::fs::rename(&partial, &staged).map_err(|e| format!("cannot keep the fetched helper: {e}"))
    }
    .await;
    DOWNLOADING.store(false, Ordering::SeqCst);
    if result.is_ok() {
        log::info!("[build] Your Own AI Build {} is in place for the update", version);
    }
    result
}

/// At start, before any project can open: a helper fetched ahead by the
/// version before this one, at the version this one pins, goes into place.
/// One for any other version is of no use and goes. Returns whether the
/// helper was replaced.
pub(crate) fn adopt_fetched_ahead(app: &AppHandle) -> bool {
    let (Ok(staged), Ok(final_dir)) = (staged_dir(app), install_dir(app)) else { return false };
    if !staged.exists() {
        return false;
    }
    if staged_version(app).as_deref() != Some(version()) || was_removed(app) {
        let _ = std::fs::remove_dir_all(&staged);
        return false;
    }
    let _ = std::fs::remove_dir_all(&final_dir);
    match std::fs::rename(&staged, &final_dir) {
        Ok(()) => {
            log::info!("[build] Your Own AI Build {} was fetched before the update: in place", version());
            true
        }
        Err(e) => {
            log::warn!("[build] the helper fetched before the update could not be put in place ({e}); the next check fetches it");
            false
        }
    }
}

/// The pinned helper as release manifests name it: (target, version,
/// sha256) for each platform with a build. `parts.json` must say the same.
#[cfg(test)]
pub(crate) fn pinned_parts() -> Vec<(String, String, String)> {
    BUILD_ARCHIVE_SHA256.iter().map(|(l, h)| (l.to_string(), version().to_string(), h.to_string())).collect()
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
            let Some(name) = entry
                .name()
                .ok()
                .and_then(|n| std::path::Path::new(n.as_ref()).file_name().and_then(|f| f.to_str()).map(String::from))
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
    // Remembered, so the background check does not bring it back.
    if let Some(m) = removed_marker(&app) {
        if let Err(e) = std::fs::write(&m, b"") {
            log::warn!("[build] could not note the removal: {}", e);
        }
    }
    let _ = app.emit("build-uninstalled", serde_json::json!({}));
    log::info!("[build] uninstalled Your Own AI Build");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_background_check_fetches_only_what_is_missing_or_another_version() {
        let pin = "0.4.3";
        // A new person, or an existing one who never had it.
        assert_eq!(what_to_do(true, false, None, pin, false, false, true), Ensure::Fetch);
        // Another version than the pin, older or newer: the pinned one.
        assert_eq!(what_to_do(true, false, Some("0.4.2"), pin, false, false, true), Ensure::Fetch);
        assert_eq!(what_to_do(true, false, Some("0.5.0"), pin, false, false, true), Ensure::Fetch);
        assert_eq!(what_to_do(true, false, Some(pin), pin, true, false, true), Ensure::Current);
        // Removed by the person: never uninvited, whatever else is true.
        assert_eq!(what_to_do(true, true, None, pin, false, false, true), Ensure::Removed);
        assert_eq!(what_to_do(true, true, Some("0.4.2"), pin, false, false, true), Ensure::Removed);
        // An agent running on the old program file holds the replacement;
        // a first install has nothing running on it.
        assert_eq!(what_to_do(true, false, Some("0.4.2"), pin, true, false, true), Ensure::Busy);
        assert_eq!(what_to_do(true, false, None, pin, true, false, true), Ensure::Fetch);
        assert_eq!(what_to_do(true, false, None, pin, false, true, true), Ensure::Busy);
        assert_eq!(what_to_do(false, false, None, pin, false, false, true), Ensure::Unavailable);
        // No signed build for this system yet: nothing is installed for the
        // first time by itself, but one the person installed is replaced.
        assert_eq!(what_to_do(true, false, None, pin, false, false, false), Ensure::NotYet);
        assert_eq!(what_to_do(true, false, Some("0.4.2"), pin, false, false, false), Ensure::Fetch);
        assert!(may_install_first("linux", false));
        assert!(!may_install_first("windows", false) && !may_install_first("macos", false));
        assert!(may_install_first("windows", true) && may_install_first("macos", true));
    }

    #[test]
    fn every_platform_with_a_build_has_a_pinned_fingerprint() {
        for label in ["linux-x86_64", "macos-arm64", "macos-x86_64", "windows-x86_64"] {
            let h = pinned_sha256(label).unwrap_or_else(|| panic!("no SHA-256 for {label}"));
            assert!(h.len() == 64 && h.chars().all(|c| c.is_ascii_hexdigit() && !c.is_ascii_uppercase()));
        }
        assert!(pinned_sha256("plan9-mips").is_none());
    }

    #[test]
    fn an_archive_is_installed_only_when_it_is_the_pinned_one() {
        let path = std::env::temp_dir().join(format!("yoai-build-archive-{}", std::process::id()));
        std::fs::write(&path, b"abc").unwrap();
        // SHA-256 of "abc".
        assert!(check_archive(&path, "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad").is_ok());
        let e = check_archive(&path, pinned_sha256("linux-x86_64").unwrap()).unwrap_err();
        assert!(e.contains("nothing was installed"));
        let _ = std::fs::remove_file(&path);
    }
}
