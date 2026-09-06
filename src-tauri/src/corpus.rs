//! The corpus: the person's own document library, owned at the user level,
//! with each AI granted access per document.
//!
//! Why this exists (audit 2026-09-05, planning/KNOWLEDGE_CORPUS.md): document
//! knowledge used to live inside each AI's encrypted embedding blob, which is
//! decrypted and parsed whole on every chat turn and rewritten whole per
//! document - fine for a few files, impossible for "ten years of articles".
//! Here every passage is a row in SQLite (text and vector encrypted under the
//! same data key as the other stores), the vectors sit in one in-memory cache
//! for recall, an import writes one document at a time, and the Vault backup
//! carries the document records only (passages are re-derivable from the
//! person's own files).
//!
//! Scope: a document belongs to the person; `grants` says which AIs may draw
//! on it. The Knowledge tab's "Add documents" means "add to my library and
//! give this AI access".
use crate::commands_holochain::HolochainState;
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use tauri::{AppHandle, Emitter, Manager, State};

/// Target passage size. Larger than the old 600 so book prose keeps its
/// sense; well under the embed server's 1400-character input cap.
pub const PASSAGE_CHARS: usize = 900;
/// A folder drop walks this deep and no deeper.
const MAX_DEPTH: usize = 8;
/// Folders a document library never means.
const SKIP_DIRS: &[&str] = &["node_modules", ".git", "target", "dist", "build", ".cache", "__pycache__"];
/// What the reader handles (mirrors the attachment reader in lib.rs).
pub const DOC_EXTENSIONS: &[&str] = &[
    "txt", "md", "markdown", "csv", "json", "xml", "yaml", "yml", "toml", "log", "ini", "cfg", "conf",
    "pdf", "docx", "doc", "xlsx", "xls", "ods", "odt", "rtf", "html", "htm", "sql", "epub",
    "py", "js", "ts", "tsx", "jsx", "rs", "go", "java", "c", "cpp", "h", "cs", "rb", "php",
];
/// Recall: the same floor as conversational memory. No relative margin here -
/// a document's fourth-best passage is still worth reading.
const RECALL_THRESHOLD: f32 = 0.45;
/// Embedding batch size (the embed server takes a batch per request).
const EMBED_BATCH: usize = 24;
/// The memory component (mirrors EMBEDDING_MODEL in src/data/recommended-models.ts).
pub const EMBEDDING_MODEL_FILE: &str = "bge-small-en-v1.5-f16.gguf";

// ---------------------------------------------------------------- records

/// Encrypted per document: everything a person could recognise a file by.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct DocMeta {
    pub filename: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Written on the device by the helper model, later.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    /// Written by the person (guessed from metadata, flippable).
    #[serde(default)]
    pub mine: bool,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct DocRecord {
    pub doc_id: String,
    pub added_at: i64,
    pub byte_size: i64,
    pub chunk_count: i64,
    pub meta: DocMeta,
    /// AIs granted this document.
    pub ai_ids: Vec<String>,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct ImportFailure {
    pub file: String,
    pub reason: String,
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct ImportReport {
    pub added: Vec<DocRecord>,
    pub failed: Vec<ImportFailure>,
    /// Files skipped because the same path was already in the library.
    pub already: usize,
    pub cancelled: bool,
}

#[derive(Serialize, Clone, Debug)]
pub struct RecallHit {
    pub doc_id: String,
    pub filename: String,
    pub idx: u32,
    pub text: String,
    pub score: f32,
    pub mine: bool,
}

// ---------------------------------------------------------------- epub + metadata

fn zip_entry_string<R: std::io::Read + std::io::Seek>(archive: &mut zip::ZipArchive<R>, name: &str) -> Option<String> {
    let mut f = archive.by_name(name).ok()?;
    let mut buf = Vec::new();
    std::io::Read::read_to_end(&mut f, &mut buf).ok()?;
    Some(String::from_utf8_lossy(&buf).into_owned())
}

/// The text of the first `<tag>` (any namespace prefix), entities decoded.
/// Small documents' metadata only - never the body.
pub fn xml_tag_text(xml: &str, tag: &str) -> Option<String> {
    let lower = xml.to_ascii_lowercase();
    let needle = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find('<') {
        let start = from + pos + 1;
        let end = lower[start..].find('>')? + start;
        let head = &lower[start..end];
        let local = head.split(|c: char| c.is_whitespace()).next().unwrap_or("");
        let local = local.rsplit(':').next().unwrap_or(local);
        if local == needle && !head.ends_with('/') {
            let close = lower[end + 1..].find("</")? + end + 1;
            let inner = xml[end + 1..close].trim();
            let text = inner
                .replace("&amp;", "&")
                .replace("&lt;", "<")
                .replace("&gt;", ">")
                .replace("&quot;", "\"")
                .replace("&apos;", "'");
            return if text.is_empty() { None } else { Some(text) };
        }
        from = end + 1;
    }
    None
}

/// An EPUB is a zip of XHTML files in the order the OPF spine gives.
pub(crate) fn extract_epub_text(path: &Path) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Failed to open EPUB: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("Failed to read EPUB archive: {e}"))?;
    let mut order: Vec<String> = Vec::new();
    if let Some(opf_path) = epub_opf_path(&mut archive) {
        if let Some(opf) = zip_entry_string(&mut archive, &opf_path) {
            let dir = match opf_path.rfind('/') {
                Some(i) => opf_path[..=i].to_string(),
                None => String::new(),
            };
            let hrefs = epub_manifest(&opf);
            for id in epub_spine(&opf) {
                if let Some(h) = hrefs.get(&id) {
                    order.push(format!("{dir}{h}"));
                }
            }
        }
    }
    if order.is_empty() {
        // No usable spine: every page in archive order.
        let mut names: Vec<String> = (0..archive.len())
            .filter_map(|i| archive.by_index(i).ok().map(|f| f.name().to_string()))
            .filter(|n| {
                let l = n.to_ascii_lowercase();
                l.ends_with(".xhtml") || l.ends_with(".html") || l.ends_with(".htm")
            })
            .collect();
        names.sort();
        order = names;
    }
    let mut text = String::new();
    for name in order {
        if let Some(page) = zip_entry_string(&mut archive, &name) {
            let body = strip_tags(&page);
            let body = body.split_whitespace().collect::<Vec<_>>().join(" ");
            if !body.trim().is_empty() {
                text.push_str(body.trim());
                text.push_str("\n\n");
            }
        }
    }
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        return Err("No text content found in EPUB".to_string());
    }
    Ok(trimmed)
}

fn epub_opf_path<R: std::io::Read + std::io::Seek>(archive: &mut zip::ZipArchive<R>) -> Option<String> {
    let container = zip_entry_string(archive, "META-INF/container.xml")?;
    let lower = container.to_ascii_lowercase();
    let i = lower.find("full-path=")? + "full-path=".len();
    let quote = container[i..].chars().next()?;
    let rest = &container[i + 1..];
    let end = rest.find(quote)?;
    Some(rest[..end].to_string())
}

