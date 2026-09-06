//! Transcript-key escrow in the user's Flowsta Vault.
//!
//! The transcript encryption key + per-user network seed live in a local
//! file (`transcript_crypto::RECOVERY_FILE`). Losing that file makes every
//! transcript - including copies on the user's own future devices and
//! archive nodes - permanently unreadable. When the user signs in with
//! Flowsta, a copy of that recovery material is stored in their Vault via
//! the standard third-party backup endpoint, under a canonical backup
//! payload's `app_keys` block, so it rides both the single-app export and
//! the Vault's full "Download Export" (the CAL-mandated one).
//!
//! Safety rules this module enforces:
//! - The escrow is written only when the Vault copy is absent or already
//!   identical. A DIFFERENT Vault copy is never overwritten silently -
//!   that's surfaced as a conflict for the user to resolve.
//! - Restoring the Vault copy onto this device preserves the old local key
//!   as a timestamped file, and refuses to run while local conversations
//!   exist unless the user explicitly accepted losing them.
//! - "Keep this device's key" (the other conflict resolution) first saves
//!   the about-to-be-replaced Vault copy to a local timestamped file, in
//!   case it was the last copy of an old key.

use crate::commands_holochain::HolochainState;
use crate::flowsta::{self, http, AUTH_STORE, VAULT_ORIGIN, YOAI_HOLOCHAIN_CLIENT_ID};
use crate::transcript_crypto::{self, RecoveryMaterial};
use holochain_types::prelude::ExternIO;
use serde::Serialize;
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_store::StoreExt;

/// Fixed label so re-escrow overwrites the same named snapshot instead of
/// accumulating timestamped ones.
const ESCROW_LABEL: &str = "recovery";

#[derive(Serialize, Clone)]
pub struct EscrowStatus {
    /// "synced" | "conflict" | "unlinked" | "vault_unavailable" |
    /// "vault_locked" | "identity_mismatch" | "error"
    pub state: String,
    /// Raw conversation-record count across local agents; only filled when
    /// it matters (state == "conflict").
    pub local_conversations: Option<u64>,
    pub error: Option<String>,
    /// Automatic data backups are suspended (restore-pending marker set by
    /// a key restore or a factory reset) - the Vault snapshot may hold data
    /// this device doesn't.
    pub backups_held: bool,
}

impl EscrowStatus {
    fn state(s: &str) -> Self {
        Self { state: s.into(), local_conversations: None, error: None, backups_held: false }
    }
    fn error(e: String) -> Self {
        Self { state: "error".into(), local_conversations: None, error: Some(e), backups_held: false }
    }
}

/// The canonical backup payload. `cells` is empty for now - this backup
/// exists to carry the recovery material in `app_keys`; record-level
/// conversation backup is a separate, later feature.
fn escrow_payload(recovery: &RecoveryMaterial) -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "_readme": "Your Own AI transcript recovery material, backed up from your device. \
                    The app_keys block holds the encryption key and network seed that make \
                    your AI conversation transcripts readable. Keep any export of this file \
                    private - anyone with the key can decrypt your transcripts.",
        "license": "Cryptographic Autonomy License v1.0 (CAL-1.0)",
        "app": { "name": "Your Own AI", "client_id": YOAI_HOLOCHAIN_CLIENT_ID },
        "exported_at_iso": iso_now(),
        "_summary": { "countsByEntryType": {}, "totalRecords": 0 },
        "cells": [],
        "app_keys": {
            "_readme": "The transcript encryption key (hex) and per-user network seed Your \
                        Own AI generates locally. Needed to decrypt this user's transcripts \
                        on any device.",
            "transcript_data_key_hex": recovery.data_key_hex,
            "transcript_network_seed": recovery.network_seed,
            "version": recovery.version,
        },
    })
}

pub(crate) fn parse_escrow(data: &serde_json::Value) -> Option<RecoveryMaterial> {
    let keys = data.get("app_keys")?;
    Some(RecoveryMaterial {
        network_seed: keys.get("transcript_network_seed")?.as_str()?.to_string(),
        data_key_hex: keys.get("transcript_data_key_hex")?.as_str()?.to_string(),
        version: keys.get("version").and_then(|v| v.as_u64()).unwrap_or(1) as u32,
    })
}

fn iso_now() -> String {
    // Seconds-precision ISO timestamp without pulling in chrono.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86400;
    let (mut y, mut rem_days) = (1970i64, days as i64);
    loop {
        let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
        let len = if leap { 366 } else { 365 };
        if rem_days < len {
            break;
        }
        rem_days -= len;
        y += 1;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let months = [31, if leap { 29 } else { 28 }, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];
    let mut m = 0usize;
    while rem_days >= months[m] {
        rem_days -= months[m];
        m += 1;
    }
    format!(
        "{:04}-{:02}-{:02}T{:02}:{:02}:{:02}Z",
        y, m + 1, rem_days + 1,
        (secs % 86400) / 3600, (secs % 3600) / 60, secs % 60
    )
}

/// Fetch the escrowed material from Vault. `Ok(None)` = no escrow stored.
async fn fetch_escrow(port: u16) -> Result<Option<RecoveryMaterial>, String> {
    let resp = http()
        .post(format!("http://127.0.0.1:{}/backup/retrieve", port))
        .header("Origin", VAULT_ORIGIN)
        .json(&serde_json::json!({
            "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
            "label": ESCROW_LABEL,
        }))
        .send()
        .await
        .map_err(|e| format!("vault unreachable: {}", e))?;

    match resp.status() {
        reqwest::StatusCode::NOT_FOUND => return Ok(None),
        s if s.is_success() => {}
        _ => {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            return Err(body["error"].as_str().unwrap_or("retrieve_failed").to_string());
        }
    }
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("bad retrieve response: {}", e))?;
    // An escrow snapshot whose app_keys can't be parsed is treated as a hard
    // error, NOT as absent - writing over it could destroy a key.
    match parse_escrow(&v["data"]) {
        Some(m) => Ok(Some(m)),
        None => Err("escrow_unreadable".into()),
    }
}

async fn write_escrow(port: u16, recovery: &RecoveryMaterial) -> Result<(), String> {
    let resp = http()
        .post(format!("http://127.0.0.1:{}/backup", port))
        .header("Origin", VAULT_ORIGIN)
        .json(&serde_json::json!({
            "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
            "app_name": "Your Own AI",
            "label": ESCROW_LABEL,
            "data": escrow_payload(recovery),
        }))
        .send()
        .await
        .map_err(|e| format!("vault unreachable: {}", e))?;
    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        return Err(body["error"].as_str().unwrap_or("backup_failed").to_string());
    }
    Ok(())
}

/// The local recovery material, read-only (never generates). Prefers the
/// running manager's copy; falls back to the file before the conductor is up.
pub(crate) fn local_recovery(app: &tauri::AppHandle) -> Result<RecoveryMaterial, String> {
    if let Some(hc) = app.try_state::<Arc<HolochainState>>() {
        if let Ok(manager) = hc.get() {
            return Ok(manager.recovery.clone());
        }
    }
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("no app data dir: {}", e))?;
    transcript_crypto::load_recovery_material(&data_dir)?
        .ok_or_else(|| "no local recovery material yet".to_string())
}

/// AUTH_STORE key recording which Vault identity this device's escrow and
/// data backups belong to. Distinct from the session `agent_pub_key`:
/// sign-out wipes the session but the data on disk still belongs to whoever
/// created it, so the owner record must survive sign-out (and the
/// SESSION_KEYS_FULL mismatch wipe). It changes only through an explicit
/// adoption: first contact, or a user-driven conversation restore.
pub(crate) const ESCROW_OWNER_KEY: &str = "escrow_owner";

/// Preconditions shared by every escrow operation. Returns the Vault port.
/// The link ceremony is never triggered from here - it rides sign-in; without
/// a completed link the escrow simply waits ("unlinked").
///
/// Identity gate: every caller that can WRITE requires the live, unlocked
/// Vault identity to equal the recorded `escrow_owner`. This gate fails
/// CLOSED on mismatch AND on cannot-confirm (locked Vault, missing key) -
/// deliberately the OPPOSITE default to `session_identity_ok`'s fail-open.
/// That asymmetry is load-bearing: an online request billed against a stale
/// session is recoverable; a backup slot overwritten under the wrong
/// identity is not. Do not "harmonize" the two.
///
/// `require_owner_match: false` is for the one deliberate adoption path
/// (conversation restore) - it still requires an unlocked Vault with a
/// readable identity, it only skips the owner comparison.
pub(crate) async fn escrow_port(
    app: &tauri::AppHandle,
    require_owner_match: bool,
) -> Result<u16, EscrowStatus> {
    let store = app
        .store(AUTH_STORE)
        .map_err(|e| EscrowStatus::error(e.to_string()))?;
    if !store.get("link_done").and_then(|v| v.as_bool()).unwrap_or(false) {
        return Err(EscrowStatus::state("unlinked"));
    }
    let vault = flowsta::find_vault().await;
    let port = match vault.port {
        Some(p) => p,
        None => return Err(EscrowStatus::state("vault_unavailable")),
    };
    // A locked Vault reports agent_pub_key: null - indistinguishable from a
    // never-created one, and no identity to compare. No writes until unlock.
    if !vault.unlocked {
        return Err(EscrowStatus::state("vault_locked"));
    }
    let live = match vault.agent_pub_key {
        Some(k) => k,
        None => return Err(EscrowStatus::state("vault_locked")),
    };
    let owner = store
        .get(ESCROW_OWNER_KEY)
        .and_then(|v| v.as_str().map(String::from));
    match owner {
        Some(o) if o == live => Ok(port),
        Some(_) if !require_owner_match => Ok(port),
        Some(_) => Err(EscrowStatus::state("identity_mismatch")),
        None => {
            // First contact: this device's data has no recorded owner yet -
            // adopt the identity in front of us. Existing installs land here
            // once on upgrade; their sync state stays valid (same identity).
            store.set(ESCROW_OWNER_KEY, serde_json::json!(live));
            let _ = store.save();
            Ok(port)
        }
    }
}

