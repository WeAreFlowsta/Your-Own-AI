//! Your Own AI's own updates: checked on start and every few hours, downloaded
//! in the background, installed at "Restart to update" or on quit.
//!
//! Two signatures stand behind an update. Tauri's updater signature on the
//! file (the release build's key) is checked by the plugin as it downloads.
//! The release manifest, signed by Team Flowsta in the Vault, names the
//! version, the channel and every file's SHA-256, and is checked here with
//! `flowsta-update-check` before anything is downloaded; the download is
//! then checked against it. Both checks are local. The requests are plain
//! GETs of two public files and carry nothing about this install.

use flowsta_update_check::{check, check_file, compare_versions, Installed, Refused, Release, Trust};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Mutex;
use std::time::Duration;
use tauri::{Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

const APP_ID: &str = "com.solar.yourowai";
/// The identities whose signature on a release manifest this build accepts:
/// Team Flowsta, as for the node manifests.
const SIGNERS: &[&str] = &["did:flowsta:uhCAkWuA-Ehm9EVzcsPE2Wo_vpc9ImljHN8NMMEXpbtG5BdooOV9a"];
const RELEASES: &str = "https://github.com/WeAreFlowsta/Your-Own-AI/releases";
const FIRST_CHECK_AFTER: Duration = Duration::from_secs(90);
const CHECK_EVERY: Duration = Duration::from_secs(4 * 60 * 60);

/// The channels this copy follows, decided by what was installed, not by a
/// setting: a beta (a pre-release version, installed by hand) follows the
/// betas and takes the stable release when it comes; a stable copy sees
/// stable releases only.
fn channels_for(version: &str) -> &'static [&'static str] {
    if version.contains('-') {
        &["beta", "stable"]
    } else {
        &["stable"]
    }
}

/// Where a channel's two files live: stable, the latest release's assets;
/// beta, one fixed pre-release whose two files each beta replaces.
fn channel_base(channel: &str) -> String {
    if channel == "beta" {
        format!("{RELEASES}/download/yoai-beta")
    } else {
        format!("{RELEASES}/latest/download")
    }
}

pub(crate) fn latest_json_url(channel: &str) -> String {
    format!("{}/latest.json", channel_base(channel))
}

pub(crate) fn manifest_url(channel: &str) -> String {
    format!("{}/release-manifest.json", channel_base(channel))
}

/// This build's platform, in the Tauri updater's names.
fn platform() -> Option<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Some("darwin-aarch64"),
        ("macos", "x86_64") => Some("darwin-x86_64"),
        ("windows", "x86_64") => Some("windows-x86_64"),
        ("linux", "x86_64") => Some("linux-x86_64"),
        _ => None,
    }
}

/// The package this copy was installed from: an update replaces it with the
/// same kind. None for a copy run from a build folder (no updates).
fn kind() -> Option<&'static str> {
    use tauri::utils::config::BundleType as B;
    match tauri::utils::platform::bundle_type()? {
        B::App => Some("app-tarball"),
        B::Nsis => Some("nsis"),
        B::Msi => Some("msi"),
        B::Deb => Some("deb"),
        B::Rpm => Some("rpm"),
        _ => None,
    }
}

/// What the updater keeps on disk: the person's choice, the newest
/// manifest sequence accepted per channel (no replay of an older one), the
/// rollout ticket (a number made once, never sent), and the version this
/// copy was updated from (said once after the restart).
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
struct Saved {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    auto: Option<bool>,
    #[serde(default)]
    last_sequence: BTreeMap<String, u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    rollout_ticket: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    updated_from: Option<UpdatedFrom>,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct UpdatedFrom {
    pub version: String,
    pub to: String,
    pub notes_url: String,
}

impl Saved {
    fn auto(&self) -> bool {
        self.auto.unwrap_or(true)
    }
}

/// The updater's state: one per install, in the device's own folder (every
/// profile on it is the same installed app).
fn updates_path(device_root: &Path) -> std::path::PathBuf {
    device_root.join("updates.json")
}

fn load(data_dir: &Path) -> Saved {
    std::fs::read(updates_path(data_dir)).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn save(data_dir: &Path, saved: &Saved) {
    let path = updates_path(data_dir);
    match serde_json::to_vec_pretty(saved) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(&path, bytes) {
                log::warn!("[update] {} not written: {}", path.display(), e);
            }
        }
        Err(e) => log::warn!("[update] state not serialized: {}", e),
    }
}

