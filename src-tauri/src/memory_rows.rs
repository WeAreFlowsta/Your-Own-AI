//! Memory rows (the design notes §3,
//! §11): one typed, dated, sealed record for everything an AI or the person
//! remembers - a rule, a command, a location, a decision, a fact, an
//! episode, a note - in a scope (the person, one AI, a project, a
//! conversation). Rows are records in the person's own transcript network
//! (coordinator 10: a `mem` link at a scope anchor), so they follow the
//! identity across devices by construction. A correction or a deletion is
//! a NEW row with the same `id`; readers reduce to the newest per id, the
//! same way on every device.

use crate::commands_holochain::HolochainState;
use holochain_types::prelude::{ActionHash, AgentPubKey, AnyLinkableHash, ExternIO, ExternalHash, Record};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{Manager, State};

/// What a row is. A `rule` is the person's words verbatim and may be
/// pinned; extraction may propose but never save one (decision 2).
pub const KINDS: &[&str] = &["rule", "command", "location", "decision", "fact", "episode", "note"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MemoryRow {
    /// Logical id - the same across revisions and devices.
    pub id: String,
    /// `person` | `ai:<founding key>` | `project:<folder identity>` | `conversation:<hash>`.
    pub scope: String,
    pub kind: String,
    /// The statement, Markdown.
    pub text: String,
    /// `fact {subject, predicate, value}`, `decision {decided_by, plan_ref}`, `command {cmd, cwd}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured: Option<serde_json::Value>,
    /// ms epoch the statement holds from (absolute).
    pub as_of: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<i64>,
    /// `person` | `ai:<founding key>`.
    pub saved_by: String,
    /// `authored` | `extracted:<action hash>` | `distilled:<conversation hash>` | `dream:<ids>` | `migrated:<store>`.
    pub provenance: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub topics: Vec<String>,
    /// Ids this revision merges or corrects.
    #[serde(default)]
    pub replaces: Vec<String>,
    /// A deletion is a record: the newest revision says so.
    #[serde(default)]
    pub deleted: bool,
    /// ms epoch of this revision.
    pub updated_at: i64,
    /// The record's action hash (hex) once read back; not part of the
    /// sealed payload's meaning, set by the reader.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_hash: Option<String>,
}

/// Reduce every revision to the newest per `id`: newer `updated_at`, then a
/// deletion over a live version, then the larger action hash (the Vault's
/// rule - the same answer on every device). Deleted ids are dropped; rows
/// another row `replaces` are dropped too. Pinned rules first, then newest.
pub fn reduce(rows: Vec<MemoryRow>) -> Vec<MemoryRow> {
    use std::collections::HashMap;
    let mut newest: HashMap<String, MemoryRow> = HashMap::new();
    for r in rows {
        match newest.get(&r.id) {
            Some(cur) if !newer(&r, cur) => {}
            _ => {
                newest.insert(r.id.clone(), r);
            }
        }
    }
    let replaced: std::collections::HashSet<String> = newest.values().flat_map(|r| r.replaces.iter().cloned()).collect();
    let mut live: Vec<MemoryRow> = newest
        .into_values()
        .filter(|r| !r.deleted && !replaced.contains(&r.id))
        .collect();
    live.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then(b.updated_at.cmp(&a.updated_at))
            .then(a.id.cmp(&b.id))
    });
    live
}

fn newer(a: &MemoryRow, b: &MemoryRow) -> bool {
    (a.updated_at, a.deleted, a.action_hash.as_deref().unwrap_or(""))
        > (b.updated_at, b.deleted, b.action_hash.as_deref().unwrap_or(""))
}

/// The anchor a scope's rows hang from. An `ai:` scope hangs from the AI's
/// founding key (the same address its conversations list at); every other
/// scope from an external hash over the scope string under this network's
/// data key, so two people with the same folder path never share an anchor
/// and the anchor says nothing about the folder to a peer.
pub fn scope_anchor(scope: &str, data_key: &[u8; 32]) -> Result<AnyLinkableHash, String> {
    if let Some(founding_hex) = scope.strip_prefix("ai:") {
        let raw = hex::decode(founding_hex).map_err(|_| "bad founding key".to_string())?;
        if raw.len() != 39 {
            return Err("bad founding key length".into());
        }
        return Ok(AgentPubKey::from_raw_39(raw).into());
    }
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(b"yoai-memory-anchor-v1\0");
    h.update(data_key);
    h.update(scope.as_bytes());
    let digest = h.finalize();
    Ok(ExternalHash::from_raw_32(digest.to_vec()).into())
}

#[derive(Serialize, Debug)]
struct SaveInput {
    cipher: Vec<u8>,
    nonce: Vec<u8>,
    anchor: AnyLinkableHash,
}

#[derive(Serialize, Debug)]
struct RowsInput {
    anchor: AnyLinkableHash,
}

