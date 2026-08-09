//! Conversation import, stage 1: parse an exported-chat file (ChatGPT,
//! Claude, Perplexity - JSON or the export ZIP), normalize it, and store it
//! as an encrypted local archive with an instant summary.
//!
//! Stage 2 (the background distiller that turns archives into memory and
//! chain records) builds on the archive this module writes. Detection is
//! typed try-parse: each source's struct requires its discriminant field
//! (`mapping` for ChatGPT, `chat_messages` for Claude), so the wrong parser
//! fails on the first element instead of mis-reading the file.

use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

/// Plaintext manifest holds ONLY metadata (counts, source, dates) - never
/// conversation content. Content lives in the per-import encrypted blobs.
const MANIFEST_FILE: &str = "manifest.json";
const IMPORTS_DIR: &str = "imports";
/// Refuse files beyond this size rather than exhausting RAM on low-spec
/// machines - typed parsing still holds the parsed form in memory.
const MAX_IMPORT_BYTES: u64 = 512 * 1024 * 1024;

// ── Normalized shapes ──────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedMessage {
    pub role: String, // "user" | "assistant"
    pub text: String,
    /// Original timestamp in microseconds, when the export carried one.
    pub ts_us: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportedConversation {
    pub source_id: Option<String>,
    pub title: String,
    pub created_at_us: Option<i64>,
    pub messages: Vec<ImportedMessage>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportArchive {
    pub id: String,
    /// "chatgpt" | "claude" | "perplexity"
    pub source: String,
    pub file_name: String,
    pub imported_at_us: i64,
    pub conversations: Vec<ImportedConversation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportSummary {
    pub archive_id: String,
    pub source: String,
    pub file_name: String,
    pub conversation_count: usize,
    pub message_count: usize,
    pub earliest_us: Option<i64>,
    pub latest_us: Option<i64>,
    pub imported_at_us: i64,
    /// AIs whose chains hold this archive's conversations (adoption).
    #[serde(default)]
    pub adopted_by: Vec<AdoptedBy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdoptedBy {
    pub ai_id: String,
    pub ai_name: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct Manifest {
    archives: Vec<ImportSummary>,
}

/// Same encrypted-blob shape as the memory stores (memory.rs,
/// transcript_memory.rs) - hex nonce + hex cipher around serialized JSON.
#[derive(Serialize, Deserialize)]
struct EncryptedArchiveFile {
    version: u32,
    nonce: String,
    cipher: String,
}

// ── Source formats (typed, discriminant fields required) ───────────────

/// ChatGPT: array of conversations, each a TREE in `mapping` (edits and
/// regenerations branch; the live thread is the last-child path).
#[derive(Deserialize)]
struct ChatGptConv {
    title: Option<String>,
    create_time: Option<f64>,
    mapping: HashMap<String, ChatGptNode>,
    #[serde(alias = "conversation_id")]
    id: Option<String>,
}

#[derive(Deserialize)]
struct ChatGptNode {
    message: Option<ChatGptMessage>,
    parent: Option<String>,
    #[serde(default)]
    children: Vec<String>,
}

#[derive(Deserialize)]
struct ChatGptMessage {
    author: ChatGptAuthor,
    create_time: Option<f64>,
    content: Option<ChatGptContent>,
}

#[derive(Deserialize)]
struct ChatGptAuthor {
    role: String,
}

#[derive(Deserialize)]
struct ChatGptContent {
    #[serde(default)]
    parts: Option<Vec<serde_json::Value>>,
}

/// Claude: array of conversations with a flat `chat_messages` list.
#[derive(Deserialize)]
struct ClaudeConv {
    uuid: Option<String>,
    name: Option<String>,
    created_at: Option<String>,
    chat_messages: Vec<ClaudeMsg>,
}

#[derive(Deserialize)]
struct ClaudeMsg {
    sender: String, // "human" | "assistant"
    #[serde(default)]
    text: String,
    created_at: Option<String>,
    /// Newer exports carry a content array alongside (or instead of) `text`.
    #[serde(default)]
    content: Vec<ClaudeContentSegment>,
}

#[derive(Deserialize)]
struct ClaudeContentSegment {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
}

/// Perplexity: account export thread shapes vary; accept both a bare array
/// of threads and a `{"threads": [...]}` wrapper. Harden against real files
/// as they arrive - detection requires the `messages`/`entries` field so
/// other sources' files can't mis-parse here.
#[derive(Deserialize)]
struct PerplexityWrapper {
    threads: Vec<PerplexityThread>,
}

#[derive(Deserialize)]
struct PerplexityThread {
    title: Option<String>,
    slug: Option<String>,
    created_at: Option<serde_json::Value>,
    #[serde(alias = "entries", alias = "chat_messages")]
    messages: Vec<PerplexityMsg>,
}

#[derive(Deserialize)]
struct PerplexityMsg {
    #[serde(alias = "sender", alias = "author")]
    role: Option<String>,
    #[serde(alias = "content", alias = "query", alias = "answer")]
    text: Option<String>,
    #[serde(alias = "timestamp")]
    created_at: Option<serde_json::Value>,
}

// ── Parsing ────────────────────────────────────────────────────────────

fn normalize_role(raw: &str) -> Option<&'static str> {
    match raw.to_ascii_lowercase().as_str() {
        "user" | "human" => Some("user"),
        "assistant" | "ai" | "model" => Some("assistant"),
        _ => None, // system / tool / unknown - skipped
    }
}

fn seconds_to_micros(s: f64) -> Option<i64> {
    if s <= 0.0 {
        return None;
    }
    Some((s * 1_000_000.0) as i64)
}

/// Flexible timestamp: ISO-8601 string, unix seconds, or unix millis.
fn flexible_ts(v: &serde_json::Value) -> Option<i64> {
    match v {
        serde_json::Value::String(s) => iso_to_micros(s),
        serde_json::Value::Number(n) => {
            let f = n.as_f64()?;
            if f > 1e14 {
                Some(f as i64) // already micros
            } else if f > 1e11 {
                Some((f * 1_000.0) as i64) // millis
            } else {
                seconds_to_micros(f)
            }
        }
        _ => None,
    }
}

/// Days-from-civil (Howard Hinnant) - inverse of the civil-from-days in
/// diagnostics.rs; keeps us free of a date-crate dependency.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = if m > 2 { m - 3 } else { m + 9 };
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// "2024-05-01T12:34:56.789Z" / "...+02:00" / date-only -> microseconds UTC.
fn iso_to_micros(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.len() < 10 {
        return None;
    }
    let y: i64 = s.get(0..4)?.parse().ok()?;
    let m: i64 = s.get(5..7)?.parse().ok()?;
    let d: i64 = s.get(8..10)?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let mut micros = days_from_civil(y, m, d) * 86_400 * 1_000_000;
    let rest = &s[10..];
    if let Some(t) = rest.strip_prefix('T').or_else(|| rest.strip_prefix(' ')) {
        let hh: i64 = t.get(0..2).and_then(|x| x.parse().ok()).unwrap_or(0);
        let mm: i64 = t.get(3..5).and_then(|x| x.parse().ok()).unwrap_or(0);
        let ss: i64 = t.get(6..8).and_then(|x| x.parse().ok()).unwrap_or(0);
        micros += (hh * 3600 + mm * 60 + ss) * 1_000_000;
        // Fractional seconds.
        if let Some(frac_start) = t.find('.') {
            let frac: String = t[frac_start + 1..]
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if !frac.is_empty() {
                let scale = 10i64.pow(6u32.saturating_sub(frac.len() as u32));
                let val: i64 = frac.get(..6.min(frac.len()))?.parse().ok()?;
                micros += val * scale.max(1);
            }
        }
        // Zone offset: Z = UTC; +HH:MM / -HH:MM shifts back to UTC.
        if let Some(pos) = t.rfind(['+', '-']) {
            // Only treat as an offset if it comes after the time part.
            if pos >= 8 {
                let sign = if t.as_bytes()[pos] == b'+' { 1 } else { -1 };
                let oh: i64 = t.get(pos + 1..pos + 3).and_then(|x| x.parse().ok()).unwrap_or(0);
                let om: i64 = t.get(pos + 4..pos + 6).and_then(|x| x.parse().ok()).unwrap_or(0);
                micros -= sign * (oh * 3600 + om * 60) * 1_000_000;
            }
        }
    }
    Some(micros)
}

/// Walk a ChatGPT conversation tree along the active (last-child) branch.
fn parse_chatgpt_conv(conv: ChatGptConv) -> ImportedConversation {
    // Root: the node with no parent (or whose parent isn't in the map).
    let root = conv
        .mapping
        .iter()
        .find(|(_, n)| {
            n.parent.is_none() || !conv.mapping.contains_key(n.parent.as_deref().unwrap_or(""))
        })
        .map(|(id, _)| id.clone());

    let mut messages = Vec::new();
    let mut current = root;
    let mut guard = 0usize;
    while let Some(id) = current {
        guard += 1;
        if guard > conv.mapping.len() + 1 {
            break; // cycle guard - malformed mapping must not hang the import
        }
        let Some(node) = conv.mapping.get(&id) else { break };
        if let Some(msg) = &node.message {
            if let Some(role) = normalize_role(&msg.author.role) {
                let text = msg
                    .content
                    .as_ref()
                    .and_then(|c| c.parts.as_ref())
                    .map(|parts| {
                        parts
                            .iter()
                            .filter_map(|p| p.as_str())
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .unwrap_or_default();
                let text = text.trim();
                if !text.is_empty() {
                    messages.push(ImportedMessage {
                        role: role.to_string(),
                        text: text.to_string(),
                        ts_us: msg.create_time.and_then(seconds_to_micros),
                    });
                }
            }
        }
        // Active branch: the LAST child (edits/regens append; not guaranteed,
        // but the standard heuristic for these exports).
        current = node.children.last().cloned();
    }

    ImportedConversation {
        source_id: conv.id,
        title: conv.title.unwrap_or_else(|| "Untitled".to_string()),
        created_at_us: conv.create_time.and_then(seconds_to_micros),
        messages,
    }
}

fn parse_claude_conv(conv: ClaudeConv) -> ImportedConversation {
    let messages = conv
        .chat_messages
        .into_iter()
        .filter_map(|m| {
            let role = normalize_role(&m.sender)?;
            let mut text = m.text.trim().to_string();
            if text.is_empty() {
                text = m
                    .content
                    .iter()
                    .filter(|s| s.kind.as_deref() != Some("thinking"))
                    .filter_map(|s| s.text.as_deref())
                    .collect::<Vec<_>>()
                    .join("\n")
                    .trim()
                    .to_string();
            }
            if text.is_empty() {
                return None;
            }
            Some(ImportedMessage {
                role: role.to_string(),
                text,
                ts_us: m.created_at.as_deref().and_then(iso_to_micros),
            })
        })
        .collect();
    ImportedConversation {
        source_id: conv.uuid,
        title: conv
            .name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| "Untitled".to_string()),
        created_at_us: conv.created_at.as_deref().and_then(iso_to_micros),
        messages,
    }
}

fn parse_perplexity_thread(t: PerplexityThread) -> ImportedConversation {
    let messages = t
        .messages
        .into_iter()
        .filter_map(|m| {
            let role = normalize_role(m.role.as_deref().unwrap_or(""))?;
            let text = m.text.unwrap_or_default().trim().to_string();
            if text.is_empty() {
                return None;
            }
            Some(ImportedMessage {
                role: role.to_string(),
                text,
                ts_us: m.created_at.as_ref().and_then(flexible_ts),
            })
        })
        .collect();
    ImportedConversation {
        source_id: t.slug,
        title: t
            .title
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| "Untitled".to_string()),
        created_at_us: t.created_at.as_ref().and_then(flexible_ts),
        messages,
    }
}

/// Detect the source of a JSON text and parse it. Typed try-parse order:
/// each candidate requires its discriminant field, so wrong sources fail
/// fast on the first element.
pub fn detect_and_parse(text: &str) -> Result<(String, Vec<ImportedConversation>), String> {
    if let Ok(convs) = serde_json::from_str::<Vec<ClaudeConv>>(text) {
        if !convs.is_empty() {
            return Ok((
                "claude".to_string(),
                convs.into_iter().map(parse_claude_conv).collect(),
            ));
        }
    }
    if let Ok(convs) = serde_json::from_str::<Vec<ChatGptConv>>(text) {
        if !convs.is_empty() {
            return Ok((
                "chatgpt".to_string(),
                convs.into_iter().map(parse_chatgpt_conv).collect(),
            ));
        }
    }
    if let Ok(w) = serde_json::from_str::<PerplexityWrapper>(text) {
        if !w.threads.is_empty() {
            return Ok((
                "perplexity".to_string(),
                w.threads.into_iter().map(parse_perplexity_thread).collect(),
            ));
        }
    }
    if let Ok(threads) = serde_json::from_str::<Vec<PerplexityThread>>(text) {
        if !threads.is_empty() {
            return Ok((
                "perplexity".to_string(),
                threads.into_iter().map(parse_perplexity_thread).collect(),
            ));
        }
    }
    Err("This file isn't a recognized export. Supported today: ChatGPT, Claude, and Perplexity data exports (the .zip or the conversations .json inside it).".to_string())
}

// ── File handling ──────────────────────────────────────────────────────

/// Read the import file: bare JSON, or a ZIP scanned for parseable JSON
/// entries (largest first - conversations.json dwarfs the metadata files).
fn read_import_file(path: &PathBuf) -> Result<(String, Vec<ImportedConversation>), String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("Could not open the file: {e}"))?;
    if meta.len() > MAX_IMPORT_BYTES {
        return Err("This file is larger than 512MB. Extract the ZIP and pick the conversations .json inside it.".to_string());
    }
    let is_zip = path
        .extension()
        .map(|e| e.eq_ignore_ascii_case("zip"))
        .unwrap_or(false);
    if !is_zip {
        let text = std::fs::read_to_string(path).map_err(|e| format!("Could not read the file: {e}"))?;
        return detect_and_parse(&text);
    }

    let file = std::fs::File::open(path).map_err(|e| format!("Could not open the ZIP: {e}"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("Not a readable ZIP: {e}"))?;
    // Collect JSON entry names, largest first.
    let mut candidates: Vec<(String, u64)> = (0..archive.len())
        .filter_map(|i| {
            let entry = archive.by_index(i).ok()?;
            let name = entry.name().to_string();
            if name.to_ascii_lowercase().ends_with(".json") && !name.starts_with("__MACOSX") {
                Some((name, entry.size()))
            } else {
                None
            }
        })
        .collect();
    candidates.sort_by(|a, b| b.1.cmp(&a.1));
    if candidates.is_empty() {
        return Err("No JSON files found inside this ZIP.".to_string());
    }
    let mut last_err = String::new();
    for (name, size) in candidates {
        if size > MAX_IMPORT_BYTES {
            continue;
        }
        let mut entry = archive
            .by_name(&name)
            .map_err(|e| format!("Could not read {name}: {e}"))?;
        let mut text = String::new();
        if entry.read_to_string(&mut text).is_err() {
            continue;
        }
        match detect_and_parse(&text) {
            Ok(parsed) => return Ok(parsed),
            Err(e) => last_err = e,
        }
    }
    Err(if last_err.is_empty() {
        "No recognizable conversation export found inside this ZIP.".to_string()
    } else {
        last_err
    })
}

// ── Archive store ──────────────────────────────────────────────────────

fn imports_dir(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| e.to_string())?
        .join(IMPORTS_DIR);
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir)
}

