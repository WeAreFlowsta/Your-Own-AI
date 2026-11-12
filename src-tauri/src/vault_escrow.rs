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
use serde::Serialize;
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_store::StoreExt;

/// Fixed label so re-escrow overwrites the same named snapshot instead of
/// accumulating timestamped ones.
pub(crate) const ESCROW_LABEL: &str = "recovery";

#[derive(Serialize, Clone)]
pub struct EscrowStatus {
    /// "synced" | "conflict" | "unlinked" | "vault_unavailable" |
    /// "vault_locked" | "identity_mismatch" | "identity_switched" | "error"
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
        .json(&crate::flowsta::with_identity(serde_json::json!({
            "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
            "label": ESCROW_LABEL,
        })))
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

/// The `recovery` label held from ANY of the person's devices (Vault 1.6.0+):
/// the material and which device wrote it (`None` = this one).
async fn fetch_escrow_across(port: u16) -> Result<Option<(RecoveryMaterial, Option<String>)>, String> {
    let resp = http()
        .post(format!("http://127.0.0.1:{}/backup/retrieve", port))
        .header("Origin", VAULT_ORIGIN)
        .json(&crate::flowsta::with_identity(serde_json::json!({
            "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
            "label": ESCROW_LABEL,
            "across": "devices",
        })))
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
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("bad retrieve response: {}", e))?;
    match parse_escrow(&v["data"]) {
        Some(m) => Ok(Some((m, v["from_device"].as_str().map(String::from)))),
        None => Err("escrow_unreadable".into()),
    }
}

/// `POST /app-secret` (Vault 1.6.1+): 32 bytes for this app and label, the
/// same on every device of the identity. `None` on a Vault that cannot
/// answer (older, or no device-hosted seed).
async fn fetch_app_secret(port: u16, label: &str) -> Result<Option<[u8; 32]>, String> {
    let resp = http()
        .post(format!("http://127.0.0.1:{}/app-secret", port))
        .header("Origin", VAULT_ORIGIN)
        .json(&crate::flowsta::with_identity(serde_json::json!({ "label": label })))
        .send()
        .await
        .map_err(|e| format!("vault unreachable: {}", e))?;
    match resp.status() {
        reqwest::StatusCode::NOT_FOUND => return Ok(None),
        s if s.is_success() => {}
        _ => {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            let err = body["error"].as_str().unwrap_or("app_secret_failed").to_string();
            return if err == "not_available" { Ok(None) } else { Err(err) };
        }
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("bad app-secret response: {}", e))?;
    let bytes = hex::decode(v["secret_hex"].as_str().unwrap_or_default()).map_err(|_| "app_secret_unreadable".to_string())?;
    let secret: [u8; 32] = bytes.try_into().map_err(|_| "app_secret_unreadable".to_string())?;
    Ok(Some(secret))
}

/// The other devices holding Your Own AI backups (`/backup/list`
/// `other_devices`, Vault 1.6.0+) - ids only; empty on an older Vault.
/// Another of the person's devices that holds a copy of this app's backups
/// here (`/backup/list other_devices`). Name, platform and generation come
/// from the Vault's device registry (1.6.1+); a 1.6.0 Vault gives ids only.
#[derive(Clone, Debug, Serialize, Default)]
pub(crate) struct SiblingDevice {
    pub id: String,
    pub name: Option<String>,
    pub platform: Option<String>,
    pub generation: Option<String>,
    /// When this device's copy of the sibling's manifest was made (the
    /// backup list's `created_at`, as text): a config-sync round reads the
    /// manifest only when this moved.
    pub manifest_at: Option<String>,
    /// The same for the sibling's small `ais` object (None: the sibling
    /// writes none yet - an older app).
    pub ais_at: Option<String>,
    /// The same for its `line` object (0.9.1 and later) and its `recovery`
    /// escrow: a round reads them only when they moved.
    pub line_at: Option<String>,
    pub escrow_at: Option<String>,
}

pub(crate) async fn sibling_devices(port: u16) -> Result<Vec<SiblingDevice>, String> {
    let resp = http()
        .get(crate::flowsta::identity_query(format!("http://127.0.0.1:{}/backup/list", port)))
        .header("Origin", VAULT_ORIGIN)
        .send()
        .await
        .map_err(|e| format!("vault unreachable: {}", e))?;
    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        return Err(body["error"].as_str().unwrap_or("list_failed").to_string());
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("bad list response: {}", e))?;
    let s = |d: &serde_json::Value, k: &str| d[k].as_str().filter(|x| !x.is_empty()).map(String::from);
    Ok(v["other_devices"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|d| {
                    // When this device's copy of the sibling's manifest was made: a
                    // config-sync round reads the manifest only when this moved.
                    let stamp = |label: &str| {
                        d["backups"]
                            .as_array()
                            .and_then(|b| b.iter().find(|x| x["label"].as_str() == Some(label)))
                            .map(|x| x["created_at"].to_string())
                    };
                    let (manifest_at, ais_at) = (stamp(MANIFEST_LABEL), stamp(AIS_LABEL));
                    let (line_at, escrow_at) = (stamp(crate::line_facts::LINE_LABEL), stamp(ESCROW_LABEL));
                    s(d, "device").map(|id| SiblingDevice { id, name: s(d, "name"), platform: s(d, "platform"), generation: s(d, "generation"), manifest_at, ais_at, line_at, escrow_at })
                })
                .collect()
        })
        .unwrap_or_default())
}

/// The identity's other devices that hold this app's data, leaving out any
/// the person stopped waiting for (a computer that is gone). A restore runs
/// only when none is left: the data arrives from a device that is there.
pub(crate) async fn devices_holding_data(app: &tauri::AppHandle, port: u16) -> Result<Vec<SiblingDevice>, String> {
    let stopped: std::collections::HashSet<String> = crate::profile::root(app)
        .ok()
        .and_then(|r| crate::line_facts::LineFacts::load(&r))
        .map(|f| f.stopped_waiting.iter().map(|w| w.id.clone()).collect())
        .unwrap_or_default();
    Ok(sibling_devices(port).await?.into_iter().filter(|d| !stopped.contains(&d.id)).collect())
}

/// The computers the person stopped waiting for whose backup is held here,
/// newest first (by their manifest's copy): what a computer that replaces
/// them restores.
pub(crate) async fn gone_backup_devices(app: &tauri::AppHandle, port: u16) -> Vec<String> {
    let stopped: std::collections::HashSet<String> = crate::profile::root(app)
        .ok()
        .and_then(|r| crate::line_facts::LineFacts::load(&r))
        .map(|f| f.stopped_waiting.iter().map(|w| w.id.clone()).collect())
        .unwrap_or_default();
    let mut held: Vec<(i64, String)> = sibling_devices(port)
        .await
        .unwrap_or_default()
        .into_iter()
        .filter(|d| stopped.contains(&d.id))
        .filter_map(|d| crate::line_facts::stamp_ms(&d.manifest_at).map(|at| (at, d.id)))
        .collect();
    held.sort_by(|a, b| b.0.cmp(&a.0));
    held.into_iter().map(|(_, id)| id).collect()
}

/// The refusal a restore gives while other devices hold the data: the code,
/// then the devices (id and name) as JSON, so the page can name them and
/// offer "it is gone" for each.
pub(crate) fn holding_refusal(devices: &[SiblingDevice]) -> String {
    let list: Vec<serde_json::Value> = devices.iter().map(|d| serde_json::json!({ "id": d.id, "name": d.name })).collect();
    format!("siblings_hold_data:{}", serde_json::Value::Array(list))
}

/// One named device's copy of a label (`/backup/retrieve` with `across` +
/// `device`, Vault 1.6.1+). `Ok(None)` = that device holds no such copy.
pub(crate) async fn retrieve_from_device(port: u16, label: &str, device: &str) -> Result<Option<serde_json::Value>, String> {
    let resp = http()
        .post(format!("http://127.0.0.1:{}/backup/retrieve", port))
        .header("Origin", VAULT_ORIGIN)
        .json(&crate::flowsta::with_identity(serde_json::json!({
            "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
            "label": label,
            "across": "devices",
            "device": device,
        })))
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
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("bad retrieve response: {}", e))?;
    Ok(Some(v))
}

/// Every other device's `recovery` material, with whether that device has
/// backed up any conversation (its manifest lists one).
async fn sibling_materials(port: u16) -> Vec<(RecoveryMaterial, bool)> {
    let mut out = Vec::new();
    for d in sibling_devices(port).await.unwrap_or_default() {
        let material = match retrieve_from_device(port, ESCROW_LABEL, &d.id).await {
            Ok(Some(v)) => parse_escrow(&v["data"]),
            _ => None,
        };
        let Some(m) = material else { continue };
        let has_data = match retrieve_from_device(port, MANIFEST_LABEL, &d.id).await {
            Ok(Some(v)) => v["data"]["conversations"].as_array().map(|a| !a.is_empty()).unwrap_or(false),
            _ => false,
        };
        out.push((m, has_data));
    }
    out
}

/// What this device does about the transcript network, given the other
/// devices' material (the design notes §5.1).
#[derive(Debug, PartialEq)]
pub(crate) enum NetworkDecision {
    /// No other device disagrees, or the others join this one.
    Stay,
    /// This device holds nothing of its own yet: join this material.
    Join(RecoveryMaterial),
    /// Both sides hold conversations under different material: the
    /// person's choice (the older "conflict").
    Conflict,
}

/// A device that has backed nothing up joins the network that holds
/// conversations. When no device holds any, exactly one side moves: the one
/// whose network seed sorts higher, so two fresh devices never swap into
/// each other's network.
pub(crate) fn decide_network(local: &RecoveryMaterial, local_has_data: bool, others: &[(RecoveryMaterial, bool)]) -> NetworkDecision {
    let differing: Vec<&(RecoveryMaterial, bool)> = others.iter().filter(|(m, _)| m != local).collect();
    if differing.is_empty() {
        return NetworkDecision::Stay;
    }
    if local_has_data {
        return if differing.iter().any(|(_, has)| *has) { NetworkDecision::Conflict } else { NetworkDecision::Stay };
    }
    if let Some((m, _)) = differing.iter().find(|(_, has)| *has) {
        return NetworkDecision::Join(m.clone());
    }
    let lowest = differing.iter().map(|(m, _)| m).min_by(|a, b| a.network_seed.cmp(&b.network_seed)).unwrap();
    if lowest.network_seed < local.network_seed {
        NetworkDecision::Join(lowest.clone())
    } else {
        NetworkDecision::Stay
    }
}

/// Keep a Vault copy of material this device is about to write over: it
/// may be the only copy of an old key.
fn preserve_replaced(app: &tauri::AppHandle, old: &RecoveryMaterial) -> Result<(), String> {
    let data_dir = crate::profile::root(app).map_err(|e| format!("no app data dir: {}", e))?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let path = data_dir.join(format!("transcript-recovery.vault-replaced-{}.json", ts));
    let json = serde_json::to_string_pretty(old).map_err(|e| e.to_string())?;
    std::fs::write(&path, &json).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    log::info!("[escrow] previous Vault key preserved at {:?}", path.file_name());
    Ok(())
}