/// The rollout ticket, made on first use and kept.
fn ticket(data_dir: &Path, saved: &mut Saved) -> u8 {
    if let Some(t) = saved.rollout_ticket {
        return t;
    }
    let t = rand::random::<u8>() % 100;
    saved.rollout_ticket = Some(t);
    save(data_dir, saved);
    t
}

/// An update downloaded and checked, waiting for "Restart to update" or quit.
struct Ready {
    update: tauri_plugin_updater::Update,
    bytes: Vec<u8>,
    version: String,
    notes_url: String,
    changes_line: bool,
    channel: String,
    sequence: u64,
}

static READY: Mutex<Option<Ready>> = Mutex::new(None);
/// Something the person should know that the updater cannot do for them
/// (install another version first, an MSI install).
static NOTICE: Mutex<Option<String>> = Mutex::new(None);

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct UpdateStatus {
    pub auto: bool,
    /// The channel this copy follows first (beta for a beta install).
    pub channel: String,
    pub version: String,
    /// A downloaded, checked update waiting for a restart.
    pub ready: Option<ReadyInfo>,
    pub notice: Option<String>,
    /// Set once after an update: what this copy was updated from.
    pub updated: Option<UpdatedFrom>,
    /// Whether this copy can update itself (installed from a package).
    pub can_update: bool,
}

#[derive(Clone, Debug, Serialize, PartialEq)]
pub struct ReadyInfo {
    pub version: String,
    pub notes_url: String,
    pub changes_line: bool,
    /// Whether installing it asks for the computer's password (Linux).
    pub asks_password: bool,
}

impl ReadyInfo {
    fn of(r: &Ready) -> Self {
        ReadyInfo { version: r.version.clone(), notes_url: r.notes_url.clone(), changes_line: r.changes_line, asks_password: INSTALLS_FIRST }
    }
}

/// Linux installs a waiting update before the conductors stop: its password
/// prompt can be cancelled, and a cancel then leaves Your Own AI as it was.
/// Windows cannot replace files in use, so there (and on macOS, where
/// nothing asks) the conductors stop first.
const INSTALLS_FIRST: bool = cfg!(target_os = "linux");

/// A channel's release manifest; None when there is none yet, or offline.
async fn fetch_manifest(client: &reqwest::Client, channel: &str) -> Option<Vec<u8>> {
    match client.get(manifest_url(channel)).send().await {
        Ok(r) if r.status().is_success() => match r.bytes().await {
            Ok(b) => Some(b.to_vec()),
            Err(e) => {
                log::info!("[update] the {} manifest was not read: {}", channel, e);
                None
            }
        },
        Ok(r) => {
            log::debug!("[update] no {} manifest ({})", channel, r.status());
            None
        }
        Err(e) => {
            log::debug!("[update] the {} manifest was not fetched: {}", channel, e);
            None
        }
    }
}