/// Write one row (a new row, a revision, or a deletion) through this
/// device's agent for the AI (`agent_key`); any provisioned agent can
/// write a person- or project-scope row, since the anchor is the scope's.
#[tauri::command]
pub async fn save_memory_row(
    agent_key: Option<String>,
    row: MemoryRow,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<String, String> {
    if !KINDS.contains(&row.kind.as_str()) {
        return Err(format!("unknown memory kind {}", row.kind));
    }
    let manager = hc_state.get()?;
    let agent_key = match agent_key {
        Some(a) => a,
        None => any_agent(&manager).await.ok_or("no agent provisioned yet")?,
    };
    let key = manager.data_key()?;
    let mut sealed = row.clone();
    sealed.action_hash = None;
    let plain = serde_json::to_vec(&sealed).map_err(|e| e.to_string())?;
    let (nonce, cipher) = crate::transcript_crypto::encrypt(&key, &plain)?;
    let anchor = scope_anchor(&row.scope, &key)?;
    let payload = ExternIO::encode(SaveInput { cipher, nonce: nonce.to_vec(), anchor }).map_err(|e| e.to_string())?;
    let out = manager.call_zome(&agent_key, "transcript", "save_memory_row", payload).await?;
    let hash: ActionHash = ExternIO::decode(&out).map_err(|e| e.to_string())?;
    Ok(hex::encode(hash.get_raw_39()))
}

/// One revision as the history view wants it: the row, and whether a
/// newer revision (or a merge) has since replaced it.
#[derive(Serialize, Clone, Debug)]
pub struct MemoryRevision {
    #[serde(flatten)]
    pub row: MemoryRow,
    pub superseded: bool,
}

/// Every revision ever written in a scope, newest first, each marked as
/// live or superseded - the modal's history ("what dreaming changed", and
/// the chain back to the record a memory came from). A live read.
#[tauri::command]
pub async fn list_memory_revisions(
    agent_key: Option<String>,
    scope: String,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<Vec<MemoryRevision>, String> {
    let manager = hc_state.get()?;
    let agent_key = match agent_key {
        Some(a) => a,
        None => any_agent(&manager).await.ok_or("no agent provisioned yet")?,
    };
    let key = manager.data_key()?;
    let anchor = scope_anchor(&scope, &key)?;
    let payload = ExternIO::encode(RowsInput { anchor }).map_err(|e| e.to_string())?;
    let out = manager.call_zome(&agent_key, "transcript", "get_memory_rows", payload).await?;
    let records: Vec<Record> = ExternIO::decode(&out).map_err(|e| e.to_string())?;
    let mut rows = Vec::with_capacity(records.len());
    for record in &records {
        let Some((plain, _)) = crate::vault_escrow::open_record(&key, record) else { continue };
        let Ok(mut row) = serde_json::from_value::<MemoryRow>(plain) else { continue };
        row.action_hash = Some(hex::encode(record.action_address().get_raw_39()));
        rows.push(row);
    }
    let live: std::collections::HashSet<Option<String>> = reduce(rows.clone()).into_iter().map(|r| r.action_hash).collect();
    let mut out: Vec<MemoryRevision> = rows.into_iter().map(|row| MemoryRevision { superseded: !live.contains(&row.action_hash), row }).collect();
    out.sort_by(|a, b| b.row.updated_at.cmp(&a.row.updated_at).then(a.row.id.cmp(&b.row.id)));
    Ok(out)
}

/// Every live row in a scope, reduced, pinned rules first.
#[tauri::command]
pub async fn list_memory_rows(
    app: tauri::AppHandle,
    agent_key: Option<String>,
    scope: String,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<Vec<MemoryRow>, String> {
    let manager = hc_state.get()?;
    let agent_key = match agent_key {
        Some(a) => a,
        None => any_agent(&manager).await.ok_or("no agent provisioned yet")?,
    };
    rows_at(&app, &manager, &agent_key, &scope).await
}

// ───────────── the local cache per scope ─────────────

pub(crate) fn cache_path(app: &tauri::AppHandle, scope: &str) -> Option<std::path::PathBuf> {
    use sha2::Digest;
    let tag = hex::encode(sha2::Sha256::digest(scope.as_bytes()));
    crate::profile::root(app).ok().map(|r| r.join(format!("memory-rows-{}.enc", &tag[..16])))
}

/// Rows of a scope that came from another device's backup copy (its rows
/// cache rides its manifest) - kept apart from the live cache so a live
/// read, which cannot see an offline device's records, never drops them.
fn siblings_path(app: &tauri::AppHandle, scope: &str) -> Option<std::path::PathBuf> {
    cache_path(app, scope).map(|p| p.with_extension("siblings.enc"))
}

fn read_sealed_rows(path: Option<std::path::PathBuf>, key: &[u8; 32]) -> Vec<MemoryRow> {
    let Some(path) = path else { return Vec::new() };
    let Ok(raw) = std::fs::read(&path) else { return Vec::new() };
    decode_sealed_rows(&raw, key)
}

/// A sealed rows file (the cache's shape) as rows, or nothing.
pub(crate) fn decode_sealed_rows(raw: &[u8], key: &[u8; 32]) -> Vec<MemoryRow> {
    let Ok(file) = serde_json::from_slice::<SealedFile>(raw) else { return Vec::new() };
    let (Ok(nonce), Ok(cipher)) = (hex::decode(&file.nonce), hex::decode(&file.cipher)) else { return Vec::new() };
    let Ok(plain) = crate::transcript_crypto::decrypt(key, &nonce, &cipher) else { return Vec::new() };
    serde_json::from_slice(&plain).unwrap_or_default()
}

fn write_sealed_rows(path: Option<std::path::PathBuf>, key: &[u8; 32], rows: &[MemoryRow]) {
    let Some(path) = path else { return };
    let Ok(plain) = serde_json::to_vec(rows) else { return };
    let Ok((nonce, cipher)) = crate::transcript_crypto::encrypt(key, &plain) else { return };
    let file = SealedFile { version: 1, nonce: hex::encode(nonce), cipher: hex::encode(cipher) };
    if let Ok(json) = serde_json::to_vec(&file) {
        let _ = std::fs::write(&path, json);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
    }
}

/// Fold another device's memory (from its manifest copy) into this
/// device's: its facts file by id (newer wins, no vector when the words
/// changed), its person-scope rows into the siblings file and the cache.
/// Returns (facts adopted, rows adopted). The facts write does not mirror
/// (the other device already wrote those rows).
pub(crate) fn fold_sibling_person_memory(app: &tauri::AppHandle, key: &[u8; 32], manifest: &serde_json::Value) -> (usize, usize) {
    use base64::Engine;
    let mut facts_adopted = 0usize;
    if let Some(b64) = manifest["memory_facts"]["raw_b64"].as_str() {
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
            if let Ok(file) = serde_json::from_slice::<SealedFile>(&bytes) {
                if let (Ok(nonce), Ok(cipher)) = (hex::decode(&file.nonce), hex::decode(&file.cipher)) {
                    if let Ok(plain) = crate::transcript_crypto::decrypt(key, &nonce, &cipher) {
                        if let Ok(theirs) = serde_json::from_slice::<Vec<crate::memory::MemoryFact>>(&plain) {
                            let mut mine = crate::memory::read_facts(app, key).unwrap_or_default();
                            facts_adopted = merge_facts(&mut mine, theirs);
                            if facts_adopted > 0 {
                                let _ = crate::memory::write_facts(app, key, &mine);
                            }
                        }
                    }
                }
            }
        }
    }
    let mut rows_adopted = 0usize;
    // Every scope the sibling carried (person + its AIs by founding key);
    // older manifests carried the person's alone.
    let mut scoped: Vec<(String, String)> = Vec::new();
    if let Some(obj) = manifest["memory_rows"]["scopes"].as_object() {
        for (scope, v) in obj {
            if let Some(b64) = v.as_str() {
                scoped.push((scope.clone(), b64.to_string()));
            }
        }
    } else if let Some(b64) = manifest["memory_rows"]["person_raw_b64"].as_str() {
        scoped.push(("person".to_string(), b64.to_string()));
    }
    for (scope, b64) in scoped {
        let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(&b64) else { continue };
        let theirs = decode_sealed_rows(&bytes, key);
        if theirs.is_empty() {
            continue;
        }
        let mut siblings = read_sealed_rows(siblings_path(app, &scope), key);
        let before: std::collections::HashSet<(String, i64)> = siblings.iter().map(|r| (r.id.clone(), r.updated_at)).collect();
        siblings.extend(theirs);
        let siblings = reduce(siblings);
        rows_adopted += siblings.iter().filter(|r| !before.contains(&(r.id.clone(), r.updated_at))).count();
        write_sealed_rows(siblings_path(app, &scope), key, &siblings);
        let mut all = read_cache(app, key, &scope);
        all.extend(siblings);
        write_cache(app, key, &scope, &reduce(all));
    }
    (facts_adopted, rows_adopted)
}

/// Merge another device's facts into ours by id: a fact we lack is added, a
/// newer one replaces ours (its vector dropped when the words changed);
/// portraits (synthesis, library) stay this device's own. Returns how many
/// changed.
pub(crate) fn merge_facts(mine: &mut Vec<crate::memory::MemoryFact>, theirs: Vec<crate::memory::MemoryFact>) -> usize {
    let mut n = 0usize;
    for t in theirs {
        if t.entry_kind == "synthesis" || t.entry_kind == "library" {
            continue;
        }
        match mine.iter_mut().find(|m| m.id == t.id) {
            Some(m) if m.updated_at >= t.updated_at => {}
            Some(m) => {
                let keep_vec = m.value == t.value;
                let vec = m.embedding.take();
                *m = t;
                if keep_vec {
                    m.embedding = vec;
                } else {
                    m.embedding = None;
                }
                n += 1;
            }
            None => {
                let mut t = t;
                t.embedding = None;
                mine.push(t);
                n += 1;
            }
        }
    }
    n
}

#[derive(Serialize, Deserialize)]
struct SealedFile {
    version: u32,
    nonce: String,
    cipher: String,
}

pub(crate) fn write_cache(app: &tauri::AppHandle, key: &[u8; 32], scope: &str, rows: &[MemoryRow]) {
    let Some(path) = cache_path(app, scope) else { return };
    let Ok(plain) = serde_json::to_vec(rows) else { return };
    let Ok((nonce, cipher)) = crate::transcript_crypto::encrypt(key, &plain) else { return };
    let file = SealedFile { version: 1, nonce: hex::encode(nonce), cipher: hex::encode(cipher) };
    if let Ok(json) = serde_json::to_vec(&file) {
        let _ = std::fs::write(&path, json);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
    }
}

pub(crate) fn read_cache(app: &tauri::AppHandle, key: &[u8; 32], scope: &str) -> Vec<MemoryRow> {
    let Some(path) = cache_path(app, scope) else { return Vec::new() };
    let Ok(raw) = std::fs::read(&path) else { return Vec::new() };
    let Ok(file) = serde_json::from_slice::<SealedFile>(&raw) else { return Vec::new() };
    let (Ok(nonce), Ok(cipher)) = (hex::decode(&file.nonce), hex::decode(&file.cipher)) else { return Vec::new() };
    let Ok(plain) = crate::transcript_crypto::decrypt(key, &nonce, &cipher) else { return Vec::new() };
    serde_json::from_slice(&plain).unwrap_or_default()
}

/// The scope's rows as of the last live read - instant, never waits on a
/// zome call (the list cache's rule for conversations).
#[tauri::command]
pub fn list_memory_rows_cached(
    app: tauri::AppHandle,
    scope: String,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<Vec<MemoryRow>, String> {
    let key = hc_state.get()?.data_key()?;
    Ok(read_cache(&app, &key, &scope))
}

/// Any agent this device has provisioned - the writer for rows whose anchor
/// is the scope's, not an AI's.
async fn any_agent(manager: &crate::holochain::HolochainManager) -> Option<String> {
    let map = manager.agents.lock().await;
    map.keys().next().cloned()
}

async fn rows_at(app: &tauri::AppHandle, manager: &crate::holochain::HolochainManager, agent: &str, scope: &str) -> Result<Vec<MemoryRow>, String> {
    let key = manager.data_key()?;
    let anchor = scope_anchor(scope, &key)?;
    let payload = ExternIO::encode(RowsInput { anchor }).map_err(|e| e.to_string())?;
    let out = manager.call_zome(agent, "transcript", "get_memory_rows", payload).await?;
    let records: Vec<Record> = ExternIO::decode(&out).map_err(|e| e.to_string())?;
    let mut rows = Vec::with_capacity(records.len());
    for record in &records {
        let Some((plain, _)) = crate::vault_escrow::open_record(&key, record) else { continue };
        let Ok(mut row) = serde_json::from_value::<MemoryRow>(plain) else { continue };
        row.action_hash = Some(hex::encode(record.action_address().get_raw_39()));
        rows.push(row);
    }
    // What an offline device's backup copy brought stays in the list.
    rows.extend(read_sealed_rows(siblings_path(app, scope), &key));
    let reduced = reduce(rows);
    write_cache(app, &key, scope, &reduced);
    Ok(reduced)
}

async fn write_row(manager: &crate::holochain::HolochainManager, agent: &str, row: &MemoryRow) -> Result<(), String> {
    let key = manager.data_key()?;
    let mut sealed = row.clone();
    sealed.action_hash = None;
    let plain = serde_json::to_vec(&sealed).map_err(|e| e.to_string())?;
    let (nonce, cipher) = crate::transcript_crypto::encrypt(&key, &plain)?;
    let anchor = scope_anchor(&row.scope, &key)?;
    let payload = ExternIO::encode(SaveInput { cipher, nonce: nonce.to_vec(), anchor }).map_err(|e| e.to_string())?;
    manager.call_zome(agent, "transcript", "save_memory_row", payload).await.map(|_| ())
}

/// `YYYY-MM-DD` (UTC) for a ms epoch; empty for a time before 1970.
pub(crate) fn ymd(ms: i64) -> String {
    if ms < 0 {
        return String::new();
    }
    // Days to civil (Howard Hinnant), no calendar crate needed.
    let z = ms / 86_400_000 + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// The rows of several scopes as text for a tool answer: pinned rules first
/// within each kind, grouped by kind, filtered by a topic (a case-insensitive
/// match over the text and the topics). From the per-scope cache, so the
/// answer is instant; the loader keeps the caches fresh during a chat.
pub(crate) fn rows_text_for(app: &tauri::AppHandle, key: &[u8; 32], scopes: &[String], topic: Option<&str>) -> String {
    let needle = topic.map(|t| t.trim().to_lowercase()).filter(|t| !t.is_empty());
    let mut rows: Vec<MemoryRow> = scopes.iter().flat_map(|sc| read_cache(app, key, sc)).collect();
    if let Some(n) = &needle {
        rows.retain(|r| r.text.to_lowercase().contains(n.as_str()) || r.topics.iter().any(|t| t.to_lowercase().contains(n.as_str())));
    }
    if rows.is_empty() {
        return String::new();
    }
    let mut out: Vec<String> = Vec::new();
    for (kind, heading) in [
        ("rule", "Rules (follow exactly)"),
        ("command", "Commands"),
        ("location", "Locations"),
        ("decision", "Decisions"),
        ("fact", "Facts"),
        ("note", "Notes"),
        ("episode", "Episodes"),
    ] {
        let mut of_kind: Vec<&MemoryRow> = rows.iter().filter(|r| r.kind == kind).collect();
        if of_kind.is_empty() {
            continue;
        }
        of_kind.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.updated_at.cmp(&a.updated_at)));
        out.push(format!("{heading}:"));
        for r in of_kind.iter().take(40) {
            let when = ymd(r.as_of);
            out.push(format!("- {}{}", r.text, if when.is_empty() { String::new() } else { format!(" ({when})") }));
        }
    }
    out.join("\n")
}

// ───────────── the tool's self-edit ─────────────

fn words(text: &str) -> std::collections::HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| w.len() > 2)
        .map(String::from)
        .collect()
}

