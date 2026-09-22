//! Full-text search across a person's conversations, own and imported.
//!
//! The records live on the chain, encrypted, a page at a time; the per-AI
//! episodic index is a recall CACHE (1,000 entries, text cut short) - not
//! an archive. Search needs its own index over the RECORDS, so:
//!
//!  - an encrypted text cache on disk (`search-cache.sqlite` beside the
//!    document library, every blob under the data key - never conversation
//!    text in the clear at rest), filled by reading the records and kept
//!    current by every turn recorded after that; rebuildable, never the
//!    source of truth;
//!  - an in-memory FTS5 index built from that cache the first time a search
//!    is asked for in a session, and fed by each new turn.
//!
//! A search never reads the chain. "Read my conversations for search" does,
//! once per conversation (a conversation whose entry count has not changed
//! is not read again), with progress on `transcript-search-progress`.
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

use crate::commands_holochain::HolochainState;

/// The in-memory index for this session, or None until a search asks.
static INDEX: Mutex<Option<(PathBuf, Connection)>> = Mutex::new(None);
static BUILDING: AtomicBool = AtomicBool::new(false);
static CANCEL: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Deserialize, Clone, Debug)]
struct ConvMeta {
    agent_key: String,
    ai_id: String,
    ai_name: String,
    title: Option<String>,
    source: Option<String>,
    started_at: i64,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
struct EntryText {
    role: String,
    at: i64,
    content: String,
}

fn cache_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = crate::profile::root(app).map_err(|e| format!("No app data dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("app data dir: {e}"))?;
    Ok(dir.join("search-cache.sqlite"))
}

fn open_cache(app: &AppHandle) -> Result<Connection, String> {
    let conn = Connection::open(cache_path(app)?).map_err(|e| format!("search cache: {e}"))?;
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE IF NOT EXISTS convs (
           hash TEXT PRIMARY KEY,
           agent_key TEXT NOT NULL,
           entries INTEGER NOT NULL,
           meta_enc BLOB NOT NULL,
           touched INTEGER NOT NULL DEFAULT 0
         );
         CREATE TABLE IF NOT EXISTS entries (
           hash TEXT NOT NULL,
           seq INTEGER NOT NULL,
           blob_enc BLOB NOT NULL,
           PRIMARY KEY (hash, seq)
         );",
    )
    .map_err(|e| format!("search cache schema: {e}"))?;
    Ok(conn)
}

fn enc(key: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>, String> {
    let (nonce, cipher) = crate::transcript_crypto::encrypt(key, plain)?;
    let mut out = Vec::with_capacity(24 + cipher.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&cipher);
    Ok(out)
}

fn dec(key: &[u8; 32], blob: &[u8]) -> Result<Vec<u8>, String> {
    if blob.len() < 24 {
        return Err("search cache: blob too short".into());
    }
    crate::transcript_crypto::decrypt(key, &blob[..24], &blob[24..])
}

fn data_key(app: &AppHandle) -> Result<[u8; 32], String> {
    let hc: State<'_, Arc<HolochainState>> = app.state();
    hc.get()?.data_key()
}

// ---------------------------------------------------------------- the index

fn fresh_index() -> Result<Connection, String> {
    let c = Connection::open_in_memory().map_err(|e| e.to_string())?;
    // Text is the only indexed column; the rest ride along for the result.
    c.execute_batch(
        "CREATE VIRTUAL TABLE e USING fts5(
           content,
           hash UNINDEXED, seq UNINDEXED, role UNINDEXED, at UNINDEXED,
           agent_key UNINDEXED, ai_id UNINDEXED, ai_name UNINDEXED,
           title UNINDEXED, source UNINDEXED,
           tokenize = 'unicode61 remove_diacritics 2'
         );",
    )
    .map_err(|e| e.to_string())?;
    Ok(c)
}

fn index_insert(c: &Connection, hash: &str, seq: u32, e: &EntryText, m: &ConvMeta) -> Result<(), String> {
    c.execute(
        "INSERT INTO e (content, hash, seq, role, at, agent_key, ai_id, ai_name, title, source) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
        params![e.content, hash, seq, e.role, e.at, m.agent_key, m.ai_id, m.ai_name, m.title, m.source],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Load the in-memory index from the cache (once per session).
fn ensure_index(app: &AppHandle) -> Result<(), String> {
    let mut guard = INDEX.lock().unwrap_or_else(|e| e.into_inner());
    let path = cache_path(app)?;
    if guard.as_ref().map(|(p, _)| p == &path).unwrap_or(false) {
        return Ok(());
    }
    let key = data_key(app)?;
    let cache = open_cache(app)?;
    let idx = fresh_index()?;
    let mut metas: std::collections::HashMap<String, ConvMeta> = std::collections::HashMap::new();
    {
        let mut stmt = cache.prepare("SELECT hash, meta_enc FROM convs").map_err(|e| e.to_string())?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))).map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            if let Ok(m) = serde_json::from_slice::<ConvMeta>(&dec(&key, &row.1)?) {
                metas.insert(row.0, m);
            }
        }
    }
    let t0 = std::time::Instant::now();
    let mut n = 0usize;
    {
        let mut stmt = cache.prepare("SELECT hash, seq, blob_enc FROM entries").map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, u32>(1)?, r.get::<_, Vec<u8>>(2)?)))
            .map_err(|e| e.to_string())?;
        for (hash, seq, blob) in rows.flatten() {
            let Some(m) = metas.get(&hash) else { continue };
            if let Ok(e) = serde_json::from_slice::<EntryText>(&dec(&key, &blob)?) {
                index_insert(&idx, &hash, seq, &e, m)?;
                n += 1;
            }
        }
    }
    log::info!("[search] index loaded: {n} messages from {} conversations in {} ms", metas.len(), t0.elapsed().as_millis());
    *guard = Some((path, idx));
    Ok(())
}