/// Check once: the manifests, then (only if one passes) the download. Says
/// what it found in one line, so every check shows in the log (field logs
/// could not tell a quiet updater from one that never ran): Ok for the
/// info level, Err for warn (it found something that did not verify).
async fn check_once(app: &tauri::AppHandle, data_dir: &Path) -> Result<String, String> {
    let mut saved = load(data_dir);
    if !saved.auto() {
        return Ok("automatic updates are off".into());
    }
    if READY.lock().unwrap().is_some() {
        return Ok("an update is already waiting to install".into());
    }
    let Some(platform) = platform() else {
        return Ok("not checked: there are no release packages for this platform".into());
    };
    let Some(kind) = kind() else {
        return Ok("not checked: this copy was not installed from a release package".into());
    };
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| format!("not checked: no HTTP client: {}", e))?;
    let rollout_ticket = ticket(data_dir, &mut saved);
    let version = env!("CARGO_PKG_VERSION");
    let channels = channels_for(version);
    // The newest release any of this copy's channels offers.
    let mut best: Option<(&str, Release)> = None;
    let (mut read, mut verified, mut rolling_out) = (0, 0, false);
    let mut install_first = None;
    for &channel in channels {
        let Some(manifest) = fetch_manifest(&client, channel).await else {
            continue;
        };
        read += 1;
        let installed = Installed { version, last_sequence: saved.last_sequence.get(channel).copied(), rollout_ticket };
        match check(&manifest, &Trust { app: APP_ID, channel, signers: SIGNERS }, &installed) {
            Ok(r) => {
                verified += 1;
                if best.as_ref().map_or(true, |(_, b)| compare_versions(&r.version, &b.version) == std::cmp::Ordering::Greater) {
                    best = Some((channel, r));
                }
            }
            Err(Refused::NotNewer { .. }) | Err(Refused::Replayed { .. }) => verified += 1,
            Err(Refused::NotYet { .. }) => {
                verified += 1;
                rolling_out = true;
            }
            Err(Refused::InstallFirst(first)) => {
                verified += 1;
                install_first = Some(first.clone());
                *NOTICE.lock().unwrap() = Some(format!("A newer Your Own AI is out, and it updates from {first} or later. Install {first} from the Your Own AI releases page first; this copy then updates itself again."));
            }
            Err(e) => log::warn!("[update] the {} manifest was refused: {}", channel, e),
        }
    }
    let Some((channel, release)) = best else {
        let on = channels.join(", ");
        return if let Some(first) = install_first {
            Ok(format!("a newer release updates only from {} or later: install that one first", first))
        } else if read == 0 {
            Ok(format!("no release manifest could be read ({}): offline, or none published yet", on))
        } else if verified == 0 {
            Err(format!("no release manifest verified ({}): nothing to install", on))
        } else if rolling_out {
            Ok(format!("up to date for now ({}; {}): a newer release is rolling out and reaches this copy later", version, on))
        } else {
            Ok(format!("up to date ({}; {})", version, on))
        };
    };
    let channel = channel.to_string();
    let Some(file) = release.file(platform, kind).cloned() else {
        return Ok(format!("{} {} has no {} for {}", APP_ID, release.version, kind, platform));
    };
    if kind == "msi" {
        // An MSI install is managed by whoever deployed it: say so, never replace it.
        *NOTICE.lock().unwrap() = Some(format!("Your Own AI {} is out. This copy was installed with the Windows Installer (MSI), so it does not update itself: get it from the Your Own AI releases page.", release.version));
        return Ok(format!("{} is out; an MSI install does not update itself", release.version));
    }
    let endpoint = url::Url::parse(&latest_json_url(&channel)).map_err(|e| format!("bad endpoint: {}", e))?;
    let updater = app
        .updater_builder()
        .endpoints(vec![endpoint])
        .and_then(|b| b.build())
        .map_err(|e| format!("the updater did not start: {}", e))?;
    let update = match updater.check().await {
        Ok(Some(u)) => u,
        Ok(None) => return Err(format!("the signed manifest offers {} but the {} version file nothing newer", release.version, channel)),
        Err(e) => return Ok(format!("the {} version file was not read: {}", channel, e)),
    };
    if update.version != release.version {
        return Err(format!("the version file names {} but the signed manifest {}: not downloaded", update.version, release.version));
    }
    log::info!("[update] downloading {} {} ({} bytes)", APP_ID, release.version, file.size);
    let bytes = update
        .download(|_, _| {}, || {})
        .await
        .map_err(|e| format!("the download failed or its updater signature did not verify: {}", e))?;
    check_file(&file, &bytes).map_err(|e| format!("the download is not the file the manifest names: {}", e))?;
    let said = format!("{} ready: installs at \"Restart to update\"{}", release.version, if INSTALLS_FIRST { "" } else { " or on quit" });
    let ready = Ready {
        update,
        bytes,
        version: release.version,
        notes_url: release.notes_url,
        changes_line: release.changes_line,
        channel,
        sequence: release.sequence,
    };
    let info = ReadyInfo::of(&ready);
    *READY.lock().unwrap() = Some(ready);
    let _ = app.emit("update-ready", info);
    Ok(said)
}

