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
    let dir = crate::profile::root(&app)
        .map_err(|e| format!("No app data dir: {}", e))?;
    Ok(dir.join(format!("conv-list-{}.enc", safe_key(agent_key))))
}

/// The user data key straight from the recovery file - deliberately NOT
/// through the conductor, so cached lists open while it is still starting.
fn data_key(app: &tauri::AppHandle) -> Result<[u8; 32], String> {
    let dir = crate::profile::root(&app)
        .map_err(|e| format!("No app data dir: {}", e))?;
    crate::transcript_crypto::load_recovery_material(&dir)?
        .ok_or_else(|| "No recovery material yet".to_string())?
        .data_key()
}

/// How long a finished live list answers the next caller for the same AI.
/// A busy app asked for one AI's list every two or three seconds while
/// each read took five to thirty (10-16: 368 reads in eleven minutes, and
/// a project note that waited five minutes behind them). Every write the
/// app makes keeps this copy current exactly as it keeps the file, so a
/// reused list is the live list.
pub(crate) const LIVE_REUSE: std::time::Duration = std::time::Duration::from_secs(15);

struct RecentList {
    at: std::time::Instant,
    list: Vec<ConversationInfo>,
}

static RECENT: std::sync::Mutex<Option<std::collections::HashMap<String, RecentList>>> = std::sync::Mutex::new(None);
static GATES: std::sync::Mutex<Option<std::collections::HashMap<String, std::sync::Arc<tokio::sync::Mutex<()>>>>> =
    std::sync::Mutex::new(None);

fn with_recent<R>(f: impl FnOnce(&mut std::collections::HashMap<String, RecentList>) -> R) -> R {
    let mut guard = RECENT.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Default::default))
}

/// One live list read per AI at a time: a caller that arrives while one
/// runs waits for it and takes its answer.
pub(crate) fn live_gate(agent_key: &str) -> std::sync::Arc<tokio::sync::Mutex<()>> {
    let mut guard = GATES.lock().unwrap_or_else(|e| e.into_inner());
    guard.get_or_insert_with(Default::default).entry(agent_key.to_string()).or_default().clone()
}

/// The list a live read finished within `LIVE_REUSE`, empty lists included
/// (an AI with no conversations has no cache file, so before this every
/// call for it was a live read).
pub(crate) fn recent_live(agent_key: &str) -> Option<Vec<ConversationInfo>> {
    with_recent(|m| m.get(agent_key).filter(|r| r.at.elapsed() < LIVE_REUSE).map(|r| r.list.clone()))
}

pub(crate) fn remember_live(agent_key: &str, list: &[ConversationInfo]) {
    with_recent(|m| {
        m.insert(agent_key.to_string(), RecentList { at: std::time::Instant::now(), list: list.to_vec() });
    });
}

// The write-through steps for the in-memory copy, beside the file's.
fn recent_insert(agent_key: &str, info: &ConversationInfo) {
    with_recent(|m| {
        if let Some(r) = m.get_mut(agent_key) {
            r.list.retain(|c| c.hash != info.hash);
            r.list.insert(0, info.clone());
        }
    });
}

fn recent_remove(agent_key: &str, hash: &str) {
    with_recent(|m| {
        if let Some(r) = m.get_mut(agent_key) {
            r.list.retain(|c| c.hash != hash);
        }
    });
}

fn recent_touch(hash: &str, at_micros: i64) {
    with_recent(|m| {
        for r in m.values_mut() {
            if let Some(pos) = r.list.iter().position(|c| c.hash == hash) {
                let mut conv = r.list.remove(pos);
                conv.last_active_at = Some(at_micros);
                r.list.insert(0, conv);
            }
        }
    });
}

fn forget_live(agent_key: Option<&str>) {
    with_recent(|m| match agent_key {
        Some(k) => {
            m.remove(k);
        }
        None => m.clear(),
    });
}

pub(crate) fn read_cache(
    app: &tauri::AppHandle,
    agent_key: &str,
) -> Result<Vec<ConversationInfo>, String> {
    read_at(app, &path_for(app, agent_key)?)
}

fn stale_marker_for(app: &tauri::AppHandle, agent_key: &str) -> Result<PathBuf, String> {
    path_for(app, agent_key).map(|p| p.with_extension("stale"))
}

/// Was this AI's cached list written within `secs`, and is it complete?
/// The cache is write-through for everything the app itself does (record,
/// continue, delete), so a fresh file is the list - a live read of a big
/// chain adds nothing but a sixty-second stall. Imports and restores, which
/// write the chain from outside that path, call `mark_stale`; the list then
/// reads as stale until a complete live read rewrites it (`clear_stale`),
/// however many write-through appends touch the file in between. (The
/// marker used to be the file's own mtime set to zero; the next append
/// reset it, and a restore on the dev laptop trusted a list missing 43
/// imported conversations - 09-06.)
pub(crate) fn fresh_within(app: &tauri::AppHandle, agent_key: &str, secs: u64) -> bool {
    let Ok(path) = path_for(app, agent_key) else { return false };
    if stale_marker_for(app, agent_key).map(|m| m.exists()).unwrap_or(true) {
        return false;
    }
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
    forget_live(Some(agent_key));
    let Ok(marker) = stale_marker_for(app, agent_key) else { return };
    if let Err(e) = std::fs::write(&marker, b"") {
        log::warn!("[conv-cache] could not mark the list stale: {}", e);
    }
}