// ---------------------------------------------------------------- building

#[derive(Serialize, Clone)]
pub struct SearchProgress {
    pub done: usize,
    pub total: usize,
    pub finished: bool,
    pub cancelled: bool,
}

#[derive(Serialize, Clone)]
pub struct SearchStatus {
    pub conversations: usize,
    pub messages: usize,
    pub building: bool,
}

#[tauri::command]
pub fn transcript_search_status(app: AppHandle) -> Result<SearchStatus, String> {
    let cache = open_cache(&app)?;
    let conversations: usize = cache.query_row("SELECT count(*) FROM convs", [], |r| r.get::<_, i64>(0)).map_err(|e| e.to_string())? as usize;
    let messages: usize = cache.query_row("SELECT count(*) FROM entries", [], |r| r.get::<_, i64>(0)).map_err(|e| e.to_string())? as usize;
    Ok(SearchStatus { conversations, messages, building: BUILDING.load(Ordering::SeqCst) })
}

#[tauri::command]
pub fn transcript_search_cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// Read this agent's conversations into the cache (and the index, when it
/// is loaded): every conversation the cache does not hold at its current
/// entry count. Runs in the background; progress on
/// `transcript-search-progress`; one build at a time.
#[tauri::command]
pub async fn transcript_search_build(app: AppHandle, agent_key: String) -> Result<(), String> {
    if BUILDING.swap(true, Ordering::SeqCst) {
        return Ok(());
    }
    CANCEL.store(false, Ordering::SeqCst);
    tauri::async_runtime::spawn(async move {
        let r = build(&app, &agent_key).await;
        BUILDING.store(false, Ordering::SeqCst);
        if let Err(e) = r {
            log::warn!("[search] reading conversations failed: {e}");
            let _ = app.emit("transcript-search-progress", SearchProgress { done: 0, total: 0, finished: true, cancelled: true });
        }
    });
    Ok(())
}