/// Record the live Vault identity as this device's escrow owner. Only for
/// explicit adoption paths (a user-driven restore). Clears the backup sync
/// state: "already uploaded" beliefs about another identity's slot would
/// otherwise produce manifests pointing at objects that don't exist there.
fn adopt_escrow_owner(app: &tauri::AppHandle, live: &str) {
    if let Ok(store) = app.store(AUTH_STORE) {
        let prev = store
            .get(ESCROW_OWNER_KEY)
            .and_then(|v| v.as_str().map(String::from));
        if prev.as_deref() != Some(live) {
            store.set(ESCROW_OWNER_KEY, serde_json::json!(live));
            let _ = store.save();
            clear_sync_state(app);
            log::info!("[escrow] escrow owner adopted for the current Vault identity");
        }
    }
}

fn clear_sync_state(app: &tauri::AppHandle) {
    if let Some(p) = sync_state_path(app) {
        if p.exists() {
            let _ = std::fs::remove_file(&p);
        }
    }
}

/// Raw conversation-record count across every local agent (no decryption -
/// undecryptable records still count as data worth protecting).
async fn count_local_conversations(app: &tauri::AppHandle) -> Result<u64, String> {
    let hc = app
        .try_state::<Arc<HolochainState>>()
        .ok_or("holochain state missing")?;
    let manager = hc.get()?;
    let keys: Vec<String> = {
        let agents = manager.agents.lock().await;
        agents.keys().cloned().collect()
    };
    let mut total = 0u64;
    for key in keys {
        let payload = ExternIO::encode(()).map_err(|e| e.to_string())?;
        match manager
            .call_zome(&key, "transcript", "get_all_conversations", payload)
            .await
        {
            Ok(result) => {
                let records: Vec<holochain_types::prelude::Record> =
                    ExternIO::decode(&result).map_err(|e| e.to_string())?;
                total += records.len() as u64;
            }
            Err(e) => {
                // A cell that can't answer might hold data - fail safe by
                // treating the count as unknown rather than zero.
                return Err(format!("conversation count failed for {}: {}", key, e));
            }
        }
    }
    Ok(total)
}

/// Reconcile the escrow with the Vault. Writes only into an empty slot;
/// reports "conflict" (with the local record count) when the Vault holds
/// different material. Safe to call repeatedly.
#[tauri::command]
pub async fn vault_escrow_sync(app: tauri::AppHandle) -> EscrowStatus {
    let held = restore_pending_path(&app).map(|p| p.exists()).unwrap_or(false);
    let mut status = vault_escrow_sync_inner(&app).await;
    status.backups_held = held;
    status
}

async fn vault_escrow_sync_inner(app: &tauri::AppHandle) -> EscrowStatus {
    let port = match escrow_port(app, true).await {
        Ok(p) => p,
        Err(s) => return s,
    };
    let local = match local_recovery(app) {
        Ok(r) => r,
        Err(e) => return EscrowStatus::error(e),
    };
    match fetch_escrow(port).await {
        Ok(None) => match write_escrow(port, &local).await {
            Ok(()) => {
                log::info!("[escrow] transcript recovery material escrowed to Vault");
                schedule_full_backup(app);
                EscrowStatus::state("synced")
            }
            Err(e) if e == "vault_never_unlocked" => EscrowStatus::state("vault_unavailable"),
            Err(e) => EscrowStatus::error(e),
        },
        Ok(Some(ref m)) if *m == local => {
            schedule_full_backup(app);
            EscrowStatus::state("synced")
        }
        Ok(Some(_)) => {
            let count = count_local_conversations(app).await.ok();
            EscrowStatus {
                state: "conflict".into(),
                local_conversations: count,
                error: None,
                backups_held: false,
            }
        }
        Err(e) if e == "vault_never_unlocked" => EscrowStatus::state("vault_unavailable"),
        Err(e) if e == "not_linked" => EscrowStatus::state("unlinked"),
        Err(e) => EscrowStatus::error(e),
    }
}

/// Conflict resolution A: adopt the Vault's key on this device. Preserves the
/// current local key as a timestamped file, wipes the conductor + lair state
/// and the derived memory caches (all keyed to the old material), then
/// relaunches so the app re-provisions onto the restored network seed.
///
/// Refuses while local conversations exist unless `accept_data_loss` - the
/// old key file is kept either way, so nothing is silently destroyed.
#[tauri::command]
pub async fn vault_escrow_restore(
    app: tauri::AppHandle,
    accept_data_loss: bool,
) -> Result<(), String> {
    // The restore is the deliberate adoption path: pulling the Vault's key
    // onto this device MEANS "this device now belongs to that identity",
    // so it may run under an owner mismatch (unlocked Vault still required).
    let port = match escrow_port(&app, false).await {
        Ok(p) => p,
        Err(s) => return Err(s.error.unwrap_or(s.state)),
    };
    let escrowed = fetch_escrow(port)
        .await?
        .ok_or("no escrow stored in Vault")?;
    let local = local_recovery(&app)?;
    if escrowed == local {
        return Err("already in sync".into());
    }
    if !accept_data_loss {
        let count = count_local_conversations(&app).await?;
        if count > 0 {
            return Err(format!("local_data_exists:{}", count));
        }
    }

    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("no app data dir: {}", e))?;

    // Stop the conductor + lair so their databases aren't held open (same
    // signal-by-PID approach as the factory reset).
    if let Some(hc) = app.try_state::<Arc<HolochainState>>() {
        if let Some(manager) = hc.manager.get() {
            for pid in [
                manager.handle.conductor_child.id(),
                manager.handle.lair_child.id(),
            ] {
                crate::process_ext::stop_pid(pid);
            }
            log::info!("[escrow] signalled conductor + lair to stop for restore");
        }
    }
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    // Wipe the key-derived state BEFORE committing the new key. The old
    // Holochain state belongs to the old key; if it cannot be removed
    // (Windows releases file locks a beat after the processes die - or not
    // at all if something still holds them), adopting the key anyway would
    // relaunch into a half-wiped hybrid where lair cannot start and NOTHING
    // works. Retry while the locks release; on persistent failure abort
    // with the old key still intact - the user just tries again.
    for dir in ["conductor", "lair"] {
        let p = data_dir.join(dir);
        let mut last_err = String::new();
        let mut removed = false;
        for _ in 0..20 {
            if !p.exists() {
                removed = true;
                break;
            }
            match std::fs::remove_dir_all(&p) {
                Ok(_) => {
                    removed = true;
                    break;
                }
                Err(e) => {
                    last_err = e.to_string();
                    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
                }
            }
        }
        if !removed {
            return Err(format!(
                "Could not release this device's Holochain state for the restore \
                 ({}: {}). Close and reopen Your Own AI, then try Restore again - \
                 nothing has been changed.",
                dir, last_err
            ));
        }
    }

    // Swap the recovery material (old file preserved with a timestamp).
    transcript_crypto::replace_recovery_material(&data_dir, &escrowed)?;

    // Adopting the Vault's key means this device now belongs to that Vault's
    // identity - record it (also clears the backup sync state, whose
    // "already uploaded" beliefs describe the previous owner's slot).
    match flowsta::find_vault().await.agent_pub_key {
        Some(live) => adopt_escrow_owner(&app, &live),
        None => {
            // Vault locked mid-restore: can't read who we just adopted.
            // Drop the owner record so the next unlocked contact re-adopts.
            if let Ok(store) = app.store(AUTH_STORE) {
                store.delete(ESCROW_OWNER_KEY);
                let _ = store.save();
            }
            clear_sync_state(&app);
        }
    }

    // Suspend automatic backups until the Vault conversations have been
    // replayed onto this device - the Vault snapshot may be the only copy.
    if let Err(e) = std::fs::write(data_dir.join(RESTORE_PENDING_FILE), b"") {
        log::warn!("[escrow] could not write restore-pending marker: {}", e);
    }

    // The derived memory caches are keyed to the old material too (the
    // Holochain state was wiped above, before the key swap). AI configs,
    // models, and the Flowsta session are untouched.
    let _ = std::fs::remove_file(data_dir.join("memory-facts.enc"));
    if let Ok(entries) = std::fs::read_dir(&data_dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("transcript-emb-") && name.ends_with(".enc") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    log::info!("[escrow] recovery material restored from Vault - relaunching");

    #[cfg(not(debug_assertions))]
    {
        app.restart()
    }
    #[cfg(debug_assertions)]
    {
        log::info!("[escrow] dev build - exiting; re-run `npm run tauri dev` to relaunch");
        app.exit(0);
        Ok(())
    }
}

/// Conflict resolution B: keep this device's key and overwrite the Vault
/// copy. The replaced Vault material is first written to a timestamped local
/// file - it may be the only copy of an old key, and transcripts encrypted
/// under it (e.g. on another machine's export) would otherwise become
/// unrecoverable with no warning.
#[tauri::command]
pub async fn vault_escrow_keep_local(app: tauri::AppHandle) -> Result<EscrowStatus, String> {
    let port = match escrow_port(&app, true).await {
        Ok(p) => p,
        Err(s) => return Err(s.error.unwrap_or(s.state)),
    };
    let local = local_recovery(&app)?;
    if let Some(old) = fetch_escrow(port).await? {
        if old != local {
            let data_dir = app
                .path()
                .app_data_dir()
                .map_err(|e| format!("no app data dir: {}", e))?;
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let path = data_dir.join(format!("transcript-recovery.vault-replaced-{}.json", ts));
            let json = serde_json::to_string_pretty(&old).map_err(|e| e.to_string())?;
            std::fs::write(&path, &json).map_err(|e| e.to_string())?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
            log::info!("[escrow] previous Vault key preserved at {:?}", path.file_name());
        }
    }
    write_escrow(port, &local).await?;
    Ok(EscrowStatus::state("synced"))
}