/// manifest id -> href
fn epub_manifest(opf: &str) -> std::collections::HashMap<String, String> {
    let mut out = std::collections::HashMap::new();
    for item in opf.split("<item ").skip(1) {
        let tag = item.split('>').next().unwrap_or("");
        if let (Some(id), Some(href)) = (attr(tag, "id"), attr(tag, "href")) {
            out.insert(id, href);
        }
    }
    out
}

fn epub_spine(opf: &str) -> Vec<String> {
    opf.split("<itemref ")
        .skip(1)
        .filter_map(|it| attr(it.split('>').next().unwrap_or(""), "idref"))
        .collect()
}

fn attr(tag: &str, name: &str) -> Option<String> {
    let lower = tag.to_ascii_lowercase();
    let mut from = 0;
    while let Some(pos) = lower[from..].find(name) {
        let at = from + pos;
        let before_ok = at == 0 || !lower.as_bytes()[at - 1].is_ascii_alphanumeric();
        let after = &lower[at + name.len()..];
        if before_ok && after.trim_start().starts_with('=') {
            let eq = after.find('=')? + at + name.len() + 1;
            let rest = tag[eq..].trim_start();
            let quote = rest.chars().next()?;
            if quote != '"' && quote != '\'' {
                return None;
            }
            let inner = &rest[1..];
            let end = inner.find(quote)?;
            return Some(inner[..end].to_string());
        }
        from = at + name.len();
    }
    None
}

/// Author and title from a file's own metadata, when it carries any.
#[derive(Default, Debug, PartialEq)]
pub struct DocInfo {
    pub author: Option<String>,
    pub title: Option<String>,
}

pub(crate) fn doc_info(path: &Path) -> DocInfo {
    let ext = ext_of(path);
    let clean = |s: Option<String>| s.map(|t| t.trim().to_string()).filter(|t| !t.is_empty() && t.len() <= 200);
    match ext.as_str() {
        "docx" | "odt" | "epub" => {
            let Ok(file) = std::fs::File::open(path) else { return DocInfo::default() };
            let Ok(mut archive) = zip::ZipArchive::new(file) else { return DocInfo::default() };
            let xml = match ext.as_str() {
                "docx" => zip_entry_string(&mut archive, "docProps/core.xml"),
                "odt" => zip_entry_string(&mut archive, "meta.xml"),
                _ => epub_opf_path(&mut archive).and_then(|p| zip_entry_string(&mut archive, &p)),
            };
            let Some(xml) = xml else { return DocInfo::default() };
            let author = xml_tag_text(&xml, "creator").or_else(|| xml_tag_text(&xml, "initial-creator"));
            DocInfo { author: clean(author), title: clean(xml_tag_text(&xml, "title")) }
        }
        "pdf" => {
            // The Info dictionary's literal strings, read from the raw file:
            // enough for a guess, and no parser to feed hostile input to.
            let Ok(raw) = std::fs::read(path) else { return DocInfo::default() };
            DocInfo { author: clean(pdf_info_string(&raw, b"/Author")), title: clean(pdf_info_string(&raw, b"/Title")) }
        }
        _ => DocInfo::default(),
    }
}

fn pdf_info_string(raw: &[u8], key: &[u8]) -> Option<String> {
    let mut from = 0;
    while let Some(pos) = raw[from..].windows(key.len()).position(|w| w == key) {
        let mut i = from + pos + key.len();
        while i < raw.len() && raw[i].is_ascii_whitespace() {
            i += 1;
        }
        if i < raw.len() && raw[i] == b'(' {
            let mut out = Vec::new();
            let mut depth = 1;
            let mut j = i + 1;
            while j < raw.len() && depth > 0 {
                match raw[j] {
                    b'\\' if j + 1 < raw.len() => {
                        out.push(raw[j + 1]);
                        j += 2;
                        continue;
                    }
                    b'(' => depth += 1,
                    b')' => {
                        depth -= 1;
                        if depth == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
                out.push(raw[j]);
                j += 1;
            }
            // UTF-16 with a byte-order mark, else treat as Latin-1/UTF-8.
            let text = if out.starts_with(&[0xFE, 0xFF]) {
                let units: Vec<u16> = out[2..].chunks(2).filter(|c| c.len() == 2).map(|c| u16::from_be_bytes([c[0], c[1]])).collect();
                String::from_utf16_lossy(&units)
            } else {
                String::from_utf8_lossy(&out).into_owned()
            };
            if !text.trim().is_empty() {
                return Some(text);
            }
        }
        from = i.max(from + pos + 1);
    }
    None
}

fn norm_name(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

/// Does the author field look like this person? Equal after normalising, or
/// one of the person's names appears in it as whole words (a first name in
/// "Eric Smith"; never "ann" inside "Hannah").
pub fn looks_mine(author: &str, names: &[String]) -> bool {
    let a = norm_name(author);
    if a.is_empty() {
        return false;
    }
    let a_words: Vec<&str> = a.split(' ').collect();
    names.iter().map(|n| norm_name(n)).filter(|n| n.len() >= 3).any(|n| {
        if n == a {
            return true;
        }
        let n_words: Vec<&str> = n.split(' ').collect();
        a_words.windows(n_words.len()).any(|w| w == n_words.as_slice())
    })
}

// ---------------------------------------------------------------- storage

fn db_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| format!("No app data dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("app data dir: {e}"))?;
    Ok(dir.join("corpus.sqlite"))
}

fn open_at(path: &Path) -> Result<Connection, String> {
    let conn = Connection::open(path).map_err(|e| format!("corpus db: {e}"))?;
    conn.execute_batch(
        "PRAGMA journal_mode = WAL;
         CREATE TABLE IF NOT EXISTS documents (
           doc_id TEXT PRIMARY KEY,
           added_at INTEGER NOT NULL,
           byte_size INTEGER NOT NULL,
           chunk_count INTEGER NOT NULL,
           path_hash TEXT,
           meta_enc BLOB NOT NULL
         );
         CREATE TABLE IF NOT EXISTS grants (
           doc_id TEXT NOT NULL,
           ai_id TEXT NOT NULL,
           PRIMARY KEY (doc_id, ai_id)
         );
         CREATE TABLE IF NOT EXISTS passages (
           doc_id TEXT NOT NULL,
           idx INTEGER NOT NULL,
           text_enc BLOB NOT NULL,
           vec_enc BLOB NOT NULL,
           PRIMARY KEY (doc_id, idx)
         );
         CREATE INDEX IF NOT EXISTS grants_ai ON grants(ai_id);",
    )
    .map_err(|e| format!("corpus schema: {e}"))?;
    Ok(conn)
}

fn open(app: &AppHandle) -> Result<Connection, String> {
    open_at(&db_path(app)?)
}

fn data_key(hc: &State<'_, Arc<HolochainState>>) -> Result<[u8; 32], String> {
    hc.get()?.data_key()
}

/// nonce || ciphertext, so one blob column carries both.
fn enc(key: &[u8; 32], plain: &[u8]) -> Result<Vec<u8>, String> {
    let (nonce, cipher) = crate::transcript_crypto::encrypt(key, plain)?;
    let mut out = Vec::with_capacity(24 + cipher.len());
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&cipher);
    Ok(out)
}

fn dec(key: &[u8; 32], blob: &[u8]) -> Result<Vec<u8>, String> {
    if blob.len() < 24 {
        return Err("corpus: blob too short".into());
    }
    crate::transcript_crypto::decrypt(key, &blob[..24], &blob[24..])
}

fn vec_to_bytes(v: &[f32]) -> Vec<u8> {
    let mut out = Vec::with_capacity(v.len() * 4);
    for x in v {
        out.extend_from_slice(&x.to_le_bytes());
    }
    out
}

fn bytes_to_vec(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

fn path_hash(path: &str) -> String {
    use sha2::{Digest, Sha256};
    hex::encode(Sha256::digest(path.as_bytes()))
}

fn new_doc_id() -> String {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("d{:x}{:03x}", nanos, COUNTER.fetch_add(1, Ordering::Relaxed) & 0xfff)
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------- the vector cache

struct CachedVec {
    doc_id: String,
    idx: u32,
    vec: Vec<f32>,
}

static CACHE: Mutex<Option<Arc<Vec<CachedVec>>>> = Mutex::new(None);

fn cache_invalidate() {
    if let Ok(mut g) = CACHE.lock() {
        *g = None;
    }
}

fn cache_load(conn: &Connection, key: &[u8; 32]) -> Result<Arc<Vec<CachedVec>>, String> {
    if let Ok(g) = CACHE.lock() {
        if let Some(c) = g.as_ref() {
            return Ok(c.clone());
        }
    }
    let mut stmt = conn
        .prepare("SELECT doc_id, idx, vec_enc FROM passages")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, Vec<u8>>(2)?)))
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        let (doc_id, idx, blob) = row.map_err(|e| e.to_string())?;
        let vec = bytes_to_vec(&dec(key, &blob)?);
        out.push(CachedVec { doc_id, idx: idx as u32, vec });
    }
    let arc = Arc::new(out);
    if let Ok(mut g) = CACHE.lock() {
        *g = Some(arc.clone());
    }
    Ok(arc)
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        0.0
    } else {
        dot / (na.sqrt() * nb.sqrt())
    }
}