/// Two rows say the same thing when one's words are mostly the other's:
/// Jaccard ≥ 0.6, or one is a sub-statement of the other. The reason the
/// old append-only project memory filled with near-duplicates.
pub(crate) fn says_the_same(a: &str, b: &str) -> bool {
    let (wa, wb) = (words(a), words(b));
    if wa.is_empty() || wb.is_empty() {
        return a.trim().eq_ignore_ascii_case(b.trim());
    }
    let inter = wa.intersection(&wb).count() as f32;
    let union = wa.union(&wb).count() as f32;
    inter / union >= 0.6 || inter >= wa.len().min(wb.len()) as f32 && wa.len().min(wb.len()) >= 3
}

/// Which live row of the scope a new statement revises, if any: one that
/// says the same thing, whatever kind it was filed under. The AI picks a
/// kind per call ("command" one time, "fact" the next), and a correction
/// filed under another kind left the old note standing beside the new one
/// (2026-10-19). Same kind first, so a rule and a note that happen to
/// agree still prefer their own; `rule` rows are the person's and are
/// never revised by an AI's note.
pub(crate) fn find_revised<'a>(live: &'a [MemoryRow], kind: &str, text: &str) -> Option<&'a MemoryRow> {
    live.iter()
        .filter(|r| r.kind == kind)
        .chain(live.iter().filter(|r| r.kind != kind && r.kind != "rule" && kind != "rule"))
        .find(|r| says_the_same(&r.text, text))
}