fn data_key(app: &AppHandle) -> Result<[u8; 32], String> {
    let app_data = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let material = crate::transcript_crypto::load_recovery_material(&app_data)?
        .ok_or_else(|| "Encryption key not initialized yet - open the app fully once first.".to_string())?;
    material.data_key()
}

fn load_manifest(app: &AppHandle) -> Result<Manifest, String> {
    let path = imports_dir(app)?.join(MANIFEST_FILE);
    if !path.exists() {
        return Ok(Manifest::default());
    }
    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
    serde_json::from_str(&text).map_err(|e| e.to_string())
}

fn save_manifest(app: &AppHandle, manifest: &Manifest) -> Result<(), String> {
    let path = imports_dir(app)?.join(MANIFEST_FILE);
    let text = serde_json::to_string_pretty(manifest).map_err(|e| e.to_string())?;
    std::fs::write(&path, text).map_err(|e| e.to_string())
}

fn now_micros() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

fn summarize(archive: &ImportArchive) -> ImportSummary {
    let mut earliest: Option<i64> = None;
    let mut latest: Option<i64> = None;
    let mut message_count = 0usize;
    for c in &archive.conversations {
        message_count += c.messages.len();
        for ts in c
            .created_at_us
            .iter()
            .chain(c.messages.iter().filter_map(|m| m.ts_us.as_ref()))
        {
            earliest = Some(earliest.map_or(*ts, |e: i64| e.min(*ts)));
            latest = Some(latest.map_or(*ts, |l: i64| l.max(*ts)));
        }
    }
    ImportSummary {
        archive_id: archive.id.clone(),
        source: archive.source.clone(),
        file_name: archive.file_name.clone(),
        conversation_count: archive.conversations.len(),
        message_count,
        earliest_us: earliest,
        latest_us: latest,
        imported_at_us: archive.imported_at_us,
        adopted_by: Vec::new(),
    }
}

