//! Identity profiles: one set of identity-scoped data per Flowsta identity
//! that has used this install (the local AIs and their agents, transcripts,
//! the recovery material, the encrypted caches, the Flowsta session).
//!
//! Layout (Phase 2 of the identity switcher):
//!
//!   <app data dir>/profiles.json            device-level index (identity -> profile, last active)
//!   <app data dir>/profiles/<folder>/        one profile - everything listed in
//!                                            PROFILE_FILES / PROFILE_PREFIXES / PROFILE_DIRS
//!   <app data dir>/<everything else>         device-level: settings, models, engines,
//!                                            machine tuning, GPU state, logs
//!
//! `<folder>` is the partition key of the Flowsta identity (first 16 hex
//! chars of sha256 over the 39-byte agent key - the same key the Vault and
//! ProofPoll use), or `local` for an install that has never signed in; a
//! `local` profile binds to the first identity that signs in and keeps its
//! folder name.
//!
//! Installs from before this module keep everything at the app data dir
//! root. The first start with this code moves that layout into a profile
//! folder, all-or-nothing, by same-filesystem renames, before any of the
//! data is opened. The key store moves with its files and its config's
//! absolute paths are rewritten; the conductor config is regenerated on
//! every start anyway.
//!
//! The profile root is chosen once per process (`init`) and read through
//! `root(app)`; outside the app (tests, the census binary) `root` falls back
//! to the app data dir.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use tauri::Manager;

pub const PROFILES_FILE: &str = "profiles.json";
pub const PROFILES_DIR: &str = "profiles";
pub const LOCAL_PROFILE: &str = "local";
pub const PARTITION_KEY_LEN: usize = 16;

/// Whole files that belong to a profile.
pub const PROFILE_FILES: &[&str] = &[
    "transcript-recovery.json",
    "flowsta-auth.json",
    "ai-data.json",
    "coordinator-version",
    "cell-lineage-report.json",
    "cell-tidy-log.json",
    "memory-facts.enc",
    "deleted-conversations.json",
    "corpus.sqlite",
    "corpus.sqlite-wal",
    "corpus.sqlite-shm",
    "project-memory-index.json",
    "mcp-secrets.json",
    "backup-sync-state.json",
    "backup-last-index.json",
    "backup-full-read-at",
    "conversation-restore-pending",
    "memory-reembed-pending",
];
/// File-name prefixes that belong to a profile (replaced recovery keys,
/// per-AI embeddings, per-agent conversation caches, project memory).
pub const PROFILE_PREFIXES: &[&str] = &["transcript-recovery.", "transcript-emb-", "conv-list-", "project-memory-"];
/// Directories that belong to a profile. `lair` last: its config rewrite
/// happens after every rename succeeded.
pub const PROFILE_DIRS: &[&str] = &["thumbnails", "imports", "tool-sessions", "conductor", "lair"];

static ROOT: OnceLock<PathBuf> = OnceLock::new();

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub struct ProfileInfo {
    /// Flowsta agent key this profile belongs to; None until the first sign-in.
    pub identity: Option<String>,
    pub created_at: i64,
    pub last_used: i64,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Profiles {
    pub version: u32,
    pub active: Option<String>,
    pub profiles: BTreeMap<String, ProfileInfo>,
}

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0)
}

pub fn profiles_path(device_root: &Path) -> PathBuf { device_root.join(PROFILES_FILE) }
pub fn profiles_dir(device_root: &Path) -> PathBuf { device_root.join(PROFILES_DIR) }
pub fn profile_root(device_root: &Path, folder: &str) -> PathBuf { profiles_dir(device_root).join(folder) }