// ── Full-data backup (label "latest") ───────────────────────────────
//
// The key-only "recovery" escrow above makes transcripts recoverABLE; this
// backup makes them recoverED: every conversation, message, and grounding
// annotation - decrypted to a human-readable view (the Vault encrypts the
// backup at rest and the user's CAL export must be readable) plus the
// on-chain encrypted bytes for a future replay-restore. Written debounced
// after transcript writes, and after each successful escrow sync.

pub(crate) const DATA_LABEL: &str = "latest";
/// LEGACY monolith mode only (Vaults without /backup/limits): stay under
/// the old 50 MB per-backup cap with headroom for JSON overhead.
const BACKUP_BUDGET_BYTES: usize = 45 * 1024 * 1024;
/// The document library's records ride in the manifest (incremental mode)
/// or the snapshot; a record with its card is ~600 bytes, so this holds
/// several thousand documents. Over it, cards go first (oldest documents
/// first), never a name, a flag or a grant.
const CORPUS_RECORDS_BUDGET_BYTES: usize = 4 * 1024 * 1024;

/// Incremental mode: the manifest object - conversation index + keys +
/// AI configs + everything small. Conversation records live in their own
/// per-conversation objects listed under `conversations[].labels`.
pub(crate) const MANIFEST_LABEL: &str = "manifest";
/// Local record of what each conversation object last uploaded, so backup
/// runs only re-send conversations with new records.
pub(crate) const SYNC_STATE_FILE: &str = "backup-sync-state.json";

/// The Vault's advertised backup contract. `None` = the Vault predates
/// GET /backup/limits - fall back to the legacy single-snapshot mode
/// (older Vaults also rotate named labels, which would eat per-conversation
/// objects, so the probe doubles as the safety gate).
struct BackupLimits {
    max_object_bytes: usize,
}

async fn fetch_backup_limits(port: u16) -> Option<BackupLimits> {
    let resp = http()
        .get(format!("http://127.0.0.1:{}/backup/limits", port))
        .header("Origin", VAULT_ORIGIN)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: serde_json::Value = resp.json().await.ok()?;
    if v["named_labels_rotate"].as_bool() != Some(false) {
        return None;
    }
    let max = v["max_object_bytes"].as_u64()? as usize;
    (max > 0).then_some(BackupLimits {
        max_object_bytes: max,
    })
}

pub(crate) fn gzip_bytes(data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Write;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::new(6));
    enc.write_all(data).map_err(|e| format!("gzip: {}", e))?;
    enc.finish().map_err(|e| format!("gzip: {}", e))
}

pub(crate) fn gunzip_bytes(data: &[u8]) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let mut out = Vec::new();
    flate2::read::GzDecoder::new(data)
        .read_to_end(&mut out)
        .map_err(|e| format!("gunzip: {}", e))?;
    Ok(out)
}

#[derive(serde::Serialize, serde::Deserialize, Clone)]
struct SyncEntry {
    records: usize,
    size: usize,
    labels: Vec<String>,
}

fn sync_state_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(SYNC_STATE_FILE))
}

fn load_sync_state(app: &tauri::AppHandle) -> std::collections::HashMap<String, SyncEntry> {
    sync_state_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_sync_state(app: &tauri::AppHandle, state: &std::collections::HashMap<String, SyncEntry>) {
    if let (Some(p), Ok(json)) = (sync_state_path(app), serde_json::to_string(state)) {
        if let Err(e) = std::fs::write(&p, json) {
            log::warn!("[escrow] could not persist backup sync state: {}", e);
        }
    }
}

/// Stable per-conversation object label, from the conversation record's
/// action hash (unique, survives renames, same across runs).
fn conv_label(c: &ConvBundle) -> Option<String> {
    let h = c.records.first()?["actionHash"].as_str()?;
    Some(format!("conv-{}", &h[..h.len().min(24)]))
}

/// Serialize a conversation into one or more gzipped part payloads, each
/// under the vault's per-object cap. Parts are built under the cap in
/// plaintext (gzip of JSON only shrinks), then verified compressed; a part
/// that somehow still exceeds the cap is split further by records.
fn build_conv_parts(
    c: &ConvBundle,
    base_label: &str,
    cap: usize,
) -> Result<Vec<(String, Vec<u8>)>, String> {
    let mut groups: Vec<Vec<&serde_json::Value>> = Vec::new();
    let mut cur: Vec<&serde_json::Value> = Vec::new();
    let mut cur_size = 0usize;
    for r in &c.records {
        let s = serde_json::to_vec(r).map(|v| v.len()).unwrap_or(0);
        if !cur.is_empty() && cur_size + s > cap {
            groups.push(std::mem::take(&mut cur));
            cur_size = 0;
        }
        cur.push(r);
        cur_size += s;
    }
    if !cur.is_empty() {
        groups.push(cur);
    }

    let mut work: std::collections::VecDeque<Vec<&serde_json::Value>> = groups.into();
    let mut compressed: Vec<Vec<u8>> = Vec::new();
    while let Some(g) = work.pop_front() {
        let payload = serde_json::json!({
            "version": 2,
            "role_name": c.installed_app_id,
            "started_at": c.started_at,
            "records": g,
        });
        let plain = serde_json::to_vec(&payload).map_err(|e| e.to_string())?;
        let gz = gzip_bytes(&plain)?;
        if gz.len() > cap {
            if g.len() <= 1 {
                return Err("one conversation record exceeds the vault's object cap".into());
            }
            let mid = g.len() / 2;
            let (a, b) = g.split_at(mid);
            work.push_front(b.to_vec());
            work.push_front(a.to_vec());
            continue;
        }
        compressed.push(gz);
    }

    let multi = compressed.len() > 1;
    Ok(compressed
        .into_iter()
        .enumerate()
        .map(|(i, bytes)| {
            let label = if multi {
                format!("{}.p{}", base_label, i)
            } else {
                base_label.to_string()
            };
            (label, bytes)
        })
        .collect())
}

async fn post_backup_object(port: u16, label: &str, bytes: &[u8]) -> Result<(), String> {
    use base64::Engine;
    let resp = http()
        .post(format!("http://127.0.0.1:{}/backup", port))
        .header("Origin", VAULT_ORIGIN)
        .json(&serde_json::json!({
            "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
            "app_name": "Your Own AI",
            "label": label,
            "data_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
            "content_type": "application/json+gzip",
        }))
        .send()
        .await
        .map_err(|e| format!("vault unreachable: {}", e))?;
    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        return Err(format!(
            "backup object {} rejected: {}",
            label,
            body["error"].as_str().unwrap_or("backup_failed")
        ));
    }
    Ok(())
}

async fn delete_backup_label(port: u16, label: &str) {
    let _ = http()
        .post(format!("http://127.0.0.1:{}/backup/delete", port))
        .header("Origin", VAULT_ORIGIN)
        .json(&serde_json::json!({
            "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
            "label": label,
        }))
        .send()
        .await;
}

/// Marker file left by the key restore: this device holds the right key
/// but has NOT replayed the Vault conversations yet. While it exists,
/// automatic backups are suspended - anything written from here (even
/// after a first chat) would overwrite the user's Vault copy with a
/// near-empty snapshot. Cleared by a successful conversation restore.
pub(crate) const RESTORE_PENDING_FILE: &str = "conversation-restore-pending";

pub(crate) fn restore_pending_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join(RESTORE_PENDING_FILE))
}

pub(crate) fn clear_restore_pending(app: &tauri::AppHandle) {
    if let Some(p) = restore_pending_path(app) {
        if p.exists() {
            let _ = std::fs::remove_file(&p);
            log::info!("[escrow] conversation restore complete - automatic backups resume");
        }
    }
}

/// Marker left by a conversation restore: the per-AI memory index (episodic
/// recall vectors + authored-knowledge vectors) is stale or missing and
/// should be rebuilt by the frontend walker once the embedding model is
/// available. Cleared by the walker when the rebuild finishes.
const REEMBED_PENDING_FILE: &str = "memory-reembed-pending";

pub(crate) fn reembed_pending_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join(REEMBED_PENDING_FILE))
}

pub(crate) fn set_reembed_pending(app: &tauri::AppHandle) {
    if let Some(p) = reembed_pending_path(app) {
        if let Err(e) = std::fs::write(&p, b"") {
            log::warn!("[escrow] could not write re-embed marker: {}", e);
        }
    }
}

/// One conversation with its entries, ready for assembly.
struct ConvBundle {
    installed_app_id: String,
    started_at: i64,
    /// Conversation record first, then its entries in chain order.
    records: Vec<serde_json::Value>,
    size: usize,
}

/// Decrypt one on-chain record; returns (plain JSON, raw entry bytes).
pub(crate) fn open_record(
    key: &[u8; 32],
    record: &holochain_types::prelude::Record,
) -> Option<(serde_json::Value, Vec<u8>)> {
    let entry = record.entry().as_option()?;
    let app_bytes = entry.as_app_entry()?;
    let raw = app_bytes.as_ref().bytes().to_vec();
    let ee: crate::commands_holochain::EncryptedEntryRaw = rmp_serde::from_slice(&raw).ok()?;
    let plain_bytes = crate::transcript_crypto::decrypt(key, &ee.nonce, &ee.cipher).ok()?;
    let plain: serde_json::Value = serde_json::from_slice(&plain_bytes).ok()?;
    Some((plain, raw))
}