async fn build(app: &AppHandle, agent_key: &str) -> Result<(), String> {
    let key = data_key(app)?;
    // The list the drawer shows (its cache is write-through, so a fresh one IS the list).
    let convs = crate::commands_holochain::get_conversations(app.clone(), agent_key.to_string(), app.state(), Some(600)).await?;
    let total = convs.len();
    let mut done = 0usize;
    let mut read = 0usize;
    let t0 = std::time::Instant::now();
    for c in convs {
        if CANCEL.load(Ordering::SeqCst) {
            let _ = app.emit("transcript-search-progress", SearchProgress { done, total, finished: true, cancelled: true });
            return Ok(());
        }
        done += 1;
        // Known and untouched since the cache read it: nothing to read. A
        // turn recorded since goes into the cache as it happens
        // (`note_recorded`), which also moves the stamp forward.
        let touched_since = c.last_active_at.unwrap_or(c.started_at);
        if touched_since <= cached_touch(app, &c.hash) {
            if done % 25 == 0 {
                let _ = app.emit("transcript-search-progress", SearchProgress { done, total, finished: false, cancelled: false });
            }
            continue;
        }
        let entries = crate::commands_holochain::get_conversation_transcript(c.agent_key.clone(), c.hash.clone(), app.state()).await.unwrap_or_default();
        let meta = ConvMeta {
            agent_key: c.agent_key.clone(),
            ai_id: c.ai_personality_id.clone(),
            ai_name: c.ai_personality_name.clone(),
            title: c.title.clone(),
            source: c.source.clone(),
            started_at: c.started_at,
        };
        store_conversation(app, &key, &c.hash, &meta, entries.iter().map(|e| (e.sequence, EntryText { role: e.role.clone(), at: e.timestamp, content: e.content.clone() })), touched_since)?;
        read += 1;
        let _ = app.emit("transcript-search-progress", SearchProgress { done, total, finished: false, cancelled: false });
    }
    log::info!("[search] read {read} of {total} conversations for search in {} ms", t0.elapsed().as_millis());
    let _ = app.emit("transcript-search-progress", SearchProgress { done, total, finished: true, cancelled: false });
    Ok(())
}

/// When the cache last read this conversation (the records' last-active
/// stamp at that moment); 0 when unknown.
fn cached_touch(app: &AppHandle, hash: &str) -> i64 {
    open_cache(app)
        .ok()
        .and_then(|c| c.query_row("SELECT touched FROM convs WHERE hash = ?1", params![hash], |r| r.get::<_, i64>(0)).ok())
        .unwrap_or(0)
}

fn store_conversation(
    app: &AppHandle,
    key: &[u8; 32],
    hash: &str,
    meta: &ConvMeta,
    entries: impl Iterator<Item = (u32, EntryText)>,
    touched: i64,
) -> Result<(), String> {
    let mut cache = open_cache(app)?;
    let tx = cache.transaction().map_err(|e| e.to_string())?;
    tx.execute("DELETE FROM entries WHERE hash = ?1", params![hash]).map_err(|e| e.to_string())?;
    let mut n = 0i64;
    let mut for_index: Vec<(u32, EntryText)> = Vec::new();
    for (seq, e) in entries {
        if e.content.trim().is_empty() {
            continue;
        }
        let blob = enc(key, &serde_json::to_vec(&e).map_err(|e| e.to_string())?)?;
        tx.execute("INSERT OR REPLACE INTO entries (hash, seq, blob_enc) VALUES (?1, ?2, ?3)", params![hash, seq, blob]).map_err(|e| e.to_string())?;
        n += 1;
        for_index.push((seq, e));
    }
    let meta_blob = enc(key, &serde_json::to_vec(meta).map_err(|e| e.to_string())?)?;
    tx.execute(
        "INSERT OR REPLACE INTO convs (hash, agent_key, entries, meta_enc, touched) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![hash, meta.agent_key, n, meta_blob, touched],
    )
    .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    // The loaded index follows the cache.
    let guard = INDEX.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, idx)) = guard.as_ref() {
        idx.execute("DELETE FROM e WHERE hash = ?1", params![hash]).map_err(|e| e.to_string())?;
        for (seq, e) in &for_index {
            index_insert(idx, hash, *seq, e, meta)?;
        }
    }
    Ok(())
}