/// A complete live read just rewrote the list: it is the list again.
pub(crate) fn clear_stale(app: &tauri::AppHandle, agent_key: &str) {
    if let Ok(marker) = stale_marker_for(app, agent_key) {
        let _ = std::fs::remove_file(marker);
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
    recent_remove(agent_key, hash);
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

const DELETED_LEDGER_FILE: &str = "deleted-conversations.json";

/// A conversation this device deleted, kept so a restore does not bring
/// it back from a backup written before the delete (43 short chats came
/// back onto the dev laptop on 09-06 from a 5-August backup). `started_at`
/// is the cross-device key (a replayed conversation gets a new hash);
/// `hash` covers a backup this same device wrote.
#[derive(serde::Serialize, serde::Deserialize, Clone)]
pub(crate) struct DeletedConversation {
    pub hash: String,
    pub started_at: i64,
    pub deleted_at: i64,
}

fn ledger_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    crate::profile::root(&app).ok().map(|d| d.join(DELETED_LEDGER_FILE))
}

pub(crate) fn deleted_ledger(app: &tauri::AppHandle) -> Vec<DeletedConversation> {
    ledger_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

/// Note a deletion. The start time comes from the cached list when the
/// conversation is there; a conversation the cache never saw is noted by
/// hash alone.
pub(crate) fn record_deleted(app: &tauri::AppHandle, agent_key: &str, hash: &str) {
    let started_at = read_cache(app, agent_key)
        .ok()
        .and_then(|l| l.iter().find(|c| c.hash == hash).map(|c| c.started_at))
        .unwrap_or(0);
    let mut ledger = deleted_ledger(app);
    ledger.retain(|d| d.hash != hash);
    ledger.push(DeletedConversation {
        hash: hash.to_string(),
        started_at,
        deleted_at: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_micros() as i64)
            .unwrap_or(0),
    });
    if let (Some(p), Ok(json)) = (ledger_path(app), serde_json::to_string(&ledger)) {
        if let Err(e) = std::fs::write(&p, json) {
            log::warn!("[conv-cache] could not note the deletion: {}", e);
        }
    }
}

/// Write-through for a conversation that just started: the cache stays
/// fresh even while full reads are failing on a loaded machine.
pub(crate) fn append_to_cache(app: &tauri::AppHandle, agent_key: &str, info: ConversationInfo) {
    recent_insert(agent_key, &info);
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
/// Drop every cached list: after records were written through a lineage
/// key (the island merge replays through whichever key answered), the list
/// filed under the AI's current key would otherwise be served as fresh and
/// empty. The next live read rebuilds each list.
pub(crate) fn drop_all(app: &tauri::AppHandle) {
    forget_live(None);
    let Ok(dir) = crate::profile::root(&app) else { return };
    let Ok(entries) = std::fs::read_dir(&dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if name.starts_with("conv-list-") && name.ends_with(".enc") {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

pub(crate) fn touch(app: &tauri::AppHandle, hash: &str, at_micros: i64) {
    recent_touch(hash, at_micros);
    let Ok(dir) = crate::profile::root(&app) else { return };
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

#[cfg(test)]
mod tests {
    use super::*;

    fn conv(hash: &str) -> ConversationInfo {
        ConversationInfo {
            hash: hash.into(),
            ai_personality_id: String::new(),
            ai_personality_name: String::new(),
            model_used: String::new(),
            started_at: 1,
            last_active_at: None,
            title: None,
            source: None,
            agent_key: "agent".into(),
        }
    }

    #[test]
    fn a_finished_read_answers_the_next_caller_and_every_write_keeps_it_current() {
        let ai = "recent-test-ai";
        assert!(recent_live(ai).is_none(), "nothing read yet: the next caller reads live");
        remember_live(ai, &[]);
        assert_eq!(recent_live(ai).map(|l| l.len()), Some(0), "an AI with no conversations is an answer too");
        recent_insert(ai, &conv("a"));
        recent_insert(ai, &conv("b"));
        let hashes = |l: Vec<ConversationInfo>| l.into_iter().map(|c| c.hash).collect::<Vec<_>>();
        assert_eq!(hashes(recent_live(ai).unwrap()), vec!["b", "a"], "a new conversation shows at once, on top");
        recent_touch("a", 99);
        let l = recent_live(ai).unwrap();
        assert_eq!(l[0].hash, "a", "a turn moves its conversation to the top");
        assert_eq!(l[0].last_active_at, Some(99));
        recent_remove(ai, "b");
        assert_eq!(hashes(recent_live(ai).unwrap()), vec!["a"], "a delete leaves at once");
        forget_live(Some(ai));
        assert!(recent_live(ai).is_none(), "an import or restore makes the next read live");
    }

    #[test]
    fn a_write_for_an_ai_nobody_read_yet_does_not_invent_a_list() {
        let ai = "recent-test-unread";
        recent_insert(ai, &conv("x"));
        assert!(recent_live(ai).is_none(), "the first read is still live - a list of one would hide the rest");
    }
}