/// Canonical-shape record object.
fn backup_record(
    entry_type: &str,
    record: &holochain_types::prelude::Record,
    human: serde_json::Value,
    raw: &[u8],
) -> serde_json::Value {
    use base64::Engine;
    let hash_hex = hex::encode(record.action_address().get_raw_39());
    let created_ms = record.action().timestamp().as_micros() / 1000;
    serde_json::json!({
        "entryType": entry_type,
        "actionHash": hash_hex,
        "createdAtMs": created_ms,
        "human_readable": human,
        "raw_record": {
            "entry_b64": base64::engine::general_purpose::STANDARD.encode(raw),
            "action_address": hash_hex,
            "action_type": "Create",
        },
    })
}

/// Walk every agent's conversations + entries, decrypting as we go.
/// Records that fail to decrypt (wrong key) are skipped, not fatal.
async fn collect_conversations(app: &tauri::AppHandle) -> Result<Vec<ConvBundle>, String> {
    use holochain_types::prelude::Record;

    let hc = app
        .try_state::<Arc<HolochainState>>()
        .ok_or("holochain state missing")?;
    let manager = hc.get()?;
    let key = manager.data_key()?;
    let agents: Vec<(String, String)> = {
        let map = manager.agents.lock().await;
        map.iter()
            .map(|(k, a)| (k.clone(), a.installed_app_id.clone()))
            .collect()
    };

    let mut bundles = Vec::new();
    for (agent_key, installed_app_id) in agents {
        let payload = ExternIO::encode(()).map_err(|e| e.to_string())?;
        let result = manager
            .call_zome(&agent_key, "transcript", "get_all_conversations", payload)
            .await
            .map_err(|e| format!("get_all_conversations({}): {}", agent_key, e))?;
        let conv_records: Vec<Record> = ExternIO::decode(&result).map_err(|e| e.to_string())?;

        for conv in &conv_records {
            let Some((conv_plain, conv_raw)) = open_record(&key, conv) else { continue };
            let started_at = conv_plain["started_at"].as_i64().unwrap_or(0);
            let conv_hash = conv.action_address().clone();
            let conv_hash_hex = hex::encode(conv_hash.get_raw_39());
            let mut records =
                vec![backup_record("Conversation", conv, conv_plain, &conv_raw)];

            let payload =
                ExternIO::encode(conv_hash).map_err(|e| e.to_string())?;
            let result = manager
                .call_zome(&agent_key, "transcript", "get_conversation_entries", payload)
                .await
                .map_err(|e| format!("get_conversation_entries: {}", e))?;
            let entry_records: Vec<Record> =
                ExternIO::decode(&result).map_err(|e| e.to_string())?;
            for er in &entry_records {
                let Some((mut plain, raw)) = open_record(&key, er) else { continue };
                let entry_type = if plain["annotation"] == "grounding" {
                    "GroundingAnnotation"
                } else {
                    "Message"
                };
                // Linkage lives in DHT links, which don't survive an export -
                // carry it inside the readable view instead.
                if let Some(obj) = plain.as_object_mut() {
                    obj.insert(
                        "conversation_action_hash".into(),
                        serde_json::json!(conv_hash_hex),
                    );
                }
                records.push(backup_record(entry_type, er, plain, &raw));
            }

            let size: usize = records
                .iter()
                .map(|r| serde_json::to_vec(r).map(|v| v.len()).unwrap_or(0))
                .sum();
            bundles.push(ConvBundle {
                installed_app_id: installed_app_id.clone(),
                started_at,
                records,
                size,
            });
        }
    }
    Ok(bundles)
}

/// Pure assembly: newest-first inclusion under the size budget, grouped
/// into cells per transcript app, canonical shape + app_keys + ai_configs.
fn assemble_full_payload(
    recovery: &RecoveryMaterial,
    ai_configs: Option<serde_json::Value>,
    mut convs: Vec<ConvBundle>,
    budget: usize,
) -> serde_json::Value {
    convs.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    let total = convs.len();
    let mut used = 0usize;
    let mut kept: Vec<ConvBundle> = Vec::new();
    for c in convs {
        if used + c.size <= budget {
            used += c.size;
            kept.push(c);
        }
    }
    let dropped = total - kept.len();

    // Oldest-first within each cell so the export reads chronologically.
    kept.sort_by(|a, b| a.started_at.cmp(&b.started_at));
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut cells: Vec<(String, Vec<serde_json::Value>)> = Vec::new();
    for c in kept {
        for r in &c.records {
            let t = r["entryType"].as_str().unwrap_or("Unknown").to_string();
            *counts.entry(t).or_insert(0) += 1;
        }
        match cells.iter_mut().find(|(id, _)| *id == c.installed_app_id) {
            Some((_, records)) => records.extend(c.records),
            None => cells.push((c.installed_app_id, c.records)),
        }
    }
    let total_records: usize = counts.values().sum();

    let mut payload = serde_json::json!({
        "version": 1,
        "_readme": "Your Own AI conversation transcripts, backed up from your device. \
                    Each record's human_readable view is the decrypted content; \
                    raw_record holds the encrypted on-chain bytes. The app_keys \
                    block holds the encryption key and network seed. Keep any \
                    export of this file private.",
        "license": "Cryptographic Autonomy License v1.0 (CAL-1.0)",
        "app": { "name": "Your Own AI", "client_id": YOAI_HOLOCHAIN_CLIENT_ID },
        "exported_at_iso": iso_now(),
        "_summary": {
            "countsByEntryType": counts,
            "totalRecords": total_records,
        },
        "cells": cells
            .into_iter()
            .map(|(id, records)| {
                serde_json::json!({
                    "role_name": id,
                    "_readme": "One AI personality's transcript chain.",
                    "records": records,
                })
            })
            .collect::<Vec<_>>(),
        "app_keys": {
            "_readme": "The transcript encryption key (hex) and per-user network \
                        seed Your Own AI generates locally. Needed to decrypt this \
                        user's transcripts on any device.",
            "transcript_data_key_hex": recovery.data_key_hex,
            "transcript_network_seed": recovery.network_seed,
            "version": recovery.version,
        },
    });
    if dropped > 0 {
        payload["truncated_conversations"] = serde_json::json!(dropped);
        payload["_truncation_note"] = serde_json::json!(format!(
            "{} conversation(s) did not fit this Vault's single-snapshot size \
             budget and were left out (a conversation larger than the budget \
             can never be included, however recent). Update Flowsta Vault - \
             newer versions store each conversation separately with no \
             overall budget.",
            dropped
        ));
    }
    if let Some(cfg) = ai_configs {
        payload["ai_configs"] = serde_json::json!({
            "_readme": "Your AI personalities (names, personas, settings) as \
                        stored on this device. Thumbnails are not included.",
            "data": cfg,
        });
    }
    payload
}

/// Whether a backup holding `local_records` records may overwrite the
/// existing Vault snapshot (`existing_records` = None when there is none).
/// An EMPTY snapshot never overwrites a non-empty one: a device that just
/// restored its key (or hasn't restored conversations yet) has zero local
/// records while the Vault may hold the user's only copy - the launch-sync
/// backup fired in that window would silently destroy it.
///
/// A NON-empty local set is allowed to shrink the snapshot (deleting a
/// conversation is legitimate) - but only because the key-coherence gate in
/// `write_full_backup` runs first: a device whose transcript key does not
/// match the Vault's escrow can never reach this predicate, so "1 local
/// conversation vs 96 in Vault" only happens on the device that authored
/// the slot (or explicitly adopted it).
fn may_overwrite(local_records: u64, existing_records: Option<u64>) -> bool {
    local_records > 0 || existing_records.unwrap_or(0) == 0
}

/// Record count of the existing snapshot in Vault - the incremental
/// manifest when present, else the legacy "latest" monolith.
/// `Ok(None)` = no snapshot stored. A snapshot whose summary can't be read
/// counts as non-empty - when in doubt, don't overwrite.
async fn fetch_existing_data_records(port: u16) -> Result<Option<u64>, String> {
    for label in [MANIFEST_LABEL, DATA_LABEL] {
        let resp = http()
            .post(format!("http://127.0.0.1:{}/backup/retrieve", port))
            .header("Origin", VAULT_ORIGIN)
            .json(&serde_json::json!({
                "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
                "label": label,
            }))
            .send()
            .await
            .map_err(|e| format!("vault unreachable: {}", e))?;
        match resp.status() {
            reqwest::StatusCode::NOT_FOUND => continue,
            s if s.is_success() => {}
            _ => {
                let body: serde_json::Value = resp.json().await.unwrap_or_default();
                return Err(body["error"].as_str().unwrap_or("retrieve_failed").to_string());
            }
        }
        let v: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| format!("bad retrieve response: {}", e))?;
        return Ok(Some(
            v["data"]["_summary"]["totalRecords"].as_u64().unwrap_or(1),
        ));
    }
    Ok(None)
}

/// Your Memory profile-facts store - a local encrypted file, NOT Holochain
/// data, so it needs its own ride in the backup.
pub(crate) const FACTS_FILE: &str = "memory-facts.enc";
/// A thumbnail bigger than this is skipped rather than bloating the backup.
const THUMBNAIL_CAP_BYTES: u64 = 2 * 1024 * 1024;

/// Decrypt the facts file to its CAL-readable view: the fact list minus the
/// per-fact `embedding` vectors (bulky derived data, not human-meaningful).
/// `None` = wrong key or unreadable envelope.
pub(crate) fn facts_human_readable(
    key: &[u8; 32],
    file_bytes: &[u8],
) -> Option<serde_json::Value> {
    let envelope: serde_json::Value = serde_json::from_slice(file_bytes).ok()?;
    let nonce = hex::decode(envelope["nonce"].as_str()?).ok()?;
    let cipher = hex::decode(envelope["cipher"].as_str()?).ok()?;
    let plain = crate::transcript_crypto::decrypt(key, &nonce, &cipher).ok()?;
    let mut facts: serde_json::Value = serde_json::from_slice(&plain).ok()?;
    if let Some(arr) = facts.as_array_mut() {
        for fact in arr {
            if let Some(obj) = fact.as_object_mut() {
                obj.remove("embedding");
            }
        }
    }
    Some(facts)
}

