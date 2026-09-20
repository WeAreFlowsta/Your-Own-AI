//! Keep a folder in sync with the library.
//!
//! A dropped folder used to be a snapshot: a known path was skipped before
//! the file was even read, so an edited note kept its old passages and a
//! deleted one went on being quoted. A FOLDER here is a root the person
//! asked to keep current. A pass walks it and sorts every file into new /
//! changed / unchanged, and every known document whose file is gone into
//! removed:
//!  - new       -> read, split, embed, stored like any import, granted to
//!                 the folder's AIs;
//!  - changed   -> size or modified time differs AND the content hash
//!                 differs: passages replaced in place (`store_reread`), so
//!                 the document keeps its id, its card, its Mine flag and
//!                 its grants;
//!  - removed   -> the document leaves the library.
//! Identity is the file's path (`path_hash`), as everywhere else in the
//! library. The cheap check (size + modified time) keeps a pass over a large
//! unchanged vault to one `stat` per file.
//!
//! A root that cannot be read (an unplugged drive, a renamed folder) removes
//! NOTHING: absence of the whole folder is not evidence that every note was
//! deleted.

use super::*;
use std::collections::{HashMap, HashSet};

/// One sync pass at a time, across all folders.
static SYNC_RUNNING: AtomicBool = AtomicBool::new(false);
static SYNC_CANCEL: AtomicBool = AtomicBool::new(false);

/// Columns and the table a synced folder needs. Safe to run on every open.
pub(super) fn ensure_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS folders (
           folder_id TEXT PRIMARY KEY,
           path_hash TEXT NOT NULL UNIQUE,
           added_at INTEGER NOT NULL,
           last_scan_at INTEGER,
           meta_enc BLOB NOT NULL
         );",
    )
    .map_err(|e| format!("corpus folders schema: {e}"))?;
    let have: HashSet<String> = {
        let mut stmt = conn.prepare("PRAGMA table_info(documents)").map_err(|e| e.to_string())?;
        let cols = stmt.query_map([], |r| r.get::<_, String>(1)).map_err(|e| e.to_string())?;
        cols.flatten().collect()
    };
    for (col, ty) in [("folder_id", "TEXT"), ("content_hash", "TEXT"), ("mtime", "INTEGER")] {
        if !have.contains(col) {
            conn.execute(&format!("ALTER TABLE documents ADD COLUMN {col} {ty}"), [])
                .map_err(|e| format!("corpus schema ({col}): {e}"))?;
        }
    }
    conn.execute_batch("CREATE INDEX IF NOT EXISTS documents_folder ON documents(folder_id);")
        .map_err(|e| e.to_string())
}

/// Encrypted per folder: where it is and who reads it.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct FolderMeta {
    pub path: String,
    /// AIs that are given every document found in the folder.
    pub ai_ids: Vec<String>,
    /// "logseq" | "obsidian" | None - only names the notes app for the UI.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

#[derive(Serialize, Clone, Debug)]
pub struct FolderRecord {
    pub folder_id: String,
    pub added_at: i64,
    pub last_scan_at: Option<i64>,
    pub documents: i64,
    pub meta: FolderMeta,
    /// False when the folder cannot be read right now (drive away, renamed).
    pub reachable: bool,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct SyncReport {
    pub folder_id: String,
    pub folder: String,
    pub added: usize,
    pub updated: usize,
    pub removed: usize,
    pub unchanged: usize,
    pub failed: Vec<ImportFailure>,
    /// The folder could not be read: nothing was changed.
    pub unreachable: bool,
    pub cancelled: bool,
}

// ---------------------------------------------------------------- the plan

/// A file found in the folder.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Seen {
    pub path: String,
    pub size: i64,
    pub mtime: i64,
}

/// A document the library already holds for this folder.
#[derive(Clone, Debug, PartialEq)]
pub(super) struct Known {
    pub doc_id: String,
    pub path_hash: String,
    pub size: i64,
    pub mtime: Option<i64>,
}