// ---------------------------------------------------------------- text

/// Cut a document into passages: whole paragraphs, then oversize paragraphs
/// by sentence, tiny fragments merged into the previous passage, and the
/// last sentence of each passage repeated at the start of the next so a
/// fact split across a boundary is still findable. Ported from the
/// frontend's chunkDocumentText with the overlap made explicit.
pub fn chunk_text(text: &str, max: usize) -> Vec<String> {
    let clean = text.replace("\r\n", "\n");
    let clean = clean
        .lines()
        .map(|l| l.split_whitespace().collect::<Vec<_>>().join(" "))
        .collect::<Vec<_>>()
        .join("\n");
    let clean = clean.trim();
    if clean.is_empty() {
        return vec![];
    }
    let mut units: Vec<String> = Vec::new();
    for para in clean.split("\n\n") {
        let p = para.trim();
        if p.is_empty() {
            continue;
        }
        if p.chars().count() <= max {
            units.push(p.to_string());
            continue;
        }
        let mut buf = String::new();
        for sentence in split_sentences(p) {
            let s_len = sentence.chars().count();
            if !buf.is_empty() && buf.chars().count() + s_len + 1 > max {
                units.push(buf.trim().to_string());
                buf.clear();
            }
            if s_len > max {
                let chars: Vec<char> = sentence.chars().collect();
                for piece in chars.chunks(max) {
                    units.push(piece.iter().collect::<String>().trim().to_string());
                }
            } else {
                if !buf.is_empty() {
                    buf.push(' ');
                }
                buf.push_str(sentence);
            }
        }
        if !buf.trim().is_empty() {
            units.push(buf.trim().to_string());
        }
    }
    let mut chunks: Vec<String> = Vec::new();
    for u in units {
        if u.is_empty() {
            continue;
        }
        if let Some(last) = chunks.last_mut() {
            if last.chars().count() + u.chars().count() + 1 <= max {
                last.push(' ');
                last.push_str(&u);
                continue;
            }
        }
        chunks.push(u);
    }
    // Overlap: carry the previous passage's last sentence forward.
    let mut out: Vec<String> = Vec::with_capacity(chunks.len());
    for (i, c) in chunks.iter().enumerate() {
        if i == 0 {
            out.push(c.clone());
            continue;
        }
        let prev_tail = split_sentences(&chunks[i - 1]).last().map(|s| s.to_string()).unwrap_or_default();
        if !prev_tail.is_empty() && prev_tail.chars().count() < max / 3 && !c.starts_with(&prev_tail) {
            out.push(format!("{prev_tail} {c}"));
        } else {
            out.push(c.clone());
        }
    }
    out
}

fn split_sentences(p: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0;
    let bytes = p.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if (b == b'.' || b == b'!' || b == b'?') && i + 1 < bytes.len() && bytes[i + 1].is_ascii_whitespace() {
            let s = p[start..=i].trim();
            if !s.is_empty() {
                out.push(s);
            }
            start = i + 1;
        }
        i += 1;
    }
    let s = p[start..].trim();
    if !s.is_empty() {
        out.push(s);
    }
    out
}

/// Strip the overlap when reassembling a document from its passages.
fn join_passages(passages: &[String]) -> String {
    let mut out = String::new();
    for (i, p) in passages.iter().enumerate() {
        if i == 0 {
            out.push_str(p);
            continue;
        }
        let prev_tail = split_sentences(&passages[i - 1]).last().map(|s| s.to_string()).unwrap_or_default();
        let body = if !prev_tail.is_empty() && p.starts_with(&prev_tail) {
            p[prev_tail.len()..].trim_start()
        } else {
            p.as_str()
        };
        out.push_str("\n\n");
        out.push_str(body);
    }
    out
}

fn ext_of(path: &Path) -> String {
    path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase()
}

fn is_document(path: &Path) -> bool {
    DOC_EXTENSIONS.contains(&ext_of(path).as_str())
}

/// Every document under the given paths: files as they are, folders walked
/// (bounded depth, hidden and build folders skipped). Order is stable.
pub fn walk(paths: &[String]) -> Vec<PathBuf> {
    fn rec(dir: &Path, depth: usize, out: &mut Vec<PathBuf>) {
        if depth > MAX_DEPTH {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with('.') {
                continue;
            }
            if p.is_dir() {
                if !SKIP_DIRS.contains(&name) {
                    rec(&p, depth + 1, out);
                }
            } else if is_document(&p) {
                out.push(p);
            }
        }
    }
    let mut out = Vec::new();
    for s in paths {
        let p = PathBuf::from(s);
        if p.is_dir() {
            rec(&p, 0, &mut out);
        } else if p.is_file() {
            out.push(p);
        }
    }
    out
}