/// Per-AI thumbnails as base64, keyed by AI id. Only ids in `ai_ids` are
/// collected (orphaned files stay local), oversized ones are skipped.
fn collect_thumbnails(app: &tauri::AppHandle, ai_ids: &[String]) -> serde_json::Map<String, serde_json::Value> {
    use base64::Engine;
    let mut out = serde_json::Map::new();
    let Ok(dir) = app.path().app_data_dir() else { return out };
    for id in ai_ids {
        let path = dir.join("thumbnails").join(format!("{}.jpg", id));
        let Ok(meta) = std::fs::metadata(&path) else { continue };
        if meta.len() > THUMBNAIL_CAP_BYTES {
            log::info!("[escrow] thumbnail for {} skipped ({} bytes over cap)", id, meta.len());
            continue;
        }
        if let Ok(bytes) = std::fs::read(&path) {
            out.insert(
                id.clone(),
                serde_json::json!(base64::engine::general_purpose::STANDARD.encode(bytes)),
            );
        }
    }
    out
}

/// Per-AI authored knowledge (kind "authored" in the transcript-embedding
/// store), keyed by AI id, text only - the vectors are derived data,
/// re-embedded on the restored device. Unlike episodic entries (rebuilt
/// from the replayed transcripts), authored knowledge exists nowhere else,
/// so leaving it out of the backup would lose it for good on device loss.
fn collect_ai_knowledge(
    app: &tauri::AppHandle,
    key: &[u8; 32],
    ai_ids: &[String],
) -> serde_json::Map<String, serde_json::Value> {
    let mut out = serde_json::Map::new();
    for id in ai_ids {
        let Ok(entries) = crate::transcript_memory::read_for(app, key, id) else { continue };
        let authored: Vec<serde_json::Value> = entries
            .iter()
            .filter(|e| e.kind == "authored")
            .map(|e| serde_json::json!({ "text": e.text, "created_at": e.created_at }))
            .collect();
        if !authored.is_empty() {
            out.insert(id.clone(), serde_json::json!(authored));
        }
    }
    out
}