/// Before the first conductor start on a profile with no material of its
/// own: the identity's material, so a second device joins the person's
/// network instead of founding one (the design notes §3.1). In
/// order, so a secret never changes once an identity has one: this
/// device's `recovery` slot, any other device's copy, two app secrets the
/// Vault derives (1.6.1+; also escrowed so an older Vault elsewhere finds
/// them). `None` = not signed in, the Vault is not reachable or locked, or
/// nothing is known yet - the caller mints a random seed as it always has
/// (and the escrow sync reconciles later).
pub(crate) async fn resolve_material_from_identity(app: &tauri::AppHandle, data_dir: &std::path::Path) -> Option<RecoveryMaterial> {
    if crate::transcript_crypto::has_recovery_material(data_dir) {
        return None;
    }
    let port = match escrow_port(app, true).await {
        Ok(p) => p,
        Err(s) => {
            log::info!("[escrow] no identity material to adopt before first start ({})", s.state);
            return None;
        }
    };
    let found = match fetch_escrow(port).await {
        Ok(Some(m)) => Some((m, "this device's Vault")),
        Ok(None) | Err(_) => match fetch_escrow_across(port).await {
            Ok(Some((m, from))) => {
                log::info!("[escrow] identity material found on another device ({})", from.as_deref().unwrap_or("?"));
                Some((m, "another device"))
            }
            Ok(None) => None,
            Err(e) => {
                log::warn!("[escrow] across-devices lookup failed: {}", e);
                None
            }
        },
    };
    let (material, source) = match found {
        Some(x) => x,
        None => {
            let network = fetch_app_secret(port, "transcript-network").await.ok().flatten()?;
            let data_key = fetch_app_secret(port, "transcript-data").await.ok().flatten()?;
            (crate::transcript_crypto::material_from_app_secrets(&network, &data_key), "the identity (derived)")
        }
    };
    if let Err(e) = crate::transcript_crypto::store_recovery_material(data_dir, &material) {
        log::warn!("[escrow] could not store the identity's material: {}", e);
        return None;
    }
    if source == "the identity (derived)" {
        if let Err(e) = write_escrow(port, &material).await {
            log::info!("[escrow] derived material not escrowed yet ({}); the sync will", e);
        }
    }
    log::info!("[escrow] transcript material adopted from {} before first start", source);
    Some(material)
}

async fn write_escrow(port: u16, recovery: &RecoveryMaterial) -> Result<(), String> {
    let resp = http()
        .post(format!("http://127.0.0.1:{}/backup", port))
        .header("Origin", VAULT_ORIGIN)
        .json(&crate::flowsta::with_identity(serde_json::json!({
            "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
            "app_name": "Your Own AI",
            "label": ESCROW_LABEL,
            "data": escrow_payload(recovery),
        })))
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
    let data_dir = crate::profile::root(&app)
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

/// Whether a locked Vault can be used for backups: only one that says it is
/// still syncing, holding the identity recorded as this profile's owner.
/// Everything else is `vault_locked` (or `identity_mismatch` when the
/// locked Vault names another identity), as before.
pub(crate) fn locked_vault_usable(syncing: Option<bool>, active: Option<&str>, owner: Option<&str>, require_owner_match: bool) -> Result<(), &'static str> {
    if syncing != Some(true) || !require_owner_match {
        return Err("vault_locked");
    }
    match (active, owner) {
        (Some(active), Some(owner)) if crate::identity_watch::same_identity(active, owner) => Ok(()),
        (Some(_), Some(_)) => Err("identity_mismatch"),
        _ => Err("vault_locked"),
    }
}

/// How the Vault is locked, for what the app says: "syncing" (locked, and
/// app backups keep passing), "stopped" (locked with syncing stopped: the
/// person chose it on that computer), "locked" (a Vault that does not say,
/// from before 1.8.0), "unlocked", or "none" (no Vault answered).
#[tauri::command]
pub async fn vault_lock_kind() -> &'static str {
    let vault = flowsta::find_vault().await;
    match (vault.port, vault.unlocked, vault.syncing_while_locked) {
        (None, _, _) => "none",
        (Some(_), true, _) => "unlocked",
        (Some(_), false, Some(true)) => "syncing",
        (Some(_), false, Some(false)) => "stopped",
        (Some(_), false, None) => "locked",
    }
}

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
    // The Vault moved to another identity while the app runs: this profile's
    // data belongs to the previous one. Nothing is written until a restart.
    if crate::identity_watch::switched() {
        return Err(EscrowStatus::state("identity_switched"));
    }
    let store = app.store(crate::profile::store_path(&app, AUTH_STORE))
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
    // never-created one. One that is locked and still syncing (Vault 1.8.0
    // on) names the identity it holds and keeps taking and handing back
    // backups: the app goes on when that identity is this profile's recorded
    // owner. It never adopts an owner from a locked Vault, and the Vault
    // itself refuses any request naming another identity. Locked with
    // syncing stopped, or an older Vault: nothing until the unlock.
    if !vault.unlocked {
        let owner = store.get(ESCROW_OWNER_KEY).and_then(|v| v.as_str().map(String::from));
        return match locked_vault_usable(vault.syncing_while_locked, vault.active_identity.as_deref(), owner.as_deref(), require_owner_match) {
            Ok(()) => Ok(port),
            Err(state) => Err(EscrowStatus::state(state)),
        };
    }
    let live = match vault.agent_pub_key {
        Some(k) => k,
        None => return Err(EscrowStatus::state("vault_locked")),
    };
    // The Vault's answer rules: a connection revoked elsewhere (or a reset
    // Vault) means no backup call at all - and no stored identity either.
    if flowsta::drop_identity_if_revoked(app, port, &live).await {
        return Err(EscrowStatus::state("unlinked"));
    }
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
            crate::profile::bind(app, &live);
            Ok(port)
        }
    }
}