#[derive(Debug, Default, PartialEq)]
pub(super) struct Plan {
    /// Indexes into `seen`: files the library does not hold.
    pub add: Vec<usize>,
    /// (index into `seen`, doc_id): size or modified time differs - look at
    /// the content before deciding.
    pub look: Vec<(usize, String)>,
    pub unchanged: usize,
    /// Documents whose file is gone.
    pub remove: Vec<String>,
}

/// Sort a walk against what the library holds. Pure: no disk, no database.
pub(super) fn plan(seen: &[Seen], known: &[Known]) -> Plan {
    let by_hash: HashMap<&str, &Known> = known.iter().map(|k| (k.path_hash.as_str(), k)).collect();
    let mut out = Plan::default();
    let mut present: HashSet<&str> = HashSet::new();
    for (i, s) in seen.iter().enumerate() {
        let h = path_hash(&s.path);
        match by_hash.get(h.as_str()) {
            None => out.add.push(i),
            Some(k) => {
                present.insert(k.doc_id.as_str());
                if k.size == s.size && k.mtime == Some(s.mtime) {
                    out.unchanged += 1;
                } else {
                    out.look.push((i, k.doc_id.clone()));
                }
            }
        }
    }
    out.remove = known.iter().filter(|k| !present.contains(k.doc_id.as_str())).map(|k| k.doc_id.clone()).collect();
    out
}

/// Folders inside a notes vault that hold copies, not notes: Logseq keeps a
/// timestamped copy of every edited page under `logseq/bak` and
/// `logseq/version-files`. (Dot-folders - `.obsidian`, `.trash`,
/// `logseq/.recycle` - are already skipped by the walk.)
pub(super) fn is_vault_noise(root: &Path, file: &Path) -> bool {
    let Ok(rel) = file.strip_prefix(root) else { return false };
    let parts: Vec<&str> = rel.components().filter_map(|c| c.as_os_str().to_str()).collect();
    parts.windows(2).any(|w| w[0] == "logseq" && (w[1] == "bak" || w[1] == "version-files"))
}

/// The hash a changed file is compared by. Keyed with the person's data key,
/// so the column cannot be used to test whether a known file is present.
fn content_hash(key: &[u8; 32], bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(key);
    h.update(bytes);
    hex::encode(h.finalize())
}

fn stat(path: &Path) -> Option<(i64, i64)> {
    let m = std::fs::metadata(path).ok()?;
    let mtime = m.modified().ok()?.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64;
    Some((m.len() as i64, mtime))
}

// ---------------------------------------------------------------- folders

fn new_folder_id() -> String {
    format!("f{}", &new_doc_id()[1..])
}

fn read_folder(key: &[u8; 32], blob: &[u8]) -> Option<FolderMeta> {
    serde_json::from_slice(&dec(key, blob).ok()?).ok()
}

fn list_folders(conn: &Connection, key: &[u8; 32]) -> Result<Vec<FolderRecord>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT f.folder_id, f.added_at, f.last_scan_at, f.meta_enc,
                    (SELECT COUNT(*) FROM documents d WHERE d.folder_id = f.folder_id)
             FROM folders f ORDER BY f.added_at",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, Option<i64>>(2)?, r.get::<_, Vec<u8>>(3)?, r.get::<_, i64>(4)?)))
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (folder_id, added_at, last_scan_at, blob, documents) in rows.flatten() {
        let Some(meta) = read_folder(key, &blob) else { continue };
        let reachable = Path::new(&meta.path).is_dir();
        out.push(FolderRecord { folder_id, added_at, last_scan_at, documents, meta, reachable });
    }
    Ok(out)
}