// ── Commands ───────────────────────────────────────────────────────────

/// Stage 1: parse the picked file, store the encrypted archive, return the
/// summary. Synchronous work measured in seconds, not minutes.
#[tauri::command]
pub async fn import_conversations_scan(app: AppHandle, path: String) -> Result<ImportSummary, String> {
    let path_buf = PathBuf::from(&path);
    let file_name = path_buf
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "export".to_string());

    // Parse on a blocking thread - big files shouldn't stall the runtime.
    let (source, mut conversations) =
        tauri::async_runtime::spawn_blocking(move || read_import_file(&path_buf))
            .await
            .map_err(|e| e.to_string())??;

    // Drop empty conversations; an export full of empty threads is noise.
    conversations.retain(|c| !c.messages.is_empty());
    if conversations.is_empty() {
        return Err("The export was recognized but contains no conversations with messages.".to_string());
    }

    let archive = ImportArchive {
        id: uuid_v4(),
        source,
        file_name,
        imported_at_us: now_micros(),
        conversations,
    };

    let key = data_key(&app)?;
    let plain = serde_json::to_vec(&archive).map_err(|e| e.to_string())?;
    let (nonce, cipher) = crate::transcript_crypto::encrypt(&key, &plain)?;
    let blob = EncryptedArchiveFile {
        version: 1,
        nonce: hex::encode(nonce),
        cipher: hex::encode(cipher),
    };
    let path_out = imports_dir(&app)?.join(format!("import-{}.enc", archive.id));
    std::fs::write(&path_out, serde_json::to_string(&blob).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path_out, std::fs::Permissions::from_mode(0o600));
    }

    let summary = summarize(&archive);
    let mut manifest = load_manifest(&app)?;
    manifest.archives.insert(0, summary.clone());
    save_manifest(&app, &manifest)?;
    log::info!(
        "[import] archived {} conversations ({} messages) from {}",
        summary.conversation_count,
        summary.message_count,
        summary.source
    );
    Ok(summary)
}