/// Record the live Vault identity as this device's escrow owner. Only for
/// explicit adoption paths (a user-driven restore). Clears the backup sync
/// state: "already uploaded" beliefs about another identity's slot would
/// otherwise produce manifests pointing at objects that don't exist there.
fn adopt_escrow_owner(app: &tauri::AppHandle, live: &str) {
    if let Ok(store) = app.store(crate::profile::store_path(&app, AUTH_STORE)) {
        let prev = store
            .get(ESCROW_OWNER_KEY)
            .and_then(|v| v.as_str().map(String::from));
        if prev.as_deref() != Some(live) {
            store.set(ESCROW_OWNER_KEY, serde_json::json!(live));
            let _ = store.save();
            crate::profile::bind(app, live);
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

/// Raw conversation-record count on this device (no decryption -
/// undecryptable records still count as data worth protecting), listed the
/// way the backup lists them: each AI's founding key once, plus each cell's
/// own key for the conversations started before. From the 0.6 cells while
/// any runs (writes go there first, so they hold every one), else from the
/// 0.7 line. A cell or line that cannot answer makes the count unknown, an
/// error, never zero: this count is what keeps a key restore from wiping a
/// device that holds conversations.
async fn count_local_conversations(app: &tauri::AppHandle) -> Result<u64, String> {
    let hc = app
        .try_state::<Arc<HolochainState>>()
        .ok_or("holochain state missing")?;
    let manager = hc.get()?;
    let timeout = std::time::Duration::from_secs(180);
    let fail = |key: &str, e: String| format!("conversation count failed for {}: {}", key, e);
    let mut keys: Vec<String> = manager.agents.lock().await.keys().cloned().collect();
    keys.sort();
    let mut total = 0u64;
    let mut foundings: std::collections::HashSet<String> = std::collections::HashSet::new();
    if !keys.is_empty() {
        for key in &keys {
            let founding = crate::commands_holochain::founding_key_for(app, key);
            if founding == *key {
                if foundings.insert(founding) {
                    total += crate::transcript_pages::conversation_count_at(&manager, key, None, timeout).await.map_err(|e| fail(key, e))?;
                }
                continue;
            }
            total += crate::transcript_pages::conversation_count_at(&manager, key, None, timeout).await.map_err(|e| fail(key, e))?;
            if foundings.insert(founding.clone()) {
                total += crate::transcript_pages::conversation_count_at(&manager, key, Some(&founding), timeout).await.map_err(|e| fail(key, e))?;
            }
        }
        return Ok(total);
    }
    if crate::conductor::ports(flowsta_hc_lines::Line::V07).is_none() {
        return Err("conversation count unknown: no line runs".into());
    }
    let mut on_07: Vec<String> = crate::hc07::agents_with_apps().await?.into_iter().collect();
    on_07.sort();
    for key in &on_07 {
        let founding = crate::commands_holochain::founding_key_for(app, key);
        if !foundings.insert(founding.clone()) {
            continue;
        }
        let anchor = hex::decode(&founding).ok().filter(|b| b.len() == 39).map(holochain_types::prelude::AgentPubKey::from_raw_39).ok_or_else(|| fail(key, "bad founding key".into()))?;
        let page = crate::hc07::app_cells()
            .conversations_page(key, &crate::hc07::ConversationsPage { anchor, before: None, limit: 1 })
            .await
            .map_err(|e| fail(key, e))?;
        total += page.total as u64;
    }
    Ok(total)
}

/// Reconcile the escrow with the Vault. Writes only into an empty slot;
/// reports "conflict" (with the local record count) when the Vault holds
/// different material. Safe to call repeatedly.

/// Stop the conductor and key store, drop this device's cells, put the
/// given material in place (the old one is kept as `transcript-recovery.
/// replaced-<ts>.json`), adopt the live identity, drop the derived index
/// files, and relaunch. The caller has already decided this is right.
pub(crate) async fn swap_material_and_restart(
    app: &tauri::AppHandle,
    data_dir: &std::path::Path,
    new_material: &RecoveryMaterial,
) -> Result<(), String> {
    // A start under way ends first, then the conductor stops, then the
    // key store.
    crate::conductor::stop_for_good_async().await;
    log::info!("[escrow] conductor + lair stopped for the material swap");

    // Both lines: the 0.7 key store's passphrase follows the data key that
    // is about to be replaced.
    for dir in ["conductor", "lair", "conductor-07", "lair-07"] {
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
    crate::move_07::remove_move_files(data_dir);
    transcript_crypto::replace_recovery_material(data_dir, new_material)?;
    match flowsta::find_vault().await.agent_pub_key {
        Some(live) => adopt_escrow_owner(app, &live),
        None => {
            if let Ok(store) = app.store(crate::profile::store_path(&app, AUTH_STORE)) {
                store.delete(ESCROW_OWNER_KEY);
                let _ = store.save();
            }
            clear_sync_state(app);
        }
    }
    let _ = std::fs::remove_file(data_dir.join("memory-facts.enc"));
    if let Ok(entries) = std::fs::read_dir(data_dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("transcript-emb-") && name.ends_with(".enc") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    log::info!("[escrow] transcript material swapped - relaunching");
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
    // The other devices decide which network is the identity's. Reading
    // only this device's own slot missed a device whose first start came
    // before sign-in: its slot was empty, it escrowed its own new seed and
    // never offered to join (Linux + Windows, 2026-10-16).
    let others = sibling_materials(port).await;
    match decide_network(&local, !load_sync_state(app).is_empty(), &others) {
        NetworkDecision::Join(_) => {
            log::info!("[escrow] another device holds the identity's network - this device joins it");
            let count = count_local_conversations(app).await.ok();
            return EscrowStatus {
                state: if island_deferred() { "island_deferred".into() } else { "island_pending".into() },
                local_conversations: count,
                error: None,
                backups_held: false,
            };
        }
        NetworkDecision::Conflict => {
            let count = count_local_conversations(app).await.ok();
            return EscrowStatus { state: "conflict".into(), local_conversations: count, error: None, backups_held: false };
        }
        NetworkDecision::Stay => {}
    }
    match fetch_escrow(port).await {
        // This device's own slot is stale (it held an island's key before a
        // join) while the other devices agree with this one: follow them.
        Ok(Some(ref old)) if *old != local && others.iter().any(|(m, _)| *m == local) => {
            if let Err(e) = preserve_replaced(app, old) {
                return EscrowStatus::error(e);
            }
            match write_escrow(port, &local).await {
                Ok(()) => {
                    log::info!("[escrow] this device's Vault slot now holds the identity's key");
                    schedule_full_backup(app);
                    EscrowStatus::state("synced")
                }
                Err(e) => EscrowStatus::error(e),
            }
        }
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
            // This device never backed anything up under its own material
            // and the identity already has a network: an island. Fold it in
            // - no door (MULTI_DEVICE.md §5.1). A device that HAS backed up
            // under its material is the older "conflict" (the person's choice).
            if load_sync_state(app).is_empty() {
                // The merge is not a choice (no door), but the RESTART is
                // announced and timed by the person: the card offers
                // "Restart now" (vault_escrow_island_join) or "Later" (this
                // session; asked again at the next launch).
                let count = count_local_conversations(app).await.ok();
                return EscrowStatus {
                    state: if island_deferred() { "island_deferred".into() } else { "island_pending".into() },
                    local_conversations: count,
                    error: None,
                    backups_held: false,
                };
            }
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
    // Another device of this identity holds the data: a replay would author
    // every conversation again as new records into the network that already
    // has them. The conversations arrive from the other devices instead.
    let holding = devices_holding_data(&app, port).await.unwrap_or_default();
    if !holding.is_empty() {
        return Err(holding_refusal(&holding));
    }
    if !accept_data_loss {
        let count = count_local_conversations(&app).await?;
        if count > 0 {
            return Err(format!("local_data_exists:{}", count));
        }
    }

    let data_dir = crate::profile::root(&app)
        .map_err(|e| format!("no app data dir: {}", e))?;
    if let Err(e) = std::fs::write(data_dir.join(RESTORE_PENDING_FILE), b"") {
        log::warn!("[escrow] could not write restore-pending marker: {}", e);
    }
    swap_material_and_restart(&app, &data_dir, &escrowed).await
}

/// Conflict resolution B: keep this device's key and overwrite the Vault
/// copy. The replaced Vault material is first written to a timestamped local
/// file - it may be the only copy of an old key, and transcripts encrypted
/// under it (e.g. on another machine's export) would otherwise become
/// unrecoverable with no warning.
static ISLAND_DEFERRED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
fn island_deferred() -> bool { ISLAND_DEFERRED.load(std::sync::atomic::Ordering::Relaxed) }

/// "Later" on the join card: stop asking for this session. The next launch
/// asks again (the flag is process-local on purpose).
#[tauri::command]
pub fn vault_escrow_island_later() -> EscrowStatus {
    ISLAND_DEFERRED.store(true, std::sync::atomic::Ordering::Relaxed);
    EscrowStatus::state("island_deferred")
}

/// "Restart now" on the join card: export this device's conversations, swap
/// to the identity's material and relaunch (the pending marker replays the
/// export on the way back up). Everything island_merge_start checks still
/// applies; a refusal is reported, nothing is touched.
#[tauri::command]
pub async fn vault_escrow_island_join(app: tauri::AppHandle) -> Result<EscrowStatus, String> {
    let vault = crate::flowsta::find_vault().await;
    let port = vault.port.ok_or("vault_unavailable")?;
    if !vault.unlocked { return Err("vault_locked".into()); }
    let local = local_recovery(&app)?;
    let others = sibling_materials(port).await;
    let identity = match decide_network(&local, !load_sync_state(&app).is_empty(), &others) {
        NetworkDecision::Join(m) => m,
        // No other device to follow: this device's own Vault slot (the
        // single-device island case).
        _ => match fetch_escrow(port).await {
            Ok(Some(m)) if m != local => m,
            Ok(_) => return Err("nothing_to_join".into()),
            Err(e) => return Err(e),
        },
    };
    let n = island_merge_start(&app, &identity).await?;
    Ok(EscrowStatus { state: "island_merging".into(), local_conversations: Some(n), error: None, backups_held: false })
}

#[tauri::command]
pub async fn vault_escrow_keep_local(app: tauri::AppHandle) -> Result<EscrowStatus, String> {
    let port = match escrow_port(&app, true).await {
        Ok(p) => p,
        Err(s) => return Err(s.error.unwrap_or(s.state)),
    };
    let local = local_recovery(&app)?;
    if let Some(old) = fetch_escrow(port).await? {
        if old != local {
            preserve_replaced(&app, &old)?;
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
/// Incremental mode: the small, fast-moving part of the manifest as its own
/// object - the AIs, their pictures, their knowledge and the document cards
/// (~400 KB) - so a new AI reaches the other devices without the 2 MB
/// manifest moving with it. The manifest still carries everything (a restore
/// and an older device read only it); it is rewritten at most every
/// `MANIFEST_MIN_GAP_SECS`, and neither object is rewritten unchanged (10-19:
/// 27 backups in a day, each a fresh 2 MB manifest, each cancelling the
/// other device's fetch of the one before).
pub(crate) const AIS_LABEL: &str = "ais";
const MANIFEST_MIN_GAP_SECS: u64 = 600;
/// Fingerprints of the objects last written, by label.
const FINGERPRINTS_FILE: &str = "backup-fingerprints.json";
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

/// Keep this many tombstones in the manifest, newest first: a year of
/// deleting on a busy machine, and a few KB.
const MAX_TOMBSTONES: usize = 5_000;

/// The `deleted` list of the manifest the Vault holds now (empty when there
/// is none, or it cannot be read - a missing list never blocks a backup).
fn tombstones_of(manifest: Option<&serde_json::Value>) -> Vec<crate::conversation_cache::DeletedConversation> {
    manifest
        .and_then(|m| serde_json::from_value(m["deleted"].clone()).ok())
        .unwrap_or_default()
}

/// This device's own copy of one named object in the Vault (None when there
/// is none, or it cannot be read).
async fn fetch_vault_label(port: u16, label: &str) -> Option<serde_json::Value> {
    let resp = http()
        .post(format!("http://127.0.0.1:{}/backup/retrieve", port))
        .header("Origin", VAULT_ORIGIN)
        .json(&crate::flowsta::with_identity(serde_json::json!({ "client_id": YOAI_HOLOCHAIN_CLIENT_ID, "label": label })))
        .send()
        .await
        .ok()
        .filter(|r| r.status().is_success())?;
    let v: serde_json::Value = resp.json().await.ok()?;
    Some(v["data"].clone())
}

/// The labels this device's app backup holds in the Vault (the list is one
/// cheap call; it says whether an object we may skip rewriting is there).
pub(crate) async fn own_backup_labels(port: u16) -> std::collections::HashSet<String> {
    own_backup_labels_read(port).await.unwrap_or_default()
}

/// The labels, or None when the list could not be read (locked, busy).
pub(crate) async fn own_backup_labels_read(port: u16) -> Option<std::collections::HashSet<String>> {
    let resp = http()
        .get(crate::flowsta::identity_query(format!("http://127.0.0.1:{}/backup/list", port)))
        .header("Origin", VAULT_ORIGIN)
        .send()
        .await
        .ok()
        .filter(|r| r.status().is_success())?;
    let v = resp.json::<serde_json::Value>().await.ok()?;
    Some(own_labels_in(&v))
}

fn own_labels_in(v: &serde_json::Value) -> std::collections::HashSet<String> {
    v["apps"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["client_id"].as_str() == Some(YOAI_HOLOCHAIN_CLIENT_ID))
        .flat_map(|a| a["labels"].as_array().into_iter().flatten())
        .filter_map(|l| l.as_str().map(String::from))
        .collect()
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default)]
pub(crate) struct Fingerprint {
    hash: String,
    /// Seconds since the epoch when the object was last written.
    written_at: u64,
}

fn fingerprints_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    crate::profile::root(&app).ok().map(|d| d.join(FINGERPRINTS_FILE))
}

fn load_fingerprints(app: &tauri::AppHandle) -> std::collections::HashMap<String, Fingerprint> {
    fingerprints_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// When each backup object was last written (seconds since the epoch), by
/// label - the truth matrix's Devices section.
pub(crate) fn backup_write_times(app: &tauri::AppHandle) -> Vec<(String, u64)> {
    let mut v: Vec<(String, u64)> = load_fingerprints(app).into_iter().map(|(k, f)| (k, f.written_at)).collect();
    v.sort();
    v
}

fn save_fingerprints(app: &tauri::AppHandle, prints: &std::collections::HashMap<String, Fingerprint>) {
    if let (Some(p), Ok(json)) = (fingerprints_path(app), serde_json::to_string(prints)) {
        let _ = std::fs::write(p, json);
    }
}

/// What an object says, apart from when it was said: the time stamp is new
/// on every run and must not make an unchanged object look changed.
pub(crate) fn fingerprint_of(object: &serde_json::Value) -> String {
    use sha2::Digest as _;
    let mut v = object.clone();
    if let Some(o) = v.as_object_mut() {
        o.remove("exported_at_iso");
    }
    hex::encode(sha2::Sha256::digest(serde_json::to_vec(&v).unwrap_or_default()))
}

#[derive(Debug, PartialEq, Clone, Copy)]
pub(crate) enum WriteDecision {
    Write,
    Unchanged,
    /// Changed, but written too recently: again in this many seconds.
    Later(u64),
}

/// Write an object the Vault holds only when it changed, and the big one
/// only when its last write is `min_gap` behind; an object the Vault lacks
/// is always written.
pub(crate) fn write_decision(changed: bool, in_vault: bool, last: Option<&Fingerprint>, now: u64, min_gap: u64) -> WriteDecision {
    let Some(last) = last.filter(|_| in_vault) else { return WriteDecision::Write };
    if !changed {
        return WriteDecision::Unchanged;
    }
    let since = now.saturating_sub(last.written_at);
    if since < min_gap {
        WriteDecision::Later(min_gap - since)
    } else {
        WriteDecision::Write
    }
}

fn now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// The small object: the manifest's fast-moving parts.
fn ais_object(manifest: &serde_json::Value) -> serde_json::Value {
    let mut out = serde_json::json!({
        "version": manifest["version"],
        "_readme": "Your AIs (names, personas, settings), their pictures, the \
                    knowledge you gave them and your document library's cards, \
                    as stored on this device - the parts of the manifest that \
                    change most, so they travel on their own.",
        "license": manifest["license"],
        "app": manifest["app"],
        "exported_at_iso": manifest["exported_at_iso"],
        "network_generation": manifest["network_generation"],
    });
    for k in ["ai_configs", "thumbnails", "ai_knowledge", "corpus"] {
        if let Some(v) = manifest.get(k) {
            out[k] = v.clone();
        }
    }
    out
}

pub(crate) async fn post_backup_json(port: u16, label: &str, data: &serde_json::Value) -> Result<(), String> {
    let resp = http()
        .post(format!("http://127.0.0.1:{}/backup", port))
        .header("Origin", VAULT_ORIGIN)
        .json(&crate::flowsta::with_identity(serde_json::json!({
            "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
            "app_name": "Your Own AI",
            "label": label,
            "data": data,
        })))
        .send()
        .await
        .map_err(|e| format!("vault unreachable: {}", e))?;
    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        return Err(body["error"].as_str().unwrap_or("backup_failed").to_string());
    }
    Ok(())
}

/// Union by hash, newest deletion first, capped.
pub(crate) fn merge_tombstones(
    mine: Vec<crate::conversation_cache::DeletedConversation>,
    theirs: Vec<crate::conversation_cache::DeletedConversation>,
) -> Vec<serde_json::Value> {
    let mut by_hash: std::collections::HashMap<String, crate::conversation_cache::DeletedConversation> =
        std::collections::HashMap::new();
    for d in theirs.into_iter().chain(mine) {
        match by_hash.get(&d.hash) {
            Some(have) if have.deleted_at >= d.deleted_at => {}
            _ => {
                by_hash.insert(d.hash.clone(), d);
            }
        }
    }
    let mut all: Vec<_> = by_hash.into_values().collect();
    all.sort_by_key(|d| std::cmp::Reverse(d.deleted_at));
    all.truncate(MAX_TOMBSTONES);
    all.into_iter()
        .map(|d| serde_json::json!({ "hash": d.hash, "started_at": d.started_at, "deleted_at": d.deleted_at }))
        .collect()
}

/// How many objects the Vault holds for this app right now (None when the
/// list cannot be read).
async fn vault_object_count(port: u16) -> Option<usize> {
    let resp = http()
        .get(crate::flowsta::identity_query(format!("http://127.0.0.1:{}/backup/list", port)))
        .header("Origin", VAULT_ORIGIN)
        .send()
        .await
        .ok()?;
    if !resp.status().is_success() {
        return None;
    }
    let v: serde_json::Value = resp.json().await.ok()?;
    v["apps"]
        .as_array()?
        .iter()
        .find(|a| a["client_id"].as_str() == Some(YOAI_HOLOCHAIN_CLIENT_ID))
        .and_then(|a| a["backup_count"].as_u64())
        .map(|n| n as usize)
}

/// Does the Vault hold this object? A 404 is a definite no; any other
/// failure is "unknown" (None) and must not be treated as missing.
async fn vault_has_object(port: u16, label: &str) -> Option<bool> {
    let resp = http()
        .post(format!("http://127.0.0.1:{}/backup/retrieve", port))
        .header("Origin", VAULT_ORIGIN)
        .json(&crate::flowsta::with_identity(serde_json::json!({ "client_id": YOAI_HOLOCHAIN_CLIENT_ID, "label": label })))
        .send()
        .await
        .ok()?;
    match resp.status() {
        reqwest::StatusCode::NOT_FOUND => Some(false),
        s if s.is_success() => Some(true),
        _ => None,
    }
}

/// The sync state is this device's belief about what the Vault holds; the
/// Vault is the truth. When the Vault holds fewer objects than the belief
/// (a Vault reset or restore, a lost object), every believed label is
/// checked and the conversations whose objects are gone are forgotten
/// from the sync state, so the next pass reads and uploads them again.
/// (Dev laptop 09-07: 622 conversations "unchanged since 5 August" whose
/// objects the Vault no longer had - listed in every manifest, present
/// in none of the backups, found only by a restore.)
async fn reconcile_sync_state(app: &tauri::AppHandle, port: u16) {
    let mut sync = load_sync_state(app);
    if sync.is_empty() {
        return;
    }
    let believed: usize = sync.values().map(|e| e.labels.len()).sum();
    let Some(held) = vault_object_count(port).await else { return };
    // The manifest is one object beside the conversation objects.
    if held >= believed {
        return;
    }
    log::warn!(
        "[escrow] the Vault holds {} object(s) but this device believed {} - checking every one",
        held, believed
    );
    let mut forgotten = 0usize;
    let mut unknown = 0usize;
    let labels: Vec<(String, Vec<String>)> = sync.iter().map(|(k, e)| (k.clone(), e.labels.clone())).collect();
    for (key, parts) in labels {
        let mut missing = false;
        for part in &parts {
            match vault_has_object(port, part).await {
                Some(false) => {
                    missing = true;
                    break;
                }
                Some(true) => {}
                None => {
                    unknown += 1;
                    break;
                }
            }
        }
        if missing {
            sync.remove(&key);
            forgotten += 1;
        }
    }
    if forgotten > 0 {
        save_sync_state(app, &sync);
        log::warn!(
            "[escrow] {} conversation(s) the Vault no longer holds will be read and uploaded again{}",
            forgotten,
            if unknown > 0 { format!(" ({unknown} could not be checked)") } else { String::new() }
        );
    }
}

async fn fetch_backup_limits(port: u16) -> Option<BackupLimits> {
    let resp = http()
        .get(crate::flowsta::identity_query(format!("http://127.0.0.1:{}/backup/limits", port)))
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
    /// Last-activity stamp at the time of the backup (see ConvBundle).
    #[serde(default)]
    active_at: Option<i64>,
    /// The conversation's start, kept so an unchanged conversation can be
    /// listed without reading it.
    #[serde(default)]
    started_at: Option<i64>,
}

fn sync_state_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    crate::profile::root(&app).ok().map(|d| d.join(SYNC_STATE_FILE))
}

fn load_sync_state(app: &tauri::AppHandle) -> std::collections::HashMap<String, SyncEntry> {
    sync_state_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Start times of every conversation this device has uploaded, by the sync
/// state's own record - a second local index for the restore's "already
/// here" check beside the list cache.
pub(crate) fn sync_state_started_ats(app: &tauri::AppHandle) -> std::collections::HashSet<i64> {
    load_sync_state(app).values().filter_map(|e| e.started_at).collect()
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
        .json(&crate::flowsta::with_identity(serde_json::json!({
            "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
            "app_name": "Your Own AI",
            "label": label,
            "data_base64": base64::engine::general_purpose::STANDARD.encode(bytes),
            "content_type": "application/json+gzip",
        })))
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
        .json(&crate::flowsta::with_identity(serde_json::json!({
            "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
            "label": label,
        })))
        .send()
        .await;
}

/// Marker file left by the key restore: this device holds the right key
/// but has NOT replayed the Vault conversations yet. While it exists,
/// automatic backups are suspended - anything written from here (even
/// after a first chat) would overwrite the user's Vault copy with a
/// near-empty snapshot. Cleared by a successful conversation restore.
pub(crate) const RESTORE_PENDING_FILE: &str = "conversation-restore-pending";
/// An island merge is pending: this device used Your Own AI before it was
/// signed in, the identity already had a network elsewhere, and this
/// device's conversations were exported (the file this marker names) so
/// they can be replayed into the identity's network after the swap.
pub(crate) const ISLAND_PENDING_FILE: &str = "island-merge-pending";

pub(crate) fn island_pending_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    crate::profile::root(&app).ok().map(|d| d.join(ISLAND_PENDING_FILE))
}

/// The export an island merge replays from, when one is pending.
pub(crate) fn island_export_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    let marker = island_pending_path(app)?;
    let name = std::fs::read_to_string(&marker).ok()?;
    let name = name.trim();
    if name.is_empty() || name.contains('/') || name.contains('\\') {
        return None;
    }
    let path = marker.parent()?.join(name);
    path.exists().then_some(path)
}

pub(crate) fn clear_island_pending(app: &tauri::AppHandle) {
    if let Some(p) = island_pending_path(app) {
        let _ = std::fs::remove_file(p);
    }
}

/// The memory facts file re-encrypted under another key (base64 of the
/// file as it will be written), so the merge restores it under the
/// identity's key instead of losing it with the swap.
fn facts_file_rekeyed(dir: &std::path::Path, old_key: &[u8; 32], new_key: &[u8; 32]) -> Option<String> {
    let bytes = std::fs::read(dir.join(FACTS_FILE)).ok()?;
    let envelope: serde_json::Value = serde_json::from_slice(&bytes).ok()?;
    let nonce = hex::decode(envelope["nonce"].as_str()?).ok()?;
    let cipher = hex::decode(envelope["cipher"].as_str()?).ok()?;
    let plain = crate::transcript_crypto::decrypt(old_key, &nonce, &cipher).ok()?;
    let (nonce2, cipher2) = crate::transcript_crypto::encrypt(new_key, &plain).ok()?;
    let out = serde_json::json!({ "version": 1, "nonce": hex::encode(nonce2), "cipher": hex::encode(cipher2) });
    use base64::Engine;
    Some(base64::engine::general_purpose::STANDARD.encode(serde_json::to_vec(&out).ok()?))
}

/// This device used Your Own AI before it was signed in and the identity
/// already has a network elsewhere: fold this device's conversations into
/// it (the design notes §5.1). Export first - the same shape the
/// restore reads, every record carried as plaintext so it is re-encrypted
/// under the identity's key - verify the count, then swap the material and
/// relaunch; the pending marker makes the next start replay the export.
/// Nothing is touched until the export is complete and checked.
pub(crate) async fn island_merge_start(app: &tauri::AppHandle, identity: &RecoveryMaterial) -> Result<u64, String> {
    let data_dir = crate::profile::root(&app).map_err(|e| format!("no app data dir: {}", e))?;
    let local = local_recovery(app)?;
    if local == *identity {
        return Err("already in sync".into());
    }
    let old_key = local.data_key()?;
    let new_key = identity.data_key()?;
    let expected = count_local_conversations(app).await?;
    let (convs, _carried) = collect_conversations(app).await?;
    if convs.len() as u64 != expected {
        return Err(format!("island_export_incomplete:{}/{}", convs.len(), expected));
    }
    let ai_configs = std::fs::read_to_string(data_dir.join("ai-data.json"))
        .ok()
        .and_then(|s| serde_json::from_str::<serde_json::Value>(&s).ok());
    let mut payload = assemble_full_payload(&local, ai_configs, convs, usize::MAX);
    if let Ok(root) = crate::profile::root(app) {
        payload["network_generation"] = serde_json::json!(crate::line_facts::own_generation(&root));
    }
    attach_extras(app, &local, &mut payload);
    // Plaintext only: the raw entries are ciphertext under the island's
    // key, and the replay re-encrypts human_readable under the identity's.
    let stamp = unix_now_secs();
    // `import:` is the convention for adopted history: the drawer lists a
    // conversation with that source as a place to pick up, and hides any
    // other source (records of the local server's calls live on the
    // Memory page instead).
    let source = format!("import:this-device-{}", stamp);
    if let Some(cells) = payload["cells"].as_array_mut() {
        for cell in cells.iter_mut() {
            if let Some(records) = cell["records"].as_array_mut() {
                for rec in records.iter_mut() {
                    if let Some(obj) = rec.as_object_mut() {
                        obj.remove("raw_record");
                    }
                    if rec["entryType"] == "Conversation" {
                        rec["human_readable"]["source"] = serde_json::json!(source);
                    }
                }
            }
        }
    }
    match facts_file_rekeyed(&data_dir, &old_key, &new_key) {
        Some(b64) => payload["memory_facts"]["raw_b64"] = serde_json::json!(b64),
        None => {
            if let Some(obj) = payload.as_object_mut() {
                obj.remove("memory_facts");
            }
        }
    }
    payload["deleted"] = serde_json::json!(crate::conversation_cache::deleted_ledger(app));
    payload["island"] = serde_json::json!({ "source": source, "conversations": expected, "exported_at": stamp });
    let file_name = format!("island-{}.json", stamp);
    let file = data_dir.join(&file_name);
    std::fs::write(&file, serde_json::to_vec(&payload).map_err(|e| e.to_string())?)
        .map_err(|e| format!("could not write the island export: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600));
    }
    let back: serde_json::Value = std::fs::read(&file)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .ok_or("the island export did not read back")?;
    if back["island"]["conversations"].as_u64() != Some(expected) {
        return Err("the island export did not read back whole".into());
    }
    std::fs::write(data_dir.join(ISLAND_PENDING_FILE), file_name.as_bytes())
        .map_err(|e| format!("could not write the island marker: {}", e))?;
    log::info!("[escrow] island: {} conversation(s) exported; swapping to the identity's material", expected);
    swap_material_and_restart(app, &data_dir, identity).await?;
    Ok(expected)
}


pub(crate) fn restore_pending_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    crate::profile::root(&app)
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
    crate::profile::root(&app)
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
    /// The cell's agent (hex) - how another device tells which AI this
    /// conversation belongs to (the founding key); the app id above carries
    /// the AI's original local id, which no other device knows.
    agent: String,
    /// The conversation's title at read time, for a drawer that lists it
    /// from the copy before its records arrive.
    title: Option<String>,
    started_at: i64,
    /// Conversation record first, then its entries in chain order.
    records: Vec<serde_json::Value>,
    size: usize,
    /// The local list cache's last-activity stamp when this was read (the
    /// app writes it through on every turn). Stored with the sync entry;
    /// a conversation whose stamp has not moved is not read again.
    active_at: Option<i64>,
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
fn backup_record(entry_type: &str, record: Sealed) -> serde_json::Value {
    use base64::Engine;
    serde_json::json!({
        "entryType": entry_type,
        "actionHash": record.id,
        "createdAtMs": record.created_ms,
        "human_readable": record.plain,
        "raw_record": {
            "entry_b64": base64::engine::general_purpose::STANDARD.encode(&record.raw),
            "action_address": record.id,
            "action_type": "Create",
        },
    })
}

/// One record as a backup keeps it, from either line: its id (a v1
/// record's action hash, or the id a v2 record carries in its plaintext),
/// when it was written, its plaintext, and its sealed entry's bytes (the
/// same `{cipher, nonce}` shape on both lines, so a restore reads either).
#[derive(Clone)]
pub(crate) struct Sealed {
    pub id: String,
    pub created_ms: i64,
    pub plain: serde_json::Value,
    pub raw: Vec<u8>,
}

fn sealed_v1(key: &[u8; 32], record: &holochain_types::prelude::Record) -> Option<Sealed> {
    let (plain, raw) = open_record(key, record)?;
    Some(Sealed { id: hex::encode(record.action_address().get_raw_39()), created_ms: record.action().timestamp().as_micros() / 1000, plain, raw })
}

fn sealed_v2(key: &[u8; 32], view: &flowsta_hc_lines::RecordView) -> Option<Sealed> {
    let raw = view.entry.clone()?;
    let ee: crate::commands_holochain::EncryptedEntryRaw = rmp_serde::from_slice(&raw).ok()?;
    let plain_bytes = crate::transcript_crypto::decrypt(key, &ee.nonce, &ee.cipher).ok()?;
    let plain: serde_json::Value = serde_json::from_slice(&plain_bytes).ok()?;
    let id = plain["id"].as_str()?.to_string();
    Some(Sealed { id, created_ms: view.timestamp_us / 1000, plain, raw })
}

/// Where a backup reads an AI's records: its 0.6 cell while it has one
/// (writes go there first, so it holds every record), else the 0.7 line.
pub(crate) enum BackupSource {
    V06,
    V07,
}

/// An AI's conversations: at its founding key and, on the 0.6 line, the
/// cell's own key for the ones started before. A list that stops part way
/// is an error, so a partial list never drops conversations.
pub(crate) async fn list_sealed(manager: &crate::holochain::HolochainManager, key: &[u8; 32], agent: &str, founding: &str, source: &BackupSource) -> Result<Vec<Sealed>, String> {
    let timeout = std::time::Duration::from_secs(180);
    match source {
        BackupSource::V06 => {
            let mut anchors: Vec<Option<&str>> = vec![None];
            if founding != agent {
                anchors.insert(0, Some(founding));
            }
            let mut out = Vec::new();
            for anchor in anchors {
                let records = crate::transcript_pages::all_conversations_at(manager, agent, anchor, timeout).await.and_then(|r| r.whole())?;
                out.extend(records.iter().filter_map(|r| sealed_v1(key, r)));
            }
            Ok(out)
        }
        BackupSource::V07 => {
            let anchor = hex::decode(founding).ok().filter(|b| b.len() == 39).map(holochain_types::prelude::AgentPubKey::from_raw_39).ok_or("bad founding key")?;
            let items = tokio::time::timeout(timeout, crate::hc07::app_cells().conversations_all(agent, &anchor))
                .await
                .unwrap_or_else(|_| Err(format!("no answer within {} s", timeout.as_secs())))?;
            Ok(items.iter().filter_map(|(view, _)| sealed_v2(key, view)).collect())
        }
    }
}

/// A conversation's entries, in order; the whole conversation or an error.
async fn entries_sealed(manager: &crate::holochain::HolochainManager, key: &[u8; 32], agent: &str, conversation_id: &str, source: &BackupSource, timeout: std::time::Duration) -> Result<Vec<Sealed>, String> {
    match source {
        BackupSource::V06 => {
            let hash = crate::transcript_ids::v1_hash(conversation_id).ok_or("not a v1 conversation")?;
            let records = crate::transcript_pages::all_entries(manager, agent, hash, timeout).await.and_then(|r| r.whole())?;
            Ok(records.iter().filter_map(|r| sealed_v1(key, r)).collect())
        }
        BackupSource::V07 => {
            let anchor = holochain_types::prelude::ExternalHash::from_raw_32(crate::transcript_ids::conversation_anchor(key, conversation_id).to_vec());
            let items = tokio::time::timeout(timeout, crate::hc07::app_cells().entries_all(agent, &anchor))
                .await
                .unwrap_or_else(|_| Err(format!("no answer within {} s", timeout.as_secs())))?;
            Ok(items.iter().filter_map(|(view, _)| sealed_v2(key, view)).collect())
        }
    }
}

/// Walk every agent's conversations + entries, decrypting as we go.
/// Records that fail to decrypt (wrong key) are skipped, not fatal.
/// Manifest entries an unreadable cell keeps from the previous backup, so
/// its conversations stay listed (their objects are still in the Vault).
const LAST_INDEX_FILE: &str = "backup-last-index.json";

fn last_index_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    crate::profile::root(&app).ok().map(|d| d.join(LAST_INDEX_FILE))
}

fn load_last_index(app: &tauri::AppHandle) -> Vec<serde_json::Value> {
    last_index_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// When a pass last left every conversation covered (read in full or
/// listed from a stamped entry), in micros. Written only by such a pass.
const FULL_READ_FILE: &str = "backup-full-read-at";

fn load_full_read_at(app: &tauri::AppHandle) -> Option<i64> {
    crate::profile::root(&app)
        .ok()
        .and_then(|d| std::fs::read_to_string(d.join(FULL_READ_FILE)).ok())
        .and_then(|s| s.trim().parse::<i64>().ok())
}

fn save_full_read_at(app: &tauri::AppHandle, at_micros: i64) {
    if let Ok(d) = crate::profile::root(&app) {
        let _ = std::fs::write(d.join(FULL_READ_FILE), at_micros.to_string());
    }
}

fn save_last_index(app: &tauri::AppHandle, index: &[serde_json::Value]) {
    if let (Some(p), Ok(json)) = (last_index_path(app), serde_json::to_string(index)) {
        let _ = std::fs::write(p, json);
    }
}

/// Every conversation of every connected agent, plus the previous manifest
/// entries of any agent whose cell could not be read this time (a cell
/// past what one read returns in time). One unreadable cell used to fail
/// the whole backup, for weeks, silently except for a log line.
async fn collect_conversations(app: &tauri::AppHandle) -> Result<(Vec<ConvBundle>, Vec<serde_json::Value>), String> {
    let hc = app
        .try_state::<Arc<HolochainState>>()
        .ok_or("holochain state missing")?;
    let manager = hc.get()?;
    let key = manager.data_key()?;
    // Each AI cell, from the line that holds its records: the 0.6 cells, and
    // the 0.7 line's apps that have no 0.6 cell here (named as their v1 app
    // would be, as the manifest's index has always named them).
    let mut agents: Vec<(String, String, BackupSource)> = {
        let map = manager.agents.lock().await;
        map.iter()
            .map(|(k, a)| (k.clone(), a.installed_app_id.clone(), BackupSource::V06))
            .collect()
    };
    if crate::conductor::ports(flowsta_hc_lines::Line::V07).is_some() {
        match crate::hc07::transcript_apps().await {
            Ok(apps) => {
                for a in apps.into_iter().filter(|a| a.enabled) {
                    if agents.iter().any(|(k, _, _)| *k == a.agent) {
                        continue;
                    }
                    let suffix = a.app_id.strip_prefix(crate::hc07::APP_ID_PREFIX).unwrap_or(&a.app_id).to_string();
                    agents.push((a.agent, format!("{}{}", crate::dna::APP_ID_PREFIX, suffix), BackupSource::V07));
                }
            }
            Err(e) => log::warn!("[escrow] the 0.7 line's AIs could not be listed: {e}"),
        }
    }

    let mut bundles = Vec::new();
    let mut unreadable: Vec<String> = Vec::new();
    // A conversation whose entries cannot be read this time stays listed
    // from the previous backup (its object is still in the Vault) instead
    // of failing everything (dev box 09-06: the big cell's list answered in
    // ten minutes, then one entries read past 60 s ended a 20-minute pass).
    let sync = load_sync_state(app);
    // An entry with no activity stamp (written before stamps existed) covers
    // a conversation only if a FULL read has happened since the conversation
    // was last active: the marker below is written at the end of a pass
    // that left nothing unread. A month-old sync state with no such pass
    // behind it proves nothing (a Windows box, 09-06: entries from
    // 08-05, conversations continued since, no full read).
    let full_read_at: Option<i64> = load_full_read_at(app);
    let mut carried: Vec<serde_json::Value> = Vec::new();
    let mut conv_unreadable = 0usize;
    // A conversation listed through two cells (two generations of one AI
    // both read its founding key) is backed up once.
    let mut listed: std::collections::HashSet<String> = std::collections::HashSet::new();
    for (agent_key, installed_app_id, source) in agents {
        let started = std::time::Instant::now();
        // Where this AI's conversations are listed: its founding key, where
        // every device writes since coordinator 9 (so a conversation this
        // device started with an AI first made on another device is backed
        // up here too, and this backup alone holds all of them), and this
        // cell's own key, for the ones started before. A page at a time; a
        // list that stops part way counts as unread, so a partial list never
        // drops conversations from the backup.
        let founding = crate::commands_holochain::founding_key_for(app, &agent_key);
        let mut conv_records: Vec<Sealed> = match list_sealed(&manager, &key, &agent_key, &founding, &source).await {
            Ok(r) => r,
            Err(e) => {
                log::warn!(
                    "[escrow] conversation list read ({}) failed - keeping this AI's conversations from the previous backup: {}",
                    agent_key, e
                );
                unreadable.push(installed_app_id.clone());
                continue;
            }
        };
        conv_records.retain(|r| listed.insert(r.id.clone()));

        // Changed-only reads: the local list cache is written through on
        // every turn the app records, so a conversation whose activity stamp
        // matches the one stored at its last backup has nothing new. It is
        // listed from the sync state and its entries are not read. Reading
        // every entry of every conversation is what made a backup of this
        // library an hour's work (dev box 09-06); after the first full pass
        // only what changed is read.
        // A conversation never continued since the list cache began has no
        // activity stamp; its start is then its last activity (every turn
        // the app records touches the stamp).
        let activity: std::collections::HashMap<String, i64> =
            crate::conversation_cache::read_cache(app, &agent_key)
                .unwrap_or_default()
                .into_iter()
                .map(|c| (c.hash, c.last_active_at.unwrap_or(c.started_at)))
                .collect();
        let mut unchanged_skipped = 0usize;
        let mut read_here = 0usize;
        let mut stalled_in_a_row = 0u32;
        for (conv_i, conv) in conv_records.iter().enumerate() {
            if conv_i > 0 && conv_i % 100 == 0 {
                log::info!(
                    "[escrow] {}: {}/{} conversations, {} read, {} unchanged",
                    installed_app_id, conv_i, conv_records.len(), read_here, unchanged_skipped
                );
            }
            {
                let hex = conv.id.clone();
                let label = format!("conv-{}", &hex[..hex.len().min(24)]);
                if let (Some(prev), Some(&stamp)) = (sync.get(&label), activity.get(&hex)) {
                    let covered = prev.active_at == Some(stamp)
                        || (prev.active_at.is_none() && full_read_at.map(|t| stamp < t).unwrap_or(false));
                    if covered {
                        // started_at from the stored entry, else from the
                        // list record itself (entries written before the
                        // field existed).
                        let started = prev.started_at.or_else(|| conv.plain["started_at"].as_i64());
                        if let Some(started) = started {
                            carried.push(serde_json::json!({
                                "label": label,
                                "role_name": installed_app_id,
                                "agent": agent_key,
                                "title": conv.plain["title"].as_str().map(String::from),
                                "started_at": started,
                                "records": prev.records,
                                "labels": prev.labels,
                                "active_at": stamp,
                            }));
                            unchanged_skipped += 1;
                            continue;
                        }
                    }
                }
            }
            read_here += 1;
            if stalled_in_a_row >= 3 {
                // Three consecutive three-minute stalls: this cell is not
                // answering at a rate a pass can finish. The rest of its
                // conversations keep their previous entries (carried) or
                // wait for the next pass; the other AIs still back up.
                let hex = conv.id.clone();
                let label = format!("conv-{}", &hex[..hex.len().min(24)]);
                if let Some(prev) = sync.get(&label) {
                    let p = &conv.plain;
                    carried.push(serde_json::json!({
                        "label": label,
                        "role_name": installed_app_id,
                        "agent": agent_key,
                        "title": p["title"].as_str(),
                        "started_at": p["started_at"].as_i64().unwrap_or(0),
                        "records": prev.records,
                        "labels": prev.labels,
                    }));
                }
                conv_unreadable += 1;
                continue;
            }
            let started_at = conv.plain["started_at"].as_i64().unwrap_or(0);
            let title = conv.plain["title"].as_str().map(String::from);
            let conv_hash_hex = conv.id.clone();
            let mut records = vec![backup_record("Conversation", conv.clone())];

            // A conversation the Vault already holds may stall at 180 s: the
            // previous copy is carried and nothing is lost. One it does NOT
            // hold yet gets the backup's long wait, because a stall means it
            // is never backed up at all - the dev laptop's "yoai build 2"
            // (1,427 entries in a 1,048-conversation cell) reads in 243 s.
            let label_now = format!("conv-{}", &conv_hash_hex[..conv_hash_hex.len().min(24)]);
            let entries_timeout = if sync.contains_key(&label_now) {
                std::time::Duration::from_secs(180)
            } else {
                std::time::Duration::from_secs(crate::dna::BACKUP_READ_TIMEOUT_SECS)
            };
            // `entries_timeout` is per page of messages; half a conversation
            // is never backed up (`whole`).
            let entry_records: Vec<Sealed> = match entries_sealed(&manager, &key, &agent_key, &conv_hash_hex, &source, entries_timeout).await {
                Ok(r) => {
                    stalled_in_a_row = 0;
                    r
                }
                Err(e) => {
                    conv_unreadable += 1;
                    stalled_in_a_row += 1;
                    if stalled_in_a_row == 3 {
                        log::warn!("[escrow] {} stalled three times in a row - the rest of its conversations wait for the next pass", installed_app_id);
                    }
                    let label = format!("conv-{}", &conv_hash_hex[..conv_hash_hex.len().min(24)]);
                    if let Some(prev) = sync.get(&label) {
                        carried.push(serde_json::json!({
                            "label": label,
                            "role_name": installed_app_id,
                            "started_at": started_at,
                            "records": prev.records,
                            "labels": prev.labels,
                        }));
                    } else {
                        log::warn!(
                            "[escrow] conversation {}… (started {}) - entries could not be read and it has no previous backup; it waits for the next pass: {}",
                            &conv_hash_hex[..conv_hash_hex.len().min(16)], started_at, e
                        );
                    }
                    continue;
                }
            };
            for mut er in entry_records {
                let plain = &mut er.plain;
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
                records.push(backup_record(entry_type, er));
            }

            let size: usize = records
                .iter()
                .map(|r| serde_json::to_vec(r).map(|v| v.len()).unwrap_or(0))
                .sum();
            bundles.push(ConvBundle {
                installed_app_id: installed_app_id.clone(),
                agent: agent_key.clone(),
                title,
                started_at,
                records,
                size,
                active_at: activity.get(&conv_hash_hex).copied(),
            });
        }
        if unchanged_skipped > 0 {
            log::info!("[escrow] {}: {} unchanged conversation(s) listed without a read", installed_app_id, unchanged_skipped);
        }
        log::info!(
            "[escrow] read {} conversation(s) of {} in {} s",
            conv_records.len(),
            installed_app_id,
            started.elapsed().as_secs()
        );
    }
    if !unreadable.is_empty() {
        let previous = load_last_index(app);
        let from_cells: Vec<serde_json::Value> = previous
            .into_iter()
            .filter(|e| e["role_name"].as_str().map(|r| unreadable.contains(&r.to_string())).unwrap_or(false))
            .collect();
        log::warn!(
            "[escrow] {} AI(s) could not be read; {} conversation(s) carried from the previous backup",
            unreadable.len(),
            from_cells.len()
        );
        carried.extend(from_cells);
    }
    if conv_unreadable > 0 {
        log::warn!("[escrow] {} conversation(s) had unreadable entries this pass", conv_unreadable);
    }
    if conv_unreadable == 0 && unreadable.is_empty() {
        // Everything is covered as of now: unstamped entries may lean on it.
        save_full_read_at(app, unix_now_secs() as i64 * 1_000_000);
    }
    Ok((bundles, carried))
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
        // The caller names this profile's own (`line_facts::own_generation`).
        "network_generation": crate::conductor::network_generation(flowsta_hc_lines::Line::V06),
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
            .json(&crate::flowsta::with_identity(serde_json::json!({
                "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
                "label": label,
            })))
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
    let Ok(dir) = crate::profile::root(&app) else { return out };
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
    if let Ok(dir) = crate::profile::root(&app) {
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
        // The person's memory rows for this device (its person-scope cache,
        // sealed under the data key): another device reads them from the
        // copy while this one is off (the memory design §10).
        {
            use base64::Engine;
            // Every scope this device knows: the person's, and each AI's by
            // its founding key (the same address on every device).
            let mut scopes: Vec<String> = vec!["person".to_string()];
            if let Some(ais) = payload["ai_configs"]["data"]["custom-ais"].as_array() {
                for a in ais {
                    if let Some(f) = a["foundingKey"].as_str().or_else(|| a["agentPubKey"].as_str()).filter(|f| !f.is_empty()) {
                        scopes.push(format!("ai:{f}"));
                    }
                }
            }
            let mut block = serde_json::json!({
                "_readme": "Memory rows (rules, commands, locations, decisions, facts, notes) by scope, as this device last read them - the encrypted rows caches, readable with app_keys.",
                "scopes": {},
            });
            for scope in scopes {
                if let Some(rows_path) = crate::memory_rows::cache_path(app, &scope) {
                    if let Ok(bytes) = std::fs::read(&rows_path) {
                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                        if scope == "person" {
                            block["person_raw_b64"] = serde_json::json!(b64);
                        }
                        block["scopes"][scope] = serde_json::json!(b64);
                    }
                }
            }
            if block["scopes"].as_object().map(|o| !o.is_empty()).unwrap_or(false) {
                payload["memory_rows"] = block;
            }
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
        // reason a backup fails.
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
    carried: Vec<serde_json::Value>,
    now_asked: bool,
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
            let labels = prev.map(|p| p.labels.clone()).unwrap_or_default();
            // Same content as last time: stamp the activity so the next
            // pass can skip the read itself.
            if let Some(e) = sync.get_mut(&label) {
                e.active_at = c.active_at;
                e.started_at = Some(c.started_at);
            }
            labels
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
                    active_at: c.active_at,
                    started_at: Some(c.started_at),
                },
            );
            labels
        };

        index.push(serde_json::json!({
            "label": label,
            "role_name": c.installed_app_id,
            "agent": c.agent,
            "title": c.title,
            "started_at": c.started_at,
            "records": c.records.len(),
            "labels": labels,
        }));
    }

    // Conversations listed without a read keep their entries, now stamped
    // with the activity the skip was based on, so the next pass skips them
    // by stamp alone.
    for c in &carried {
        if let (Some(label), Some(stamp)) = (c["label"].as_str(), c["active_at"].as_i64()) {
            if let Some(e) = sync.get_mut(label) {
                e.active_at = Some(stamp);
                if e.started_at.is_none() {
                    e.started_at = c["started_at"].as_i64();
                }
            }
        }
    }
    // Conversations of a cell that could not be read this time stay listed
    // from the previous manifest: their objects are still in the Vault.
    let carried_n = carried.len();
    for c in &carried {
        total_records += c["records"].as_u64().unwrap_or(0);
    }
    index.extend(carried);
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
    manifest["network_generation"] = serde_json::json!(crate::profile::root(app).map(|r| crate::line_facts::own_generation(&r)).unwrap_or(crate::conductor::network_generation(flowsta_hc_lines::Line::V06)));
    attach_extras(app, recovery, &mut manifest);
    // Deletions ride the manifest so a restore on ANOTHER device leaves them
    // deleted too: this device's ledger merged with what the Vault's
    // manifest already carries (the other device's), newest kept.
    let held = own_backup_labels(port).await;
    let vault_manifest = if held.contains(MANIFEST_LABEL) { fetch_vault_label(port, MANIFEST_LABEL).await } else { None };
    manifest["deleted"] = serde_json::Value::Array(merge_tombstones(
        crate::conversation_cache::deleted_ledger(app),
        tombstones_of(vault_manifest.as_ref()),
    ));

    // The small object whenever it changed; the manifest when it changed
    // and its last write is ten minutes behind (a held write is caught up
    // on its own). Neither is rewritten unchanged.
    let now = now_secs();
    let mut prints = load_fingerprints(app);
    let ais = ais_object(&manifest);
    let ais_print = fingerprint_of(&ais);
    let ais_state = match write_decision(prints.get(AIS_LABEL).map(|p| p.hash != ais_print).unwrap_or(true), held.contains(AIS_LABEL), prints.get(AIS_LABEL), now, 0) {
        WriteDecision::Unchanged => "unchanged",
        _ => {
            post_backup_json(port, AIS_LABEL, &ais).await?;
            prints.insert(AIS_LABEL.to_string(), Fingerprint { hash: ais_print, written_at: now });
            "written"
        }
    };
    let manifest_print = fingerprint_of(&manifest);
    let manifest_state = match write_decision(prints.get(MANIFEST_LABEL).map(|p| p.hash != manifest_print).unwrap_or(true), held.contains(MANIFEST_LABEL), prints.get(MANIFEST_LABEL), now, if now_asked { 0 } else { MANIFEST_MIN_GAP_SECS }) {
        WriteDecision::Unchanged => "unchanged".to_string(),
        WriteDecision::Later(secs) => {
            schedule_backup_after(app, secs + 5);
            format!("held, again in {} s", secs)
        }
        WriteDecision::Write => {
            post_backup_json(port, MANIFEST_LABEL, &manifest).await?;
            prints.insert(MANIFEST_LABEL.to_string(), Fingerprint { hash: manifest_print, written_at: now });
            "written".to_string()
        }
    };
    save_fingerprints(app, &prints);

    // The manifest is now the authoritative snapshot - retire the legacy
    // monolith so a stale copy can't shadow it (restore prefers the
    // manifest, but Your Data would show two conflicting snapshots).
    delete_backup_label(port, DATA_LABEL).await;
    save_sync_state(app, &sync);
    save_last_index(app, &index);
    if carried_n > 0 {
        log::info!("[escrow] {} conversation(s) listed from the previous backup without a read (unchanged, or their cell did not answer)", carried_n);
    }

    log::info!(
        "[escrow] incremental backup: {} conversations ({} records), {} object(s) uploaded, {} unchanged; manifest {}, ais {}",
        index.len(),
        total_records,
        uploaded_objects,
        unchanged,
        manifest_state,
        ais_state,
    );
    // The backup must hold everything this device holds: when it lists
    // fewer, say so (some are still unread, or a line did not answer) rather
    // than let a smaller backup pass as complete.
    let on_device = count_local_conversations(app).await.ok();
    if let Some(n) = on_device.filter(|n| (index.len() as u64) < *n) {
        log::warn!("[escrow] the backup lists {} conversation(s) and this device holds {}: {} not in it yet", index.len(), n, n - index.len() as u64);
    }
    Ok(serde_json::json!({
        "mode": "incremental",
        "conversations": index.len(),
        "device_conversations": on_device,
        "records": total_records,
        "truncated_conversations": 0,
        "uploaded_objects": uploaded_objects,
        "unchanged_conversations": unchanged,
        "manifest": manifest_state,
        "ais": ais_state,
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
    write_full_backup_with(app, false).await
}

/// What the backup passes know of their own freshness: how many changes
/// have asked for a backup, how many of them the last complete pass had
/// seen when it started, and when that pass finished.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Freshness {
    pub changes: u64,
    pub covered: Option<u64>,
    pub finished_at_secs: u64,
}

static CHANGES: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
static COVERED: std::sync::Mutex<Option<(u64, u64)>> = std::sync::Mutex::new(None);

/// A change that wants a backup (every scheduling call counts one).
fn note_change() {
    CHANGES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
}

pub(crate) fn freshness() -> Freshness {
    let covered = COVERED.lock().ok().and_then(|c| *c);
    Freshness {
        changes: CHANGES.load(std::sync::atomic::Ordering::SeqCst),
        covered: covered.map(|(n, _)| n),
        finished_at_secs: covered.map(|(_, at)| at).unwrap_or(0),
    }
}

/// Pure: a complete pass finished no more than `max_age_secs` ago and
/// nothing has asked for a backup since it started. Then another pass would
/// read everything again to write nothing ("Restart to update", 11-11:
/// minutes of waiting, three times in a day).
pub(crate) fn is_current(f: Freshness, now_secs: u64, max_age_secs: u64) -> bool {
    f.covered == Some(f.changes) && f.finished_at_secs > 0 && now_secs.saturating_sub(f.finished_at_secs) <= max_age_secs
}

/// How long ago the last complete pass finished, when the backup is current.
pub(crate) fn current_backup_age_secs(max_age_secs: u64) -> Option<u64> {
    let (f, now) = (freshness(), unix_now_secs());
    is_current(f, now, max_age_secs).then(|| now.saturating_sub(f.finished_at_secs))
}

/// `now`: the person asked for it (Settings' "Back up now") - the full
/// record is written whatever its ten-minute hold says; unchanged objects
/// are still not rewritten. (10-20: the button was held like the timer.)
pub async fn write_full_backup_with(app: &tauri::AppHandle, now: bool) -> Result<serde_json::Value, String> {
    // One backup at a time: the debounce, the launch pass, the daily check
    // and the button can all fire within a minute of launch, and two
    // passes reading a thousand-conversation cell at once doubled the
    // conductor's load and raced on the sync state (dev box 09-06 10:12
    // and 10:14). A second caller simply yields; the running pass carries
    // every pending write.
    use std::sync::atomic::{AtomicBool, Ordering};
    static RUNNING: AtomicBool = AtomicBool::new(false);
    if RUNNING.swap(true, Ordering::SeqCst) {
        log::debug!("[escrow] backup already in progress - this trigger yields to it");
        return Err("backup_in_progress".to_string());
    }
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            RUNNING.store(false, Ordering::SeqCst);
        }
    }
    let _done = Done;
    // A debug build's live test holds a pass open this long, so that
    // "Restart to update" can be pressed while one is running.
    if cfg!(debug_assertions) {
        if let Some(ms) = std::env::var("YOAI_TEST_BACKUP_HOLD_MS").ok().and_then(|v| v.trim().parse::<u64>().ok()) {
            tokio::time::sleep(std::time::Duration::from_millis(ms)).await;
        }
    }
    let seen = CHANGES.load(Ordering::SeqCst);
    let result = write_full_backup_inner(app, now).await;
    if let Ok(v) = &result {
        // A held pass wrote nothing and is not a backup of this moment.
        if v.get("skipped").is_none() {
            if let Ok(mut c) = COVERED.lock() {
                *c = Some((seen, unix_now_secs()));
            }
        }
    }
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
                        "at_secs": unix_now_secs(),
                    }),
                );
            }
        }
        // Not-linked / Vault-away / already-running are idle states, not
        // refusals - no notice for those.
        Err(e) if e == "unlinked" || e == "vault_unavailable" || e == "backup_in_progress" || e == "identity_switched" => {}
        Err(e) => {
            record_backup_outcome(
                app,
                serde_json::json!({ "status": "failed", "reason": e, "at": iso_now() }),
            );
        }
    }
    result
}