/// Register a folder for an AI (or add the AI to a folder already kept).
fn add_folder(conn: &Connection, key: &[u8; 32], path: &str, ai_id: &str, kind: Option<String>) -> Result<String, String> {
    let ph = path_hash(path);
    let existing: Option<(String, Vec<u8>)> = conn
        .query_row("SELECT folder_id, meta_enc FROM folders WHERE path_hash = ?1", params![ph], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()
        .map_err(|e| e.to_string())?;
    if let Some((folder_id, blob)) = existing {
        let mut meta = read_folder(key, &blob).unwrap_or(FolderMeta { path: path.to_string(), ..Default::default() });
        if !meta.ai_ids.iter().any(|a| a == ai_id) {
            meta.ai_ids.push(ai_id.to_string());
        }
        if kind.is_some() {
            meta.kind = kind;
        }
        let json = serde_json::to_vec(&meta).map_err(|e| e.to_string())?;
        conn.execute("UPDATE folders SET meta_enc = ?2 WHERE folder_id = ?1", params![folder_id, enc(key, &json)?])
            .map_err(|e| e.to_string())?;
        return Ok(folder_id);
    }
    let folder_id = new_folder_id();
    let meta = FolderMeta { path: path.to_string(), ai_ids: vec![ai_id.to_string()], kind };
    let json = serde_json::to_vec(&meta).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO folders (folder_id, path_hash, added_at, meta_enc) VALUES (?1, ?2, ?3, ?4)",
        params![folder_id, ph, now_secs(), enc(key, &json)?],
    )
    .map_err(|e| e.to_string())?;
    Ok(folder_id)
}

/// Documents the library holds for a folder: the ones tagged with it, plus
/// any dropped earlier from inside it (adopted now, so a folder dropped
/// before it was kept in sync does not become a second copy of itself).
fn known_for(conn: &Connection, folder_id: &str, seen: &[Seen]) -> Result<Vec<Known>, String> {
    for s in seen {
        conn.execute(
            "UPDATE documents SET folder_id = ?1 WHERE path_hash = ?2 AND folder_id IS NULL",
            params![folder_id, path_hash(&s.path)],
        )
        .map_err(|e| e.to_string())?;
    }
    let mut stmt = conn
        .prepare("SELECT doc_id, path_hash, byte_size, mtime FROM documents WHERE folder_id = ?1 AND path_hash IS NOT NULL")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![folder_id], |r| Ok(Known { doc_id: r.get(0)?, path_hash: r.get(1)?, size: r.get(2)?, mtime: r.get(3)? }))
        .map_err(|e| e.to_string())?;
    Ok(rows.flatten().collect())
}

fn stamp(conn: &Connection, doc_id: &str, folder_id: &str, size: i64, mtime: i64, hash: &str) -> Result<(), String> {
    conn.execute(
        "UPDATE documents SET folder_id = ?2, byte_size = ?3, mtime = ?4, content_hash = ?5 WHERE doc_id = ?1",
        params![doc_id, folder_id, size, mtime, hash],
    )
    .map(|_| ())
    .map_err(|e| e.to_string())
}

// ---------------------------------------------------------------- a pass

/// Read, split and embed one file. Err = why it could not be taken in.
async fn read_and_embed(
    app: &AppHandle,
    llm_state: &State<'_, crate::llm::LLMState>,
    path: &Path,
) -> Result<(Vec<String>, Vec<Vec<f32>>), String> {
    let text = extract_text_safe(path)?;
    let passages = chunk_text(&text, PASSAGE_CHARS);
    if passages.is_empty() {
        return Err("no readable text (a scanned PDF or an empty file)".into());
    }
    let mut vectors: Vec<Vec<f32>> = Vec::with_capacity(passages.len());
    for batch in passages.chunks(EMBED_BATCH) {
        if SYNC_CANCEL.load(Ordering::SeqCst) {
            return Err("stopped".into());
        }
        let v = crate::llm::embed_texts(app.clone(), llm_state.clone(), batch.to_vec(), EMBEDDING_MODEL_FILE.to_string())
            .await
            .map_err(|e| format!("embedding: {e}"))?;
        vectors.extend(v);
    }
    if vectors.len() != passages.len() {
        return Err("embedding returned fewer pieces than were sent".into());
    }
    Ok((passages, vectors))
}