/// First 16 hex chars of sha256 over the decoded 39-byte agent key -
/// identical to the Vault's and ProofPoll's partition key.
pub fn partition_key(agent_pub_key: &str) -> Option<String> {
    use base64::Engine;
    use sha2::{Digest, Sha256};
    let b64 = agent_pub_key.trim().strip_prefix('u')?;
    let raw = base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(b64).ok()?;
    if raw.len() != 39 { return None; }
    Some(hex::encode(Sha256::digest(&raw))[..PARTITION_KEY_LEN].to_string())
}

impl Profiles {
    pub fn load(device_root: &Path) -> Profiles {
        match std::fs::read_to_string(profiles_path(device_root)) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
                log::warn!("[profile] profiles.json unreadable ({}), starting a fresh index", e);
                Profiles { version: 1, ..Default::default() }
            }),
            Err(_) => Profiles { version: 1, ..Default::default() },
        }
    }

    pub fn save(&self, device_root: &Path) -> Result<(), String> {
        let p = profiles_path(device_root);
        let tmp = p.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        std::fs::rename(&tmp, &p).map_err(|e| e.to_string())
    }

    pub fn folder_for_identity(&self, agent_pub_key: &str) -> Option<String> {
        self.profiles.iter().find(|(_, i)| i.identity.as_deref() == Some(agent_pub_key)).map(|(f, _)| f.clone())
    }

    pub fn unbound_folder(&self) -> Option<String> {
        self.profiles.iter().find(|(_, i)| i.identity.is_none()).map(|(f, _)| f.clone())
    }

    fn touch(&mut self, folder: &str, identity: Option<&str>) {
        let e = self.profiles.entry(folder.to_string()).or_insert_with(|| ProfileInfo { identity: None, created_at: now(), last_used: now() });
        if let Some(id) = identity { e.identity = Some(id.to_string()); }
        e.last_used = now();
        self.active = Some(folder.to_string());
    }
}

/// True when this install still keeps identity data at the app data root.
pub fn legacy_layout_present(device_root: &Path) -> bool {
    ["transcript-recovery.json", "flowsta-auth.json", "ai-data.json"].iter().any(|f| device_root.join(f).exists())
        || device_root.join("lair").is_dir()
        || device_root.join("conductor").is_dir()
}

/// The identity a legacy install belongs to: its escrow owner, else the
/// signed-in session's key. Read straight from the JSON file - nothing
/// may open the store before the move.
fn legacy_identity(device_root: &Path) -> Option<String> {
    let s = std::fs::read_to_string(device_root.join("flowsta-auth.json")).ok()?;
    let v: serde_json::Value = serde_json::from_str(&s).ok()?;
    v["escrow_owner"].as_str().or_else(|| v["agent_pub_key"].as_str()).map(String::from)
}

/// Whether the key store socket under this root fits the platform's Unix
/// socket path limit (Linux 108, macOS 104, minus a margin; Windows named
/// pipes always fit).
pub fn lair_socket_path_fits(root: &Path) -> bool {
    #[cfg(windows)]
    { let _ = root; true }
    #[cfg(not(windows))]
    {
        let limit: usize = if cfg!(target_os = "macos") { 104 } else { 108 };
        root.join("lair").join("socket").as_os_str().len() + 1 <= limit.saturating_sub(4)
    }
}

fn profile_entries(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for name in PROFILE_FILES {
        let p = root.join(name);
        if p.is_file() && seen.insert(p.clone()) { out.push(p); }
    }
    if let Ok(rd) = std::fs::read_dir(root) {
        for e in rd.flatten() {
            let name = e.file_name().to_string_lossy().to_string();
            if e.path().is_file() && PROFILE_PREFIXES.iter().any(|p| name.starts_with(p)) && seen.insert(e.path()) {
                out.push(e.path());
            }
        }
    }
    for dir in PROFILE_DIRS {
        let p = root.join(dir);
        if p.is_dir() { out.push(p); }
    }
    out
}