async fn write_full_backup_inner(app: &tauri::AppHandle, now: bool) -> Result<serde_json::Value, String> {
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

    // Belief vs truth before deciding what is unchanged: a conversation the
    // Vault no longer holds must be read and uploaded, not listed.
    reconcile_sync_state(app, port).await;
    let (convs, carried) = collect_conversations(app).await?;
    // Conversations listed without a read are local conversations too: the
    // empty-device gate below must see them, or a pass that skipped every
    // unchanged conversation reads as an empty device and refuses to write
    // (a Windows box, 09-06 21:57).
    let local_records: u64 = convs.iter().map(|c| c.records.len() as u64).sum::<u64>()
        + carried.iter().map(|c| c["records"].as_u64().unwrap_or(0)).sum::<u64>();

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
    let ai_configs = crate::profile::root(&app)
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
        return write_incremental_backup(app, port, &limits, &recovery, ai_configs, convs, carried, now).await;
    }

    let conversations = convs.len();
    let mut payload = assemble_full_payload(&recovery, ai_configs, convs, BACKUP_BUDGET_BYTES);
    if let Ok(root) = crate::profile::root(app) {
        payload["network_generation"] = serde_json::json!(crate::line_facts::own_generation(&root));
    }
    let total_records = payload["_summary"]["totalRecords"].as_u64().unwrap_or(0);
    let truncated = payload["truncated_conversations"].as_u64().unwrap_or(0);

    attach_extras(app, &recovery, &mut payload);

    let resp = http()
        .post(format!("http://127.0.0.1:{}/backup", port))
        .header("Origin", VAULT_ORIGIN)
        .json(&crate::flowsta::with_identity(serde_json::json!({
            "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
            "app_name": "Your Own AI",
            "label": DATA_LABEL,
            "data": payload,
        })))
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
    note_change();
    let my_gen = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(60)).await;
        if GENERATION.load(Ordering::SeqCst) != my_gen {
            return; // a newer write re-armed the debounce
        }
        match write_full_backup(&app).await {
            Ok(_) => {}
            Err(e) if e == "unlinked" || e == "vault_unavailable" || e == "backup_in_progress" || e == "identity_switched" => {
                log::debug!("[escrow] full backup skipped: {}", e);
            }
            Err(e) if e == "vault_locked" => {
                log::info!("[escrow] full backup waits for the Vault to unlock");
                backup_when_vault_unlocks(&app);
            }
            Err(e) => log::warn!("[escrow] full backup failed: {}", e),
        }
    });
}