/// Write a row, revising a row that says the same thing instead of adding
/// a twin: the new row carries `replaces: [old id]` and the old id, so the
/// reducer shows one. Returns (action hash hex, the replaced text if any).
pub(crate) async fn upsert_row(app: &tauri::AppHandle, manager: &crate::holochain::HolochainManager, agent: &str, mut row: MemoryRow) -> Result<(String, Option<String>), String> {
    let live = rows_at(app, manager, agent, &row.scope).await.unwrap_or_default();
    let mut replaced = None;
    if let Some(old) = find_revised(&live, &row.kind, &row.text) {
        row.id = old.id.clone();
        row.pinned = row.pinned || old.pinned;
        if old.text != row.text {
            replaced = Some(old.text.clone());
        }
    }
    let key = manager.data_key()?;
    let mut sealed = row.clone();
    sealed.action_hash = None;
    let plain = serde_json::to_vec(&sealed).map_err(|e| e.to_string())?;
    let (nonce, cipher) = crate::transcript_crypto::encrypt(&key, &plain)?;
    let anchor = scope_anchor(&row.scope, &key)?;
    let payload = ExternIO::encode(SaveInput { cipher, nonce: nonce.to_vec(), anchor }).map_err(|e| e.to_string())?;
    let out = manager.call_zome(agent, "transcript", "save_memory_row", payload).await?;
    let hash: ActionHash = ExternIO::decode(&out).map_err(|e| e.to_string())?;
    // The cache sees the change at once.
    let mut after: Vec<MemoryRow> = live.into_iter().filter(|r| r.id != row.id).collect();
    row.action_hash = Some(hex::encode(hash.get_raw_39()));
    after.push(row.clone());
    write_cache(app, &key, &row.scope, &reduce(after));
    Ok((hex::encode(hash.get_raw_39()), replaced))
}

/// Live rows of a scope through `agent` (refreshes the cache) - for dreaming.
pub(crate) async fn rows_live(app: &tauri::AppHandle, manager: &crate::holochain::HolochainManager, agent: &str, scope: &str) -> Result<Vec<MemoryRow>, String> {
    rows_at(app, manager, agent, scope).await
}

/// Write one row through `agent` - for dreaming's revisions.
pub(crate) async fn write_row_pub(manager: &crate::holochain::HolochainManager, agent: &str, row: &MemoryRow) -> Result<(), String> {
    write_row(manager, agent, row).await
}