async fn sync_one(
    app: &AppHandle,
    llm_state: &State<'_, crate::llm::LLMState>,
    key: &[u8; 32],
    folder: &FolderRecord,
) -> Result<SyncReport, String> {
    let mut report = SyncReport { folder_id: folder.folder_id.clone(), folder: folder.meta.path.clone(), ..Default::default() };
    let root = PathBuf::from(&folder.meta.path);
    if !root.is_dir() || std::fs::read_dir(&root).is_err() {
        report.unreachable = true;
        return Ok(report);
    }
    let seen: Vec<Seen> = walk(&[folder.meta.path.clone()])
        .into_iter()
        .filter(|p| !is_vault_noise(&root, p))
        .filter_map(|p| stat(&p).map(|(size, mtime)| Seen { path: p.to_string_lossy().to_string(), size, mtime }))
        .collect();
    let mut conn = open(app)?;
    let known = known_for(&conn, &folder.folder_id, &seen)?;
    let plan = plan(&seen, &known);
    report.unchanged = plan.unchanged;
    let total = plan.add.len() + plan.look.len();
    let ai_label = folder.meta.ai_ids.first().cloned().unwrap_or_default();
    let mut done = 0usize;
    let progress = |phase: &'static str, file: &str, done: usize, added: usize, failed: usize| {
        emit_progress(app, &Progress { phase, file: file.to_string(), done, total, added, failed, pieces_done: 0, pieces_total: 0, ai_id: ai_label.clone() });
    };

    // Changed files first: a person who just edited a note asks about it next.
    for (i, doc_id) in &plan.look {
        if SYNC_CANCEL.load(Ordering::SeqCst) {
            report.cancelled = true;
            break;
        }
        let s = &seen[*i];
        let path = Path::new(&s.path);
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("document").to_string();
        progress("reading", &name, done, report.added, report.failed.len());
        done += 1;
        let Ok(bytes) = std::fs::read(path) else {
            report.failed.push(ImportFailure { file: name, reason: "could not be read".into() });
            continue;
        };
        let hash = content_hash(key, &bytes);
        let stored: Option<String> = conn
            .query_row("SELECT content_hash FROM documents WHERE doc_id = ?1", params![doc_id], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())?
            .flatten();
        // Touched but not changed - or a document from before folders kept a
        // hash, whose size still matches: note the hash and move on.
        let same_size = known.iter().any(|k| &k.doc_id == doc_id && k.size == s.size);
        if stored.as_deref() == Some(hash.as_str()) || (stored.is_none() && same_size) {
            stamp(&conn, doc_id, &folder.folder_id, s.size, s.mtime, &hash)?;
            report.unchanged += 1;
            continue;
        }
        progress("embedding", &name, done, report.added, report.failed.len());
        match read_and_embed(app, llm_state, path).await {
            Ok((passages, vectors)) => {
                let mut meta = document_meta(&conn, key, doc_id)?.unwrap_or_else(|| DocMeta { filename: name.clone(), ..Default::default() });
                // The words changed, so what was written ABOUT them is stale:
                // the card is written again from the new text. (Mine, author
                // and title are about the document, not its wording - kept.)
                meta.summary = None;
                store_reread(&mut conn, key, doc_id, meta, &s.path, s.size, &passages, &vectors)?;
                stamp(&conn, doc_id, &folder.folder_id, s.size, s.mtime, &hash)?;
                report.updated += 1;
            }
            Err(e) if e == "stopped" => {
                report.cancelled = true;
                break;
            }
            Err(e) => report.failed.push(ImportFailure { file: name, reason: e }),
        }
    }

    // Records that came back from a backup without their text: a file of the
    // folder that matches one fills THAT record (its card, Mine flag and
    // grants kept) instead of becoming a second document beside it.
    let mut waiting = waiting_records(&conn, key)?;

    if !report.cancelled {
        for i in &plan.add {
            if SYNC_CANCEL.load(Ordering::SeqCst) {
                report.cancelled = true;
                break;
            }
            let s = &seen[*i];
            let path = Path::new(&s.path);
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("document").to_string();
            progress("embedding", &name, done, report.added, report.failed.len());
            done += 1;
            let hash = std::fs::read(path).map(|b| content_hash(key, &b)).unwrap_or_default();
            match read_and_embed(app, llm_state, path).await {
                Ok((passages, vectors)) => {
                    if let Some(w) = match_waiting(&waiting, &name, s.size) {
                        let (doc_id, _, meta) = waiting.remove(w);
                        store_reread(&mut conn, key, &doc_id, meta, &s.path, s.size, &passages, &vectors)?;
                        stamp(&conn, &doc_id, &folder.folder_id, s.size, s.mtime, &hash)?;
                        report.updated += 1;
                        continue;
                    }
                    let info = doc_info(path);
                    let meta = DocMeta { filename: name.clone(), path: Some(s.path.clone()), author: info.author, title: info.title, ..Default::default() };
                    let first = folder.meta.ai_ids.first().cloned().unwrap_or_default();
                    match insert_document(&mut conn, key, &meta, s.size, &passages, &vectors, &first) {
                        Ok(rec) => {
                            for ai in folder.meta.ai_ids.iter().skip(1) {
                                conn.execute("INSERT OR IGNORE INTO grants (doc_id, ai_id) VALUES (?1, ?2)", params![rec.doc_id, ai])
                                    .map_err(|e| e.to_string())?;
                            }
                            stamp(&conn, &rec.doc_id, &folder.folder_id, s.size, s.mtime, &hash)?;
                            report.added += 1;
                        }
                        Err(e) => report.failed.push(ImportFailure { file: name, reason: e }),
                    }
                }
                Err(e) if e == "stopped" => {
                    report.cancelled = true;
                    break;
                }
                Err(e) => report.failed.push(ImportFailure { file: name, reason: e }),
            }
        }
    }

    // A file that is gone takes its document with it - but never on a pass
    // that was stopped part way, and never when the walk came back empty
    // for a folder that held documents (a vault mid-move looks like that).
    if !report.cancelled && !(seen.is_empty() && !known.is_empty()) {
        for doc_id in &plan.remove {
            delete_document(&conn, doc_id)?;
            report.removed += 1;
        }
    }
    // Every AI of the folder holds every document of the folder.
    for ai in &folder.meta.ai_ids {
        conn.execute(
            "INSERT OR IGNORE INTO grants (doc_id, ai_id) SELECT doc_id, ?2 FROM documents WHERE folder_id = ?1",
            params![folder.folder_id, ai],
        )
        .map_err(|e| e.to_string())?;
    }
    conn.execute("UPDATE folders SET last_scan_at = ?2 WHERE folder_id = ?1", params![folder.folder_id, now_secs()])
        .map_err(|e| e.to_string())?;
    if report.added + report.updated + report.removed > 0 {
        cache_invalidate();
    }
    progress("done", "", total, report.added, report.failed.len());
    log::info!(
        "[corpus] folder sync {}: {} new, {} changed, {} gone, {} unchanged, {} failed{}",
        folder.folder_id,
        report.added,
        report.updated,
        report.removed,
        report.unchanged,
        report.failed.len(),
        if report.cancelled { " (stopped)" } else { "" }
    );
    Ok(report)
}