/// The document's text, uncapped: the attachment reader cuts at 256 KB
/// because it feeds one prompt; a library entry is read whole.
pub(crate) fn extract_text(path: &Path) -> Result<String, String> {
    let ext = ext_of(path);
    match ext.as_str() {
        "docx" | "doc" => crate::extract_docx_text(path),
        "odt" => crate::extract_odt_text(path),
        "pdf" => crate::extract_pdf_text(path),
        "epub" => extract_epub_text(path),
        "xlsx" | "xls" | "ods" => crate::extract_spreadsheet_text(path),
        "html" | "htm" => {
            let raw = std::fs::read(path).map_err(|e| e.to_string())?;
            Ok(strip_tags(&String::from_utf8_lossy(&raw)))
        }
        _ => {
            let raw = std::fs::read(path).map_err(|e| e.to_string())?;
            Ok(String::from_utf8_lossy(&raw).into_owned())
        }
    }
}

fn strip_tags(html: &str) -> String {
    let mut out = String::with_capacity(html.len());
    let mut in_tag = false;
    let mut in_script = false;
    let lower = html.to_ascii_lowercase();
    let mut i = 0;
    let bytes = html.as_bytes();
    while i < bytes.len() {
        if !in_tag && lower[i..].starts_with("<script") || lower[i..].starts_with("<style") {
            in_script = true;
        }
        if in_script && (lower[i..].starts_with("</script>") || lower[i..].starts_with("</style>")) {
            in_script = false;
        }
        let c = bytes[i] as char;
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
            out.push(' ');
        } else if !in_tag && !in_script {
            out.push(c);
        }
        i += 1;
    }
    out
}

// ---------------------------------------------------------------- import

static CANCEL: AtomicBool = AtomicBool::new(false);

#[derive(Serialize, Clone)]
struct Progress {
    phase: &'static str,
    file: String,
    done: usize,
    total: usize,
    added: usize,
    failed: usize,
}

fn emit_progress(app: &AppHandle, p: &Progress) {
    let _ = app.emit("corpus-progress", p);
}

fn insert_document(
    conn: &mut Connection,
    key: &[u8; 32],
    meta: &DocMeta,
    byte_size: i64,
    passages: &[String],
    vectors: &[Vec<f32>],
    ai_id: &str,
) -> Result<DocRecord, String> {
    let doc_id = new_doc_id();
    let added_at = now_secs();
    let meta_json = serde_json::to_vec(meta).map_err(|e| e.to_string())?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO documents (doc_id, added_at, byte_size, chunk_count, path_hash, meta_enc) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            doc_id,
            added_at,
            byte_size,
            passages.len() as i64,
            meta.path.as_deref().map(path_hash),
            enc(key, &meta_json)?
        ],
    )
    .map_err(|e| e.to_string())?;
    for (i, (text, vec)) in passages.iter().zip(vectors.iter()).enumerate() {
        tx.execute(
            "INSERT INTO passages (doc_id, idx, text_enc, vec_enc) VALUES (?1, ?2, ?3, ?4)",
            params![doc_id, i as i64, enc(key, text.as_bytes())?, enc(key, &vec_to_bytes(vec))?],
        )
        .map_err(|e| e.to_string())?;
    }
    tx.execute("INSERT OR IGNORE INTO grants (doc_id, ai_id) VALUES (?1, ?2)", params![doc_id, ai_id])
        .map_err(|e| e.to_string())?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(DocRecord {
        doc_id,
        added_at,
        byte_size,
        chunk_count: passages.len() as i64,
        meta: meta.clone(),
        ai_ids: vec![ai_id.to_string()],
    })
}

fn already_have(conn: &Connection, path: &str) -> Result<Option<String>, String> {
    conn.query_row(
        "SELECT doc_id FROM documents WHERE path_hash = ?1",
        params![path_hash(path)],
        |r| r.get::<_, String>(0),
    )
    .optional()
    .map_err(|e| e.to_string())
}