/// A change to an AI (its settings, its picture): backed up after ten
/// seconds, not the sixty a conversation write waits for. The AIs travel in
/// the small `ais` object, so the other devices see the change in about a
/// minute and a half (10-20: 2 min 29 s, a minute of it this wait). Its own
/// debounce: a conversation write meanwhile does not push it back.
pub fn schedule_ai_backup(app: &tauri::AppHandle) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static GENERATION: AtomicU64 = AtomicU64::new(0);
    note_change();
    let my_gen = GENERATION.fetch_add(1, Ordering::SeqCst) + 1;
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(AI_BACKUP_SETTLE_SECS)).await;
        if GENERATION.load(Ordering::SeqCst) != my_gen {
            return; // another AI change re-armed it
        }
        match write_full_backup(&app).await {
            Ok(_) => {}
            Err(e) if e == "vault_locked" => backup_when_vault_unlocks(&app),
            // A backup already running carries it; the regular debounce
            // still follows any conversation write.
            Err(e) => log::debug!("[escrow] AI change backup: {}", e),
        }
    });
}

const AI_BACKUP_SETTLE_SECS: u64 = 10;

/// A manifest write held back (its last write was under ten minutes ago)
/// runs when the gap has passed - one timer at a time; a backup that runs
/// sooner for another reason writes it then.
pub fn schedule_backup_after(app: &tauri::AppHandle, secs: u64) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static ARMED: AtomicBool = AtomicBool::new(false);
    if ARMED.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(secs)).await;
        ARMED.store(false, Ordering::SeqCst);
        match write_full_backup(&app).await {
            Ok(_) => {}
            Err(e) if e == "vault_locked" => backup_when_vault_unlocks(&app),
            Err(e) => log::debug!("[escrow] held manifest write: {}", e),
        }
    });
}

