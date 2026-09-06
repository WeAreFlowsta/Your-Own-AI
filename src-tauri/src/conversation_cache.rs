//! Last-known-good conversation lists, encrypted at rest.
//!
//! Why: listing conversations reads every generation of an AI's records,
//! and on a large chain (one import day alone wrote 10k+ entries) the
//! zome read can outlive the websocket's 60s limit - on a busy machine
//! the drawer then shows nothing at all for that AI. The cache serves
//! the last successful list instantly - including BEFORE the conductor
//! is up, because the data key comes from the recovery file, not the
//! conductor - while live reads refresh it in the background.
//!
//! Rules: encrypted with the user data key (same story as transcript
//! content - titles never sit plaintext on disk); an empty result never
//! overwrites a non-empty cache (a failed or warming read must not
//! erase a good list).

use crate::commands_holochain::ConversationInfo;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::Manager;

#[derive(Serialize, Deserialize)]
struct EncryptedListFile {
    version: u32,
    nonce: String,
    cipher: String,
}

/// Filename-safe, bounded form of the agent key.
fn safe_key(agent_key: &str) -> String {
    agent_key
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(78)
        .collect()
}

fn path_for(app: &tauri::AppHandle, agent_key: &str) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("No app data dir: {}", e))?;
    Ok(dir.join(format!("conv-list-{}.enc", safe_key(agent_key))))
}

/// The user data key straight from the recovery file - deliberately NOT
/// through the conductor, so cached lists open while it is still starting.
fn data_key(app: &tauri::AppHandle) -> Result<[u8; 32], String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("No app data dir: {}", e))?;
    crate::transcript_crypto::load_recovery_material(&dir)?
        .ok_or_else(|| "No recovery material yet".to_string())?
        .data_key()
}

pub(crate) fn read_cache(
    app: &tauri::AppHandle,
    agent_key: &str,
) -> Result<Vec<ConversationInfo>, String> {
    read_at(app, &path_for(app, agent_key)?)
}

/// Was this AI's cached list written within `secs`? The cache is
/// write-through for everything the app itself does (record, continue,
/// delete), so a fresh file is the list - a live read of a big chain adds
/// nothing but a sixty-second stall. Imports and restores, which write the
/// chain from outside that path, call `mark_stale` so the next read is live.
pub(crate) fn fresh_within(app: &tauri::AppHandle, agent_key: &str, secs: u64) -> bool {
    let Ok(path) = path_for(app, agent_key) else { return false };
    let Ok(meta) = std::fs::metadata(&path) else { return false };
    let Ok(modified) = meta.modified() else { return false };
    std::time::SystemTime::now()
        .duration_since(modified)
        .map(|age| age.as_secs() <= secs)
        .unwrap_or(false)
}

/// The next list read must be live: the chain changed outside the
/// write-through path (an import, a restore).
pub(crate) fn mark_stale(app: &tauri::AppHandle, agent_key: &str) {
    let Ok(path) = path_for(app, agent_key) else { return };
    if let Ok(f) = std::fs::File::options().write(true).open(&path) {
        let _ = f.set_modified(std::time::UNIX_EPOCH);
    }
}

fn read_at(app: &tauri::AppHandle, path: &PathBuf) -> Result<Vec<ConversationInfo>, String> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(path)
        .map_err(|e| format!("Failed to read conversation cache: {}", e))?;
    let file: EncryptedListFile =
        serde_json::from_str(&raw).map_err(|e| format!("Failed to parse cache: {}", e))?;
    let key = data_key(app)?;
    let nonce = hex::decode(&file.nonce).map_err(|e| format!("Bad nonce: {}", e))?;
    let cipher = hex::decode(&file.cipher).map_err(|e| format!("Bad cipher: {}", e))?;
    let plain = crate::transcript_crypto::decrypt(&key, &nonce, &cipher)?;
    serde_json::from_slice(&plain).map_err(|e| format!("Failed to deserialize cache: {}", e))
}

pub(crate) fn write_cache(
    app: &tauri::AppHandle,
    agent_key: &str,
    list: &[ConversationInfo],
) -> Result<(), String> {
    write_at(app, &path_for(app, agent_key)?, list)
}

fn write_at(app: &tauri::AppHandle, path: &PathBuf, list: &[ConversationInfo]) -> Result<(), String> {
    // Never clobber a good list with an empty one.
    if list.is_empty() {
        return Ok(());
    }
    let key = data_key(app)?;
    let plain = serde_json::to_vec(list).map_err(|e| format!("Failed to serialize: {}", e))?;
    let (nonce, cipher) = crate::transcript_crypto::encrypt(&key, &plain)?;
    let file = EncryptedListFile {
        version: 1,
        nonce: hex::encode(nonce),
        cipher: hex::encode(cipher),
    };
    std::fs::write(
        &path,
        serde_json::to_string(&file).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("Failed to write conversation cache: {}", e))?;
    // Owner-only, like every other encrypted store.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Remove one conversation from the cached list - a deleted conversation
/// must not linger in the drawer until a successful live read (slow on a
/// large chain) confirms the removal. Deleting the last row removes the
/// file itself, since write_cache refuses empty lists by design.
pub(crate) fn remove_from_cache(app: &tauri::AppHandle, agent_key: &str, hash: &str) {
    let Ok(mut list) = read_cache(app, agent_key) else { return };
    let before = list.len();
    list.retain(|c| c.hash != hash);
    if list.len() == before {
        return;
    }
    if list.is_empty() {
        if let Ok(path) = path_for(app, agent_key) {
            let _ = std::fs::remove_file(path);
        }
    } else if let Err(e) = write_cache(app, agent_key, &list) {
        log::warn!("[conv-cache] remove failed: {}", e);
    }
}

/// Write-through for a conversation that just started: the cache stays
/// fresh even while full reads are failing on a loaded machine.
pub(crate) fn append_to_cache(app: &tauri::AppHandle, agent_key: &str, info: ConversationInfo) {
    let mut list = read_cache(app, agent_key).unwrap_or_default();
    list.retain(|c| c.hash != info.hash);
    list.insert(0, info);
    if let Err(e) = write_cache(app, agent_key, &list) {
        log::warn!("[conv-cache] append failed: {}", e);
    }
}

/// A turn was just recorded in `hash`: stamp its last activity and move it
/// to the top, in whichever AI's cached list holds it. The caller may only
/// know the lineage agent that holds the conversation, not the AI's current
/// key the list is filed under, so every cached list is checked - there are
/// a handful of small files. Silent when the conversation is not cached yet
/// (the next live read lists it by its start time).
pub(crate) fn touch(app: &tauri::AppHandle, hash: &str, at_micros: i64) {
    let Ok(dir) = app.path().app_data_dir() else { return };
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        if !(name.starts_with("conv-list-") && name.ends_with(".enc")) {
            continue;
        }
        let Ok(mut list) = read_at(app, &path) else { continue };
        let Some(pos) = list.iter().position(|c| c.hash == hash) else { continue };
        let mut conv = list.remove(pos);
        conv.last_active_at = Some(at_micros);
        list.insert(0, conv);
        if let Err(e) = write_at(app, &path, &list) {
            log::warn!("[conv-cache] touch failed: {}", e);
        }
        return;
    }
}