/// For the permission card, before a `remember` runs: the text of the live
/// row (same scope and kind) the new line would revise, from the cache -
/// so the card can say "replaces: <old>" and the person approves knowing.
#[tauri::command]
pub fn memory_would_replace(
    app: tauri::AppHandle,
    scope_word: String,
    kind: String,
    text: String,
    agent_key: Option<String>,
    folder: Option<String>,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Option<String> {
    let key = hc_state.get().ok()?.data_key().ok()?;
    let scope = match scope_word.as_str() {
        "ai" | "me" | "self" => format!("ai:{}", crate::commands_holochain::founding_key_for(&app, agent_key.as_deref()?)),
        "project" => format!("project:{}", crate::project_identity::project_identity(std::path::Path::new(folder.as_deref()?))),
        _ => "person".to_string(),
    };
    let kind = match kind.as_str() {
        "command" | "location" | "decision" | "fact" | "note" => kind,
        _ => "note".to_string(),
    };
    let live = read_cache(&app, &key, &scope);
    find_revised(&live, &kind, text.trim()).filter(|r| r.text != text.trim()).map(|r| r.text.clone())
}

/// The `remember` tool: a row the AI saves for the person (`person`) or
/// for itself (`ai:<founding>`), kinds limited to what an AI may save - a
/// `rule` is the person's to save (decision 2), so a rule-shaped request
/// becomes a `note` with the words kept.
pub(crate) async fn remember_tool(app: &tauri::AppHandle, agent_key: &str, scope_word: &str, kind: &str, text: &str, project_id: Option<&str>) -> Result<String, String> {
    let hc = app.try_state::<Arc<HolochainState>>().ok_or("memory is not available yet")?;
    let manager = hc.get()?;
    let writer = if !agent_key.is_empty() && manager.is_provisioned(agent_key).await { agent_key.to_string() } else { any_agent(&manager).await.ok_or("no agent provisioned yet")? };
    let scope = match (scope_word, project_id) {
        ("ai" | "me" | "self", _) if !agent_key.is_empty() => format!("ai:{}", crate::commands_holochain::founding_key_for(app, agent_key)),
        ("project", Some(pid)) => format!("project:{pid}"),
        ("project", None) => return Err("no project is open".into()),
        _ => "person".to_string(),
    };
    let kind = match kind {
        "command" | "location" | "decision" | "fact" | "note" => kind.to_string(),
        _ => "note".to_string(),
    };
    let text = text.trim();
    if text.is_empty() {
        return Err("nothing to remember - the text was empty".into());
    }
    let row = MemoryRow {
        id: { use rand::RngCore; let mut b = [0u8; 16]; rand::thread_rng().fill_bytes(&mut b); hex::encode(b) },
        scope: scope.clone(),
        kind: kind.clone(),
        text: text.to_string(),
        structured: None,
        as_of: now_ms(),
        valid_to: None,
        saved_by: if agent_key.is_empty() { "person".into() } else { format!("ai:{}", crate::commands_holochain::founding_key_for(app, agent_key)) },
        provenance: "authored".into(),
        pinned: false,
        topics: vec![],
        replaces: vec![],
        deleted: false,
        updated_at: now_ms(),
        action_hash: None,
    };
    let (_, replaced) = upsert_row(app, &manager, &writer, row).await?;
    Ok(match replaced {
        Some(old) => format!("Updated what was remembered (replaces: \"{old}\")."),
        None => format!("Remembered as a {kind} for {}.", if scope == "person" { "them" } else if scope.starts_with("project:") { "this project" } else { "you" }),
    })
}

// ───────────── the person's facts file ↔ person-scope rows ─────────────

/// A `MemoryFact` (the facts file's record) as a person-scope row: facts
/// keep their triple in `structured`, notes are `note` rows; the fact's id
/// is the row's id, so every later edit of the fact is a revision.
pub(crate) fn fact_to_row(f: &crate::memory::MemoryFact) -> Option<MemoryRow> {
    if f.entry_kind == "synthesis" || f.entry_kind == "library" {
        return None; // derived portraits, rewritten on each device
    }
    let kind = if f.entry_kind == "note" { "note" } else { "fact" };
    let text = if kind == "fact" { format!("{} {}: {}", f.subject, f.predicate.replace('_', " "), f.value) } else { f.value.clone() };
    let provenance = match (f.provenance.as_str(), f.source_action_hash.as_deref()) {
        ("authored", _) => "authored".to_string(),
        (_, Some(h)) => format!("extracted:{h}"),
        _ => "extracted".to_string(),
    };
    let saved_by = match f.source_ai_id.as_deref() {
        Some(ai) if f.provenance != "authored" => format!("ai:{ai}"),
        _ => "person".to_string(),
    };
    Some(MemoryRow {
        id: f.id.clone(),
        scope: "person".into(),
        kind: kind.into(),
        text,
        structured: Some(serde_json::json!({
            "subject": f.subject, "predicate": f.predicate, "value": f.value,
            "confidence": f.confidence, "entry_kind": f.entry_kind, "provenance": f.provenance,
            "source_action_hash": f.source_action_hash, "source_ai_id": f.source_ai_id,
            "group_id": f.group_id, "author_identity": f.author_identity,
        })),
        as_of: f.valid_from / 1000,
        valid_to: f.valid_to.map(|t| t / 1000),
        saved_by,
        provenance,
        pinned: false,
        topics: vec![],
        replaces: vec![],
        deleted: false,
        updated_at: f.updated_at.max(1),
        action_hash: None,
    })
}

/// The reverse, for a row made on another device: the fact as the file
/// wants it (no embedding - the front end embeds notes it finds bare).
pub(crate) fn row_to_fact(r: &MemoryRow) -> Option<crate::memory::MemoryFact> {
    if r.scope != "person" || !(r.kind == "fact" || r.kind == "note") {
        return None;
    }
    let st = r.structured.clone().unwrap_or_default();
    let s = |k: &str| st[k].as_str().map(String::from);
    Some(crate::memory::MemoryFact {
        id: r.id.clone(),
        subject: s("subject").unwrap_or_else(|| "user".into()),
        predicate: s("predicate").unwrap_or_else(|| if r.kind == "note" { "note".into() } else { "fact".into() }),
        value: s("value").unwrap_or_else(|| r.text.clone()),
        confidence: st["confidence"].as_f64().unwrap_or(1.0) as f32,
        valid_from: r.as_of * 1000,
        valid_to: r.valid_to.map(|t| t * 1000),
        source_action_hash: s("source_action_hash"),
        source_ai_id: s("source_ai_id"),
        owner_scope: "personal".into(),
        key_scope: "personal".into(),
        entry_kind: s("entry_kind").unwrap_or_else(|| r.kind.clone()),
        provenance: s("provenance").unwrap_or_else(|| if r.provenance == "authored" { "authored".into() } else { "extracted".into() }),
        author_identity: s("author_identity"),
        embedding: None,
        group_id: s("group_id"),
        created_at: r.as_of * 1000,
        updated_at: r.updated_at,
    })
}

/// What a facts-file save changed, as rows: a changed or new fact is a
/// revision, a removed fact a deletion. Pure; the caller writes them.
pub(crate) fn facts_delta(before: &[crate::memory::MemoryFact], after: &[crate::memory::MemoryFact]) -> Vec<MemoryRow> {
    use std::collections::HashMap;
    let was: HashMap<&str, &crate::memory::MemoryFact> = before.iter().map(|f| (f.id.as_str(), f)).collect();
    let now_ids: std::collections::HashSet<&str> = after.iter().map(|f| f.id.as_str()).collect();
    let mut out = Vec::new();
    for f in after {
        let changed = match was.get(f.id.as_str()) {
            None => true,
            Some(b) => b.value != f.value || b.valid_to != f.valid_to || b.predicate != f.predicate || b.subject != f.subject,
        };
        if changed {
            if let Some(mut r) = fact_to_row(f) {
                r.updated_at = r.updated_at.max(now_ms());
                out.push(r);
            }
        }
    }
    for f in before {
        if !now_ids.contains(f.id.as_str()) {
            if let Some(mut r) = fact_to_row(f) {
                r.deleted = true;
                r.updated_at = now_ms();
                out.push(r);
            }
        }
    }
    out
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// Called by every facts-file save: the delta goes to the network in the
/// background through any provisioned agent. Nothing waits on it.
pub(crate) fn mirror_person_facts(app: &tauri::AppHandle, before: Vec<crate::memory::MemoryFact>, after: Vec<crate::memory::MemoryFact>) {
    let rows = facts_delta(&before, &after);
    if rows.is_empty() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let Some(hc) = app.try_state::<Arc<HolochainState>>() else { return };
        let Ok(manager) = hc.get() else { return };
        let Some(agent) = any_agent(&manager).await else { return };
        let mut written = 0usize;
        for r in &rows {
            match write_row(&manager, &agent, r).await {
                Ok(()) => written += 1,
                Err(e) => log::warn!("[memory-rows] mirror of fact {} failed: {}", &r.id[..r.id.len().min(8)], e),
            }
        }
        log::info!("[memory-rows] {} of {} fact change(s) mirrored to the person's rows", written, rows.len());
    });
}

// ───────────── an AI's authored knowledge ↔ ai-scope rows ─────────────

/// Knowledge the person gave the AI by hand: authored, not from a document
/// (document chunks are the library's; the episodic index is derived).
fn is_authored_note(e: &crate::transcript_memory::TranscriptEmbedding) -> bool {
    e.kind == "authored" && e.source.is_none()
}

pub(crate) fn knowledge_to_row(e: &crate::transcript_memory::TranscriptEmbedding, founding: &str) -> MemoryRow {
    MemoryRow {
        id: e.id.clone(),
        scope: format!("ai:{founding}"),
        kind: "note".into(),
        text: e.text.clone(),
        structured: None,
        as_of: e.created_at,
        valid_to: None,
        saved_by: "person".into(),
        provenance: "authored".into(),
        pinned: false,
        topics: vec![],
        replaces: vec![],
        deleted: false,
        updated_at: e.created_at.max(1),
        action_hash: None,
    }
}

/// The reverse: a note from another device as the index wants it - no
/// vector (the front end embeds bare authored items it finds).
pub(crate) fn row_to_knowledge(r: &MemoryRow) -> Option<crate::transcript_memory::TranscriptEmbedding> {
    if r.kind != "note" || !r.scope.starts_with("ai:") {
        return None;
    }
    Some(crate::transcript_memory::TranscriptEmbedding {
        id: r.id.clone(),
        conversation_hash: "authored".into(),
        role: "authored".into(),
        text: r.text.clone(),
        vector: vec![],
        kind: "authored".into(),
        created_at: r.as_of,
        source: None,
    })
}