fn document_meta(conn: &Connection, key: &[u8; 32], doc_id: &str) -> Result<Option<DocMeta>, String> {
    let blob: Option<Vec<u8>> = conn
        .query_row("SELECT meta_enc FROM documents WHERE doc_id = ?1", params![doc_id], |r| r.get(0))
        .optional()
        .map_err(|e| e.to_string())?;
    Ok(blob.and_then(|b| dec(key, &b).ok()).and_then(|p| serde_json::from_slice(&p).ok()))
}

/// Sync one folder, or every kept folder. One pass at a time: a second call
/// while one runs returns at once with nothing done.
pub async fn sync_folders(
    app: &AppHandle,
    hc_state: &State<'_, Arc<HolochainState>>,
    llm_state: &State<'_, crate::llm::LLMState>,
    only: Option<String>,
) -> Result<Vec<SyncReport>, String> {
    if SYNC_RUNNING.swap(true, Ordering::SeqCst) {
        return Ok(Vec::new());
    }
    struct Done;
    impl Drop for Done {
        fn drop(&mut self) {
            SYNC_RUNNING.store(false, Ordering::SeqCst);
        }
    }
    let _done = Done;
    SYNC_CANCEL.store(false, Ordering::SeqCst);
    let key = data_key(hc_state)?;
    let folders = {
        let conn = open(app)?;
        list_folders(&conn, &key)?
    };
    let mut out = Vec::new();
    for f in folders.iter().filter(|f| only.as_deref().map_or(true, |id| id == f.folder_id)) {
        let r = sync_one(app, llm_state, &key, f).await?;
        let stop = r.cancelled;
        out.push(r);
        if stop {
            break;
        }
    }
    Ok(out)
}