/// A turn just recorded: into the cache (when the conversation is known to
/// it) and the loaded index, so search sees it without a rebuild.
pub fn note_recorded(app: &AppHandle, hash: &str, seq: u32, role: &str, content: &str, at: i64) {
    if content.trim().is_empty() {
        return;
    }
    let Ok(key) = data_key(app) else { return };
    let Ok(cache) = open_cache(app) else { return };
    let meta: Option<ConvMeta> = cache
        .query_row("SELECT meta_enc FROM convs WHERE hash = ?1", params![hash], |r| r.get::<_, Vec<u8>>(0))
        .optional()
        .ok()
        .flatten()
        .and_then(|b| dec(&key, &b).ok())
        .and_then(|p| serde_json::from_slice(&p).ok());
    let Some(meta) = meta else { return }; // not read yet: the next build takes it whole
    let e = EntryText { role: role.to_string(), at, content: content.to_string() };
    let Ok(plain) = serde_json::to_vec(&e) else { return };
    let Ok(blob) = enc(&key, &plain) else { return };
    let _ = cache.execute("INSERT OR REPLACE INTO entries (hash, seq, blob_enc) VALUES (?1, ?2, ?3)", params![hash, seq, blob]);
    let _ = cache.execute("UPDATE convs SET entries = entries + 1, touched = ?2 WHERE hash = ?1", params![hash, at]);
    let guard = INDEX.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((_, idx)) = guard.as_ref() {
        let _ = index_insert(idx, hash, seq, &e, &meta);
    }
}

// ---------------------------------------------------------------- searching

#[derive(Serialize, Clone, Debug)]
pub struct SearchHit {
    pub hash: String,
    pub seq: u32,
    pub role: String,
    pub at: i64,
    pub ai_id: String,
    pub ai_name: String,
    pub title: Option<String>,
    pub source: Option<String>,
    /// The matching stretch, matches wrapped in `\u{1}` … `\u{2}` (the page
    /// turns them into marks; never HTML from here).
    pub snippet: String,
    /// Other matching messages in the same conversation.
    pub more: usize,
}

#[derive(Serialize, Clone, Debug)]
pub struct SearchAnswer {
    pub hits: Vec<SearchHit>,
    /// The cache holds nothing for this agent yet: offer to read.
    pub needs_read: bool,
    pub building: bool,
}

/// What a person typed, as an FTS5 query: each word quoted (no operators
/// leak), the last one a prefix so typing narrows as it goes.
pub fn fts_query(text: &str) -> String {
    let words: Vec<String> = text
        .split_whitespace()
        .map(|w| w.replace('"', ""))
        .filter(|w| !w.is_empty())
        .collect();
    let n = words.len();
    words
        .iter()
        .enumerate()
        .map(|(i, w)| if i + 1 == n && w.chars().count() >= 2 { format!("\"{w}\"*") } else { format!("\"{w}\"") })
        .collect::<Vec<_>>()
        .join(" ")
}