/// Build + write the full-data backup. Returns small stats for callers/UI.
/// Everything that rides the backup besides conversation records: Your
/// Memory profile facts, per-AI thumbnails, and authored knowledge. Applied
/// to the legacy monolith and the incremental manifest identically.
fn attach_extras(
    app: &tauri::AppHandle,
    recovery: &RecoveryMaterial,
    payload: &mut serde_json::Value,
) {
    // Your Memory profile facts: local encrypted file, not Holochain data.
    // Carried readable (CAL) + as the raw file for a lossless restore.
    if let Ok(dir) = app.path().app_data_dir() {
        let facts_path = dir.join(FACTS_FILE);
        if let Ok(bytes) = std::fs::read(&facts_path) {
            use base64::Engine;
            let mut block = serde_json::json!({
                "_readme": "Your Memory - the profile facts your AIs have learned \
                            (or you authored), as stored on this device. \
                            human_readable is the decrypted view; raw_b64 is the \
                            encrypted file itself, restorable with app_keys.",
                "raw_b64": base64::engine::general_purpose::STANDARD.encode(&bytes),
            });
            if let Ok(key) = recovery.data_key() {
                if let Some(human) = facts_human_readable(&key, &bytes) {
                    block["human_readable"] = human;
                }
            }
            payload["memory_facts"] = block;
        }
    }

    // Per-AI thumbnails (small jpgs), keyed by AI id.
    let ai_ids: Vec<String> = payload["ai_configs"]["data"]["custom-ais"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .filter_map(|a| a["id"].as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    let thumbs = collect_thumbnails(app, &ai_ids);
    if !thumbs.is_empty() {
        payload["thumbnails"] = serde_json::json!({
            "_readme": "Your AIs' images (base64 JPEG), keyed by AI id.",
            "data": thumbs,
        });
    }

    // Per-AI authored knowledge - user-given, not derivable from transcripts.
    if let Ok(key) = recovery.data_key() {
        let knowledge = collect_ai_knowledge(app, &key, &ai_ids);
        if !knowledge.is_empty() {
            payload["ai_knowledge"] = serde_json::json!({
                "_readme": "Knowledge you gave each AI (its Remembers tab), keyed \
                            by AI id. Embedding vectors are rebuilt on restore.",
                "data": knowledge,
            });
        }
        // The document library: records, summaries, flags and grants only.
        // Passages and vectors stay on the device - they are re-derivable
        // from the person's own files, and a library must never be the
        // reason a backup fails (planning/KNOWLEDGE_CORPUS.md).
        match crate::corpus::records_for_backup(app, &key) {
            Ok(mut records) if !records.documents.is_empty() => {
                let trimmed = fit_corpus_records(&mut records, CORPUS_RECORDS_BUDGET_BYTES);
                if trimmed > 0 {
                    log::warn!("[escrow] corpus records over budget: {} cards left out of the backup", trimmed);
                }
                payload["corpus"] = serde_json::json!({
                    "_readme": "Your document library: which documents your AIs \
                                were given, their summaries and flags, and which \
                                AI may draw on each. The text itself is read \
                                again from your files after a restore.",
                    "data": records,
                });
            }
            Ok(_) => {}
            Err(e) => log::warn!("[escrow] corpus records skipped: {e}"),
        }
    }
}

/// Keep the library's records under `budget` serialized bytes by dropping
/// cards from the oldest documents first. Returns how many cards went.
fn fit_corpus_records(records: &mut crate::corpus::CorpusRecords, budget: usize) -> usize {
    let size = |r: &crate::corpus::CorpusRecords| serde_json::to_vec(r).map(|v| v.len()).unwrap_or(0);
    if size(records) <= budget {
        return 0;
    }
    let mut order: Vec<usize> = (0..records.documents.len()).collect();
    order.sort_by_key(|&i| records.documents[i].added_at);
    let mut dropped = 0;
    for i in order {
        if records.documents[i].meta.summary.take().is_some() {
            dropped += 1;
            if size(records) <= budget {
                break;
            }
        }
    }
    dropped
}

/// Incremental mode: one gzipped object per conversation + a manifest.
/// Only conversations with new records upload; nothing is ever left out.
async fn write_incremental_backup(
    app: &tauri::AppHandle,
    port: u16,
    limits: &BackupLimits,
    recovery: &RecoveryMaterial,
    ai_configs: Option<serde_json::Value>,
    convs: Vec<ConvBundle>,
) -> Result<serde_json::Value, String> {
    let mut sync = load_sync_state(app);
    let mut counts: std::collections::BTreeMap<String, u64> = std::collections::BTreeMap::new();
    let mut total_records = 0u64;
    let mut index: Vec<serde_json::Value> = Vec::new();
    let mut uploaded_objects = 0usize;
    let mut unchanged = 0usize;

    let mut ordered = convs;
    ordered.sort_by(|a, b| a.started_at.cmp(&b.started_at));

    for c in &ordered {
        let Some(label) = conv_label(c) else { continue };
        for r in &c.records {
            let t = r["entryType"].as_str().unwrap_or("Unknown").to_string();
            *counts.entry(t).or_insert(0) += 1;
            total_records += 1;
        }

        let prev = sync.get(&label);
        let labels: Vec<String> = if prev
            .map(|p| p.records == c.records.len() && p.size == c.size)
            .unwrap_or(false)
        {
            unchanged += 1;
            prev.map(|p| p.labels.clone()).unwrap_or_default()
        } else {
            let parts = build_conv_parts(c, &label, limits.max_object_bytes)?;
            for (part_label, bytes) in &parts {
                post_backup_object(port, part_label, bytes).await?;
                uploaded_objects += 1;
            }
            let labels: Vec<String> = parts.into_iter().map(|(l, _)| l).collect();
            sync.insert(
                label.clone(),
                SyncEntry {
                    records: c.records.len(),
                    size: c.size,
                    labels: labels.clone(),
                },
            );
            labels
        };

        index.push(serde_json::json!({
            "label": label,
            "role_name": c.installed_app_id,
            "started_at": c.started_at,
            "records": c.records.len(),
            "labels": labels,
        }));
    }

    let mut manifest = serde_json::json!({
        "version": 2,
        "_readme": "Your Own AI backup manifest. Conversations are stored as \
                    separate compressed objects - one per conversation, listed \
                    under conversations[].labels. The app_keys block holds the \
                    encryption key and network seed. Keep any export of this \
                    file private.",
        "license": "Cryptographic Autonomy License v1.0 (CAL-1.0)",
        "app": { "name": "Your Own AI", "client_id": YOAI_HOLOCHAIN_CLIENT_ID },
        "exported_at_iso": iso_now(),
        "_summary": {
            "countsByEntryType": counts,
            "totalRecords": total_records,
        },
        "conversations": index,
        "app_keys": {
            "_readme": "The transcript encryption key (hex) and per-user network \
                        seed Your Own AI generates locally. Needed to decrypt this \
                        user's transcripts on any device.",
            "transcript_data_key_hex": recovery.data_key_hex,
            "transcript_network_seed": recovery.network_seed,
            "version": recovery.version,
        },
    });
    if let Some(cfg) = ai_configs {
        manifest["ai_configs"] = serde_json::json!({
            "_readme": "Your AI personalities (names, personas, settings) as \
                        stored on this device. Thumbnails are not included.",
            "data": cfg,
        });
    }
    attach_extras(app, recovery, &mut manifest);

    let resp = http()
        .post(format!("http://127.0.0.1:{}/backup", port))
        .header("Origin", VAULT_ORIGIN)
        .json(&serde_json::json!({
            "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
            "app_name": "Your Own AI",
            "label": MANIFEST_LABEL,
            "data": manifest,
        }))
        .send()
        .await
        .map_err(|e| format!("vault unreachable: {}", e))?;
    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        return Err(body["error"].as_str().unwrap_or("backup_failed").to_string());
    }

    // The manifest is now the authoritative snapshot - retire the legacy
    // monolith so a stale copy can't shadow it (restore prefers the
    // manifest, but Your Data would show two conflicting snapshots).
    delete_backup_label(port, DATA_LABEL).await;
    save_sync_state(app, &sync);

    log::info!(
        "[escrow] incremental backup: {} conversations ({} records), {} object(s) uploaded, {} unchanged",
        index.len(),
        total_records,
        uploaded_objects,
        unchanged,
    );
    Ok(serde_json::json!({
        "mode": "incremental",
        "conversations": index.len(),
        "records": total_records,
        "truncated_conversations": 0,
        "uploaded_objects": uploaded_objects,
        "unchanged_conversations": unchanged,
    }))
}

fn skipped(reason: &str) -> serde_json::Value {
    serde_json::json!({
        "skipped": reason,
        "conversations": 0, "records": 0, "truncated_conversations": 0,
    })
}

// ── Write gates (probe seam for unit tests) ────────────────────────────
//
// The gate CASCADE is the protection and its ORDER is load-bearing:
// identity gate (inside escrow_port) → restore-pending hold → key
// coherence → slot read-before-write. The probes are Vault bridge calls,
// so the decision logic lives behind this trait; tests script probe
// results and assert outcomes AND call order.

pub(crate) trait GateProbes {
    /// Record count of the existing snapshot; `Ok(None)` = no snapshot.
    async fn existing_data_records(&self) -> Result<Option<u64>, String>;
    /// The escrowed recovery material; `Ok(None)` = no escrow stored.
    async fn escrow(&self) -> Result<Option<RecoveryMaterial>, String>;
}

struct VaultGateProbes {
    port: u16,
}

impl GateProbes for VaultGateProbes {
    async fn existing_data_records(&self) -> Result<Option<u64>, String> {
        fetch_existing_data_records(self.port).await
    }
    async fn escrow(&self) -> Result<Option<RecoveryMaterial>, String> {
        fetch_escrow(self.port).await
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum PreflightSkip {
    /// Conversations haven't been replayed from the Vault yet - a snapshot
    /// from this device would shadow the user's copy.
    RestorePending,
    /// The Vault escrow holds recovery material for a DIFFERENT transcript
    /// key - this device may not touch the data slots.
    EscrowConflict,
    /// The escrow could not be read - when in doubt, never write.
    EscrowProbeFailed(String),
}

pub(crate) enum Preflight {
    Proceed { recovery: RecoveryMaterial },
    Skip(PreflightSkip),
}

/// The pre-collection gates, in order.
///
/// Restore-pending hold: the key came from Vault but the conversations
/// haven't been replayed yet - sit tight until the restore runs, unless
/// the Vault slot turns out to be empty (nothing to protect;
/// `on_pending_resolved` fires so the caller can clear the marker, even
/// when a LATER gate then skips - the pending question itself is settled).
///
/// Key-coherence gate: the data backup never writes while the Vault's
/// escrowed recovery material differs from this device's. A fresh install
/// generates a NEW random transcript key, so a machine that was never
/// restored can't pass this gate while the Vault still holds another
/// device's world - resolving the conflict (restore, or an explicit "keep
/// this device's key") is what unlocks data backups. This closes the
/// launch-sync window where one first chat message used to bypass the
/// empty-slot guard and rewrite the manifest.
///
/// `local_recovery` is a closure on purpose: it must not run (or fail)
/// before the restore-pending hold has had its say.
pub(crate) async fn preflight_gates<P: GateProbes>(
    probes: &P,
    restore_pending: bool,
    on_pending_resolved: impl FnOnce(),
    local_recovery: impl FnOnce() -> Result<RecoveryMaterial, String>,
) -> Result<Preflight, String> {
    if restore_pending {
        match probes.existing_data_records().await {
            Ok(existing) if existing.unwrap_or(0) == 0 => on_pending_resolved(),
            _ => return Ok(Preflight::Skip(PreflightSkip::RestorePending)),
        }
    }

    let recovery = local_recovery()?;

    match probes.escrow().await {
        Ok(Some(ref m)) if *m == recovery => {}
        Ok(None) => {}
        Ok(Some(_)) => return Ok(Preflight::Skip(PreflightSkip::EscrowConflict)),
        Err(e) => return Ok(Preflight::Skip(PreflightSkip::EscrowProbeFailed(e))),
    }

    Ok(Preflight::Proceed { recovery })
}

#[derive(Debug, PartialEq)]
pub(crate) enum SlotSkip {
    EmptyWouldOverwrite,
    ProbeFailed(String),
}

/// Slot gate: never write a slot that was not first successfully read.
/// The probe runs on EVERY backup (a confirmed-absent slot counts as a
/// successful read); any probe failure skips - when in doubt, never
/// overwrite. Plus the standing rule: an empty snapshot never overwrites
/// records.
pub(crate) async fn slot_gate<P: GateProbes>(
    probes: &P,
    local_records: u64,
) -> Option<SlotSkip> {
    match probes.existing_data_records().await {
        Ok(existing) if may_overwrite(local_records, existing) => None,
        Ok(_) => Some(SlotSkip::EmptyWouldOverwrite),
        Err(e) => Some(SlotSkip::ProbeFailed(e)),
    }
}

/// Outcome of the most recent backup attempt, for the settings panel.
/// A refusal - the app's own gates OR the Vault refusing the write - must
/// be visible in the UI, never log-only: the panel says backups are
/// automatic, so silence reads as success.
static LAST_BACKUP_OUTCOME: std::sync::Mutex<Option<serde_json::Value>> =
    std::sync::Mutex::new(None);

fn record_backup_outcome(app: &tauri::AppHandle, outcome: serde_json::Value) {
    if let Ok(mut slot) = LAST_BACKUP_OUTCOME.lock() {
        *slot = Some(outcome.clone());
    }
    use tauri::Emitter;
    let _ = app.emit("escrow-backup-outcome", outcome);
}

/// The most recent backup attempt's outcome (None = no attempt yet).
#[tauri::command]
pub fn last_backup_outcome() -> Option<serde_json::Value> {
    LAST_BACKUP_OUTCOME.lock().ok().and_then(|s| s.clone())
}

pub async fn write_full_backup(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    let result = write_full_backup_inner(app).await;
    match &result {
        Ok(v) => {
            if let Some(reason) = v.get("skipped").and_then(|r| r.as_str()) {
                record_backup_outcome(
                    app,
                    serde_json::json!({ "status": "held", "reason": reason, "at": iso_now() }),
                );
            } else {
                record_backup_outcome(
                    app,
                    serde_json::json!({
                        "status": "ok",
                        "records": v.get("records"),
                        "at": iso_now(),
                    }),
                );
            }
        }
        // Not-linked / Vault-away are idle states, not refusals - clear any
        // stale notice rather than alarming about a Vault that's just closed.
        Err(e) if e == "unlinked" || e == "vault_unavailable" => {}
        Err(e) => {
            record_backup_outcome(
                app,
                serde_json::json!({ "status": "failed", "reason": e, "at": iso_now() }),
            );
        }
    }
    result
}

async fn write_full_backup_inner(app: &tauri::AppHandle) -> Result<serde_json::Value, String> {
    let port = match escrow_port(app, true).await {
        Ok(p) => p,
        Err(s) => return Err(s.error.unwrap_or(s.state)),
    };
    let probes = VaultGateProbes { port };
    let pending = restore_pending_path(app).map(|p| p.exists()).unwrap_or(false);

    let recovery = match preflight_gates(
        &probes,
        pending,
        || clear_restore_pending(app),
        || local_recovery(app),
    )
    .await?
    {
        Preflight::Proceed { recovery } => recovery,
        Preflight::Skip(PreflightSkip::RestorePending) => {
            log::warn!(
                "[escrow] full backup skipped: conversation restore from Vault is \
                 pending on this device (Settings → Restore conversations from Vault)"
            );
            return Ok(skipped("restore_pending"));
        }
        Preflight::Skip(PreflightSkip::EscrowConflict) => {
            log::warn!(
                "[escrow] full backup skipped: the Vault holds recovery material \
                 for a different transcript key - resolve the key conflict first \
                 (Settings → Flowsta Account)"
            );
            return Ok(skipped("escrow_conflict"));
        }
        Preflight::Skip(PreflightSkip::EscrowProbeFailed(e)) => {
            log::warn!("[escrow] full backup skipped: could not read the escrow: {}", e);
            return Ok(skipped("probe_failed"));
        }
    };

    let convs = collect_conversations(app).await?;
    let local_records: u64 = convs.iter().map(|c| c.records.len() as u64).sum();

    match slot_gate(&probes, local_records).await {
        None => {}
        Some(SlotSkip::EmptyWouldOverwrite) => {
            log::warn!(
                "[escrow] full backup skipped: no local conversations but the \
                 Vault snapshot has records - refusing to overwrite it \
                 (restore conversations from Vault to bring them back)"
            );
            return Ok(skipped("empty_would_overwrite"));
        }
        Some(SlotSkip::ProbeFailed(e)) => {
            log::warn!("[escrow] full backup skipped: could not probe the existing snapshot: {}", e);
            return Ok(skipped("probe_failed"));
        }
    }
    let ai_configs = app
        .path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("ai-data.json"))
        .filter(|p| p.exists())
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());

    // Per-conversation incremental mode whenever the Vault supports it
    // (advertises /backup/limits and guarantees named labels never rotate).
    // EVERYTHING is backed up - no budget, nothing left out; only changed
    // conversations re-upload. Older Vaults get the legacy single snapshot.
    if let Some(limits) = fetch_backup_limits(port).await {
        return write_incremental_backup(app, port, &limits, &recovery, ai_configs, convs).await;
    }

    let conversations = convs.len();
    let mut payload = assemble_full_payload(&recovery, ai_configs, convs, BACKUP_BUDGET_BYTES);
    let total_records = payload["_summary"]["totalRecords"].as_u64().unwrap_or(0);
    let truncated = payload["truncated_conversations"].as_u64().unwrap_or(0);

    attach_extras(app, &recovery, &mut payload);

    let resp = http()
        .post(format!("http://127.0.0.1:{}/backup", port))
        .header("Origin", VAULT_ORIGIN)
        .json(&serde_json::json!({
            "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
            "app_name": "Your Own AI",
            "label": DATA_LABEL,
            "data": payload,
        }))
        .send()
        .await
        .map_err(|e| format!("vault unreachable: {}", e))?;
    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        return Err(body["error"].as_str().unwrap_or("backup_failed").to_string());
    }
    log::info!(
        "[escrow] full backup written: {} conversations, {} records{}",
        conversations,
        total_records,
        if truncated > 0 { format!(" ({} truncated)", truncated) } else { String::new() }
    );
    Ok(serde_json::json!({
        "conversations": conversations,
        "records": total_records,
        "truncated_conversations": truncated,
    }))
}