/// The imported archives, newest first (metadata only - no content).
#[tauri::command]
pub fn import_archives_list(app: AppHandle) -> Result<Vec<ImportSummary>, String> {
    Ok(load_manifest(&app)?.archives)
}

fn load_archive(app: &AppHandle, archive_id: &str) -> Result<ImportArchive, String> {
    let path = imports_dir(app)?.join(format!("import-{archive_id}.enc"));
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("Could not read the archive: {e}"))?;
    let blob: EncryptedArchiveFile =
        serde_json::from_str(&text).map_err(|e| format!("Archive file is malformed: {e}"))?;
    let key = data_key(app)?;
    let nonce = hex::decode(&blob.nonce).map_err(|e| e.to_string())?;
    let cipher = hex::decode(&blob.cipher).map_err(|e| e.to_string())?;
    let plain = crate::transcript_crypto::decrypt(&key, &nonce, &cipher)?;
    serde_json::from_slice(&plain).map_err(|e| format!("Archive content is malformed: {e}"))
}

/// Decrypt and return one archive's full content - the stage-2 distiller
/// reads conversations through this.
#[tauri::command]
pub fn import_archive_get(app: AppHandle, archive_id: String) -> Result<ImportArchive, String> {
    load_archive(&app, &archive_id)
}

/// Delete an imported archive (its encrypted blob + manifest row).
#[tauri::command]
pub fn import_archive_delete(app: AppHandle, archive_id: String) -> Result<(), String> {
    let dir = imports_dir(&app)?;
    let blob = dir.join(format!("import-{archive_id}.enc"));
    if blob.exists() {
        std::fs::remove_file(&blob).map_err(|e| e.to_string())?;
    }
    let mut manifest = load_manifest(&app)?;
    manifest.archives.retain(|a| a.archive_id != archive_id);
    save_manifest(&app, &manifest)
}