/// Add files and folders to the library and grant them to one AI. Reads,
/// cuts, embeds and stores one document at a time, reporting progress on
/// `corpus-progress`; `corpus_cancel` stops after the current document.
#[tauri::command]
pub async fn corpus_import(
    app: AppHandle,
    hc_state: State<'_, Arc<HolochainState>>,
    llm_state: State<'_, crate::llm::LLMState>,
    paths: Vec<String>,
    ai_id: String,
    // The person's names (display name, username, what they told an AI)
    // for the Mine guess; the tag stays flippable on the row.
    names: Option<Vec<String>>,
) -> Result<ImportReport, String> {
    CANCEL.store(false, Ordering::SeqCst);
    let names = names.unwrap_or_default();
    let key = data_key(&hc_state)?;
    let files = walk(&paths);
    let total = files.len();
    let mut report = ImportReport::default();
    let mut conn = open(&app)?;
    let embed_model = EMBEDDING_MODEL_FILE.to_string();
    for (n, path) in files.iter().enumerate() {
        if CANCEL.load(Ordering::SeqCst) {
            report.cancelled = true;
            break;
        }
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("document").to_string();
        let path_str = path.to_string_lossy().to_string();
        emit_progress(&app, &Progress { phase: "reading", file: name.clone(), done: n, total, added: report.added.len(), failed: report.failed.len() });
        if let Some(existing) = already_have(&conn, &path_str)? {
            // Same file again: make sure this AI has it, count it, move on.
            conn.execute("INSERT OR IGNORE INTO grants (doc_id, ai_id) VALUES (?1, ?2)", params![existing, ai_id])
                .map_err(|e| e.to_string())?;
            report.already += 1;
            continue;
        }
        let text = match extract_text(path) {
            Ok(t) => t,
            Err(e) => {
                report.failed.push(ImportFailure { file: name, reason: e });
                continue;
            }
        };
        let passages = chunk_text(&text, PASSAGE_CHARS);
        if passages.is_empty() {
            report.failed.push(ImportFailure { file: name, reason: "no readable text (a scanned PDF or an empty file)".into() });
            continue;
        }
        emit_progress(&app, &Progress { phase: "embedding", file: name.clone(), done: n, total, added: report.added.len(), failed: report.failed.len() });
        let mut vectors: Vec<Vec<f32>> = Vec::with_capacity(passages.len());
        let mut failed = None;
        for batch in passages.chunks(EMBED_BATCH) {
            if CANCEL.load(Ordering::SeqCst) {
                break;
            }
            match crate::llm::embed_texts(app.clone(), llm_state.clone(), batch.to_vec(), embed_model.clone()).await {
                Ok(v) => vectors.extend(v),
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
        }
        if let Some(e) = failed {
            report.failed.push(ImportFailure { file: name, reason: format!("embedding: {e}") });
            continue;
        }
        if vectors.len() != passages.len() {
            report.cancelled = true;
            break;
        }
        let byte_size = std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(text.len() as i64);
        let info = doc_info(path);
        let mine = info.author.as_deref().map(|a| looks_mine(a, &names)).unwrap_or(false);
        let meta = DocMeta { filename: name.clone(), path: Some(path_str), author: info.author, title: info.title, mine, ..Default::default() };
        match insert_document(&mut conn, &key, &meta, byte_size, &passages, &vectors, &ai_id) {
            Ok(rec) => report.added.push(rec),
            Err(e) => report.failed.push(ImportFailure { file: name, reason: e }),
        }
    }
    cache_invalidate();
    emit_progress(&app, &Progress { phase: "done", file: String::new(), done: total, total, added: report.added.len(), failed: report.failed.len() });
    log::info!(
        "[corpus] import for AI {}: {} added, {} failed, {} already, cancelled={}",
        &ai_id[..8.min(ai_id.len())],
        report.added.len(),
        report.failed.len(),
        report.already,
        report.cancelled
    );
    Ok(report)
}

#[derive(Serialize, Clone, Debug, Default)]
pub struct RereadReport {
    /// Documents that got their text back.
    pub restored: usize,
    /// Files in the folder that matched no waiting record.
    pub unmatched: usize,
    /// Records still waiting for their file after this pass.
    pub remaining: usize,
    pub failed: Vec<ImportFailure>,
    pub cancelled: bool,
}

/// After a restore the records are back (name, card, flags, grants) with no
/// passages: the backup never carries them. The person points at the folder
/// their files live in; each file that matches a waiting record by name
/// (same size preferred) is read and embedded into that record, keeping
/// its id, its card, its Mine flag and every AI's grant.
#[tauri::command]
pub async fn corpus_reread(
    app: AppHandle,
    hc_state: State<'_, Arc<HolochainState>>,
    llm_state: State<'_, crate::llm::LLMState>,
    paths: Vec<String>,
) -> Result<RereadReport, String> {
    CANCEL.store(false, Ordering::SeqCst);
    let key = data_key(&hc_state)?;
    let mut conn = open(&app)?;
    // Waiting records: (doc_id, byte_size, meta)
    let mut waiting: Vec<(String, i64, DocMeta)> = {
        let mut stmt = conn
            .prepare("SELECT doc_id, byte_size, meta_enc FROM documents WHERE chunk_count = 0")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?, r.get::<_, Vec<u8>>(2)?)))
            .map_err(|e| e.to_string())?;
        let mut out = Vec::new();
        for row in rows {
            let (id, size, blob) = row.map_err(|e| e.to_string())?;
            let meta: DocMeta = serde_json::from_slice(&dec(&key, &blob)?).map_err(|e| e.to_string())?;
            out.push((id, size, meta));
        }
        out
    };
    let mut report = RereadReport::default();
    if waiting.is_empty() {
        return Ok(report);
    }
    let files = walk(&paths);
    let total = files.len();
    let embed_model = EMBEDDING_MODEL_FILE.to_string();
    for (n, path) in files.iter().enumerate() {
        if CANCEL.load(Ordering::SeqCst) {
            report.cancelled = true;
            break;
        }
        let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("document").to_string();
        let size = std::fs::metadata(path).map(|m| m.len() as i64).unwrap_or(-1);
        let lower = name.to_lowercase();
        let by_name: Vec<usize> = waiting
            .iter()
            .enumerate()
            .filter(|(_, (_, _, m))| m.filename.to_lowercase() == lower)
            .map(|(i, _)| i)
            .collect();
        let pick = by_name
            .iter()
            .copied()
            .find(|&i| waiting[i].1 == size)
            .or_else(|| if by_name.len() == 1 { Some(by_name[0]) } else { None });
        let Some(i) = pick else {
            report.unmatched += 1;
            continue;
        };
        emit_progress(&app, &Progress { phase: "reading", file: name.clone(), done: n, total, added: report.restored, failed: report.failed.len() });
        let text = match extract_text(path) {
            Ok(t) => t,
            Err(e) => {
                report.failed.push(ImportFailure { file: name, reason: e });
                continue;
            }
        };
        let passages = chunk_text(&text, PASSAGE_CHARS);
        if passages.is_empty() {
            report.failed.push(ImportFailure { file: name, reason: "no readable text".into() });
            continue;
        }
        emit_progress(&app, &Progress { phase: "embedding", file: name.clone(), done: n, total, added: report.restored, failed: report.failed.len() });
        let mut vectors: Vec<Vec<f32>> = Vec::with_capacity(passages.len());
        let mut failed = None;
        for batch in passages.chunks(EMBED_BATCH) {
            if CANCEL.load(Ordering::SeqCst) {
                break;
            }
            match crate::llm::embed_texts(app.clone(), llm_state.clone(), batch.to_vec(), embed_model.clone()).await {
                Ok(v) => vectors.extend(v),
                Err(e) => {
                    failed = Some(e);
                    break;
                }
            }
        }
        if let Some(e) = failed {
            report.failed.push(ImportFailure { file: name, reason: format!("embedding: {e}") });
            continue;
        }
        if vectors.len() != passages.len() {
            report.cancelled = true;
            break;
        }
        let (doc_id, _, mut meta) = waiting.remove(i);
        let path_str = path.to_string_lossy().to_string();
        meta.path = Some(path_str.clone());
        let meta_json = serde_json::to_vec(&meta).map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        tx.execute("DELETE FROM passages WHERE doc_id = ?1", params![doc_id]).map_err(|e| e.to_string())?;
        for (idx, (text, vec)) in passages.iter().zip(vectors.iter()).enumerate() {
            tx.execute(
                "INSERT INTO passages (doc_id, idx, text_enc, vec_enc) VALUES (?1, ?2, ?3, ?4)",
                params![doc_id, idx as i64, enc(&key, text.as_bytes())?, enc(&key, &vec_to_bytes(vec))?],
            )
            .map_err(|e| e.to_string())?;
        }
        tx.execute(
            "UPDATE documents SET chunk_count = ?2, byte_size = ?3, path_hash = ?4, meta_enc = ?5 WHERE doc_id = ?1",
            params![doc_id, passages.len() as i64, size.max(0), path_hash(&path_str), enc(&key, &meta_json)?],
        )
        .map_err(|e| e.to_string())?;
        tx.commit().map_err(|e| e.to_string())?;
        report.restored += 1;
    }
    report.remaining = waiting.len();
    cache_invalidate();
    emit_progress(&app, &Progress { phase: "done", file: String::new(), done: total, total, added: report.restored, failed: report.failed.len() });
    log::info!(
        "[corpus] re-read: {} restored, {} unmatched, {} failed, {} still waiting, cancelled={}",
        report.restored, report.unmatched, report.failed.len(), report.remaining, report.cancelled
    );
    Ok(report)
}

#[tauri::command]
pub fn corpus_cancel() {
    CANCEL.store(true, Ordering::SeqCst);
}

/// Bring a document in from the old per-AI store (text + vectors already
/// known) - the one-time migration, driven by the frontend.
#[tauri::command]
pub fn corpus_import_prepared(
    app: AppHandle,
    hc_state: State<'_, Arc<HolochainState>>,
    ai_id: String,
    filename: String,
    byte_size: i64,
    passages: Vec<String>,
    vectors: Vec<Vec<f32>>,
) -> Result<DocRecord, String> {
    if passages.is_empty() || passages.len() != vectors.len() {
        return Err("corpus: passages and vectors must match".into());
    }
    let key = data_key(&hc_state)?;
    let mut conn = open(&app)?;
    let meta = DocMeta { filename, ..Default::default() };
    let rec = insert_document(&mut conn, &key, &meta, byte_size, &passages, &vectors, &ai_id)?;
    cache_invalidate();
    Ok(rec)
}