/// Lair's config pins absolute paths (connectionUrl, pidFile, storeFile);
/// after the directory moved, point them at the new location. The socket
/// address is a percent-encoded URL, so this goes through the URL-aware
/// repoint, never a plain text replace.
fn rewrite_lair_paths(new_lair_dir: &Path, _old_lair_dir: &Path) -> Result<(), String> {
    crate::lair::repoint_config(new_lair_dir).map(|_| ())
}

/// Move the legacy root layout into `profiles/<folder>/`, all-or-nothing.
pub fn relocate_legacy(device_root: &Path, folder: &str) -> Result<PathBuf, String> {
    let root = profile_root(device_root, folder);
    if !lair_socket_path_fits(&root) {
        return Err(format!("profile path too long for the key store socket ({} bytes)", root.as_os_str().len()));
    }
    if root.join("lair").exists() || root.join("transcript-recovery.json").exists() {
        return Err(format!("{:?} already holds a profile", root));
    }
    std::fs::create_dir_all(&root).map_err(|e| format!("cannot create {:?}: {}", root, e))?;
    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    for from in profile_entries(device_root) {
        let to = root.join(from.file_name().unwrap());
        if to.exists() {
            rollback(&done);
            return Err(format!("{:?} already exists in the profile", to));
        }
        if let Err(e) = std::fs::rename(&from, &to) {
            rollback(&done);
            return Err(format!("could not move {:?}: {}", from, e));
        }
        done.push((from, to));
    }
    if let Err(e) = rewrite_lair_paths(&root.join("lair"), &device_root.join("lair")) {
        rollback(&done);
        return Err(format!("could not rewrite the key store paths: {}", e));
    }
    Ok(root)
}

fn rollback(done: &[(PathBuf, PathBuf)]) {
    for (from, to) in done.iter().rev() {
        if let Err(e) = std::fs::rename(to, from) {
            log::error!("[profile] rollback could not restore {:?}: {}", from, e);
        }
    }
}

/// Where data written by an OLDER copy of the app is kept once profiles
/// exist. Outside `profiles/` on purpose: it is never a profile.
pub const OLDER_COPY_DIR: &str = "older-copy-data";

/// Identity-named files and folders at the device root while profiles ALREADY
/// exist were not left behind by a move - an older copy of the app (one that
/// knows nothing of profiles) ran, found its old location empty and set
/// itself up there as a brand new install. Treating that as "a legacy install
/// to move" promoted the empty newcomer to the active profile and the
/// person's real data vanished from view (field 2026-09-19: an installed
/// 0.7.1 opened once beside a dev build).
///
/// So it is moved ASIDE: kept whole under `older-copy-data/<unix time>/`,
/// never listed in `profiles.json`, never made active, its key store config
/// left pointing where the older copy expects it. All-or-nothing like the
/// real move.
pub fn set_aside_older_copy(device_root: &Path) -> Result<PathBuf, String> {
    let entries = profile_entries(device_root);
    if entries.is_empty() {
        return Err("nothing to set aside".into());
    }
    let mut dir = device_root.join(OLDER_COPY_DIR).join(now().to_string());
    let mut n = 1;
    while dir.exists() {
        dir = device_root.join(OLDER_COPY_DIR).join(format!("{}-{}", now(), n));
        n += 1;
    }
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {:?}: {}", dir, e))?;
    let mut done: Vec<(PathBuf, PathBuf)> = Vec::new();
    for from in entries {
        let to = dir.join(from.file_name().unwrap());
        if let Err(e) = std::fs::rename(&from, &to) {
            rollback(&done);
            let _ = std::fs::remove_dir(&dir);
            return Err(format!("could not move {:?}: {}", from, e));
        }
        done.push((from, to));
    }
    Ok(dir)
}