// ── Adoption (2b): write an archive onto a chosen AI's chain ───────────
//
// Follows vault_restore::replay_group: direct zome calls with caller-built
// encrypted plaintext, which is what lets original timestamps survive
// (the Tauri write commands stamp "now"). Serialized + head-moved-retried
// by HolochainManager::call_zome.

/// What the zome expects for start_conversation.
#[derive(Serialize, Debug)]
struct ZomeEncryptedInput {
    cipher: Vec<u8>,
    nonce: Vec<u8>,
}

/// What the zome expects for record_message.
#[derive(Serialize, Debug)]
struct ZomeRecordMessageInput {
    conversation_hash: holochain_types::prelude::ActionHash,
    cipher: Vec<u8>,
    nonce: Vec<u8>,
}

/// The zome-side encrypted entry, for decoding existing records.
#[derive(Deserialize)]
struct EncryptedEntryRaw {
    cipher: Vec<u8>,
    nonce: Vec<u8>,
}

/// The integrity zome rejects ciphertext over 1 MiB; stay well under it.
/// A rejected write would lose the message mid-adoption.
const ADOPT_CONTENT_BUDGET: usize = 900_000;

fn bounded_content(text: &str) -> String {
    if text.len() <= ADOPT_CONTENT_BUDGET {
        return text.to_string();
    }
    let mut end = ADOPT_CONTENT_BUDGET;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}\n\n(Message truncated during import - the original exceeded the record size limit.)",
        &text[..end]
    )
}