/// Check on start and every few hours, while this run of Your Own AI lasts.
pub fn spawn(app: tauri::AppHandle, data_dir: std::path::PathBuf) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK_AFTER).await;
        loop {
            match check_once(&app, &data_dir).await {
                Ok(said) => log::info!("[update] checked: {}", said),
                Err(said) => log::warn!("[update] checked: {}", said),
            }
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

/// Install the waiting update; returns whether one was installed. The
/// sequence and the "updated from" line are kept first, and put back with
/// the update if the install fails, so the same update is offered again.
/// The conductors stop cleanly (which protects the databases) before the
/// install or, on Linux, after it (`INSTALLS_FIRST`). `stage` hears each
/// step as it starts: "password" or "installing", then "restarting".
pub fn install_ready(app: &tauri::AppHandle, data_dir: &Path, stage: &dyn Fn(&str)) -> Result<bool, String> {
    let Some(ready) = READY.lock().unwrap().take() else {
        return Ok(false);
    };
    let before = load(data_dir);
    let mut saved = before.clone();
    saved.last_sequence.insert(ready.channel.clone(), ready.sequence);
    saved.updated_from = Some(UpdatedFrom { version: env!("CARGO_PKG_VERSION").into(), to: ready.version.clone(), notes_url: ready.notes_url.clone() });
    save(data_dir, &saved);
    if INSTALLS_FIRST {
        stage("password");
    } else {
        stage("installing");
        stop_conductors(app);
    }
    log::info!("[update] installing {}{}", ready.version, if ready.changes_line { " (it moves the data to another Holochain line on its first start)" } else { "" });
    let installed = ready.update.install(&ready.bytes);
    if let Err(e) = installed {
        save(data_dir, &before);
        *READY.lock().unwrap() = Some(ready);
        return Err(format!("the update did not install: {e}"));
    }
    stage("restarting");
    if INSTALLS_FIRST {
        stop_conductors(app);
    }
    Ok(true)
}

/// Everything the app runs stops: the engines and agents, then the
/// conductors (each asked and given its grace).
fn stop_conductors(app: &tauri::AppHandle) {
    crate::stop_everything(app);
}

/// On quit: install a waiting update where that needs no one (Windows,
/// macOS). Linux asks for a password, so it installs at the button or the
/// next start's prompt instead.
pub fn install_on_quit(app: &tauri::AppHandle, data_dir: &Path) {
    if cfg!(target_os = "linux") {
        return;
    }
    if let Err(e) = install_ready(app, data_dir, &|_| {}) {
        log::warn!("[update] {}", e);
    }
}

fn status_of(data_dir: &Path) -> UpdateStatus {
    let saved = load(data_dir);
    let updated = saved.updated_from.clone().filter(|u| u.to == env!("CARGO_PKG_VERSION"));
    UpdateStatus {
        auto: saved.auto(),
        channel: channels_for(env!("CARGO_PKG_VERSION"))[0].into(),
        version: env!("CARGO_PKG_VERSION").into(),
        ready: READY.lock().unwrap().as_ref().map(ReadyInfo::of),
        notice: NOTICE.lock().unwrap().clone(),
        updated,
        can_update: platform().is_some() && matches!(kind(), Some(k) if k != "msi"),
    }
}

/// The updater's folder: the device's own (every profile on it is the same
/// installed app).
fn device_root(app: &tauri::AppHandle) -> std::path::PathBuf {
    crate::profile::device_root(app).unwrap_or_else(|_| std::env::temp_dir())
}

#[tauri::command]
pub fn update_status(app: tauri::AppHandle) -> UpdateStatus {
    status_of(&device_root(&app))
}

/// The person's choice: automatic updates on or off.
#[tauri::command]
pub fn set_update_prefs(app: tauri::AppHandle, auto: bool) -> UpdateStatus {
    let data_dir = device_root(&app);
    let mut saved = load(&data_dir);
    saved.auto = Some(auto);
    save(&data_dir, &saved);
    if !auto {
        // Off stops the check itself; a download already waiting is dropped.
        *READY.lock().unwrap() = None;
    }
    status_of(&data_dir)
}

/// After the "updated" line has been seen.
#[tauri::command]
pub fn dismiss_updated(app: tauri::AppHandle) {
    let data_dir = device_root(&app);
    let mut saved = load(&data_dir);
    if saved.updated_from.take().is_some() {
        save(&data_dir, &saved);
    }
}

/// "Restart to update". The install runs off the main thread, so the window
/// keeps drawing (and says each step, as `update-stage`) while the
/// conductors stop and the package installs.
#[tauri::command]
pub async fn restart_to_update(app: tauri::AppHandle) -> Result<(), String> {
    // A Vault backup first, for a person who uses the Vault: bounded, and
    // without the Vault it ends at once (no app needs the Vault to update).
    if READY.lock().unwrap().is_some() {
        let _ = app.emit("update-stage", "backup");
        match tokio::time::timeout(Duration::from_secs(90), crate::vault_escrow::write_full_backup(&app)).await {
            Ok(Ok(_)) => log::info!("[update] backed up to the Vault before the install"),
            Ok(Err(e)) => log::info!("[update] no Vault backup before the install ({e})"),
            Err(_) => log::warn!("[update] the Vault backup took over 90 s: installing without waiting for it"),
        }
    }
    let data_dir = device_root(&app);
    let worker = app.clone();
    let installed = tauri::async_runtime::spawn_blocking(move || {
        install_ready(&worker, &data_dir, &|stage| {
            let _ = worker.emit("update-stage", stage);
        })
    })
    .await
    .map_err(|e| format!("the install did not run: {e}"))?;
    match installed {
        Ok(true) => app.restart(),
        Ok(false) => Ok(()),
        Err(e) => {
            log::warn!("[update] {}", e);
            if !INSTALLS_FIRST {
                // The conductors stopped before the install: a restart brings
                // them back, and the update downloads again at its next check.
                app.restart();
            }
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_channel_reads_its_own_two_files() {
        assert_eq!(latest_json_url("stable"), "https://github.com/WeAreFlowsta/Your-Own-AI/releases/latest/download/latest.json");
        assert_eq!(manifest_url("stable"), "https://github.com/WeAreFlowsta/Your-Own-AI/releases/latest/download/release-manifest.json");
        assert_eq!(manifest_url("beta"), "https://github.com/WeAreFlowsta/Your-Own-AI/releases/download/yoai-beta/release-manifest.json");
    }

    #[test]
    fn a_beta_install_follows_betas_and_the_stable_release_a_stable_one_stable_only() {
        assert_eq!(channels_for("0.9.1-beta.1"), &["beta", "stable"]);
        assert_eq!(channels_for("0.9.1"), &["stable"]);
    }

    #[test]
    fn the_saved_choice_defaults_to_automatic() {
        let dir = tempfile::tempdir().unwrap();
        let mut saved = load(dir.path());
        assert!(saved.auto());
        // The ticket is made once and kept.
        let t = ticket(dir.path(), &mut saved);
        assert!(t < 100);
        assert_eq!(load(dir.path()).rollout_ticket, Some(t));
        let mut again = load(dir.path());
        assert_eq!(ticket(dir.path(), &mut again), t);
    }

    #[test]
    fn the_signer_list_holds_team_flowsta() {
        assert_eq!(SIGNERS, &["did:flowsta:uhCAkWuA-Ehm9EVzcsPE2Wo_vpc9ImljHN8NMMEXpbtG5BdooOV9a"]);
    }
}