/// A backup that found the Vault locked runs the moment it unlocks: poll
/// the Vault every two minutes for up to a day, single-flight. Before this
/// a write with the Vault locked was simply dropped, and a person who
/// chats with the Vault closed could go weeks without a backup (a person's
/// Windows box, 2026-09-06: last backup 32 days old).
pub fn backup_when_vault_unlocks(app: &tauri::AppHandle) {
    use std::sync::atomic::{AtomicBool, Ordering};
    static WAITING: AtomicBool = AtomicBool::new(false);
    if WAITING.swap(true, Ordering::SeqCst) {
        return; // one watcher is enough; it backs up everything pending
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut waited = 0u32;
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(120)).await;
            waited += 1;
            let vault = flowsta::find_vault().await;
            // Unlocked, or locked again and still syncing: either takes backups.
            if vault.port.is_some() && (vault.unlocked || vault.syncing_while_locked == Some(true)) {
                match write_full_backup(&app).await {
                    Ok(_) => log::info!("[escrow] backup written after the Vault unlocked ({} min wait)", waited * 2),
                    Err(e) if e == "vault_locked" => continue,
                    Err(e) => log::warn!("[escrow] backup after unlock failed: {}", e),
                }
                break;
            }
            if waited >= 720 {
                log::info!("[escrow] the Vault stayed locked for a day - the next write or launch tries again");
                break;
            }
        }
        WAITING.store(false, Ordering::SeqCst);
    });
}