/// Decide which profile this launch runs, moving a legacy layout first if
/// there is one. `live_identity` = the Vault's unlocked agent key when the
/// Vault answered at launch, else None.
pub fn select_profile_root(device_root: &Path, live_identity: Option<&str>) -> PathBuf {
    let mut profiles = Profiles::load(device_root);

    if legacy_layout_present(device_root) && !profiles.profiles.is_empty() {
        // The move already happened on this machine. What sits at the root
        // now came from an older copy of the app - keep it, never use it.
        match set_aside_older_copy(device_root) {
            Ok(dir) => log::warn!(
                "[profile] data written by an older copy of the app was found beside the profiles - kept at {:?}, not used; the active profile is unchanged",
                dir
            ),
            Err(e) => log::warn!("[profile] older-copy data could not be set aside ({}) - continuing with the profile", e),
        }
    } else if legacy_layout_present(device_root) {
        let bound = legacy_identity(device_root);
        let folder = bound.as_deref().and_then(partition_key).unwrap_or_else(|| LOCAL_PROFILE.to_string());
        match relocate_legacy(device_root, &folder) {
            Ok(root) => {
                profiles.touch(&folder, bound.as_deref());
                if let Err(e) = profiles.save(device_root) { log::warn!("[profile] profiles.json not saved: {}", e); }
                log::info!("[profile] identity data moved into profile {:?}", root);
            }
            Err(e) if profile_root(device_root, &folder).join("lair").exists()
                || profile_root(device_root, &folder).join("transcript-recovery.json").exists() =>
            {
                // The move already happened; something recreated a legacy
                // name at the device root. The profile is the truth.
                log::warn!("[profile] legacy names at the device root beside a finished profile ({}) - using the profile", e);
            }
            Err(e) => {
                log::warn!("[profile] relocation skipped: {} - staying on the legacy layout", e);
                return device_root.to_path_buf();
            }
        }
    }

    let folder = match live_identity {
        Some(id) => profiles
            .folder_for_identity(id)
            .or_else(|| profiles.unbound_folder())
            .or_else(|| partition_key(id))
            .unwrap_or_else(|| LOCAL_PROFILE.to_string()),
        None => profiles
            .active
            .clone()
            .or_else(|| profiles.profiles.keys().next().cloned())
            .unwrap_or_else(|| LOCAL_PROFILE.to_string()),
    };
    let root = profile_root(device_root, &folder);
    if !lair_socket_path_fits(&root) {
        log::warn!("[profile] path {:?} too long for the key store socket - using the app data root", root);
        return device_root.to_path_buf();
    }
    if let Err(e) = std::fs::create_dir_all(&root) {
        log::warn!("[profile] cannot create {:?}: {} - using the app data root", root, e);
        return device_root.to_path_buf();
    }
    let known = profiles.profiles.get(&folder).and_then(|p| p.identity.clone());
    profiles.touch(&folder, known.as_deref());
    if let Err(e) = profiles.save(device_root) { log::warn!("[profile] profiles.json not saved: {}", e); }
    root
}

/// Choose this process's profile root. Called once, first thing in setup,
/// before anything opens a store or a file.
pub fn init(device_root: &Path, live_identity: Option<&str>) -> PathBuf {
    let root = select_profile_root(device_root, live_identity);
    let _ = ROOT.set(root.clone());
    root
}

/// The app data dir: profiles.json, profiles/, and every device-level file.
pub fn device_root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    app.path().app_data_dir().map_err(|e| format!("no app data dir: {}", e))
}

/// This profile's root - where every identity-scoped file lives.
pub fn root(app: &tauri::AppHandle) -> Result<PathBuf, String> {
    match ROOT.get() {
        Some(r) => Ok(r.clone()),
        None => device_root(app),
    }
}

/// Absolute path for a tauri-plugin-store file inside the profile (the
/// plugin resolves a relative name against the app data dir, which would be
/// the device root).
pub fn store_path(app: &tauri::AppHandle, name: &str) -> PathBuf {
    match root(app) {
        Ok(r) => r.join(name),
        Err(_) => PathBuf::from(name),
    }
}