// ---------------------------------------------------------------- records

fn read_records(conn: &Connection, key: &[u8; 32], ai_id: Option<&str>) -> Result<Vec<DocRecord>, String> {
    let mut stmt = conn
        .prepare("SELECT doc_id, added_at, byte_size, chunk_count, meta_enc FROM documents ORDER BY added_at DESC")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, Vec<u8>>(4)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for row in rows {
        let (doc_id, added_at, byte_size, chunk_count, meta_enc) = row.map_err(|e| e.to_string())?;
        let meta: DocMeta = serde_json::from_slice(&dec(key, &meta_enc)?).map_err(|e| e.to_string())?;
        let mut g = conn.prepare("SELECT ai_id FROM grants WHERE doc_id = ?1").map_err(|e| e.to_string())?;
        let ai_ids: Vec<String> = g
            .query_map(params![doc_id], |r| r.get::<_, String>(0))
            .map_err(|e| e.to_string())?
            .flatten()
            .collect();
        if let Some(want) = ai_id {
            if !ai_ids.iter().any(|a| a == want) {
                continue;
            }
        }
        out.push(DocRecord { doc_id, added_at, byte_size, chunk_count, meta, ai_ids });
    }
    Ok(out)
}

/// The library, or the part of it one AI may draw on.
#[tauri::command]
pub fn corpus_documents(
    app: AppHandle,
    hc_state: State<'_, Arc<HolochainState>>,
    ai_id: Option<String>,
) -> Result<Vec<DocRecord>, String> {
    let key = data_key(&hc_state)?;
    let conn = open(&app)?;
    read_records(&conn, &key, ai_id.as_deref())
}

fn delete_document(conn: &Connection, doc_id: &str) -> Result<(), String> {
    conn.execute("DELETE FROM passages WHERE doc_id = ?1", params![doc_id]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM grants WHERE doc_id = ?1", params![doc_id]).map_err(|e| e.to_string())?;
    conn.execute("DELETE FROM documents WHERE doc_id = ?1", params![doc_id]).map_err(|e| e.to_string())?;
    Ok(())
}

/// Grant or withhold one document for one AI. Withholding the last grant
/// removes the document from the library (nothing could read it anyway).
#[tauri::command]
pub fn corpus_grant(
    app: AppHandle,
    doc_id: String,
    ai_id: String,
    on: bool,
) -> Result<(), String> {
    let conn = open(&app)?;
    if on {
        conn.execute("INSERT OR IGNORE INTO grants (doc_id, ai_id) VALUES (?1, ?2)", params![doc_id, ai_id])
            .map_err(|e| e.to_string())?;
    } else {
        conn.execute("DELETE FROM grants WHERE doc_id = ?1 AND ai_id = ?2", params![doc_id, ai_id])
            .map_err(|e| e.to_string())?;
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM grants WHERE doc_id = ?1", params![doc_id], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        if left == 0 {
            delete_document(&conn, &doc_id)?;
            cache_invalidate();
        }
    }
    Ok(())
}

/// Remove a document from the library for every AI.
#[tauri::command]
pub fn corpus_delete(app: AppHandle, doc_id: String) -> Result<(), String> {
    let conn = open(&app)?;
    delete_document(&conn, &doc_id)?;
    cache_invalidate();
    Ok(())
}