/// Daily safety net while the app runs: if no backup has succeeded in the
/// last 24 hours, try one (the debounce only fires on writes, and a launch
/// with the Vault locked used to be the end of it).
/// One escrow sync shortly after the conductor is ready: a signed-in
/// device whose material the identity does not know gets reconciled
/// without anyone opening Settings (the island merge, or the first escrow).
pub fn start_startup_escrow_sync(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(25)).await;
        let status = vault_escrow_sync_inner(&app).await;
        log::info!("[escrow] startup sync: {}", status.state);
        // A Vault unlocked after launch: the join notice now.
        if status.state == "island_pending" {
            if let Ok(port) = escrow_port(&app, true).await {
                if let Some(devices) = crate::device_sync::sibling_names(port).await {
                    crate::device_sync::report(&app, &status.state, &devices);
                }
            }
        }
    });
}

pub fn start_daily_backup_check(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // First pass three minutes after launch (records warmed, the Vault
        // probed), then every six hours. Opening the app with the Vault
        // unlocked used to back nothing up until the next chat.
        let mut wait = std::time::Duration::from_secs(180);
        loop {
            tokio::time::sleep(wait).await;
            wait = std::time::Duration::from_secs(6 * 3600);
            let fresh = last_backup_outcome()
                .map(|o| {
                    o.get("status").and_then(|s| s.as_str()) == Some("ok")
                        && o.get("at_secs").and_then(|a| a.as_u64()).map(|a| unix_now_secs().saturating_sub(a) < 24 * 3600).unwrap_or(false)
                })
                .unwrap_or(false);
            if fresh {
                continue;
            }
            match write_full_backup(&app).await {
                Ok(v) if v.get("skipped").is_some() => {
                    log::warn!("[escrow] daily check: backup held ({})", v["skipped"].as_str().unwrap_or("?"))
                }
                Ok(_) => log::info!("[escrow] daily check: backup written"),
                Err(e) if e == "unlinked" || e == "vault_unavailable" || e == "backup_in_progress" || e == "identity_switched" => {}
                Err(e) if e == "vault_locked" => backup_when_vault_unlocks(&app),
                Err(e) => log::warn!("[escrow] daily check: backup failed: {}", e),
            }
        }
    });
}