/// Record that the current profile belongs to `agent_pub_key` (first
/// sign-in, first escrow contact, or a restore that adopts an identity).
/// Frozen after an identity switch: the profile belongs to the previous
/// identity until the app restarts.
pub fn bind(app: &tauri::AppHandle, agent_pub_key: &str) {
    if crate::identity_watch::switched() {
        log::warn!("[profile] not rebinding this profile after an identity switch");
        return;
    }
    let (Ok(device), Ok(current)) = (device_root(app), root(app)) else { return };
    if current.parent() != Some(&profiles_dir(&device)) { return; } // legacy root: nothing to record
    let Some(folder) = current.file_name().map(|f| f.to_string_lossy().to_string()) else { return };
    let mut profiles = Profiles::load(&device);
    let already = profiles.profiles.get(&folder).and_then(|p| p.identity.as_deref()) == Some(agent_pub_key);
    profiles.touch(&folder, Some(agent_pub_key));
    if let Err(e) = profiles.save(&device) { log::warn!("[profile] profiles.json not saved: {}", e); }
    if !already { log::info!("[profile] profile {} bound to the signed-in identity", folder); }
}

/// The identity recorded for the current profile, if any. None for a
/// profile nobody has signed into yet, and for the legacy root.
pub fn bound_identity(app: &tauri::AppHandle) -> Option<String> {
    let (Ok(device), Ok(current)) = (device_root(app), root(app)) else { return None };
    if current.parent() != Some(&profiles_dir(&device)) {
        return None;
    }
    let folder = current.file_name()?.to_string_lossy().to_string();
    Profiles::load(&device).profiles.get(&folder).and_then(|p| p.identity.clone())
}