/// Debounced trigger: transcript writes call this freely; one backup runs
/// ~60s after the last write. Silent no-op when not linked / Vault away.
pub fn schedule_full_backup(app: &tauri::AppHandle) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static GENERATION: AtomicU64 = AtomicU64::new(0);
    let my_gen = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        if GENERATION.load(Ordering::SeqCst) != my_gen {
            return; // a newer write re-armed the debounce
        }
        match write_full_backup(&app).await {
            Ok(_) => {}
            Err(e) if e == "unlinked" || e == "vault_unavailable" => {
                log::debug!("[escrow] full backup skipped: {}", e);
            }
            Err(e) => log::warn!("[escrow] full backup failed: {}", e),
        }
    });
}

/// Manual full backup for the UI ("Back up now") and for testing.
#[tauri::command]
pub async fn vault_backup_now(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    write_full_backup(&app).await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bundle_with(n_records: usize, pad: usize) -> ConvBundle {
        let records: Vec<serde_json::Value> = (0..n_records)
            .map(|i| {
                serde_json::json!({
                    "entryType": "Message",
                    "actionHash": format!("{:039x}", i + 0xabc),
                    "human_readable": {"content": "x".repeat(pad)},
                })
            })
            .collect();
        let size = records
            .iter()
            .map(|r| serde_json::to_vec(r).unwrap().len())
            .sum();
        ConvBundle {
            installed_app_id: "ai-1".into(),
            started_at: 1000,
            records,
            size,
        }
    }

    #[test]
    fn conv_label_is_stable_hash_prefix() {
        let c = bundle_with(2, 10);
        let l = conv_label(&c).unwrap();
        assert!(l.starts_with("conv-"));
        assert_eq!(l, conv_label(&c).unwrap());
        assert_eq!(l.len(), "conv-".len() + 24);
    }

    #[test]
    fn small_conversation_is_one_object_that_roundtrips() {
        let c = bundle_with(5, 100);
        let parts = build_conv_parts(&c, "conv-x", 1024 * 1024).unwrap();
        assert_eq!(parts.len(), 1);
        assert_eq!(parts[0].0, "conv-x");
        let plain = gunzip_bytes(&parts[0].1).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&plain).unwrap();
        assert_eq!(v["records"].as_array().unwrap().len(), 5);
        assert_eq!(v["role_name"], "ai-1");
    }

    #[test]
    fn oversized_conversation_splits_ordered_under_cap() {
        // Records of ~30KB against a 64KB cap force multiple parts.
        // Incompressible-ish content (repeat compresses well, so use the
        // record COUNT to force plaintext grouping over the cap).
        let c = bundle_with(40, 30_000);
        let cap = 64 * 1024;
        let parts = build_conv_parts(&c, "conv-big", cap).unwrap();
        assert!(parts.len() > 1);
        let mut all: Vec<String> = Vec::new();
        for (i, (label, bytes)) in parts.iter().enumerate() {
            assert_eq!(label, &format!("conv-big.p{}", i));
            assert!(bytes.len() <= cap);
            let v: serde_json::Value =
                serde_json::from_slice(&gunzip_bytes(bytes).unwrap()).unwrap();
            for r in v["records"].as_array().unwrap() {
                all.push(r["actionHash"].as_str().unwrap().to_string());
            }
        }
        // Every record present exactly once, in original chain order.
        let expected: Vec<String> = c
            .records
            .iter()
            .map(|r| r["actionHash"].as_str().unwrap().to_string())
            .collect();
        assert_eq!(all, expected);
    }

    fn material() -> RecoveryMaterial {
        RecoveryMaterial {
            network_seed: "yoai-user-00ff".into(),
            data_key_hex: "ab".repeat(32),
            version: 1,
        }
    }

    #[test]
    fn payload_roundtrips_through_app_keys() {
        let m = material();
        let payload = escrow_payload(&m);
        // The shape Vault recognizes as canonical (version 1 + cells array).
        assert_eq!(payload["version"], 1);
        assert!(payload["cells"].is_array());
        let parsed = parse_escrow(&payload).expect("app_keys parse");
        assert!(parsed == m);
    }

    #[test]
    fn parse_rejects_missing_keys() {
        assert!(parse_escrow(&serde_json::json!({})).is_none());
        assert!(parse_escrow(&serde_json::json!({ "app_keys": {} })).is_none());
        // Missing version defaults to 1 instead of failing.
        let v = serde_json::json!({ "app_keys": {
            "transcript_data_key_hex": "aa",
            "transcript_network_seed": "seed",
        }});
        assert_eq!(parse_escrow(&v).unwrap().version, 1);
    }

    #[test]
    fn facts_roundtrip_strips_embeddings() {
        let key = [9u8; 32];
        let facts = serde_json::json!([
            { "id": "f1", "subject": "user", "predicate": "likes", "value": "sailing",
              "embedding": [0.1, 0.2, 0.3] },
            { "id": "f2", "subject": "user", "predicate": "name", "value": "Eric" },
        ]);
        let plain = serde_json::to_vec(&facts).unwrap();
        let (nonce, cipher) = crate::transcript_crypto::encrypt(&key, &plain).unwrap();
        let file = serde_json::to_vec(&serde_json::json!({
            "version": 1, "nonce": hex::encode(nonce), "cipher": hex::encode(cipher),
        }))
        .unwrap();
        let human = facts_human_readable(&key, &file).expect("decrypts");
        assert_eq!(human[0]["value"], "sailing");
        assert!(human[0].get("embedding").is_none()); // bulky vector stripped
        assert_eq!(human[1]["value"], "Eric");
        // Wrong key = None, never garbage.
        assert!(facts_human_readable(&[1u8; 32], &file).is_none());
    }

    #[test]
    fn empty_snapshot_never_overwrites_records() {
        assert!(!may_overwrite(0, Some(96))); // the launch-sync-after-key-restore trap
        assert!(may_overwrite(0, None)); // first-ever backup
        assert!(may_overwrite(0, Some(0))); // existing snapshot already empty
        // Non-empty writes may shrink the slot - safe ONLY behind the
        // key-coherence gate (a foreign device can't hold the slot's key).
        assert!(may_overwrite(1, Some(96)));
    }

    #[test]
    fn iso_now_shape() {
        let s = iso_now();
        assert_eq!(s.len(), 20);
        assert!(s.ends_with('Z'));
        assert!(s.starts_with("20"));
    }

    fn bundle(app_id: &str, started_at: i64, n_msgs: usize, pad: usize) -> ConvBundle {
        let mut records = vec![serde_json::json!({
            "entryType": "Conversation",
            "human_readable": { "started_at": started_at, "pad": "x".repeat(pad) },
        })];
        for i in 0..n_msgs {
            records.push(serde_json::json!({
                "entryType": "Message",
                "human_readable": { "sequence": i, "pad": "x".repeat(pad) },
            }));
        }
        let size = records
            .iter()
            .map(|r| serde_json::to_vec(r).unwrap().len())
            .sum();
        ConvBundle {
            installed_app_id: app_id.to_string(),
            started_at,
            records,
            size,
        }
    }

    #[test]
    fn full_payload_shape_counts_and_grouping() {
        let m = material();
        let payload = assemble_full_payload(
            &m,
            Some(serde_json::json!({ "ais": [{ "name": "Reeves" }] })),
            vec![bundle("transcript_a", 100, 2, 0), bundle("transcript_a", 200, 1, 0),
                 bundle("transcript_b", 150, 3, 0)],
            usize::MAX,
        );
        assert_eq!(payload["version"], 1);
        assert_eq!(payload["_summary"]["countsByEntryType"]["Conversation"], 3);
        assert_eq!(payload["_summary"]["countsByEntryType"]["Message"], 6);
        assert_eq!(payload["_summary"]["totalRecords"], 9);
        // Two cells, records grouped per app; app_keys + ai_configs present.
        assert_eq!(payload["cells"].as_array().unwrap().len(), 2);
        assert!(parse_escrow(&payload).unwrap() == m);
        assert_eq!(payload["ai_configs"]["data"]["ais"][0]["name"], "Reeves");
        assert!(payload.get("truncated_conversations").is_none());
    }

    #[test]
    fn full_payload_truncates_oldest_first() {
        let m = material();
        // (see fit_corpus_records_drops_cards_oldest_first below)
        // Three conversations of ~equal size; budget fits roughly two.
        let convs = vec![
            bundle("app", 100, 1, 400), // oldest - should be dropped
            bundle("app", 300, 1, 400),
            bundle("app", 200, 1, 400),
        ];
        let budget = convs[0].size * 2 + 10;
        let payload = assemble_full_payload(&m, None, convs, budget);
        assert_eq!(payload["truncated_conversations"], 1);
        assert_eq!(payload["_summary"]["countsByEntryType"]["Conversation"], 2);
        // The kept ones are the two NEWEST (started_at 300 + 200).
        let records = payload["cells"][0]["records"].as_array().unwrap();
        let starts: Vec<i64> = records
            .iter()
            .filter(|r| r["entryType"] == "Conversation")
            .map(|r| r["human_readable"]["started_at"].as_i64().unwrap())
            .collect();
        assert_eq!(starts, vec![200, 300]); // chronological within the cell
    }

    // ── Guard sequencing (the write gates behind GateProbes) ────────────

    use std::cell::RefCell;
    use std::collections::VecDeque;

    struct Scripted {
        existing: RefCell<VecDeque<Result<Option<u64>, String>>>,
        escrow: RefCell<VecDeque<Result<Option<RecoveryMaterial>, String>>>,
        calls: RefCell<Vec<&'static str>>,
    }

    impl Scripted {
        fn new(
            existing: Vec<Result<Option<u64>, String>>,
            escrow: Vec<Result<Option<RecoveryMaterial>, String>>,
        ) -> Self {
            Self {
                existing: RefCell::new(existing.into()),
                escrow: RefCell::new(escrow.into()),
                calls: RefCell::new(Vec::new()),
            }
        }
        fn calls(&self) -> Vec<&'static str> {
            self.calls.borrow().clone()
        }
    }

    impl GateProbes for Scripted {
        async fn existing_data_records(&self) -> Result<Option<u64>, String> {
            self.calls.borrow_mut().push("existing");
            self.existing
                .borrow_mut()
                .pop_front()
                .expect("gate probed existing_data_records more often than scripted")
        }
        async fn escrow(&self) -> Result<Option<RecoveryMaterial>, String> {
            self.calls.borrow_mut().push("escrow");
            self.escrow
                .borrow_mut()
                .pop_front()
                .expect("gate probed escrow more often than scripted")
        }
    }

    fn other_material() -> RecoveryMaterial {
        RecoveryMaterial {
            network_seed: "yoai-user-ffff".into(),
            data_key_hex: "cd".repeat(32),
            version: 1,
        }
    }

    #[tokio::test]
    async fn happy_path_probes_escrow_only_then_slot_reads_before_write() {
        let p = Scripted::new(
            vec![Ok(Some(5))],
            vec![Ok(Some(material()))],
        );
        let out = preflight_gates(&p, false, || {}, || Ok(material()))
            .await
            .unwrap();
        assert!(matches!(out, Preflight::Proceed { .. }));
        // Not pending: the snapshot is NOT probed during preflight.
        assert_eq!(p.calls(), vec!["escrow"]);

        assert_eq!(slot_gate(&p, 3).await, None);
        assert_eq!(p.calls(), vec!["escrow", "existing"]);
    }

    #[tokio::test]
    async fn pending_hold_blocks_before_recovery_or_escrow() {
        let p = Scripted::new(vec![Ok(Some(4))], vec![]);
        let events = RefCell::new(Vec::<&str>::new());
        let out = preflight_gates(
            &p,
            true,
            || events.borrow_mut().push("cleared"),
            || {
                events.borrow_mut().push("local_recovery");
                Ok(material())
            },
        )
        .await
        .unwrap();
        assert!(matches!(out, Preflight::Skip(PreflightSkip::RestorePending)));
        // The hold decides FIRST: no marker clear, no key material read,
        // no escrow probe.
        assert!(events.borrow().is_empty());
        assert_eq!(p.calls(), vec!["existing"]);
    }

    #[tokio::test]
    async fn pending_probe_failure_keeps_the_hold() {
        let p = Scripted::new(vec![Err("vault unreachable".into())], vec![]);
        let out = preflight_gates(&p, true, || {}, || Ok(material()))
            .await
            .unwrap();
        assert!(matches!(out, Preflight::Skip(PreflightSkip::RestorePending)));
    }

    #[tokio::test]
    async fn pending_resolves_against_empty_slot_in_probe_then_recovery_then_escrow_order() {
        let p = Scripted::new(vec![Ok(None)], vec![Ok(None)]);
        let events = RefCell::new(Vec::<&str>::new());
        let out = preflight_gates(
            &p,
            true,
            || events.borrow_mut().push("cleared"),
            || {
                events.borrow_mut().push("local_recovery");
                Ok(material())
            },
        )
        .await
        .unwrap();
        assert!(matches!(out, Preflight::Proceed { .. }));
        assert_eq!(*events.borrow(), vec!["cleared", "local_recovery"]);
        assert_eq!(p.calls(), vec!["existing", "escrow"]);
    }

    #[tokio::test]
    async fn pending_resolution_survives_a_later_escrow_conflict() {
        // The pending question is settled by the empty slot even though the
        // key-coherence gate then refuses - the marker clear must happen.
        let p = Scripted::new(vec![Ok(Some(0))], vec![Ok(Some(other_material()))]);
        let cleared = RefCell::new(false);
        let out = preflight_gates(&p, true, || *cleared.borrow_mut() = true, || Ok(material()))
            .await
            .unwrap();
        assert!(matches!(out, Preflight::Skip(PreflightSkip::EscrowConflict)));
        assert!(*cleared.borrow());
    }

    #[tokio::test]
    async fn escrow_conflict_refuses() {
        let p = Scripted::new(vec![], vec![Ok(Some(other_material()))]);
        let out = preflight_gates(&p, false, || {}, || Ok(material()))
            .await
            .unwrap();
        assert!(matches!(out, Preflight::Skip(PreflightSkip::EscrowConflict)));
    }

    #[tokio::test]
    async fn escrow_probe_failure_refuses_with_detail() {
        let p = Scripted::new(vec![], vec![Err("boom".into())]);
        let out = preflight_gates(&p, false, || {}, || Ok(material()))
            .await
            .unwrap();
        match out {
            Preflight::Skip(PreflightSkip::EscrowProbeFailed(e)) => assert_eq!(e, "boom"),
            _ => panic!("expected EscrowProbeFailed"),
        }
    }

    #[tokio::test]
    async fn absent_escrow_proceeds() {
        let p = Scripted::new(vec![], vec![Ok(None)]);
        let out = preflight_gates(&p, false, || {}, || Ok(material()))
            .await
            .unwrap();
        assert!(matches!(out, Preflight::Proceed { .. }));
    }

    #[tokio::test]
    async fn local_recovery_failure_propagates_before_the_escrow_probe() {
        let p = Scripted::new(vec![], vec![]);
        // No unwrap_err: Preflight deliberately has no Debug (it carries
        // RecoveryMaterial, which must never be printable).
        match preflight_gates(&p, false, || {}, || Err::<RecoveryMaterial, _>("no key".into()))
            .await
        {
            Err(e) => assert_eq!(e, "no key"),
            Ok(_) => panic!("expected the local recovery error to propagate"),
        }
        assert!(p.calls().is_empty());
    }

    #[tokio::test]
    async fn slot_gate_empty_local_never_overwrites_records() {
        let p = Scripted::new(vec![Ok(Some(96))], vec![]);
        assert_eq!(slot_gate(&p, 0).await, Some(SlotSkip::EmptyWouldOverwrite));
    }

    #[tokio::test]
    async fn slot_gate_confirmed_absent_or_empty_slot_allows_first_write() {
        let p = Scripted::new(vec![Ok(None), Ok(Some(0))], vec![]);
        assert_eq!(slot_gate(&p, 0).await, None);
        assert_eq!(slot_gate(&p, 0).await, None);
    }

    #[tokio::test]
    async fn slot_gate_probe_failure_refuses() {
        let p = Scripted::new(vec![Err("timeout".into())], vec![]);
        assert_eq!(
            slot_gate(&p, 7).await,
            Some(SlotSkip::ProbeFailed("timeout".into()))
        );
    }

    #[test]
    fn may_overwrite_matrix() {
        // Local records always may write (the identity + coherence gates
        // upstream are what keep a foreign device out).
        assert!(may_overwrite(1, Some(96)));
        assert!(may_overwrite(1, None));
        // An empty snapshot only ever lands on a confirmed-empty slot.
        assert!(may_overwrite(0, None));
        assert!(may_overwrite(0, Some(0)));
        assert!(!may_overwrite(0, Some(1)));
    }
}