/// Encrypt a plaintext JSON value and commit it through a transcript zome
/// function, returning the new record's action hash.
async fn commit_encrypted(
    manager: &crate::holochain::HolochainManager,
    agent_key: &str,
    key: &[u8; 32],
    fn_name: &str,
    conversation_hash: Option<holochain_types::prelude::ActionHash>,
    plain: &serde_json::Value,
) -> Result<holochain_types::prelude::ActionHash, String> {
    use holochain_types::prelude::ExternIO;
    let bytes = serde_json::to_vec(plain).map_err(|e| e.to_string())?;
    let (nonce, cipher) = crate::transcript_crypto::encrypt(key, &bytes)?;
    let payload = match conversation_hash {
        None => ExternIO::encode(ZomeEncryptedInput {
            cipher,
            nonce: nonce.to_vec(),
        }),
        Some(h) => ExternIO::encode(ZomeRecordMessageInput {
            conversation_hash: h,
            cipher,
            nonce: nonce.to_vec(),
        }),
    }
    .map_err(|e| e.to_string())?;
    let result = manager
        .call_zome(agent_key, "transcript", fn_name, payload)
        .await?;
    ExternIO::decode(&result).map_err(|e| e.to_string())
}

/// Adopt an archive into one AI's conversations: every conversation is
/// written onto that AI's chain with its ORIGINAL timestamps and an
/// "import:<source>" label. Re-running after an interruption completes the
/// missing conversations instead of duplicating finished ones (dedup on
/// started_at, the restore pattern).
#[tauri::command]
pub async fn import_archive_adopt(
    app: AppHandle,
    archive_id: String,
    ai_id: String,
    ai_name: String,
    hc_state: tauri::State<'_, std::sync::Arc<crate::commands_holochain::HolochainState>>,
) -> Result<u32, String> {
    use holochain_types::prelude::ExternIO;
    use tauri::Emitter;

    let manager = hc_state.get()?;
    let key = manager.data_key()?;
    let archive = load_archive(&app, &archive_id)?;

    // Existing started_at values on the adopting agent's chain.
    let mut existing_started: std::collections::HashSet<i64> = std::collections::HashSet::new();
    let payload = ExternIO::encode(()).map_err(|e| e.to_string())?;
    if let Ok(result) = manager
        .call_zome(&ai_id, "transcript", "get_all_conversations", payload)
        .await
    {
        if let Ok(records) = ExternIO::decode::<Vec<holochain_types::prelude::Record>>(&result) {
            for record in &records {
                let Some(entry) = record.entry().as_option() else { continue };
                let Some(app_bytes) = entry.as_app_entry() else { continue };
                let Ok(ee) = rmp_serde::from_slice::<EncryptedEntryRaw>(app_bytes.as_ref().bytes())
                else {
                    continue;
                };
                let Ok(plain) = crate::transcript_crypto::decrypt(&key, &ee.nonce, &ee.cipher)
                else {
                    continue;
                };
                if let Ok(v) = serde_json::from_slice::<serde_json::Value>(&plain) {
                    if let Some(s) = v["started_at"].as_i64() {
                        existing_started.insert(s);
                    }
                }
            }
        }
    }

    let total = archive.conversations.len();
    let mut written = 0u32;
    let mut last_pct = 101u32;
    for (idx, conv) in archive.conversations.iter().enumerate() {
        // Timestamp fallback chain: conversation -> first message -> import
        // time (+idx keeps fallback values unique within one archive).
        let started_at = conv
            .created_at_us
            .or_else(|| conv.messages.first().and_then(|m| m.ts_us))
            .unwrap_or(archive.imported_at_us + idx as i64);
        if existing_started.contains(&started_at) {
            continue;
        }

        let meta = serde_json::json!({
            "ai_personality_id": ai_id,
            "ai_personality_name": ai_name,
            "model_used": "imported",
            "started_at": started_at,
            "title": conv.title,
            "source": format!("import:{}", archive.source),
        });
        let conv_hash =
            commit_encrypted(manager, &ai_id, &key, "start_conversation", None, &meta).await?;

        for (seq, msg) in conv.messages.iter().enumerate() {
            let plain = serde_json::json!({
                "role": msg.role,
                "content": bounded_content(&msg.text),
                "sequence": seq as u32,
                "timestamp": msg.ts_us.unwrap_or(started_at),
                "model": "imported",
                "thinking": null,
                "tokens": null,
            });
            commit_encrypted(
                manager,
                &ai_id,
                &key,
                "record_message",
                Some(conv_hash.clone()),
                &plain,
            )
            .await?;
        }
        written += 1;

        let pct = (((idx + 1) * 100) / total.max(1)) as u32;
        if pct != last_pct {
            let _ = app.emit(
                "import-adopt-progress",
                serde_json::json!({
                    "archiveId": archive_id,
                    "aiId": ai_id,
                    "done": idx + 1,
                    "total": total,
                    "percent": pct,
                }),
            );
            last_pct = pct;
        }
    }

    // Record the adoption in the manifest (drives the card's state).
    let mut manifest = load_manifest(&app)?;
    if let Some(row) = manifest
        .archives
        .iter_mut()
        .find(|a| a.archive_id == archive_id)
    {
        if !row.adopted_by.iter().any(|a| a.ai_id == ai_id) {
            row.adopted_by.push(AdoptedBy {
                ai_id: ai_id.clone(),
                ai_name: ai_name.clone(),
            });
        }
    }
    save_manifest(&app, &manifest)?;

    // One backup refresh for the whole adoption, not one per write.
    crate::vault_escrow::schedule_full_backup(&app);
    log::info!(
        "[import] adopted {} conversation(s) from {} into {}",
        written,
        archive.source,
        ai_name
    );
    Ok(written)
}