/// What a knowledge save changed, as rows (new or changed text = revision,
/// removed = deletion). Pure.
pub(crate) fn knowledge_delta(before: &[crate::transcript_memory::TranscriptEmbedding], after: &[crate::transcript_memory::TranscriptEmbedding], founding: &str) -> Vec<MemoryRow> {
    use std::collections::HashMap;
    let was: HashMap<&str, &crate::transcript_memory::TranscriptEmbedding> = before.iter().filter(|e| is_authored_note(e)).map(|e| (e.id.as_str(), e)).collect();
    let now: HashMap<&str, &crate::transcript_memory::TranscriptEmbedding> = after.iter().filter(|e| is_authored_note(e)).map(|e| (e.id.as_str(), e)).collect();
    let mut out = Vec::new();
    for (id, e) in &now {
        if was.get(id).map(|b| b.text != e.text).unwrap_or(true) {
            let mut r = knowledge_to_row(e, founding);
            r.updated_at = r.updated_at.max(now_ms());
            out.push(r);
        }
    }
    for (id, e) in &was {
        if !now.contains_key(id) {
            let mut r = knowledge_to_row(e, founding);
            r.deleted = true;
            r.updated_at = now_ms();
            out.push(r);
        }
    }
    out
}

/// Called by every knowledge-index save for an AI (`ai_id` = its agent on
/// this device): the authored notes' delta goes to the AI's rows in the
/// background, through the AI's own agent.
pub(crate) fn mirror_ai_knowledge(app: &tauri::AppHandle, ai_id: &str, before: Vec<crate::transcript_memory::TranscriptEmbedding>, after: Vec<crate::transcript_memory::TranscriptEmbedding>) {
    let founding = crate::commands_holochain::founding_key_for(app, ai_id);
    let rows = knowledge_delta(&before, &after, &founding);
    if rows.is_empty() {
        return;
    }
    let (app, agent) = (app.clone(), ai_id.to_string());
    tauri::async_runtime::spawn(async move {
        let Some(hc) = app.try_state::<Arc<HolochainState>>() else { return };
        let Ok(manager) = hc.get() else { return };
        let writer = if manager.is_provisioned(&agent).await { agent } else { match any_agent(&manager).await { Some(a) => a, None => return } };
        let mut written = 0usize;
        for r in &rows {
            match write_row(&manager, &writer, r).await {
                Ok(()) => written += 1,
                Err(e) => log::warn!("[memory-rows] mirror of knowledge {} failed: {}", &r.id[..r.id.len().min(8)], e),
            }
        }
        log::info!("[memory-rows] {} of {} knowledge change(s) mirrored to the AI's rows", written, rows.len());
    });
}

/// Once per AI per profile: its authored notes become `ai:` rows; then
/// every round, notes from the other devices join its index here (bare,
/// embedded by the front end).
async fn migrate_and_reconcile_ai_knowledge(app: &tauri::AppHandle, manager: &crate::holochain::HolochainManager, agent: &str) -> Result<(usize, usize), String> {
    let key = manager.data_key()?;
    let founding = crate::commands_holochain::founding_key_for(app, agent);
    let scope = format!("ai:{founding}");
    let root = crate::profile::root(app)?;
    let marker = root.join(format!("memory-rows-migrated-{}", &founding[..founding.len().min(16)]));
    let items = crate::transcript_memory::read_for(app, &key, agent)?;
    let mut live = rows_at(app, manager, agent, &scope).await?;
    let mut migrated = 0usize;
    if !marker.exists() {
        let have: std::collections::HashSet<&str> = live.iter().map(|r| r.id.as_str()).collect();
        for e in items.iter().filter(|e| is_authored_note(e)) {
            if have.contains(e.id.as_str()) {
                continue;
            }
            let mut r = knowledge_to_row(e, &founding);
            r.provenance = "migrated:ai-knowledge;authored".into();
            write_row(manager, agent, &r).await?;
            migrated += 1;
        }
        let _ = std::fs::write(&marker, now_ms().to_string());
        if migrated > 0 {
            live = rows_at(app, manager, agent, &scope).await?;
        }
    }
    let mut merged = items.clone();
    let mut adopted = 0usize;
    for r in &live {
        let Some(incoming) = row_to_knowledge(r) else { continue };
        match merged.iter_mut().find(|e| e.id == incoming.id) {
            Some(cur) if cur.text == incoming.text => {}
            Some(cur) => {
                cur.text = incoming.text;
                cur.vector.clear(); // the words changed: embed again here
                adopted += 1;
            }
            None => {
                merged.push(incoming);
                adopted += 1;
            }
        }
    }
    let live_ids: std::collections::HashSet<&str> = live.iter().map(|r| r.id.as_str()).collect();
    let before_len = merged.len();
    merged.retain(|e| !is_authored_note(e) || live_ids.contains(e.id.as_str()) || e.created_at > now_ms() - 120_000);
    if adopted > 0 || merged.len() != before_len {
        crate::transcript_memory::write_for(app, &key, agent, &merged)?;
        log::info!("[memory-rows] {}: knowledge index updated from the AI's rows: {} adopted, {} removed", &agent[..agent.len().min(8)], adopted, before_len - merged.len());
    }
    Ok((migrated, adopted))
}

const MIGRATED_MARKER: &str = "memory-rows-migrated";

/// Once per profile: every fact in the file becomes a person-scope row
/// (provenance kept), so the person's memory joins their network. Then
/// `reconcile_person_rows` keeps the file current with rows written on
/// the other devices. Runs after the conductor is up; silent until then.
pub async fn migrate_and_reconcile_person_memory(app: &tauri::AppHandle) -> Result<(usize, usize), String> {
    let hc = app.try_state::<Arc<HolochainState>>().ok_or("holochain state missing")?;
    let manager = hc.get()?;
    let key = manager.data_key()?;
    let agent = any_agent(&manager).await.ok_or("no agent provisioned yet")?;
    let root = crate::profile::root(app)?;
    let marker = root.join(MIGRATED_MARKER);
    let facts = crate::memory::read_facts(app, &key)?;
    let mut live = rows_at(app, &manager, &agent, "person").await?;
    let mut migrated = 0usize;
    if !marker.exists() {
        let have: std::collections::HashSet<&str> = live.iter().map(|r| r.id.as_str()).collect();
        for f in &facts {
            if have.contains(f.id.as_str()) {
                continue;
            }
            if let Some(mut r) = fact_to_row(f) {
                r.provenance = format!("migrated:memory-facts;{}", r.provenance);
                write_row(&manager, &agent, &r).await?;
                migrated += 1;
            }
        }
        let _ = std::fs::write(&marker, now_ms().to_string());
        if migrated > 0 {
            log::info!("[memory-rows] {} fact(s) migrated to the person's rows", migrated);
            live = rows_at(app, &manager, &agent, "person").await?;
        }
    }
    // Rows from the other devices (or edits made there) into this file.
    let mut merged = facts.clone();
    let mut adopted = 0usize;
    for r in &live {
        let Some(incoming) = row_to_fact(r) else { continue };
        match merged.iter_mut().find(|f| f.id == incoming.id) {
            Some(cur) if cur.updated_at >= incoming.updated_at => {}
            Some(cur) => {
                let embedding = cur.embedding.take();
                *cur = incoming;
                if cur.value == r.structured.as_ref().and_then(|s| s["value"].as_str()).unwrap_or("") {
                    cur.embedding = embedding; // same words, same vector
                }
                adopted += 1;
            }
            None => {
                merged.push(incoming);
                adopted += 1;
            }
        }
    }
    // Deletions made elsewhere: a fact whose row is gone (reduced away).
    let live_ids: std::collections::HashSet<&str> = live.iter().map(|r| r.id.as_str()).collect();
    let before_len = merged.len();
    if marker.exists() {
        let deleted_elsewhere: std::collections::HashSet<String> = facts
            .iter()
            .filter(|f| f.entry_kind != "synthesis" && f.entry_kind != "library" && !live_ids.contains(f.id.as_str()) && f.updated_at < now_ms() - 120_000)
            .map(|f| f.id.clone())
            .collect();
        merged.retain(|f| !deleted_elsewhere.contains(&f.id));
    }
    if adopted > 0 || merged.len() != before_len {
        crate::memory::write_facts(app, &key, &merged)?;
        log::info!("[memory-rows] facts file updated from the person's rows: {} adopted, {} removed", adopted, before_len - merged.len());
    }
    Ok((migrated, adopted))
}