#[cfg(test)]
mod corpus_budget_tests {
    use super::fit_corpus_records;
    use crate::corpus::{CorpusRecords, DocMeta, DocRecord};

    fn doc(i: i64, card: &str) -> DocRecord {
        DocRecord {
            doc_id: format!("d{i}"),
            added_at: i,
            byte_size: 10,
            chunk_count: 1,
            meta: DocMeta { filename: format!("f{i}.txt"), summary: Some(card.to_string()), ..Default::default() },
            ai_ids: vec!["ai".into()],
        }
    }

    #[test]
    fn fit_corpus_records_drops_cards_oldest_first() {
        let card = "x".repeat(200);
        let mut r = CorpusRecords { version: 1, documents: (1..=5).map(|i| doc(i, &card)).collect() };
        let full = serde_json::to_vec(&r).unwrap().len();
        assert_eq!(fit_corpus_records(&mut r, full), 0);
        let dropped = fit_corpus_records(&mut r, full - 300);
        assert_eq!(dropped, 2, "two cards of 200 bytes cover a 300-byte overshoot");
        assert!(r.documents[0].meta.summary.is_none() && r.documents[1].meta.summary.is_none());
        assert!(r.documents[4].meta.summary.is_some());
        assert_eq!(r.documents.len(), 5, "no document record is ever dropped");
        assert!(serde_json::to_vec(&r).unwrap().len() <= full - 300);
    }
}