// ---------------------------------------------------------------- commands

/// Keep a folder in sync for an AI. Registers it; the caller runs the first
/// pass with `corpus_folder_sync` (so progress shows like an import).
#[tauri::command]
pub fn corpus_folder_add(
    app: AppHandle,
    hc_state: State<'_, Arc<HolochainState>>,
    path: String,
    ai_id: String,
    kind: Option<String>,
) -> Result<String, String> {
    if !Path::new(&path).is_dir() {
        return Err("That is not a folder this computer can read.".into());
    }
    let key = data_key(&hc_state)?;
    add_folder(&open(&app)?, &key, &path, &ai_id, kind)
}

#[tauri::command]
pub fn corpus_folders(app: AppHandle, hc_state: State<'_, Arc<HolochainState>>) -> Result<Vec<FolderRecord>, String> {
    let key = data_key(&hc_state)?;
    list_folders(&open(&app)?, &key)
}

/// Stop keeping a folder in sync. Its documents stay in the library as
/// ordinary documents unless `remove_documents` is set.
#[tauri::command]
pub fn corpus_folder_remove(app: AppHandle, folder_id: String, remove_documents: bool) -> Result<(), String> {
    let conn = open(&app)?;
    if remove_documents {
        let ids: Vec<String> = {
            let mut stmt = conn.prepare("SELECT doc_id FROM documents WHERE folder_id = ?1").map_err(|e| e.to_string())?;
            let rows = stmt.query_map(params![folder_id], |r| r.get::<_, String>(0)).map_err(|e| e.to_string())?;
            rows.flatten().collect()
        };
        for id in ids {
            delete_document(&conn, &id)?;
        }
        cache_invalidate();
    } else {
        conn.execute("UPDATE documents SET folder_id = NULL WHERE folder_id = ?1", params![folder_id]).map_err(|e| e.to_string())?;
    }
    conn.execute("DELETE FROM folders WHERE folder_id = ?1", params![folder_id]).map(|_| ()).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn corpus_folder_sync(
    app: AppHandle,
    hc_state: State<'_, Arc<HolochainState>>,
    llm_state: State<'_, crate::llm::LLMState>,
    folder_id: Option<String>,
) -> Result<Vec<SyncReport>, String> {
    sync_folders(&app, &hc_state, &llm_state, folder_id).await
}

#[tauri::command]
pub fn corpus_folder_sync_cancel() {
    SYNC_CANCEL.store(true, Ordering::SeqCst);
}

/// Kept folders are looked at a few minutes after launch and then every half
/// hour. Quiet when there are none, when the records are not open yet, or
/// when the embedding model is not on this computer.
pub fn start_folder_sync(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut wait = std::time::Duration::from_secs(240);
        loop {
            tokio::time::sleep(wait).await;
            wait = std::time::Duration::from_secs(30 * 60);
            let hc = app.state::<Arc<HolochainState>>();
            let llm = app.state::<crate::llm::LLMState>();
            match sync_folders(&app, &hc, &llm, None).await {
                Ok(reports) => {
                    let changed: usize = reports.iter().map(|r| r.added + r.updated + r.removed).sum();
                    if changed > 0 {
                        let _ = app.emit("corpus-folders-synced", &reports);
                    }
                }
                Err(e) => log::debug!("[corpus] folder sync skipped: {e}"),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seen(path: &str, size: i64, mtime: i64) -> Seen {
        Seen { path: path.into(), size, mtime }
    }
    fn known(id: &str, path: &str, size: i64, mtime: Option<i64>) -> Known {
        Known { doc_id: id.into(), path_hash: path_hash(path), size, mtime }
    }

    #[test]
    fn a_walk_is_sorted_into_new_changed_unchanged_and_gone() {
        let s = vec![
            seen("/v/pages/same.md", 10, 100),
            seen("/v/pages/edited.md", 12, 200),
            seen("/v/pages/touched.md", 10, 300),
            seen("/v/pages/new.md", 5, 100),
        ];
        let k = vec![
            known("d1", "/v/pages/same.md", 10, Some(100)),
            known("d2", "/v/pages/edited.md", 10, Some(100)),
            known("d3", "/v/pages/touched.md", 10, Some(100)),
            known("d4", "/v/pages/deleted.md", 10, Some(100)),
        ];
        let p = plan(&s, &k);
        assert_eq!(p.add, vec![3]);
        assert_eq!(p.look, vec![(1, "d2".to_string()), (2, "d3".to_string())]);
        assert_eq!(p.unchanged, 1);
        assert_eq!(p.remove, vec!["d4".to_string()]);
    }

    #[test]
    fn a_document_from_before_folders_is_looked_at_once_not_assumed() {
        let p = plan(&[seen("/v/a.md", 10, 100)], &[known("d1", "/v/a.md", 10, None)]);
        assert_eq!(p.look, vec![(0, "d1".to_string())]);
        assert_eq!(p.unchanged, 0);
    }

    #[test]
    fn logseq_copies_are_not_notes() {
        let root = Path::new("/home/me/graph");
        assert!(is_vault_noise(root, Path::new("/home/me/graph/logseq/bak/pages/a/2026-09-01.md")));
        assert!(is_vault_noise(root, Path::new("/home/me/graph/logseq/version-files/x.md")));
        assert!(!is_vault_noise(root, Path::new("/home/me/graph/pages/bak.md")));
        assert!(!is_vault_noise(root, Path::new("/home/me/graph/journals/2026_09_20.md")));
        assert!(!is_vault_noise(root, Path::new("/home/me/graph/notes/logseq/tips.md")));
    }

    #[test]
    fn the_store_gains_its_columns_once_and_folders_round_trip() {
        let dir = std::env::temp_dir().join(format!("yoai-sync-{}", new_doc_id()));
        std::fs::create_dir_all(&dir).unwrap();
        let conn = open_at(&dir.join("corpus.sqlite")).unwrap();
        ensure_schema(&conn).unwrap(); // a second run changes nothing
        let key = [7u8; 32];
        let id = add_folder(&conn, &key, "/home/me/My Notes", "ai-1", Some("obsidian".into())).unwrap();
        let again = add_folder(&conn, &key, "/home/me/My Notes", "ai-2", None).unwrap();
        assert_eq!(id, again, "the same folder is one folder");
        let folders = list_folders(&conn, &key).unwrap();
        assert_eq!(folders.len(), 1);
        assert_eq!(folders[0].meta.ai_ids, vec!["ai-1".to_string(), "ai-2".to_string()]);
        assert_eq!(folders[0].meta.kind.as_deref(), Some("obsidian"));
        assert!(!folders[0].reachable);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_content_hash_is_keyed() {
        assert_ne!(content_hash(&[1u8; 32], b"note"), content_hash(&[2u8; 32], b"note"));
        assert_eq!(content_hash(&[1u8; 32], b"note"), content_hash(&[1u8; 32], b"note"));
    }
}