/// Random v4-style UUID without a uuid-crate dependency.
fn uuid_v4() -> String {
    use rand::RngCore;
    let mut b = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut b);
    b[6] = (b[6] & 0x0f) | 0x40;
    b[8] = (b[8] & 0x3f) | 0x80;
    format!(
        "{:02x}{:02x}{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}-{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11], b[12], b[13], b[14], b[15]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn iso_parse_variants() {
        let z = iso_to_micros("2024-05-01T12:00:00Z").unwrap();
        let offset = iso_to_micros("2024-05-01T14:00:00+02:00").unwrap();
        assert_eq!(z, offset, "offset must normalize to the same UTC instant");
        let frac = iso_to_micros("2024-05-01T12:00:00.500Z").unwrap();
        assert_eq!(frac - z, 500_000);
        assert!(iso_to_micros("2024-05-01").is_some());
        assert!(iso_to_micros("garbage").is_none());
    }

    #[test]
    fn claude_export_parses() {
        let json = r#"[{
            "uuid": "abc",
            "name": "Trip planning",
            "created_at": "2025-01-02T03:04:05.000000Z",
            "chat_messages": [
                {"sender": "human", "text": "I live in Berlin", "created_at": "2025-01-02T03:04:05Z"},
                {"sender": "assistant", "text": "Nice!", "created_at": "2025-01-02T03:04:09Z"},
                {"sender": "human", "text": "", "created_at": null,
                 "content": [{"type": "text", "text": "From content array"}]}
            ]
        }]"#;
        let (source, convs) = detect_and_parse(json).unwrap();
        assert_eq!(source, "claude");
        assert_eq!(convs.len(), 1);
        assert_eq!(convs[0].title, "Trip planning");
        assert_eq!(convs[0].messages.len(), 3);
        assert_eq!(convs[0].messages[0].role, "user");
        assert_eq!(convs[0].messages[2].text, "From content array");
        assert!(convs[0].messages[0].ts_us.is_some());
    }

    #[test]
    fn chatgpt_tree_walks_active_branch() {
        // root -> a(user) -> [b(edited away), c(active)] ; c -> d(assistant)
        let json = r#"[{
            "title": "Tree test",
            "create_time": 1714000000.0,
            "id": "conv1",
            "mapping": {
                "root": {"message": null, "parent": null, "children": ["a"]},
                "a": {"message": {"author": {"role": "user"}, "create_time": 1714000001.0,
                       "content": {"parts": ["original question"]}},
                      "parent": "root", "children": ["b", "c"]},
                "b": {"message": {"author": {"role": "assistant"}, "create_time": 1714000002.0,
                       "content": {"parts": ["old answer"]}},
                      "parent": "a", "children": []},
                "c": {"message": {"author": {"role": "assistant"}, "create_time": 1714000003.0,
                       "content": {"parts": ["regenerated answer"]}},
                      "parent": "a", "children": ["d"]},
                "d": {"message": {"author": {"role": "user"}, "create_time": 1714000004.0,
                       "content": {"parts": ["follow-up"]}},
                      "parent": "c", "children": []}
            }
        }]"#;
        let (source, convs) = detect_and_parse(json).unwrap();
        assert_eq!(source, "chatgpt");
        let texts: Vec<&str> = convs[0].messages.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, vec!["original question", "regenerated answer", "follow-up"]);
        assert!(convs[0].messages[0].ts_us.is_some());
    }

    #[test]
    fn chatgpt_skips_system_and_null_nodes() {
        let json = r#"[{
            "title": null,
            "mapping": {
                "root": {"message": {"author": {"role": "system"},
                          "content": {"parts": [""]}}, "parent": null, "children": ["a"]},
                "a": {"message": {"author": {"role": "user"},
                       "content": {"parts": ["hello there"]}}, "parent": "root", "children": []}
            }
        }]"#;
        let (_, convs) = detect_and_parse(json).unwrap();
        assert_eq!(convs[0].messages.len(), 1);
        assert_eq!(convs[0].title, "Untitled");
    }

    #[test]
    fn chatgpt_cycle_does_not_hang() {
        let json = r#"[{
            "title": "cycle",
            "mapping": {
                "a": {"message": null, "parent": "b", "children": ["b"]},
                "b": {"message": null, "parent": "a", "children": ["a"]}
            }
        }]"#;
        let (_, convs) = detect_and_parse(json).unwrap();
        assert!(convs[0].messages.is_empty());
    }

    #[test]
    fn perplexity_shapes_parse() {
        let wrapped = r#"{"threads": [{
            "title": "Research",
            "slug": "research-1",
            "created_at": 1714000000,
            "messages": [
                {"role": "user", "content": "What is Holochain?", "timestamp": 1714000000},
                {"role": "assistant", "content": "An agent-centric framework.", "timestamp": 1714000005}
            ]
        }]}"#;
        let (source, convs) = detect_and_parse(wrapped).unwrap();
        assert_eq!(source, "perplexity");
        assert_eq!(convs[0].messages.len(), 2);
        assert!(convs[0].messages[0].ts_us.is_some());
    }

    #[test]
    fn unrecognized_is_a_clear_error() {
        let err = detect_and_parse(r#"{"foo": "bar"}"#).unwrap_err();
        assert!(err.contains("ChatGPT"));
    }

    #[test]
    fn detection_is_not_fooled_across_sources() {
        // A Claude file must not parse as ChatGPT or Perplexity, and vice versa.
        let claude = r#"[{"chat_messages": [{"sender": "human", "text": "hi"}]}]"#;
        assert_eq!(detect_and_parse(claude).unwrap().0, "claude");
        let chatgpt = r#"[{"mapping": {"r": {"message": null, "parent": null, "children": []}}}]"#;
        // No messages -> conversations retained only at command layer; detection still says chatgpt.
        assert_eq!(detect_and_parse(chatgpt).unwrap().0, "chatgpt");
    }
}