#[tauri::command]
pub fn transcript_search(app: AppHandle, agent_key: String, query: String, limit: Option<u32>) -> Result<SearchAnswer, String> {
    let q = fts_query(&query);
    let building = BUILDING.load(Ordering::SeqCst);
    let known: i64 = {
        let cache = open_cache(&app)?;
        cache.query_row("SELECT count(*) FROM convs WHERE agent_key = ?1", params![agent_key], |r| r.get(0)).unwrap_or(0)
    };
    if q.is_empty() || known == 0 {
        return Ok(SearchAnswer { hits: vec![], needs_read: known == 0, building });
    }
    ensure_index(&app)?;
    let guard = INDEX.lock().unwrap_or_else(|e| e.into_inner());
    let Some((_, idx)) = guard.as_ref() else { return Ok(SearchAnswer { hits: vec![], needs_read: true, building }) };
    // Best match per conversation, ranked; the count of other matches rides along.
    let mut stmt = idx
        .prepare(
            "SELECT hash, seq, role, at, ai_id, ai_name, title, source,
                    snippet(e, 0, char(1), char(2), ' … ', 18) AS snip,
                    bm25(e) AS score
             FROM e WHERE e MATCH ?1 AND agent_key = ?2
             ORDER BY score LIMIT 400",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![q, agent_key], |r| {
            Ok(SearchHit {
                hash: r.get(0)?,
                seq: r.get(1)?,
                role: r.get(2)?,
                at: r.get(3)?,
                ai_id: r.get(4)?,
                ai_name: r.get(5)?,
                title: r.get(6)?,
                source: r.get(7)?,
                snippet: r.get(8)?,
                more: 0,
            })
        })
        .map_err(|e| e.to_string())?;
    let mut by_conv: Vec<SearchHit> = Vec::new();
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    for h in rows.flatten() {
        match seen.get(&h.hash) {
            Some(&i) => by_conv[i].more += 1,
            None => {
                seen.insert(h.hash.clone(), by_conv.len());
                by_conv.push(h);
            }
        }
    }
    by_conv.truncate(limit.unwrap_or(30) as usize);
    Ok(SearchAnswer { hits: by_conv, needs_read: false, building })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_typed_query_becomes_a_safe_prefix_match() {
        assert_eq!(fts_query("boat survey"), "\"boat\" \"survey\"*");
        assert_eq!(fts_query("port"), "\"port\"*");
        assert_eq!(fts_query("a"), "\"a\"");
        // operators and quotes never reach the engine as syntax
        assert_eq!(fts_query("NOT \"quote\" OR"), "\"NOT\" \"quote\" \"OR\"*");
        assert_eq!(fts_query("   "), "");
    }

    #[test]
    fn the_index_finds_words_and_marks_them() {
        let idx = fresh_index().unwrap();
        let m = ConvMeta { agent_key: "a".into(), ai_id: "ai".into(), ai_name: "Reeves".into(), title: Some("Boat".into()), source: None, started_at: 1 };
        index_insert(&idx, "h1", 1, &EntryText { role: "user".into(), at: 1, content: "The survey of the sloop Wren was done at Port Quillon.".into() }, &m).unwrap();
        index_insert(&idx, "h1", 2, &EntryText { role: "assistant".into(), at: 2, content: "Two planks on the starboard side need refastening.".into() }, &m).unwrap();
        index_insert(&idx, "h2", 1, &EntryText { role: "user".into(), at: 3, content: "what colour is the sky".into() }, &m).unwrap();
        let q = fts_query("quill");
        let snip: String = idx
            .query_row("SELECT snippet(e, 0, char(1), char(2), ' … ', 18) FROM e WHERE e MATCH ?1 AND agent_key = 'a'", params![q], |r| r.get(0))
            .unwrap();
        assert!(snip.contains("\u{1}Quillon\u{2}"), "{snip}");
        // diacritics do not matter
        let n: i64 = idx.query_row("SELECT count(*) FROM e WHERE e MATCH ?1", params![fts_query("Quíllon")], |r| r.get(0)).unwrap();
        assert_eq!(n, 1);
        // another agent sees nothing
        let n: i64 = idx.query_row("SELECT count(*) FROM e WHERE e MATCH ?1 AND agent_key = 'b'", params![fts_query("sloop")], |r| r.get(0)).unwrap();
        assert_eq!(n, 0);
    }
}