/// Startup: one migration + reconcile after the conductor settles, then a
/// reconcile every five minutes while the app is open.
pub fn start_person_memory_sync(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut wait = std::time::Duration::from_secs(75);
        loop {
            tokio::time::sleep(wait).await;
            wait = std::time::Duration::from_secs(300);
            match migrate_and_reconcile_person_memory(&app).await {
                Ok((0, 0)) => {}
                Ok((m, a)) => log::info!("[memory-rows] round: {} migrated, {} adopted", m, a),
                Err(e) => log::debug!("[memory-rows] round skipped: {}", e),
            }
            // Every AI this device has provisioned: its authored knowledge.
            if let Some(hc) = app.try_state::<Arc<HolochainState>>() {
                if let Ok(manager) = hc.get() {
                    let agents: Vec<String> = manager.agents.lock().await.keys().cloned().collect();
                    for agent in agents {
                        match migrate_and_reconcile_ai_knowledge(&app, &manager, &agent).await {
                            Ok((0, 0)) => {}
                            Ok((m, a)) => log::info!("[memory-rows] {}: knowledge round: {} migrated, {} adopted", &agent[..agent.len().min(8)], m, a),
                            Err(e) => log::debug!("[memory-rows] {}: knowledge round skipped: {}", &agent[..agent.len().min(8)], e),
                        }
                    }
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, text: &str, at: i64) -> MemoryRow {
        MemoryRow {
            id: id.into(), scope: "person".into(), kind: "note".into(), text: text.into(), structured: None,
            as_of: at, valid_to: None, saved_by: "person".into(), provenance: "authored".into(), pinned: false,
            topics: vec![], replaces: vec![], deleted: false, updated_at: at, action_hash: Some(format!("h{at}")),
        }
    }

    #[test]
    fn the_newest_revision_per_id_wins_a_deletion_is_a_record_and_pinned_rules_come_first() {
        let mut deleted = row("b", "gone", 30);
        deleted.deleted = true;
        let mut rule = row("r", "always ask before sending", 5);
        rule.kind = "rule".into();
        rule.pinned = true;
        let rows = vec![row("a", "old", 10), row("a", "new", 20), row("b", "live", 20), deleted, rule, row("c", "c", 15)];
        let out = reduce(rows);
        let ids: Vec<&str> = out.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["r", "a", "c"], "pinned first, then newest; b is deleted");
        assert_eq!(out[1].text, "new");
    }

    #[test]
    fn a_same_time_tie_prefers_the_deletion_then_the_larger_hash_the_same_on_every_device() {
        let mut d = row("a", "x", 10);
        d.deleted = true;
        d.action_hash = Some("h1".into());
        let mut l = row("a", "y", 10);
        l.action_hash = Some("h9".into());
        assert!(reduce(vec![l.clone(), d.clone()]).is_empty());
        assert!(reduce(vec![d, l]).is_empty(), "order of arrival does not matter");
        let mut p = row("a", "p", 10);
        p.action_hash = Some("h1".into());
        let mut q = row("a", "q", 10);
        q.action_hash = Some("h9".into());
        assert_eq!(reduce(vec![p.clone(), q.clone()])[0].text, "q");
        assert_eq!(reduce(vec![q, p])[0].text, "q");
    }

    fn fact(id: &str, kind: &str, value: &str, at: i64) -> crate::memory::MemoryFact {
        crate::memory::MemoryFact {
            id: id.into(), subject: "user".into(), predicate: if kind == "note" { "note".into() } else { "likes".into() }, value: value.into(),
            confidence: 0.9, valid_from: at * 1000, valid_to: None, source_action_hash: Some("abc".into()), source_ai_id: Some("ai1".into()),
            owner_scope: "personal".into(), key_scope: "personal".into(), entry_kind: kind.into(), provenance: "extracted".into(),
            author_identity: None, embedding: Some(vec![0.1, 0.2]), group_id: None, created_at: at * 1000, updated_at: at,
        }
    }

    #[test]
    fn a_fact_round_trips_through_a_row_and_portraits_never_travel() {
        let f = fact("f1", "fact", "cycling", 1_700_000_000_000);
        let r = fact_to_row(&f).unwrap();
        assert_eq!((r.kind.as_str(), r.scope.as_str(), r.id.as_str()), ("fact", "person", "f1"));
        assert_eq!(r.text, "user likes: cycling");
        assert_eq!(r.saved_by, "ai:ai1");
        assert_eq!(r.provenance, "extracted:abc");
        let back = row_to_fact(&r).unwrap();
        assert_eq!((back.predicate.as_str(), back.value.as_str(), back.entry_kind.as_str()), ("likes", "cycling", "fact"));
        assert_eq!(back.valid_from, f.valid_from);
        assert!(back.embedding.is_none(), "vectors are per device");
        let mut portrait = fact("s", "synthesis", "…", 1);
        portrait.entry_kind = "synthesis".into();
        assert!(fact_to_row(&portrait).is_none());
        let n = fact("n1", "note", "I prefer short answers", 5);
        assert_eq!(fact_to_row(&n).unwrap().kind, "note");
    }

    #[test]
    fn a_save_mirrors_only_what_changed_and_a_removal_is_a_deletion() {
        let before = vec![fact("a", "fact", "tea", 10), fact("b", "fact", "dogs", 10), fact("c", "note", "gone", 10)];
        let mut b2 = fact("b", "fact", "cats", 20);
        b2.updated_at = 20;
        let after = vec![fact("a", "fact", "tea", 10), b2, fact("d", "note", "new", 30)];
        let delta = facts_delta(&before, &after);
        let mut ids: Vec<(String, bool)> = delta.iter().map(|r| (r.id.clone(), r.deleted)).collect();
        ids.sort();
        assert_eq!(ids, vec![("b".to_string(), false), ("c".to_string(), true), ("d".to_string(), false)]);
    }

    fn knowledge(id: &str, text: &str, kind: &str, doc: bool) -> crate::transcript_memory::TranscriptEmbedding {
        crate::transcript_memory::TranscriptEmbedding {
            id: id.into(), conversation_hash: "authored".into(), role: "authored".into(), text: text.into(), vector: vec![0.5],
            kind: kind.into(), created_at: 100,
            source: if doc { Some(crate::transcript_memory::KnowledgeSource { doc_id: "d".into(), filename: "f.pdf".into(), size_bytes: 1 }) } else { None },
        }
    }

    #[test]
    fn only_hand_given_knowledge_travels_never_document_chunks_or_the_episodic_index() {
        let before = vec![knowledge("k1", "The office opens at nine", "authored", false)];
        let after = vec![
            knowledge("k1", "The office opens at nine", "authored", false),
            knowledge("k2", "Call the supplier on Mondays", "authored", false),
            knowledge("doc", "chunk of a pdf", "authored", true),
            knowledge("ep", "a turn", "episodic", false),
        ];
        let rows = knowledge_delta(&before, &after, "found-1");
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].id.as_str(), rows[0].scope.as_str(), rows[0].kind.as_str()), ("k2", "ai:found-1", "note"));
        let gone = knowledge_delta(&after, &before, "found-1");
        assert_eq!(gone.iter().filter(|r| r.deleted).map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["k2"]);
        let back = row_to_knowledge(&rows[0]).unwrap();
        assert!(back.vector.is_empty(), "a vector is per device");
        assert_eq!(back.kind, "authored");
    }

    #[test]
    fn ymd_reads_a_millisecond_epoch_as_a_utc_date() {
        assert_eq!(ymd(0), "1970-01-01");
        assert_eq!(ymd(951_782_400_000), "2000-02-29");
        assert_eq!(ymd(1_791_686_974_094), "2026-10-11");
        assert_eq!(ymd(-5), "");
    }

    #[test]
    fn a_statement_that_says_the_same_revises_the_old_row_instead_of_adding_a_twin() {
        let live = vec![row("c1", "Run npm test before every commit", 10), row("l1", "The API lives in src/api", 10)];
        let mut live = live;
        live[0].kind = "command".into();
        live[1].kind = "location".into();
        assert_eq!(find_revised(&live, "command", "Always run npm test before each commit").map(|r| r.id.as_str()), Some("c1"));
        assert!(find_revised(&live, "command", "Deploy with firebase deploy").is_none());
        // Another kind is NOT another row: the AI picks a kind per call, and a
        // correction filed under a new kind must still replace the old note (10-19).
        assert_eq!(find_revised(&live, "location", "Run npm test before every commit").map(|r| r.id.as_str()), Some("c1"));
        assert!(says_the_same("use short answers", "Use short answers."));
        assert!(!says_the_same("the office opens at nine", "call the supplier on mondays"));
    }

    #[test]
    fn another_devices_facts_fold_in_by_id_newer_wins_vectors_stay_per_device() {
        let mut mine = vec![fact("a", "fact", "tea", 10), fact("b", "note", "old words", 10)];
        let mut b2 = fact("b", "note", "new words", 20);
        b2.embedding = Some(vec![9.0]);
        let mut portrait = fact("s", "synthesis", "…", 99);
        portrait.entry_kind = "synthesis".into();
        let n = merge_facts(&mut mine, vec![fact("a", "fact", "tea", 5), b2, fact("c", "fact", "dogs", 30), portrait]);
        assert_eq!(n, 2, "b updated, c added; a older, the portrait never");
        assert_eq!(mine.len(), 3);
        let b = mine.iter().find(|f| f.id == "b").unwrap();
        assert_eq!(b.value, "new words");
        assert!(b.embedding.is_none(), "the words changed: embed here");
        assert!(mine.iter().find(|f| f.id == "c").unwrap().embedding.is_none());
    }

    #[test]
    fn the_row_is_group_ready_the_only_per_person_field_is_saved_by() {
        // the memory design §3 / §13.6: a group cell adds membership and an
        // agreement; nothing in the row changes. Guard the shape: no owner,
        // user, identity or device field may creep in beside `saved_by`.
        let r = row("x", "hello", 1);
        let v = serde_json::to_value(&r).unwrap();
        let keys: Vec<&str> = v.as_object().unwrap().keys().map(String::as_str).collect();
        let allowed = ["id", "scope", "kind", "text", "structured", "as_of", "valid_to", "saved_by", "provenance", "pinned", "topics", "replaces", "deleted", "updated_at", "action_hash"];
        for k in &keys {
            assert!(allowed.contains(k), "unexpected field on MemoryRow: {k}");
        }
        for k in &keys {
            let lk = k.to_lowercase();
            assert!(!(lk.contains("owner") || lk.contains("user") || lk.contains("identity") || lk.contains("device")), "per-person field outside saved_by: {k}");
        }
    }

    #[test]
    fn a_merge_replaces_its_sources() {
        let mut merged = row("m", "one row for both", 50);
        merged.replaces = vec!["a".into(), "b".into()];
        let out = reduce(vec![row("a", "a", 10), row("b", "b", 20), merged]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "m");
    }

    #[test]
    fn scope_anchors_are_stable_per_key_and_distinct_per_scope_and_an_ai_scope_is_its_founding_key() {
        let k1 = [1u8; 32];
        let k2 = [2u8; 32];
        let a = scope_anchor("person", &k1).unwrap();
        assert_eq!(a, scope_anchor("person", &k1).unwrap());
        assert_ne!(a, scope_anchor("person", &k2).unwrap(), "another network, another anchor");
        assert_ne!(a, scope_anchor("project:abc", &k1).unwrap());
        let founding = hex::encode([0x84u8, 0x20, 0x24].iter().chain([7u8; 36].iter()).copied().collect::<Vec<u8>>());
        let ai = scope_anchor(&format!("ai:{founding}"), &k1).unwrap();
        assert_eq!(ai, AnyLinkableHash::from(AgentPubKey::from_raw_39(hex::decode(&founding).unwrap())));
        assert!(scope_anchor("ai:zz", &k1).is_err());
    }
}