/// Frontend access to a store inside the profile (the AI configs).
#[tauri::command]
pub fn profile_store_path(app: tauri::AppHandle, name: String) -> Result<String, String> {
    if name.is_empty() || name.contains(['/', '\\']) || name.starts_with('.') {
        return Err("invalid store name".into());
    }
    Ok(store_path(&app, &name).to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    const KEY_A: &str = "uhCAk75qJ5oobyfm3Lh-akZIQSe2zpSTtG1Pcxs23qTFoQwY_GDWY";
    const KEY_B: &str = "uhCAk0O4EJ97RZ7eX2wf9x08PWjNj3Avt2K1SdU8tgPzWoQBwWk0s";

    fn legacy_install(root: &Path, owner: Option<&str>) {
        std::fs::write(root.join("transcript-recovery.json"), b"{}").unwrap();
        std::fs::write(root.join("transcript-recovery.replaced-1.json"), b"{}").unwrap();
        std::fs::write(root.join("ai-data.json"), b"{}").unwrap();
        std::fs::write(root.join("settings.json"), b"{}").unwrap();
        std::fs::write(root.join("memory-facts.enc"), b"x").unwrap();
        std::fs::write(root.join("transcript-emb-veebo.enc"), b"x").unwrap();
        std::fs::write(root.join("conv-list-uhCAk.enc"), b"x").unwrap();
        std::fs::write(root.join("corpus.sqlite"), b"x").unwrap();
        std::fs::write(root.join("model-stats.json"), b"{}").unwrap();
        std::fs::create_dir_all(root.join("thumbnails")).unwrap();
        std::fs::write(root.join("thumbnails/veebo.jpg"), b"j").unwrap();
        std::fs::create_dir_all(root.join("models")).unwrap();
        std::fs::write(root.join("models/m.gguf"), b"g").unwrap();
        std::fs::create_dir_all(root.join("conductor/databases")).unwrap();
        std::fs::write(root.join("conductor/databases/x"), b"d").unwrap();
        let lair = root.join("lair");
        std::fs::create_dir_all(&lair).unwrap();
        std::fs::write(lair.join("lair-keystore-config.yaml"), format!("connectionUrl: unix://{0}/socket?k=abc\npidFile: {0}/pid_file\nstoreFile: {0}/store_file\n", lair.display())).unwrap();
        std::fs::write(lair.join("store_file"), b"s").unwrap();
        if let Some(k) = owner {
            std::fs::write(root.join("flowsta-auth.json"), serde_json::json!({"escrow_owner": k, "link_done": true}).to_string()).unwrap();
        }
    }

    #[test]
    fn partition_key_matches_the_vaults_shape() {
        let k = partition_key(KEY_A).unwrap();
        assert_eq!(k.len(), PARTITION_KEY_LEN);
        assert_ne!(partition_key(KEY_B).unwrap(), k);
        assert_eq!(partition_key("nope"), None);
    }

    #[test]
    fn owned_legacy_install_moves_identity_data_and_keeps_device_data() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        legacy_install(root, Some(KEY_A));
        let chosen = select_profile_root(root, None);
        let pk = partition_key(KEY_A).unwrap();
        assert_eq!(chosen, profile_root(root, &pk));
        for rel in ["transcript-recovery.json", "transcript-recovery.replaced-1.json", "ai-data.json", "flowsta-auth.json", "memory-facts.enc", "transcript-emb-veebo.enc", "conv-list-uhCAk.enc", "corpus.sqlite", "thumbnails/veebo.jpg", "conductor/databases/x", "lair/store_file"] {
            assert!(chosen.join(rel).exists(), "{} in profile", rel);
            assert!(!root.join(rel).exists(), "{} left the root", rel);
        }
        for rel in ["settings.json", "model-stats.json", "models/m.gguf"] {
            assert!(root.join(rel).exists(), "{} stays at the device root", rel);
        }
        let yaml = std::fs::read_to_string(chosen.join("lair/lair-keystore-config.yaml")).unwrap();
        assert!(yaml.contains(&chosen.join("lair").display().to_string()));
        assert!(!yaml.contains(&format!("{}/socket", root.join("lair").display())));
        let p = Profiles::load(root);
        assert_eq!(p.active.as_deref(), Some(pk.as_str()));
        assert_eq!(p.profiles[&pk].identity.as_deref(), Some(KEY_A));
        assert!(!legacy_layout_present(root));
        assert_eq!(select_profile_root(root, None), chosen);
        // a different live identity gets its own profile; A still resolves to hers
        let other = select_profile_root(root, Some(KEY_B));
        assert_eq!(other, profile_root(root, &partition_key(KEY_B).unwrap()));
        assert_eq!(select_profile_root(root, Some(KEY_A)), chosen);
    }

    #[test]
    fn never_signed_in_legacy_install_becomes_local_and_is_reused_by_the_first_identity() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        legacy_install(root, None);
        let chosen = select_profile_root(root, None);
        assert_eq!(chosen, profile_root(root, LOCAL_PROFILE));
        assert!(Profiles::load(root).profiles[LOCAL_PROFILE].identity.is_none());
        assert_eq!(select_profile_root(root, Some(KEY_A)), chosen);
    }

    /// Field 2026-09-19, replayed: the move happened; later an OLDER copy of
    /// the app ran once, found its old location empty and set itself up
    /// there as a new install; the next launch of this version must keep
    /// the person's real profile active, not the newcomer.
    #[test]
    fn data_left_by_an_older_copy_is_set_aside_and_never_becomes_the_profile() {
        let tmp = tempfile::tempdir().unwrap();
        let device = tmp.path();
        // 1. A signed-in 0.7.2 install, moved on the first launch.
        legacy_install(device, Some(KEY_A));
        let real = select_profile_root(device, None);
        let folder = partition_key(KEY_A).unwrap();
        assert_eq!(real, profile_root(device, &folder));
        std::fs::write(real.join("conv-list-REAL.enc"), b"years of conversations").unwrap();
        assert!(!legacy_layout_present(device));

        // 2. The older copy runs: a brand new, never-signed-in install at the root.
        legacy_install(device, None);
        assert!(legacy_layout_present(device));

        // 3. This version launches again, Vault not answering.
        let chosen = select_profile_root(device, None);
        assert_eq!(chosen, real, "the real profile stays in charge");
        assert!(chosen.join("conv-list-REAL.enc").exists());

        let profiles = Profiles::load(device);
        assert_eq!(profiles.active.as_deref(), Some(folder.as_str()), "active is unchanged");
        assert_eq!(profiles.profiles.len(), 1, "the newcomer is not a profile");
        assert!(!profile_root(device, LOCAL_PROFILE).exists(), "and no `local` profile was made of it");

        // The older copy's data is kept whole, outside profiles/, and the root is clean again.
        assert!(!legacy_layout_present(device));
        let kept: Vec<_> = std::fs::read_dir(device.join(OLDER_COPY_DIR)).unwrap().flatten().collect();
        assert_eq!(kept.len(), 1);
        let kept = kept[0].path();
        for name in ["transcript-recovery.json", "ai-data.json", "lair/store_file", "conductor/databases/x", "thumbnails/veebo.jpg"] {
            assert!(kept.join(name).exists(), "{name} kept");
        }
        // Its key store config still names the root - it was never repointed or run.
        let yaml = std::fs::read_to_string(kept.join("lair/lair-keystore-config.yaml")).unwrap();
        assert!(yaml.contains(&format!("{}/socket", device.join("lair").display())));
        // Device-level files never move.
        assert!(device.join("settings.json").exists() && device.join("models/m.gguf").exists());

        // 4. It can happen again (the older copy is opened a second time): same outcome.
        legacy_install(device, None);
        assert_eq!(select_profile_root(device, Some(KEY_A)), real);
        assert_eq!(std::fs::read_dir(device.join(OLDER_COPY_DIR)).unwrap().count(), 2);
        assert_eq!(Profiles::load(device).profiles.len(), 1);
    }

    /// The first-time move is untouched by that rule: no profiles yet = a
    /// real legacy install = moved and made active, as before.
    #[test]
    fn the_first_move_still_activates_the_moved_install() {
        let tmp = tempfile::tempdir().unwrap();
        legacy_install(tmp.path(), None);
        let root = select_profile_root(tmp.path(), None);
        assert_eq!(root, profile_root(tmp.path(), LOCAL_PROFILE));
        assert!(root.join("ai-data.json").exists());
        assert!(!tmp.path().join(OLDER_COPY_DIR).exists());
        assert_eq!(Profiles::load(tmp.path()).active.as_deref(), Some(LOCAL_PROFILE));
    }

    #[test]
    fn a_failed_move_rolls_back_and_stays_legacy() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        legacy_install(root, Some(KEY_A));
        let pk = partition_key(KEY_A).unwrap();
        std::fs::create_dir_all(profile_root(root, &pk).join("conductor")).unwrap();
        assert_eq!(select_profile_root(root, None), root);
        for rel in ["transcript-recovery.json", "ai-data.json", "flowsta-auth.json", "thumbnails/veebo.jpg", "lair/store_file"] {
            assert!(root.join(rel).exists(), "{} back at the root", rel);
        }
        let yaml = std::fs::read_to_string(root.join("lair/lair-keystore-config.yaml")).unwrap();
        assert!(yaml.contains(&format!("{}/socket", root.join("lair").display())));
    }

    #[test]
    fn fresh_install_starts_in_local_or_the_live_identity() {
        let d1 = tempfile::tempdir().unwrap();
        assert_eq!(select_profile_root(d1.path(), None), profile_root(d1.path(), LOCAL_PROFILE));
        let d2 = tempfile::tempdir().unwrap();
        assert_eq!(select_profile_root(d2.path(), Some(KEY_B)), profile_root(d2.path(), &partition_key(KEY_B).unwrap()));
    }
}