fn unix_now_secs() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

/// Manual full backup for the UI ("Back up now") and for testing.
#[tauri::command]
pub async fn vault_backup_now(app: tauri::AppHandle) -> Result<serde_json::Value, String> {
    write_full_backup_with(&app, true).await
}

#[cfg(test)]
mod tombstone_tests {
    use super::*;
    use crate::conversation_cache::DeletedConversation;

    fn d(hash: &str, started: i64, at: i64) -> DeletedConversation {
        DeletedConversation { hash: hash.into(), started_at: started, deleted_at: at }
    }

    #[test]
    fn merge_unions_by_hash_newest_first_and_caps() {
        let mine = vec![d("a", 1, 10), d("b", 2, 30)];
        let theirs = vec![d("a", 1, 20), d("c", 3, 5)];
        let out = merge_tombstones(mine, theirs);
        let hashes: Vec<&str> = out.iter().map(|v| v["hash"].as_str().unwrap()).collect();
        assert_eq!(hashes, vec!["b", "a", "c"]);
        assert_eq!(out[1]["deleted_at"].as_i64(), Some(20));
        let many: Vec<_> = (0..MAX_TOMBSTONES as i64 + 10).map(|i| d(&format!("h{i}"), i, i)).collect();
        assert_eq!(merge_tombstones(many, vec![]).len(), MAX_TOMBSTONES);
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_locked_vault_is_used_only_when_it_still_syncs_this_profiles_identity() {
        const A: &str = "uhCAkXJ2rQ8oO0e0iTqk8m0C9mSxWcZ0sQy8k3o1mT5nE6fJ8K2pL";
        const B: &str = "uhCAkQw3zR9pP1f1jUrl9n1D0nTyXdA1tRz9l4p2nU6oF7gK9L3qM";
        assert_eq!(locked_vault_usable(Some(true), Some(A), Some(A), true), Ok(()), "locked, still syncing, this profile's identity");
        assert_eq!(locked_vault_usable(Some(true), Some(B), Some(A), true), Err("identity_mismatch"), "another identity in the locked Vault");
        assert_eq!(locked_vault_usable(Some(true), Some(A), None, true), Err("vault_locked"), "an owner is never adopted from a locked Vault");
        assert_eq!(locked_vault_usable(Some(true), None, Some(A), true), Err("vault_locked"), "no identity named: nothing to compare");
        assert_eq!(locked_vault_usable(Some(false), Some(A), Some(A), true), Err("vault_locked"), "syncing was stopped on that computer");
        assert_eq!(locked_vault_usable(None, Some(A), Some(A), true), Err("vault_locked"), "a Vault from before it said so");
        assert_eq!(locked_vault_usable(Some(true), Some(A), Some(A), false), Err("vault_locked"), "the adoption path needs the unlocked Vault");
    }

    #[test]
    fn a_refused_restore_names_the_computers_for_the_page() {
        let d = |id: &str, name: Option<&str>| super::SiblingDevice { id: id.into(), name: name.map(String::from), ..Default::default() };
        let msg = super::holding_refusal(&[d("aaaa", Some("Office PC")), d("bbbb", None)]);
        let rest = msg.strip_prefix("siblings_hold_data:").expect("the code first");
        let list: serde_json::Value = serde_json::from_str(rest).unwrap();
        assert_eq!(list[0]["id"], "aaaa");
        assert_eq!(list[0]["name"], "Office PC");
        assert!(list[1]["name"].is_null());
    }

    use super::{ais_object, fingerprint_of, write_decision, Fingerprint, WriteDecision};

    #[test]
    fn the_fingerprint_ignores_when_the_object_was_written() {
        let a = serde_json::json!({ "exported_at_iso": "2026-10-19T01:00:00Z", "ai_configs": { "data": [1] } });
        let b = serde_json::json!({ "exported_at_iso": "2026-10-19T02:00:00Z", "ai_configs": { "data": [1] } });
        let c = serde_json::json!({ "exported_at_iso": "2026-10-19T02:00:00Z", "ai_configs": { "data": [2] } });
        assert_eq!(fingerprint_of(&a), fingerprint_of(&b));
        assert_ne!(fingerprint_of(&a), fingerprint_of(&c));
    }

    #[test]
    fn an_object_is_written_when_missing_or_changed_and_the_manifest_waits_out_its_gap() {
        let last = Fingerprint { hash: "x".into(), written_at: 1_000 };
        assert_eq!(write_decision(false, false, Some(&last), 1_100, 600), WriteDecision::Write, "the Vault lacks it");
        assert_eq!(write_decision(true, true, None, 1_100, 600), WriteDecision::Write, "never written");
        assert_eq!(write_decision(false, true, Some(&last), 1_100, 600), WriteDecision::Unchanged);
        assert_eq!(write_decision(true, true, Some(&last), 1_100, 600), WriteDecision::Later(500));
        assert_eq!(write_decision(true, true, Some(&last), 1_700, 600), WriteDecision::Write);
        assert_eq!(write_decision(true, true, Some(&last), 1_001, 0), WriteDecision::Write, "the small object has no gap");
    }

    #[test]
    fn the_small_object_carries_the_fast_parts_and_none_of_the_conversations() {
        let m = serde_json::json!({
            "version": 2, "license": "CAL", "app": { "name": "Your Own AI" }, "exported_at_iso": "t",
            "network_generation": "holochain-0.6",
            "conversations": [1, 2, 3], "memory_rows": { "scopes": {} }, "app_keys": { "k": 1 },
            "ai_configs": { "data": { "custom-ais": [] } }, "thumbnails": { "data": {} }, "ai_knowledge": { "data": {} }, "corpus": { "data": {} },
        });
        let a = ais_object(&m);
        for k in ["ai_configs", "thumbnails", "ai_knowledge", "corpus", "network_generation", "exported_at_iso"] {
            assert!(a.get(k).is_some(), "{k}");
        }
        for k in ["conversations", "memory_rows", "app_keys"] {
            assert!(a.get(k).is_none(), "{k}");
        }
    }

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
            active_at: None,
            installed_app_id: "ai-1".into(),
            agent: String::new(),
            title: None,
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
            { "id": "f2", "subject": "user", "predicate": "name", "value": "Ada" },
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
        assert_eq!(human[1]["value"], "Ada");
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
            active_at: None,
            installed_app_id: app_id.to_string(),
            agent: String::new(),
            title: None,
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
            link: Default::default(),
        }
    }

    #[test]
    fn fit_corpus_records_drops_cards_oldest_first() {
        let card = "x".repeat(200);
        let mut r = CorpusRecords { version: 1, documents: (1..=5).map(|i| doc(i, &card)).collect(), folders: Vec::new() };
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

#[cfg(test)]
mod network_decision_tests {
    use super::{decide_network, NetworkDecision};
    use crate::transcript_crypto::RecoveryMaterial;

    fn m(seed: &str) -> RecoveryMaterial {
        RecoveryMaterial { network_seed: seed.into(), data_key_hex: format!("{:0>64}", seed.len()), version: 1 }
    }

    #[test]
    fn a_device_started_before_sign_in_joins_the_device_with_conversations() {
        // Linux + Windows, 2026-10-16: Linux started unlinked (its own seed,
        // nothing backed up), Windows holds the chats.
        let linux = m("yoai-user-aaaa");
        let windows = m("yoai-user-zzzz");
        assert_eq!(decide_network(&linux, false, &[(windows.clone(), true)]), NetworkDecision::Join(windows));
    }

    #[test]
    fn the_device_with_conversations_never_moves() {
        let windows = m("yoai-user-zzzz");
        assert_eq!(decide_network(&windows, true, &[(m("yoai-user-aaaa"), false)]), NetworkDecision::Stay);
    }

    #[test]
    fn conversations_on_both_sides_are_the_persons_choice() {
        assert_eq!(decide_network(&m("a"), true, &[(m("b"), true)]), NetworkDecision::Conflict);
    }

    #[test]
    fn agreeing_devices_stay() {
        let same = m("yoai-user-same");
        assert_eq!(decide_network(&same, false, &[(same.clone(), true)]), NetworkDecision::Stay);
        assert_eq!(decide_network(&same, false, &[]), NetworkDecision::Stay);
    }

    #[test]
    fn two_fresh_devices_never_swap_into_each_other() {
        let a = m("yoai-user-aaaa");
        let b = m("yoai-user-bbbb");
        // Exactly one side moves: the higher seed joins the lower.
        assert_eq!(decide_network(&b, false, &[(a.clone(), false)]), NetworkDecision::Join(a.clone()));
        assert_eq!(decide_network(&a, false, &[(b, false)]), NetworkDecision::Stay);
    }

    #[test]
    fn a_device_with_conversations_wins_over_a_lower_fresh_seed() {
        let fresh_low = m("yoai-user-0000");
        let used = m("yoai-user-9999");
        let me = m("yoai-user-5555");
        assert_eq!(decide_network(&me, false, &[(fresh_low, false), (used.clone(), true)]), NetworkDecision::Join(used));
    }
}

#[cfg(test)]
mod freshness_tests {
    use super::*;

    #[test]
    fn a_pass_that_just_finished_with_nothing_written_since_is_current() {
        let f = Freshness { changes: 7, covered: Some(7), finished_at_secs: 1_000 };
        assert!(is_current(f, 1_030, 600));
        assert!(is_current(f, 1_600, 600), "the limit itself still counts");
    }

    #[test]
    fn a_change_since_the_pass_started_wants_another_pass() {
        let f = Freshness { changes: 8, covered: Some(7), finished_at_secs: 1_000 };
        assert!(!is_current(f, 1_030, 600), "written during or after the pass");
    }

    #[test]
    fn an_old_pass_or_none_is_not_current() {
        assert!(!is_current(Freshness { changes: 7, covered: Some(7), finished_at_secs: 1_000 }, 1_601, 600));
        assert!(!is_current(Freshness { changes: 0, covered: None, finished_at_secs: 0 }, 1_000, 600), "no complete pass this run");
    }
}