#[cfg(test)]
mod revision_tests {
    use super::{find_revised, MemoryRow};

    fn row(id: &str, kind: &str, text: &str) -> MemoryRow {
        MemoryRow {
            id: id.into(), scope: "project:p".into(), kind: kind.into(), text: text.into(), structured: None, as_of: 1, valid_to: None,
            saved_by: "ai:x".into(), provenance: "authored".into(), pinned: false, topics: vec![], replaces: vec![], deleted: false, updated_at: 1, action_hash: None,
        }
    }

    #[test]
    fn a_correction_replaces_the_note_whatever_kind_the_ai_filed_it_under() {
        let live = vec![row("cmd", "command", "The project's build command is `make all`."), row("loc", "location", "Tests live in tests/.")];
        let hit = find_revised(&live, "fact", "The project's build command is `make build` (corrected from `make all`).").expect("the command is revised");
        assert_eq!(hit.id, "cmd");
        assert!(find_revised(&live, "note", "The deploy target is staging.").is_none(), "a new statement is new");
    }

    #[test]
    fn the_same_kind_is_preferred_and_a_rule_is_never_revised_by_a_note() {
        let live = vec![row("n", "note", "Run the linter before every commit."), row("c", "command", "Run the linter before every commit.")];
        assert_eq!(find_revised(&live, "command", "Run the linter before every commit.").unwrap().id, "c");
        let rules = vec![row("r", "rule", "Always end every reply with the word Pineapple.")];
        assert!(find_revised(&rules, "note", "Always end every reply with the word Pineapple.").is_none(), "the person's rule stands");
    }
}