fn update_meta<F: FnOnce(&mut DocMeta)>(app: &AppHandle, key: &[u8; 32], doc_id: &str, f: F) -> Result<DocMeta, String> {
    let conn = open(app)?;
    let blob: Vec<u8> = conn
        .query_row("SELECT meta_enc FROM documents WHERE doc_id = ?1", params![doc_id], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    let mut meta: DocMeta = serde_json::from_slice(&dec(key, &blob)?).map_err(|e| e.to_string())?;
    f(&mut meta);
    let json = serde_json::to_vec(&meta).map_err(|e| e.to_string())?;
    conn.execute("UPDATE documents SET meta_enc = ?1 WHERE doc_id = ?2", params![enc(key, &json)?, doc_id])
        .map_err(|e| e.to_string())?;
    Ok(meta)
}

/// Flip "written by me".
#[tauri::command]
pub fn corpus_set_mine(
    app: AppHandle,
    hc_state: State<'_, Arc<HolochainState>>,
    doc_id: String,
    mine: bool,
) -> Result<DocMeta, String> {
    let key = data_key(&hc_state)?;
    update_meta(&app, &key, &doc_id, |m| m.mine = mine)
}

/// Store a summary written on the device.
#[tauri::command]
pub fn corpus_set_summary(
    app: AppHandle,
    hc_state: State<'_, Arc<HolochainState>>,
    doc_id: String,
    summary: Option<String>,
) -> Result<DocMeta, String> {
    let key = data_key(&hc_state)?;
    update_meta(&app, &key, &doc_id, |m| m.summary = summary)
}

// ---------------------------------------------------------------- recall + whole document

/// The passages one AI may draw on that best match a question, scored by
/// meaning. Grouped so the caller can name the documents; no relative
/// margin, so a document's further passages are not dropped for scoring
/// below its best one.
#[tauri::command]
pub fn corpus_recall(
    app: AppHandle,
    hc_state: State<'_, Arc<HolochainState>>,
    ai_id: String,
    query: Vec<f32>,
    max_passages: usize,
) -> Result<Vec<RecallHit>, String> {
    if query.is_empty() || max_passages == 0 {
        return Ok(vec![]);
    }
    let key = data_key(&hc_state)?;
    let conn = open(&app)?;
    let mut g = conn.prepare("SELECT doc_id FROM grants WHERE ai_id = ?1").map_err(|e| e.to_string())?;
    let allowed: std::collections::HashSet<String> = g
        .query_map(params![ai_id], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?
        .flatten()
        .collect();
    if allowed.is_empty() {
        return Ok(vec![]);
    }
    let cache = cache_load(&conn, &key)?;
    let mut scored: Vec<(f32, &CachedVec)> = cache
        .iter()
        .filter(|c| allowed.contains(&c.doc_id))
        .map(|c| (cosine(&query, &c.vec), c))
        .filter(|(s, _)| *s >= RECALL_THRESHOLD)
        .collect();
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let above = scored.len();
    scored.truncate(max_passages);
    // Counts and scores only - never a filename or a passage in the log.
    let docs: std::collections::HashSet<&str> = scored.iter().map(|(_, c)| c.doc_id.as_str()).collect();
    log::info!(
        "[corpus] recall for AI {}: {} passage(s) from {} document(s) (of {} above {:.2}; top {:.2}; {} granted, {} in cache)",
        &ai_id[..8.min(ai_id.len())],
        scored.len(),
        docs.len(),
        above,
        RECALL_THRESHOLD,
        scored.first().map(|s| s.0).unwrap_or(0.0),
        allowed.len(),
        cache.len()
    );
    let mut out = Vec::with_capacity(scored.len());
    for (score, c) in scored {
        let text_blob: Vec<u8> = conn
            .query_row(
                "SELECT text_enc FROM passages WHERE doc_id = ?1 AND idx = ?2",
                params![c.doc_id, c.idx as i64],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let meta_blob: Vec<u8> = conn
            .query_row("SELECT meta_enc FROM documents WHERE doc_id = ?1", params![c.doc_id], |r| r.get(0))
            .map_err(|e| e.to_string())?;
        let meta: DocMeta = serde_json::from_slice(&dec(&key, &meta_blob)?).map_err(|e| e.to_string())?;
        out.push(RecallHit {
            doc_id: c.doc_id.clone(),
            filename: meta.filename,
            idx: c.idx,
            text: String::from_utf8_lossy(&dec(&key, &text_blob)?).into_owned(),
            score,
            mine: meta.mine,
        });
    }
    Ok(out)
}

#[derive(Serialize)]
pub struct DocumentText {
    pub doc_id: String,
    pub filename: String,
    pub text: String,
    pub truncated: bool,
    pub chunk_count: i64,
}

/// The whole document, reassembled from its passages in order with the
/// overlap removed, capped at `max_chars` (the caller sizes it to the
/// model's reading room).
#[tauri::command]
pub fn corpus_document_text(
    app: AppHandle,
    hc_state: State<'_, Arc<HolochainState>>,
    doc_id: String,
    max_chars: usize,
) -> Result<DocumentText, String> {
    let key = data_key(&hc_state)?;
    let conn = open(&app)?;
    let (meta_blob, chunk_count): (Vec<u8>, i64) = conn
        .query_row(
            "SELECT meta_enc, chunk_count FROM documents WHERE doc_id = ?1",
            params![doc_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .map_err(|e| e.to_string())?;
    let meta: DocMeta = serde_json::from_slice(&dec(&key, &meta_blob)?).map_err(|e| e.to_string())?;
    let mut stmt = conn
        .prepare("SELECT text_enc FROM passages WHERE doc_id = ?1 ORDER BY idx")
        .map_err(|e| e.to_string())?;
    let mut passages: Vec<String> = Vec::new();
    for row in stmt.query_map(params![doc_id], |r| r.get::<_, Vec<u8>>(0)).map_err(|e| e.to_string())? {
        let blob = row.map_err(|e| e.to_string())?;
        passages.push(String::from_utf8_lossy(&dec(&key, &blob)?).into_owned());
    }
    let mut text = join_passages(&passages);
    let mut truncated = false;
    if text.chars().count() > max_chars {
        text = text.chars().take(max_chars).collect();
        truncated = true;
    }
    Ok(DocumentText { doc_id, filename: meta.filename, text, truncated, chunk_count })
}

// ---------------------------------------------------------------- backup

/// What the Vault backup carries: the records and grants, never the
/// passages or vectors (re-derivable from the person's own files).
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct CorpusRecords {
    pub version: u32,
    pub documents: Vec<DocRecord>,
}

pub(crate) fn records_for_backup(app: &AppHandle, key: &[u8; 32]) -> Result<CorpusRecords, String> {
    let conn = open(app)?;
    let mut documents = read_records(&conn, key, None)?;
    for d in documents.iter_mut() {
        d.meta.path = None; // a path names a machine; the record should not
    }
    Ok(CorpusRecords { version: 1, documents })
}

/// Restore records from a backup: documents that are not here yet come back
/// with their summary, flags and grants and no passages ("Re-read from
/// folder" brings those back). Existing documents keep what they have.
pub(crate) fn restore_records(app: &AppHandle, key: &[u8; 32], records: &CorpusRecords) -> Result<usize, String> {
    let mut conn = open(app)?;
    let mut restored = 0usize;
    for d in &records.documents {
        let exists: Option<String> = conn
            .query_row("SELECT doc_id FROM documents WHERE doc_id = ?1", params![d.doc_id], |r| r.get(0))
            .optional()
            .map_err(|e| e.to_string())?;
        let tx = conn.transaction().map_err(|e| e.to_string())?;
        if exists.is_none() {
            let meta_json = serde_json::to_vec(&d.meta).map_err(|e| e.to_string())?;
            tx.execute(
                "INSERT INTO documents (doc_id, added_at, byte_size, chunk_count, path_hash, meta_enc) VALUES (?1, ?2, ?3, 0, NULL, ?4)",
                params![d.doc_id, d.added_at, d.byte_size, enc(key, &meta_json)?],
            )
            .map_err(|e| e.to_string())?;
            restored += 1;
        }
        for ai in &d.ai_ids {
            tx.execute("INSERT OR IGNORE INTO grants (doc_id, ai_id) VALUES (?1, ?2)", params![d.doc_id, ai])
                .map_err(|e| e.to_string())?;
        }
        tx.commit().map_err(|e| e.to_string())?;
    }
    Ok(restored)
}

// ---------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    #[test]
    fn xml_tag_text_reads_prefixed_tags() {
        let xml = r#"<?xml version="1.0"?><cp:coreProperties xmlns:dc="x"><dc:title>A &amp; B</dc:title><dc:creator>Eric Smith</dc:creator><dc:description/></cp:coreProperties>"#;
        assert_eq!(super::xml_tag_text(xml, "creator").as_deref(), Some("Eric Smith"));
        assert_eq!(super::xml_tag_text(xml, "title").as_deref(), Some("A & B"));
        assert_eq!(super::xml_tag_text(xml, "description"), None);
        assert_eq!(super::xml_tag_text(xml, "subject"), None);
    }

    #[test]
    fn looks_mine_matches_whole_words_only() {
        let names = vec!["Eric".to_string(), "ericflowsta".to_string(), "ab".to_string()];
        assert!(super::looks_mine("Eric Smith", &names));
        assert!(super::looks_mine("eric", &names));
        assert!(super::looks_mine("E. Smith, ericflowsta", &names));
        assert!(!super::looks_mine("Frederic Jones", &names));
        assert!(!super::looks_mine("Hannah", &["ann".to_string()]));
        assert!(!super::looks_mine("Abner", &names));
        assert!(!super::looks_mine("", &names));
    }

    #[test]
    fn pdf_info_strings_decode() {
        let raw = b"%PDF-1.4\n1 0 obj << /Title (My \\(quoted\\) Book) /Author (Eric Smith) >> endobj";
        assert_eq!(super::pdf_info_string(raw, b"/Author").as_deref(), Some("Eric Smith"));
        assert_eq!(super::pdf_info_string(raw, b"/Title").as_deref(), Some("My (quoted) Book"));
        let utf16 = b"/Author (\xFE\xFF\x00E\x00r\x00i\x00c)";
        assert_eq!(super::pdf_info_string(utf16, b"/Author").as_deref(), Some("Eric"));
        assert_eq!(super::pdf_info_string(b"/Author <41>", b"/Author"), None);
    }

    #[test]
    fn epub_reads_spine_order_and_metadata() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("yoai-epub-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("book.epub");
        {
            let f = std::fs::File::create(&path).unwrap();
            let mut zip = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default();
            let mut put = |name: &str, body: &str| {
                zip.start_file(name, opts).unwrap();
                zip.write_all(body.as_bytes()).unwrap();
            };
            put("META-INF/container.xml", r#"<container><rootfiles><rootfile full-path="OEBPS/content.opf" media-type="application/oebps-package+xml"/></rootfiles></container>"#);
            put("OEBPS/content.opf", r#"<package xmlns:dc="http://purl.org/dc/elements/1.1/"><metadata><dc:title>Tides</dc:title><dc:creator opf:role="aut">Eric Smith</dc:creator></metadata><manifest><item id="two" href="ch2.xhtml" media-type="application/xhtml+xml"/><item id="one" href="ch1.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="one"/><itemref idref="two"/></spine></package>"#);
            put("OEBPS/ch2.xhtml", "<html><body><p>Second chapter.</p></body></html>");
            put("OEBPS/ch1.xhtml", "<html><head><style>p{}</style></head><body><h1>First</h1><p>chapter one.</p></body></html>");
            zip.finish().unwrap();
        }
        let text = super::extract_epub_text(&path).unwrap();
        assert!(text.starts_with("First chapter one."), "{text}");
        assert!(text.contains("Second chapter."));
        let info = super::doc_info(&path);
        assert_eq!(info.author.as_deref(), Some("Eric Smith"));
        assert_eq!(info.title.as_deref(), Some("Tides"));
        std::fs::remove_dir_all(&dir).ok();
    }

    use super::*;

    #[test]
    fn chunks_keep_paragraphs_and_overlap() {
        let text = "First para sentence one. First para sentence two.\n\nSecond para is here and it runs quite a bit longer than the first one did. It has two sentences.";
        let c = chunk_text(text, 100);
        assert!(c.len() >= 2, "{c:?}");
        assert!(c[0].starts_with("First para"));
        // the second passage begins with the first's last sentence
        assert!(c[1].starts_with("First para sentence two."), "{}", c[1]);
        let joined = join_passages(&c);
        assert!(joined.contains("Second para is here and it runs"));
        assert_eq!(joined.matches("First para sentence two.").count(), 1);
    }

    #[test]
    fn long_sentence_is_hard_split_and_empty_is_empty() {
        let long = "a".repeat(2500);
        let c = chunk_text(&long, 900);
        assert_eq!(c.len(), 3);
        assert!(chunk_text("   \n\n  ", 900).is_empty());
    }

    #[test]
    fn walk_filters_and_recurses() {
        let dir = std::env::temp_dir().join(format!("corpus-walk-{}", new_doc_id()));
        std::fs::create_dir_all(dir.join("sub/node_modules")).unwrap();
        std::fs::create_dir_all(dir.join(".hidden")).unwrap();
        std::fs::write(dir.join("a.md"), "x").unwrap();
        std::fs::write(dir.join("b.exe"), "x").unwrap();
        std::fs::write(dir.join("sub/c.pdf"), "x").unwrap();
        std::fs::write(dir.join("sub/node_modules/d.md"), "x").unwrap();
        std::fs::write(dir.join(".hidden/e.md"), "x").unwrap();
        let files = walk(&[dir.to_string_lossy().to_string()]);
        let names: Vec<String> = files.iter().map(|p| p.file_name().unwrap().to_string_lossy().to_string()).collect();
        assert_eq!(names, vec!["a.md", "c.pdf"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn vectors_roundtrip_and_cosine() {
        let v = vec![0.5f32, -1.25, 3.0];
        assert_eq!(bytes_to_vec(&vec_to_bytes(&v)), v);
        assert!((cosine(&v, &v) - 1.0).abs() < 1e-6);
        assert_eq!(cosine(&v, &[1.0, 2.0]), 0.0);
    }

    #[test]
    fn store_roundtrip_recall_and_grants() {
        let path = std::env::temp_dir().join(format!("corpus-{}.sqlite", new_doc_id()));
        let mut conn = open_at(&path).unwrap();
        let key = [7u8; 32];
        let meta = DocMeta { filename: "book.txt".into(), path: Some("/tmp/book.txt".into()), ..Default::default() };
        let passages = vec!["the sky is blue because of scattering".to_string(), "the sea is salty from rivers".to_string()];
        let vectors = vec![vec![1.0f32, 0.0, 0.0], vec![0.0f32, 1.0, 0.0]];
        let rec = insert_document(&mut conn, &key, &meta, 42, &passages, &vectors, "ai-1").unwrap();
        assert_eq!(rec.chunk_count, 2);
        let recs = read_records(&conn, &key, Some("ai-1")).unwrap();
        assert_eq!(recs.len(), 1);
        assert_eq!(recs[0].meta.filename, "book.txt");
        assert!(read_records(&conn, &key, Some("ai-2")).unwrap().is_empty());
        assert_eq!(already_have(&conn, "/tmp/book.txt").unwrap().as_deref(), Some(rec.doc_id.as_str()));
        // recall through the cache: the sky question finds the sky passage first
        cache_invalidate();
        let cache = cache_load(&conn, &key).unwrap();
        assert_eq!(cache.len(), 2);
        let best = cache.iter().map(|c| (cosine(&[0.9, 0.1, 0.0], &c.vec), c.idx)).fold((0.0, 9), |a, b| if b.0 > a.0 { b } else { a });
        assert_eq!(best.1, 0);
        // records for backup carry no path; restore into a fresh store keeps grants
        let backup = CorpusRecords { version: 1, documents: recs.clone().into_iter().map(|mut d| { d.meta.path = None; d }).collect() };
        let path2 = std::env::temp_dir().join(format!("corpus-{}.sqlite", new_doc_id()));
        let conn2 = open_at(&path2).unwrap();
        drop(conn2);
        // restore_records needs an AppHandle in the real app; exercise the SQL directly here
        let mut c2 = open_at(&path2).unwrap();
        let d = &backup.documents[0];
        let meta_json = serde_json::to_vec(&d.meta).unwrap();
        let tx = c2.transaction().unwrap();
        tx.execute("INSERT INTO documents (doc_id, added_at, byte_size, chunk_count, path_hash, meta_enc) VALUES (?1, ?2, ?3, 0, NULL, ?4)", params![d.doc_id, d.added_at, d.byte_size, enc(&key, &meta_json).unwrap()]).unwrap();
        tx.execute("INSERT OR IGNORE INTO grants (doc_id, ai_id) VALUES (?1, ?2)", params![d.doc_id, "ai-1"]).unwrap();
        tx.commit().unwrap();
        let back = read_records(&c2, &key, Some("ai-1")).unwrap();
        assert_eq!(back[0].meta.filename, "book.txt");
        assert!(back[0].meta.path.is_none());
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&path2);
        cache_invalidate();
    }
}
