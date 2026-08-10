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

// ── Coding-assistant sources ───────────────────────────────────────────

/// One line of a Claude Code session file (only the fields we read).
#[derive(Deserialize)]
struct ClaudeCodeLine {
    #[serde(rename = "type")]
    kind: Option<String>,
    message: Option<ClaudeCodeMessage>,
    timestamp: Option<String>,
    #[serde(rename = "aiTitle")]
    ai_title: Option<String>,
    /// A title the user set by hand; wins over the generated one.
    #[serde(rename = "customTitle")]
    custom_title: Option<String>,
    #[serde(rename = "isSidechain", default)]
    is_sidechain: bool,
    #[serde(rename = "isMeta", default)]
    is_meta: bool,
    /// Synthetic assistant lines carrying an API error, not a reply.
    #[serde(rename = "isApiErrorMessage", default)]
    is_api_error_message: bool,
}

#[derive(Deserialize)]
struct ClaudeCodeMessage {
    content: Option<serde_json::Value>,
}

/// Synthetic / harness text a human never typed - skipped on import.
fn is_harness_text(t: &str) -> bool {
    let t = t.trim_start();
    t.starts_with("<system-reminder")
        || t.starts_with("<local-command")
        || t.starts_with("<command-name")
        || t.starts_with("<task-notification")
        || t.starts_with("<ide_")
        || t.starts_with("Caveat: The messages below")
        || t.starts_with("This session is being continued")
        || t.starts_with("[Request interrupted")
        // Codex wraps its harness context in user-role items with these tags.
        || t.starts_with("<environment_context")
        || t.starts_with("<user_instructions")
}

/// Extract the text blocks from a Claude Code message content value
/// (string, or an array of typed blocks - only `text` blocks count; tool
/// results, tool calls, and thinking are never imported). Harness blocks
/// are dropped PER BLOCK: real sessions mix an IDE-context block with the
/// user's actual words in one message, and a whole-message check would
/// throw the real words away with the noise.
fn claude_code_text(content: &serde_json::Value) -> String {
    match content {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Array(blocks) => blocks
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .filter(|t| !is_harness_text(t))
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}

/// Parse a Claude Code session (.jsonl lines) into one conversation. The
/// reader form keeps a 100MB session from ever living in memory whole and
/// lets tests feed lines directly.
fn parse_claude_code_lines(
    reader: impl std::io::BufRead,
    source_id: Option<String>,
) -> ImportedConversation {
    let mut ai_title: Option<String> = None;
    let mut custom_title: Option<String> = None;
    let mut created: Option<i64> = None;
    let mut messages: Vec<ImportedMessage> = Vec::new();

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        let Ok(entry) = serde_json::from_str::<ClaudeCodeLine>(&line) else { continue };
        if entry.is_sidechain || entry.is_meta || entry.is_api_error_message {
            continue;
        }
        if let Some(t) = entry.custom_title {
            if !t.trim().is_empty() {
                custom_title = Some(t.trim().to_string());
            }
            continue;
        }
        if let Some(t) = entry.ai_title {
            if !t.trim().is_empty() {
                ai_title = Some(t.trim().to_string());
            }
            continue;
        }
        let kind = entry.kind.as_deref().unwrap_or("");
        if kind != "user" && kind != "assistant" {
            continue;
        }
        let Some(msg) = entry.message else { continue };
        let Some(content) = msg.content else { continue };
        let text = claude_code_text(&content);
        let text = text.trim();
        if text.is_empty() || is_harness_text(text) {
            continue;
        }
        let ts = entry.timestamp.as_deref().and_then(iso_to_micros);
        if created.is_none() {
            created = ts;
        }
        messages.push(ImportedMessage {
            role: if kind == "user" { "user" } else { "assistant" }.to_string(),
            text: text.to_string(),
            ts_us: ts,
        });
    }

    // The user's own title wins; then the generated one; then first words.
    let title = custom_title.or(ai_title).unwrap_or_else(|| {
        messages
            .iter()
            .find(|m| m.role == "user")
            .map(|m| m.text.chars().take(60).collect::<String>())
            .unwrap_or_else(|| "Coding session".to_string())
    });

    ImportedConversation {
        source_id,
        title,
        created_at_us: created,
        messages,
    }
}

fn parse_claude_code_session(path: &PathBuf) -> Result<ImportedConversation, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("Could not open {path:?}: {e}"))?;
    Ok(parse_claude_code_lines(
        std::io::BufReader::new(file),
        path.file_stem().map(|s| s.to_string_lossy().to_string()),
    ))
}

/// A folder of Claude Code sessions: every .jsonl becomes a conversation.
/// Recurses ONE level so picking ~/.claude/projects itself (sessions live
/// in per-project subfolders) works as naturally as picking one project.
fn parse_claude_code_dir(dir: &PathBuf) -> Result<(String, Vec<ImportedConversation>), String> {
    fn scan_files(dir: &PathBuf, recurse: bool, convs: &mut Vec<ImportedConversation>) {
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries {
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            if path.is_dir() {
                // Session folders keep subagent transcripts in a
                // "subagents" subfolder - sidechain chatter, never imported.
                let name = entry.file_name();
                if recurse && name != "subagents" {
                    scan_files(&path, false, convs);
                }
                continue;
            }
            if path.extension().is_none_or(|e| e != "jsonl") {
                continue;
            }
            if entry
                .metadata()
                .map(|m| m.len() > MAX_IMPORT_BYTES)
                .unwrap_or(true)
            {
                continue;
            }
            match parse_claude_code_session(&path) {
                Ok(c) if !c.messages.is_empty() => convs.push(c),
                _ => {}
            }
        }
    }

    std::fs::read_dir(dir).map_err(|e| format!("Could not read the folder: {e}"))?;
    let mut convs = Vec::new();
    scan_files(dir, true, &mut convs);
    if convs.is_empty() {
        return Err(
            "No coding sessions found in this folder. Pick your ~/.claude/projects folder (or one project folder inside it) containing .jsonl session files.".to_string(),
        );
    }
    convs.sort_by_key(|c| c.created_at_us.unwrap_or(0));
    Ok(("claude-code".to_string(), convs))
}

/// Aider's .aider.chat.history.md: "# aider chat started at <ts>" opens a
/// session; "#### " lines are the user's prompts; other prose is the
/// assistant. One file = many conversations.
fn parse_aider_history(text: &str) -> Result<(String, Vec<ImportedConversation>), String> {
    let mut convs: Vec<ImportedConversation> = Vec::new();
    let mut current: Option<ImportedConversation> = None;
    // Accumulators for run-together lines of the same speaker.
    let mut user_buf = String::new();
    let mut ai_buf = String::new();

    fn flush(buf: &mut String, role: &str, conv: &mut Option<ImportedConversation>) {
        let t = buf.trim();
        if !t.is_empty() {
            if let Some(c) = conv.as_mut() {
                c.messages.push(ImportedMessage {
                    role: role.to_string(),
                    text: t.to_string(),
                    ts_us: None,
                });
            }
        }
        buf.clear();
    }

    for line in text.lines() {
        if let Some(rest) = line.strip_prefix("# aider chat started at ") {
            flush(&mut user_buf, "user", &mut current);
            flush(&mut ai_buf, "assistant", &mut current);
            if let Some(c) = current.take() {
                if !c.messages.is_empty() {
                    convs.push(c);
                }
            }
            let ts = iso_to_micros(rest.trim());
            current = Some(ImportedConversation {
                source_id: None,
                title: format!("Aider session {}", rest.trim()),
                created_at_us: ts,
                messages: Vec::new(),
            });
        } else if let Some(prompt) = line.strip_prefix("#### ") {
            flush(&mut ai_buf, "assistant", &mut current);
            if !user_buf.is_empty() {
                user_buf.push('\n');
            }
            user_buf.push_str(prompt);
        } else {
            flush(&mut user_buf, "user", &mut current);
            if !ai_buf.is_empty() {
                ai_buf.push('\n');
            }
            ai_buf.push_str(line);
        }
    }
    flush(&mut user_buf, "user", &mut current);
    flush(&mut ai_buf, "assistant", &mut current);
    if let Some(c) = current.take() {
        if !c.messages.is_empty() {
            convs.push(c);
        }
    }

    if convs.is_empty() {
        return Err("This looks like an Aider history file, but no sessions were found in it.".to_string());
    }
    Ok(("aider".to_string(), convs))
}

/// The "Just what you said" import choice: keep only the user's own
/// messages. Applied at parse time so a user-only archive simply never
/// stores assistant text - honest to the label, nothing downstream needs
/// a mode flag. Conversations left empty by the filter are dropped.
fn apply_user_only(conversations: &mut Vec<ImportedConversation>) {
    for conv in conversations.iter_mut() {
        conv.messages.retain(|m| m.role == "user");
    }
    conversations.retain(|c| !c.messages.is_empty());
}

// ── OpenCode ───────────────────────────────────────────────────────────
//
// OpenCode stores sessions in a SQLite db (opencode.db in its data dir,
// ~/.local/share/opencode on every OS - it applies the XDG layout even on
// Windows). Schema verified against a real 1.18 db generated locally:
// session(id, parent_id, title, time_created ms), message(id, session_id,
// time_created ms, data JSON w/ role), part(id, message_id, data JSON w/
// type; "text" parts carry the content, tool/step/reasoning parts are
// separate types). parent_id marks subagent sessions - skipped, like
// Claude Code sidechains. NOTE: older OpenCode versions used a JSON-file
// tree instead; current versions migrate it into the db on first launch.

/// Message envelope JSON from the `message.data` column.
#[derive(Deserialize)]
struct OpenCodeMessageData {
    role: Option<String>,
}

/// Part JSON from the `part.data` column (only the fields we read).
#[derive(Deserialize)]
struct OpenCodePartData {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
    #[serde(default)]
    synthetic: bool,
    #[serde(default)]
    ignored: bool,
}

/// Open a source app's SQLite db READ-ONLY, in place. Never copies (real
/// stores reach many GB - Eric's Cursor db is 7.1GB) and never write-locks
/// the owning app; a busy timeout rides out short write bursts, and the
/// friendly error tells the user what to close if the db stays locked.
fn open_sqlite_readonly(
    path: &std::path::Path,
    app_name: &str,
) -> Result<rusqlite::Connection, String> {
    let conn = rusqlite::Connection::open_with_flags(
        path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|e| format!("Could not open the {app_name} data: {e}"))?;
    let _ = conn.busy_timeout(std::time::Duration::from_secs(5));
    Ok(conn)
}

/// Parse an OpenCode session database into conversations. Read-only, in
/// place - a running OpenCode is never locked or raced.
fn parse_opencode_db(db_path: &std::path::Path) -> Result<(String, Vec<ImportedConversation>), String> {
    let conn = open_sqlite_readonly(db_path, "OpenCode")?;

    // Text parts per message, in part-id order (prt_ ids are monotonic).
    let mut texts: HashMap<String, Vec<String>> = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT message_id, data FROM part ORDER BY id")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
            })
            .map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            let (message_id, data) = row;
            let Ok(part) = serde_json::from_str::<OpenCodePartData>(&data) else { continue };
            if part.kind.as_deref() != Some("text") || part.synthetic || part.ignored {
                continue;
            }
            let Some(text) = part.text else { continue };
            if text.trim().is_empty() {
                continue;
            }
            texts.entry(message_id).or_default().push(text);
        }
    }

    // Messages per session, in time order.
    let mut by_session: HashMap<String, Vec<ImportedMessage>> = HashMap::new();
    {
        let mut stmt = conn
            .prepare("SELECT id, session_id, time_created, data FROM message ORDER BY time_created")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                    r.get::<_, String>(3)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            let (id, session_id, time_ms, data) = row;
            let Ok(msg) = serde_json::from_str::<OpenCodeMessageData>(&data) else { continue };
            let Some(role) = msg.role.as_deref().and_then(normalize_role) else { continue };
            let Some(parts) = texts.remove(&id) else { continue };
            let text = parts.join("\n");
            let text = text.trim();
            if text.is_empty() {
                continue;
            }
            by_session.entry(session_id).or_default().push(ImportedMessage {
                role: role.to_string(),
                text: text.to_string(),
                ts_us: Some(time_ms * 1000),
            });
        }
    }

    // Top-level sessions (parent_id set = subagent chatter, skipped).
    let mut convs: Vec<ImportedConversation> = Vec::new();
    {
        let mut stmt = conn
            .prepare("SELECT id, title, time_created FROM session WHERE parent_id IS NULL ORDER BY time_created")
            .map_err(|e| e.to_string())?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, i64>(2)?,
                ))
            })
            .map_err(|e| e.to_string())?;
        for row in rows.flatten() {
            let (id, title, time_ms) = row;
            let Some(messages) = by_session.remove(&id) else { continue };
            if messages.is_empty() {
                continue;
            }
            let title = if title.trim().is_empty() {
                messages
                    .iter()
                    .find(|m| m.role == "user")
                    .map(|m| m.text.chars().take(60).collect::<String>())
                    .unwrap_or_else(|| "Coding session".to_string())
            } else {
                title.trim().to_string()
            };
            convs.push(ImportedConversation {
                source_id: Some(id),
                title,
                created_at_us: Some(time_ms * 1000),
                messages,
            });
        }
    }

    if convs.is_empty() {
        return Err(
            "No sessions found in this OpenCode data. If you use an older OpenCode version, update it and open it once - it moves your history into the new format.".to_string(),
        );
    }
    Ok(("opencode".to_string(), convs))
}

/// OpenCode's data dir - the XDG path on every OS (a known OpenCode quirk:
/// it applies the Linux layout on Windows and macOS too).
fn opencode_db_path(home: &std::path::Path) -> PathBuf {
    home.join(".local").join("share").join("opencode").join("opencode.db")
}

// ── Codex (OpenAI) ─────────────────────────────────────────────────────
//
// Codex stores sessions as JSONL "rollouts" under
// ~/.codex/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl. Live-verified
// against a real codex-cli 0.147.0 run + the serializer itself
// (openai/codex codex-rs/protocol/src/models.rs). New-format line =
// {timestamp, type, payload}; type "response_item" with payload
// {type:"message", role, content:[{type: input_text|output_text, text}]}.
// Roles other than user/assistant (developer, system) are harness; tool
// calls / reasoning / web searches are separate payload types we never
// touch. Old-format files (pre-envelope) are bare item lines - parsed by
// fallback. Harness text INSIDE user items (<environment_context>,
// <user_instructions>) is dropped per block, like Claude Code.

#[derive(Deserialize)]
struct CodexLine {
    timestamp: Option<String>,
    #[serde(rename = "type")]
    kind: Option<String>,
    payload: Option<serde_json::Value>,
}

#[derive(Deserialize)]
struct CodexMessageItem {
    #[serde(rename = "type")]
    kind: Option<String>,
    role: Option<String>,
    #[serde(default)]
    content: Vec<CodexContentItem>,
}

#[derive(Deserialize)]
struct CodexContentItem {
    #[serde(rename = "type")]
    kind: Option<String>,
    text: Option<String>,
}

fn codex_item_to_message(item: &serde_json::Value, ts_us: Option<i64>) -> Option<ImportedMessage> {
    let msg: CodexMessageItem = serde_json::from_value(item.clone()).ok()?;
    if msg.kind.as_deref() != Some("message") {
        return None;
    }
    let role = normalize_role(msg.role.as_deref()?)?;
    let text = msg
        .content
        .iter()
        .filter(|c| matches!(c.kind.as_deref(), Some("input_text") | Some("output_text")))
        .filter_map(|c| c.text.as_deref())
        .filter(|t| !is_harness_text(t))
        .collect::<Vec<_>>()
        .join("\n");
    let text = text.trim();
    if text.is_empty() || is_harness_text(text) {
        return None;
    }
    Some(ImportedMessage {
        role: role.to_string(),
        text: text.to_string(),
        ts_us,
    })
}

/// Parse one Codex rollout .jsonl into one conversation.
fn parse_codex_rollout(
    reader: impl std::io::BufRead,
    source_id: Option<String>,
) -> ImportedConversation {
    let mut created: Option<i64> = None;
    let mut messages: Vec<ImportedMessage> = Vec::new();

    for line in reader.lines() {
        let Ok(line) = line else { continue };
        if line.trim().is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<CodexLine>(&line) {
            if let Some(payload) = &entry.payload {
                let ts = entry.timestamp.as_deref().and_then(iso_to_micros);
                match entry.kind.as_deref() {
                    Some("session_meta") => {
                        if created.is_none() {
                            created = ts.or_else(|| {
                                payload["timestamp"].as_str().and_then(iso_to_micros)
                            });
                        }
                        continue;
                    }
                    Some("response_item") => {
                        if let Some(m) = codex_item_to_message(payload, ts) {
                            if created.is_none() {
                                created = m.ts_us;
                            }
                            messages.push(m);
                        }
                        continue;
                    }
                    _ => continue,
                }
            }
        }
        // Old-format rollouts: the line IS the bare response item.
        if let Ok(item) = serde_json::from_str::<serde_json::Value>(&line) {
            if let Some(m) = codex_item_to_message(&item, None) {
                messages.push(m);
            }
        }
    }

    let title = messages
        .iter()
        .find(|m| m.role == "user")
        .map(|m| m.text.chars().take(60).collect::<String>())
        .unwrap_or_else(|| "Coding session".to_string());

    ImportedConversation {
        source_id,
        title,
        created_at_us: created,
        messages,
    }
}

/// A Codex sessions tree (~/.codex/sessions, date-sharded YYYY/MM/DD):
/// every rollout file becomes a conversation.
fn parse_codex_dir(dir: &PathBuf) -> Result<(String, Vec<ImportedConversation>), String> {
    fn scan(dir: &std::path::Path, depth: usize, convs: &mut Vec<ImportedConversation>) {
        if depth > 4 {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else { return };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                scan(&path, depth + 1, convs);
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if !name.starts_with("rollout-") || !name.ends_with(".jsonl") {
                continue;
            }
            let Ok(file) = std::fs::File::open(&path) else { continue };
            let conv = parse_codex_rollout(
                std::io::BufReader::new(file),
                path.file_stem().map(|s| s.to_string_lossy().to_string()),
            );
            if !conv.messages.is_empty() {
                convs.push(conv);
            }
        }
    }
    // The user may pick ~/.codex itself, or the sessions folder inside it.
    let root = if dir.join("sessions").is_dir() {
        dir.join("sessions")
    } else {
        dir.clone()
    };
    let mut convs = Vec::new();
    scan(&root, 0, &mut convs);
    if convs.is_empty() {
        return Err(
            "No Codex sessions found in this folder. Pick your .codex folder (or the sessions folder inside it).".to_string(),
        );
    }
    convs.sort_by_key(|c| c.created_at_us.unwrap_or(0));
    Ok(("codex".to_string(), convs))
}

// ── Cursor ─────────────────────────────────────────────────────────────
//
// Cursor keeps every conversation in ONE global SQLite db:
// <config>/Cursor/User/globalStorage/state.vscdb. Live-verified 2026-08-10
// against a real 7.1GB db (160 conversations): cursorDiskKV holds
// composerData:<id> JSON (name, createdAt ms, fullConversationHeadersOnly
// = ordered [{bubbleId, type}]) and bubbleId:<composerId>:<bubbleId> JSON
// (text, createdAt ISO). type 1 = user, 2 = assistant; assistant bubbles
// with empty text are tool/thinking steps - excluded by the text filter
// alone. The composerHeaders table is a recent addition covering only new
// conversations, so enumeration walks the composerData keys; headers are
// joined for isSubagent when present. NEVER copy this db (size) - read
// only, in place.

#[derive(Deserialize)]
struct CursorComposerData {
    #[serde(rename = "composerId")]
    composer_id: Option<String>,
    name: Option<String>,
    #[serde(rename = "createdAt")]
    created_at: Option<i64>,
    #[serde(rename = "fullConversationHeadersOnly", default)]
    headers: Vec<CursorBubbleHeader>,
    #[serde(rename = "isBestOfNSubcomposer", default)]
    is_best_of_n_subcomposer: bool,
}

#[derive(Deserialize)]
struct CursorBubbleHeader {
    #[serde(rename = "bubbleId")]
    bubble_id: String,
    /// 1 = user, 2 = assistant; number or string in the wild.
    #[serde(rename = "type")]
    kind: serde_json::Value,
}

#[derive(Deserialize)]
struct CursorBubble {
    text: Option<String>,
    #[serde(rename = "createdAt")]
    created_at: Option<String>,
}

fn parse_cursor_db(db_path: &std::path::Path) -> Result<(String, Vec<ImportedConversation>), String> {
    let conn = open_sqlite_readonly(db_path, "Cursor")?;

    // Subagent composers per the (newer) headers table; absent in old dbs.
    let mut subagents: std::collections::HashSet<String> = std::collections::HashSet::new();
    if let Ok(mut stmt) = conn.prepare("SELECT composerId FROM composerHeaders WHERE isSubagent = 1") {
        if let Ok(rows) = stmt.query_map([], |r| r.get::<_, String>(0)) {
            subagents.extend(rows.flatten());
        }
    }

    let mut convs: Vec<ImportedConversation> = Vec::new();
    let mut bubble_stmt = conn
        .prepare("SELECT value FROM cursorDiskKV WHERE key = ?")
        .map_err(|e| e.to_string())?;
    let mut comp_stmt = conn
        .prepare("SELECT key, value FROM cursorDiskKV WHERE key LIKE 'composerData:%' AND value IS NOT NULL")
        .map_err(|e| e.to_string())?;
    let comp_rows = comp_stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| e.to_string())?;

    for row in comp_rows.flatten() {
        let (_, value) = row;
        let Ok(comp) = serde_json::from_str::<CursorComposerData>(&value) else { continue };
        let Some(cid) = comp.composer_id else { continue };
        if comp.is_best_of_n_subcomposer || subagents.contains(&cid) {
            continue;
        }
        let mut messages: Vec<ImportedMessage> = Vec::new();
        for h in &comp.headers {
            let kind = h.kind.as_i64().or_else(|| h.kind.as_str().and_then(|s| s.parse().ok()));
            let role = match kind {
                Some(1) => "user",
                Some(2) => "assistant",
                _ => continue,
            };
            let bubble_raw: Option<String> = bubble_stmt
                .query_row([format!("bubbleId:{cid}:{}", h.bubble_id)], |r| {
                    r.get::<_, Option<String>>(0)
                })
                .ok()
                .flatten();
            let Some(bubble_raw) = bubble_raw else { continue };
            let Ok(bubble) = serde_json::from_str::<CursorBubble>(&bubble_raw) else { continue };
            let text = bubble.text.unwrap_or_default();
            let text = text.trim();
            if text.is_empty() || is_harness_text(text) {
                continue;
            }
            messages.push(ImportedMessage {
                role: role.to_string(),
                text: text.to_string(),
                ts_us: bubble.created_at.as_deref().and_then(iso_to_micros),
            });
        }
        if messages.is_empty() {
            continue;
        }
        let title = comp
            .name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or_else(|| {
                messages
                    .iter()
                    .find(|m| m.role == "user")
                    .map(|m| m.text.chars().take(60).collect::<String>())
                    .unwrap_or_else(|| "Coding session".to_string())
            });
        convs.push(ImportedConversation {
            source_id: Some(cid),
            title,
            created_at_us: comp.created_at.map(|ms| ms * 1000),
            messages,
        });
    }

    if convs.is_empty() {
        return Err("No conversations found in this Cursor data.".to_string());
    }
    convs.sort_by_key(|c| c.created_at_us.unwrap_or(0));
    Ok(("cursor".to_string(), convs))
}

/// The Aider history file a repo folder carries.
const AIDER_HISTORY_FILE: &str = ".aider.chat.history.md";

/// An Aider import where the user picks their PROJECT folder - we find the
/// hidden history file for them (nobody should have to toggle hidden files
/// in a picker dialog).
fn parse_aider_dir(dir: &PathBuf) -> Result<(String, Vec<ImportedConversation>), String> {
    let file = dir.join(AIDER_HISTORY_FILE);
    if !file.is_file() {
        return Err(
            "No Aider history found in this folder. Pick the project folder where you ran Aider - it keeps its chat history in a file there automatically.".to_string(),
        );
    }
    let text =
        std::fs::read_to_string(&file).map_err(|e| format!("Could not read the history: {e}"))?;
    parse_aider_history(&text)
}

// ── File handling ──────────────────────────────────────────────────────

/// Read the import source: a folder of coding sessions, a Claude Code
/// .jsonl, an Aider history .md, bare JSON, or a ZIP scanned for parseable
/// JSON entries (largest first - conversations.json dwarfs the metadata).
/// `source_hint` carries the tool the user chose in the UI, so a folder
/// pick parses as what they meant instead of guessing.
fn read_import_file(
    path: &PathBuf,
    source_hint: Option<&str>,
) -> Result<(String, Vec<ImportedConversation>), String> {
    let meta = std::fs::metadata(path).map_err(|e| format!("Could not open the file: {e}"))?;
    if meta.is_dir() {
        let oc_db = path.join("opencode.db");
        // Cursor: the picked folder may be globalStorage itself or the
        // Cursor config root above it.
        let cursor_db = ["state.vscdb", "User/globalStorage/state.vscdb"]
            .iter()
            .map(|p| path.join(p))
            .find(|p| p.is_file());
        return match source_hint {
            Some("aider") => parse_aider_dir(path),
            Some("claude-code") => parse_claude_code_dir(path),
            Some("codex") => parse_codex_dir(path),
            Some("opencode") => {
                if oc_db.is_file() {
                    parse_opencode_db(&oc_db)
                } else {
                    Err("No OpenCode data found in this folder. Pick OpenCode's data folder - it contains a file named opencode.db.".to_string())
                }
            }
            Some("cursor") => match cursor_db {
                Some(db) => parse_cursor_db(&db),
                None => Err("No Cursor data found in this folder. Pick Cursor's data folder - it contains a file named state.vscdb under User/globalStorage.".to_string()),
            },
            // No hint: try each coding source in turn.
            _ => parse_claude_code_dir(path).or_else(|first_err| {
                if oc_db.is_file() {
                    parse_opencode_db(&oc_db)
                } else {
                    Err(first_err.clone())
                }
                .or_else(|_| match &cursor_db {
                    Some(db) => parse_cursor_db(db),
                    None => Err(first_err.clone()),
                })
                .or_else(|_| parse_codex_dir(path).map_err(|_| first_err.clone()))
                .or_else(|_| parse_aider_dir(path).map_err(|_| first_err))
            }),
        };
    }
    let ext = path
        .extension()
        .map(|e| e.to_ascii_lowercase().to_string_lossy().to_string())
        .unwrap_or_default();
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    // SQLite and line-streamed sources never hold the whole file in memory,
    // so the size guard below must NOT apply (a real Cursor db is 7GB+).
    if ext == "vscdb" {
        return parse_cursor_db(path);
    }
    if ext == "db" {
        return parse_opencode_db(path);
    }
    if ext == "jsonl" {
        // Codex rollouts are also .jsonl - the filename distinguishes them.
        if stem.starts_with("rollout-") {
            let file = std::fs::File::open(path).map_err(|e| format!("Could not open the file: {e}"))?;
            let conv = parse_codex_rollout(
                std::io::BufReader::new(file),
                Some(stem.clone()),
            );
            if !conv.messages.is_empty() {
                return Ok(("codex".to_string(), vec![conv]));
            }
        }
        let conv = parse_claude_code_session(path)?;
        if conv.messages.is_empty() {
            return Err("This session file contains no conversation messages.".to_string());
        }
        return Ok(("claude-code".to_string(), vec![conv]));
    }
    if meta.len() > MAX_IMPORT_BYTES {
        return Err("This file is larger than 512MB. Extract the ZIP and pick the conversations .json inside it.".to_string());
    }
    let is_zip = ext == "zip";
    if !is_zip {
        let text = std::fs::read_to_string(path).map_err(|e| format!("Could not read the file: {e}"))?;
        if text.trim_start().starts_with("# aider chat started at") {
            return parse_aider_history(&text);
        }
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
/// summary. Synchronous work measured in seconds, not minutes. `mode`
/// "user_only" keeps just what the user said - the assistant's replies are
/// never stored (the "Just what you said" import choice); default is full.
#[tauri::command]
pub async fn import_conversations_scan(
    app: AppHandle,
    path: String,
    mode: Option<String>,
    source_hint: Option<String>,
) -> Result<ImportSummary, String> {
    let path_buf = PathBuf::from(&path);
    let file_name = path_buf
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "export".to_string());

    // Parse on a blocking thread - big files shouldn't stall the runtime.
    let (source, mut conversations) = tauri::async_runtime::spawn_blocking(move || {
        read_import_file(&path_buf, source_hint.as_deref())
    })
    .await
    .map_err(|e| e.to_string())??;

    if mode.as_deref() == Some("user_only") {
        apply_user_only(&mut conversations);
    }

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

/// Where a coding assistant's history lives on this machine, found for the
/// user - nobody should have to hunt hidden folders in a picker dialog.
#[derive(Serialize)]
pub struct CodingSourceDetect {
    pub found: bool,
    /// The folder to import when found (feed straight back into scan).
    pub path: Option<String>,
    pub project_count: usize,
    pub session_count: usize,
}

/// Look for OpenCode's session database in its standard location. Counts
/// top-level sessions and distinct projects with a read-only open (a
/// running OpenCode is unaffected).
#[tauri::command]
pub fn import_detect_opencode(app: AppHandle) -> CodingSourceDetect {
    let none = CodingSourceDetect {
        found: false,
        path: None,
        project_count: 0,
        session_count: 0,
    };
    let Ok(home) = app.path().home_dir() else { return none };
    let db = opencode_db_path(&home);
    if !db.is_file() {
        return none;
    }
    let Ok(conn) = rusqlite::Connection::open_with_flags(
        &db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) else {
        // Present but unreadable right now - still offer the import; the
        // copy-based parse gets its own chance (and its own error).
        return CodingSourceDetect {
            found: true,
            path: Some(db.to_string_lossy().to_string()),
            project_count: 0,
            session_count: 0,
        };
    };
    let sessions: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM session WHERE parent_id IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    let projects: i64 = conn
        .query_row(
            "SELECT COUNT(DISTINCT project_id) FROM session WHERE parent_id IS NULL",
            [],
            |r| r.get(0),
        )
        .unwrap_or(0);
    CodingSourceDetect {
        found: sessions > 0,
        path: Some(db.to_string_lossy().to_string()),
        project_count: projects as usize,
        session_count: sessions as usize,
    }
}

/// Look for Codex's session rollouts in their standard location
/// (~/.codex/sessions, date-sharded folders of rollout-*.jsonl).
#[tauri::command]
pub fn import_detect_codex(app: AppHandle) -> CodingSourceDetect {
    let none = CodingSourceDetect {
        found: false,
        path: None,
        project_count: 0,
        session_count: 0,
    };
    let Ok(home) = app.path().home_dir() else { return none };
    let root = home.join(".codex").join("sessions");
    if !root.is_dir() {
        return none;
    }
    fn count(dir: &std::path::Path, depth: usize) -> usize {
        if depth > 4 {
            return 0;
        }
        let Ok(entries) = std::fs::read_dir(dir) else { return 0 };
        entries
            .flatten()
            .map(|e| {
                let p = e.path();
                if p.is_dir() {
                    count(&p, depth + 1)
                } else {
                    let n = e.file_name().to_string_lossy().to_string();
                    usize::from(n.starts_with("rollout-") && n.ends_with(".jsonl"))
                }
            })
            .sum()
    }
    let sessions = count(&root, 0);
    CodingSourceDetect {
        found: sessions > 0,
        path: Some(root.to_string_lossy().to_string()),
        project_count: 0,
        session_count: sessions,
    }
}

/// Look for Cursor's global conversation store in its standard location
/// (the VS Code-style config dir: <config>/Cursor/User/globalStorage).
#[tauri::command]
pub fn import_detect_cursor(app: AppHandle) -> CodingSourceDetect {
    let none = CodingSourceDetect {
        found: false,
        path: None,
        project_count: 0,
        session_count: 0,
    };
    let Ok(config) = app.path().config_dir() else { return none };
    let db = config
        .join("Cursor")
        .join("User")
        .join("globalStorage")
        .join("state.vscdb");
    if !db.is_file() {
        return none;
    }
    let Ok(conn) = rusqlite::Connection::open_with_flags(
        &db,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ) else {
        // Locked right now (Cursor running) - still offer the import.
        return CodingSourceDetect {
            found: true,
            path: Some(db.to_string_lossy().to_string()),
            project_count: 0,
            session_count: 0,
        };
    };
    // Count conversations that actually contain messages (drafts and
    // empty composers would inflate the number the card promises).
    let with_bubbles: Result<i64, _> = conn.query_row(
        "SELECT COUNT(*) FROM cursorDiskKV WHERE key LIKE 'composerData:%' AND value IS NOT NULL \
         AND json_array_length(json_extract(value, '$.fullConversationHeadersOnly')) > 0",
        [],
        |r| r.get(0),
    );
    let sessions: i64 = with_bubbles.or_else(|_| {
        // JSON1 unavailable in some builds - fall back to the plain count.
        conn.query_row(
            "SELECT COUNT(*) FROM cursorDiskKV WHERE key LIKE 'composerData:%' AND value IS NOT NULL",
            [],
            |r| r.get(0),
        )
    })
    .unwrap_or(0);
    CodingSourceDetect {
        found: sessions > 0,
        path: Some(db.to_string_lossy().to_string()),
        project_count: 0,
        session_count: sessions as usize,
    }
}

/// Look for Claude Code's session store in its standard location
/// (~/.claude/projects, per-project subfolders of .jsonl sessions).
#[tauri::command]
pub fn import_detect_claude_code(app: AppHandle) -> CodingSourceDetect {
    let none = CodingSourceDetect {
        found: false,
        path: None,
        project_count: 0,
        session_count: 0,
    };
    let Ok(home) = app.path().home_dir() else { return none };
    let root = home.join(".claude").join("projects");
    if !root.is_dir() {
        return none;
    }
    let mut project_count = 0usize;
    let mut session_count = 0usize;
    let Ok(entries) = std::fs::read_dir(&root) else { return none };
    for entry in entries.flatten() {
        let path = entry.path();
        let count_sessions = |dir: &std::path::Path| -> usize {
            std::fs::read_dir(dir)
                .map(|es| {
                    es.flatten()
                        .filter(|e| e.path().extension().is_some_and(|x| x == "jsonl"))
                        .count()
                })
                .unwrap_or(0)
        };
        if path.is_dir() {
            let in_project = count_sessions(&path);
            if in_project > 0 {
                project_count += 1;
                session_count += in_project;
            }
        } else if path.extension().is_some_and(|x| x == "jsonl") {
            session_count += 1;
        }
    }
    CodingSourceDetect {
        found: session_count > 0,
        path: Some(root.to_string_lossy().to_string()),
        project_count,
        session_count,
    }
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

/// A running adoption's live progress - also the overlap guard's record.
#[derive(Clone, Serialize)]
pub struct AdoptProgressStatus {
    pub archive_id: String,
    pub ai_id: String,
    pub done: usize,
    pub total: usize,
}

/// Adoptions currently writing, keyed "archive_id:ai_id". A second click
/// (or a launch-resume racing a click) must not start an overlapping loop:
/// both would read the existing-conversations set before either writes,
/// and every conversation would land twice. The map doubles as the live
/// progress store so a remounted page can pick the count back up.
fn adoptions_in_flight(
) -> &'static std::sync::Mutex<std::collections::HashMap<String, AdoptProgressStatus>> {
    static IN_FLIGHT: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<String, AdoptProgressStatus>>,
    > = std::sync::OnceLock::new();
    IN_FLIGHT.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

/// Removes its key on every exit path, including errors.
struct AdoptionGuard(String);
impl Drop for AdoptionGuard {
    fn drop(&mut self) {
        if let Ok(mut map) = adoptions_in_flight().lock() {
            map.remove(&self.0);
        }
    }
}

/// The adoptions running right now - lets the memory page re-attach its
/// progress display after navigation instead of waiting for the next
/// progress event (which can be minutes away during a long conversation).
#[tauri::command]
pub fn import_adopt_status() -> Vec<AdoptProgressStatus> {
    adoptions_in_flight()
        .lock()
        .map(|m| m.values().cloned().collect())
        .unwrap_or_default()
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

    let flight_key = format!("{archive_id}:{ai_id}");
    {
        let mut map = adoptions_in_flight()
            .lock()
            .map_err(|_| "Adoption tracking unavailable".to_string())?;
        if map.contains_key(&flight_key) {
            return Err(
                "This archive is already being added to that AI's conversations - it's still running in the background.".to_string(),
            );
        }
        map.insert(
            flight_key.clone(),
            AdoptProgressStatus {
                archive_id: archive_id.clone(),
                ai_id: ai_id.clone(),
                done: 0,
                total: 0,
            },
        );
    }
    let _guard = AdoptionGuard(flight_key.clone());

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
        // Keep the pollable status current every conversation, even the
        // dedup-skipped ones - the progress display reads this on remount.
        if let Ok(mut map) = adoptions_in_flight().lock() {
            if let Some(s) = map.get_mut(&flight_key) {
                s.done = idx;
                s.total = total;
            }
        }
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
    // The page that started this may be long gone - the done event lets
    // whoever is mounted now refresh and kick the episodic summaries.
    let _ = app.emit(
        "import-adopt-done",
        serde_json::json!({
            "archiveId": archive_id,
            "aiId": ai_id,
            "aiName": ai_name,
            "written": written,
        }),
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

    /// Build an OpenCode-shaped SQLite fixture matching the real 1.18
    /// schema (verified against a locally generated db) and parse it.
    #[test]
    fn opencode_db_parses() {
        let dir = std::env::temp_dir().join(format!("yoai-oc-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("opencode.db");
        let _ = std::fs::remove_file(&db_path);
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE session (id TEXT PRIMARY KEY, project_id TEXT, parent_id TEXT, title TEXT NOT NULL, time_created INTEGER NOT NULL);
                 CREATE TABLE message (id TEXT PRIMARY KEY, session_id TEXT NOT NULL, time_created INTEGER NOT NULL, data TEXT NOT NULL);
                 CREATE TABLE part (id TEXT PRIMARY KEY, message_id TEXT NOT NULL, data TEXT NOT NULL);",
            )
            .unwrap();
            conn.execute_batch(r#"
                INSERT INTO session VALUES ('ses_1','prj_1',NULL,'Fix the login bug',1786275050508);
                INSERT INTO session VALUES ('ses_sub','prj_1','ses_1','subagent chatter',1786275050600);
                INSERT INTO message VALUES ('msg_1','ses_1',1786275050633,'{"role":"user","time":{"created":1786275050633}}');
                INSERT INTO message VALUES ('msg_2','ses_1',1786275051218,'{"role":"assistant","time":{"created":1786275051218}}');
                INSERT INTO message VALUES ('msg_sub','ses_sub',1786275051300,'{"role":"user"}');
                INSERT INTO part VALUES ('prt_1','msg_1','{"type":"text","text":"the login times out on slow networks"}');
                INSERT INTO part VALUES ('prt_2','msg_2','{"type":"step-start"}');
                INSERT INTO part VALUES ('prt_3','msg_2','{"type":"text","text":"Found it - the token check races the redirect."}');
                INSERT INTO part VALUES ('prt_4','msg_2','{"type":"tool","text":"tool noise"}');
                INSERT INTO part VALUES ('prt_5','msg_2','{"type":"text","text":"injected","synthetic":true}');
                INSERT INTO part VALUES ('prt_6','msg_2','{"type":"step-finish"}');
                INSERT INTO part VALUES ('prt_7','msg_sub','{"type":"text","text":"sub text"}');
            "#)
            .unwrap();
        }
        let (source, convs) = parse_opencode_db(&db_path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(source, "opencode");
        assert_eq!(convs.len(), 1, "subagent session must be skipped");
        assert_eq!(convs[0].title, "Fix the login bug");
        let texts: Vec<&str> = convs[0].messages.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "the login times out on slow networks",
                "Found it - the token check races the redirect."
            ],
            "only text parts survive; tool/step/synthetic parts are dropped"
        );
        assert_eq!(convs[0].messages[0].role, "user");
        assert_eq!(convs[0].messages[0].ts_us, Some(1786275050633 * 1000));
        assert_eq!(convs[0].created_at_us, Some(1786275050508 * 1000));
    }

    /// Line shapes replicate a REAL codex-cli 0.147.0 rollout captured
    /// 2026-08-10 (envelope {timestamp,type,payload}) plus an old-format
    /// bare-item line.
    #[test]
    fn codex_rollout_parses() {
        let jsonl = r#"{"timestamp":"2026-08-09T22:09:03.162Z","type":"session_meta","payload":{"id":"019fe892","timestamp":"2026-08-09T22:09:03.162Z","cwd":"/tmp/p","originator":"codex_exec"}}
{"timestamp":"2026-08-09T22:09:03.500Z","type":"event_msg","payload":{"type":"task_started"}}
{"timestamp":"2026-08-09T22:09:03.600Z","type":"response_item","payload":{"type":"message","id":"msg_1","role":"developer","content":[{"type":"input_text","text":"<skills_instructions>stuff</skills_instructions>"}]}}
{"timestamp":"2026-08-09T22:09:03.700Z","type":"response_item","payload":{"type":"message","id":"msg_2","role":"user","content":[{"type":"input_text","text":"<environment_context>\n  <cwd>/tmp/p</cwd>\n</environment_context>"}]}}
{"timestamp":"2026-08-09T22:09:03.800Z","type":"response_item","payload":{"type":"message","id":"msg_3","role":"user","content":[{"type":"input_text","text":"<user_instructions>agents md text</user_instructions>"},{"type":"input_text","text":"add a retry to the fetch call"}]}}
{"timestamp":"2026-08-09T22:09:04.000Z","type":"event_msg","payload":{"type":"user_message","message":"add a retry to the fetch call"}}
{"timestamp":"2026-08-09T22:09:05.000Z","type":"response_item","payload":{"type":"reasoning","id":"rs_1","summary":[]}}
{"timestamp":"2026-08-09T22:09:06.000Z","type":"response_item","payload":{"type":"function_call","id":"fc_1","name":"shell","arguments":"{}"}}
{"timestamp":"2026-08-09T22:09:07.000Z","type":"response_item","payload":{"type":"message","id":"msg_4","role":"assistant","content":[{"type":"output_text","text":"Done - retry loop added with backoff."}]}}
{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Old-format assistant line."}]}
"#;
        let conv = parse_codex_rollout(std::io::Cursor::new(jsonl), Some("rollout-x".into()));
        let texts: Vec<&str> = conv.messages.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(
            texts,
            vec![
                "add a retry to the fetch call",
                "Done - retry loop added with backoff.",
                "Old-format assistant line."
            ],
            "developer role, env-context, user_instructions, reasoning, and tool calls all skipped; mixed harness+real user item keeps the real words"
        );
        assert_eq!(conv.messages[0].role, "user");
        assert_eq!(conv.title, "add a retry to the fetch call");
        assert!(conv.created_at_us.is_some());
        assert!(conv.messages[0].ts_us.is_some());
    }

    /// Cursor fixture mirroring the REAL state.vscdb schema (verified
    /// 2026-08-10 against a 7.1GB live db): composerData + bubbleId keys
    /// in cursorDiskKV, partial composerHeaders, NULL rows, subagents.
    #[test]
    fn cursor_db_parses() {
        let dir = std::env::temp_dir().join(format!("yoai-cursor-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("state.vscdb");
        let _ = std::fs::remove_file(&db_path);
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute_batch(
                "CREATE TABLE cursorDiskKV (key TEXT PRIMARY KEY, value TEXT);
                 CREATE TABLE composerHeaders (composerId TEXT PRIMARY KEY, workspaceId TEXT, createdAt INTEGER, lastUpdatedAt INTEGER, isArchived INTEGER, isSubagent INTEGER, recency INTEGER, checkpointAt INTEGER, value TEXT);",
            )
            .unwrap();
            conn.execute_batch(r#"
                INSERT INTO composerHeaders (composerId, isSubagent) VALUES ('comp-sub', 1);
                INSERT INTO cursorDiskKV VALUES ('composerData:comp-1',
                  '{"composerId":"comp-1","name":"Fix login","createdAt":1760130362369,"fullConversationHeadersOnly":[{"bubbleId":"b1","type":1},{"bubbleId":"b2","type":2},{"bubbleId":"b3","type":2},{"bubbleId":"b4","type":2}]}');
                INSERT INTO cursorDiskKV VALUES ('bubbleId:comp-1:b1','{"type":1,"text":"the login times out","createdAt":"2025-10-11T03:16:34.655Z"}');
                INSERT INTO cursorDiskKV VALUES ('bubbleId:comp-1:b2','{"type":2,"text":"","toolFormerData":{}}');
                INSERT INTO cursorDiskKV VALUES ('bubbleId:comp-1:b3','{"type":2,"text":"Found the race in the token check.","createdAt":"2025-10-11T03:16:40.000Z"}');
                INSERT INTO cursorDiskKV VALUES ('composerData:comp-sub',
                  '{"composerId":"comp-sub","name":"sub","createdAt":1,"fullConversationHeadersOnly":[{"bubbleId":"s1","type":1}]}');
                INSERT INTO cursorDiskKV VALUES ('bubbleId:comp-sub:s1','{"type":1,"text":"subagent chatter"}');
                INSERT INTO cursorDiskKV VALUES ('composerData:comp-bestofn',
                  '{"composerId":"comp-bestofn","isBestOfNSubcomposer":true,"fullConversationHeadersOnly":[{"bubbleId":"n1","type":1}]}');
                INSERT INTO cursorDiskKV VALUES ('composerData:comp-null', NULL);
                INSERT INTO cursorDiskKV VALUES ('composerData:empty-state-draft','{"composerId":"empty-state-draft","fullConversationHeadersOnly":[]}');
            "#)
            .unwrap();
        }
        let (source, convs) = parse_cursor_db(&db_path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(source, "cursor");
        assert_eq!(convs.len(), 1, "subagent, best-of-n, NULL, and empty composers skipped");
        assert_eq!(convs[0].title, "Fix login");
        let texts: Vec<&str> = convs[0].messages.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(
            texts,
            vec!["the login times out", "Found the race in the token check."],
            "empty-text assistant bubble (tool step) and missing bubble b4 skipped"
        );
        assert_eq!(convs[0].messages[0].role, "user");
        assert_eq!(convs[0].created_at_us, Some(1760130362369 * 1000));
        assert!(convs[0].messages[0].ts_us.is_some());
    }

    /// Dogfood against the developer's REAL Cursor database. Ignored by
    /// default; run with:
    /// YOAI_CURSOR_DB=~/.config/Cursor/User/globalStorage/state.vscdb \
    ///   cargo test cursor_dogfood -- --ignored --nocapture
    /// Prints counts only - never content.
    #[test]
    #[ignore]
    fn cursor_dogfood_real_db() {
        let Ok(path) = std::env::var("YOAI_CURSOR_DB") else { return };
        let (source, convs) = parse_cursor_db(std::path::Path::new(&path)).unwrap();
        assert_eq!(source, "cursor");
        let (mut users, mut assts, mut no_ts, mut untitled) = (0, 0, 0, 0);
        for c in &convs {
            if c.title.trim().is_empty() {
                untitled += 1;
            }
            for m in &c.messages {
                if m.role == "user" { users += 1 } else { assts += 1 }
                if m.ts_us.is_none() {
                    no_ts += 1;
                }
            }
        }
        println!(
            "conversations: {} | user msgs: {} | assistant msgs: {} | msgs missing ts: {} | untitled: {}",
            convs.len(), users, assts, no_ts, untitled
        );
        assert!(!convs.is_empty());
        assert_eq!(untitled, 0);
    }

    /// Dogfood against a REAL OpenCode database (generated by running the
    /// actual OpenCode CLI). Ignored by default; run with:
    /// YOAI_OC_DB=/path/to/opencode.db cargo test oc_dogfood -- --ignored --nocapture
    #[test]
    #[ignore]
    fn oc_dogfood_real_db() {
        let Ok(path) = std::env::var("YOAI_OC_DB") else { return };
        let (source, convs) = parse_opencode_db(std::path::Path::new(&path)).unwrap();
        assert_eq!(source, "opencode");
        let (mut users, mut assts) = (0, 0);
        for c in &convs {
            assert!(!c.title.trim().is_empty());
            for m in &c.messages {
                if m.role == "user" { users += 1 } else { assts += 1 }
                assert!(m.ts_us.is_some());
            }
        }
        println!(
            "conversations: {} | user msgs: {} | assistant msgs: {}",
            convs.len(), users, assts
        );
        assert!(!convs.is_empty());
    }

    /// Dogfood harness against the developer's own local Claude Code data.
    /// Ignored by default (depends on ~/.claude/projects existing); run
    /// with: cargo test dogfood -- --ignored --nocapture
    /// Prints counts only - never session content.
    #[test]
    #[ignore]
    fn dogfood_real_claude_code_sessions() {
        let Ok(home) = std::env::var("HOME") else { return };
        let root = PathBuf::from(home).join(".claude").join("projects");
        if !root.exists() {
            return;
        }
        let (source, convs) = parse_claude_code_dir(&root).expect("real tree should parse");
        assert_eq!(source, "claude-code");
        let (mut users, mut assts, mut leaks, mut mentions, mut no_ts, mut untitled) =
            (0, 0, 0, 0, 0, 0);
        for c in &convs {
            if c.title.trim().is_empty() {
                untitled += 1;
            }
            for m in &c.messages {
                if m.role == "user" { users += 1 } else { assts += 1 }
                if is_harness_text(&m.text) {
                    leaks += 1;
                }
                // Mid-text marker MENTIONS are only printed - sessions about
                // harness internals legitimately quote these strings.
                if m.text.contains("<system-reminder") || m.text.contains("<task-notification") {
                    mentions += 1;
                }
                if m.ts_us.is_none() {
                    no_ts += 1;
                }
            }
        }
        println!(
            "conversations: {} | user msgs: {} | assistant msgs: {} | harness leaks: {} | marker mentions: {} | msgs missing ts: {} | untitled: {}",
            convs.len(), users, assts, leaks, mentions, no_ts, untitled
        );
        assert!(!convs.is_empty());
        assert_eq!(leaks, 0, "harness text leaked into imported messages");
        assert_eq!(untitled, 0);
    }

    #[test]
    fn unrecognized_is_a_clear_error() {
        let err = detect_and_parse(r#"{"foo": "bar"}"#).unwrap_err();
        assert!(err.contains("ChatGPT"));
    }

    fn parse_cc(lines: &str) -> ImportedConversation {
        parse_claude_code_lines(std::io::Cursor::new(lines), Some("session-1".to_string()))
    }

    #[test]
    fn claude_code_session_parses() {
        let jsonl = r#"{"type":"queue-operation","operation":"enqueue"}
{"type":"user","timestamp":"2026-07-09T02:53:42.711Z","message":{"role":"user","content":"fix the login bug"}}
{"type":"assistant","timestamp":"2026-07-09T02:53:50.000Z","message":{"role":"assistant","content":[{"type":"thinking","thinking":"let me look"},{"type":"text","text":"Found it - the token check."},{"type":"tool_use","name":"Read","input":{}}]}}
{"type":"user","timestamp":"2026-07-09T02:54:00.000Z","message":{"role":"user","content":[{"type":"tool_result","content":"file contents here"}]}}
{"type":"ai-title","aiTitle":"Login bug hunt"}
{"type":"user","isSidechain":true,"message":{"role":"user","content":"subagent chatter"}}
{"type":"user","isMeta":true,"message":{"role":"user","content":"meta line"}}
{"type":"assistant","isApiErrorMessage":true,"message":{"role":"assistant","content":[{"type":"text","text":"API Error: overloaded"}]}}
not even json
"#;
        let conv = parse_cc(jsonl);
        assert_eq!(conv.title, "Login bug hunt");
        assert_eq!(conv.source_id.as_deref(), Some("session-1"));
        let texts: Vec<&str> = conv.messages.iter().map(|m| m.text.as_str()).collect();
        assert_eq!(texts, vec!["fix the login bug", "Found it - the token check."]);
        assert_eq!(conv.messages[0].role, "user");
        assert_eq!(conv.messages[1].role, "assistant");
        assert!(conv.created_at_us.is_some());
        assert_eq!(conv.created_at_us, conv.messages[0].ts_us);
    }

    #[test]
    fn claude_code_custom_title_wins() {
        let jsonl = r#"{"type":"ai-title","aiTitle":"Generated title"}
{"type":"custom-title","customTitle":"my sprint"}
{"type":"ai-title","aiTitle":"Later generated title"}
{"type":"user","message":{"role":"user","content":"hello"}}
"#;
        assert_eq!(parse_cc(jsonl).title, "my sprint");
    }

    #[test]
    fn claude_code_title_falls_back_to_first_user_words() {
        let jsonl =
            r#"{"type":"user","message":{"role":"user","content":"rename the button"}}"#;
        assert_eq!(parse_cc(jsonl).title, "rename the button");
    }

    #[test]
    fn claude_code_mixed_harness_block_keeps_user_text() {
        // Real sessions mix an IDE-context block with the user's actual
        // words in ONE message - the noise goes, the words stay.
        let jsonl = r#"{"type":"user","message":{"role":"user","content":[{"type":"text","text":"<ide_opened_file>The user opened a file</ide_opened_file>"},{"type":"text","text":"remove the line break on the home page"}]}}
"#;
        let conv = parse_cc(jsonl);
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].text, "remove the line break on the home page");
    }

    #[test]
    fn claude_code_harness_strings_skipped() {
        let jsonl = r#"{"type":"user","message":{"role":"user","content":"<local-command-stdout>ok</local-command-stdout>"}}
{"type":"user","message":{"role":"user","content":"Caveat: The messages below were generated by the user while running local commands."}}
{"type":"user","message":{"role":"user","content":"This session is being continued from a previous conversation."}}
{"type":"user","message":{"role":"user","content":"[Request interrupted by user]"}}
{"type":"user","message":{"role":"user","content":"<task-notification>agent done</task-notification>"}}
{"type":"user","message":{"role":"user","content":"a real question"}}
"#;
        let conv = parse_cc(jsonl);
        assert_eq!(conv.messages.len(), 1);
        assert_eq!(conv.messages[0].text, "a real question");
    }

    #[test]
    fn aider_history_parses_sessions() {
        let md = "# aider chat started at 2024-05-10 09:48:53\n\
\n\
#### add a retry to the fetch call\n\
#### and log the failure\n\
\n\
Sure - I added a retry loop with a warning log.\n\
More assistant prose.\n\
\n\
# aider chat started at 2024-06-01 20:00:00\n\
\n\
#### rename the config struct\n\
\n\
Done, renamed it everywhere.\n";
        let (source, convs) = parse_aider_history(md).unwrap();
        assert_eq!(source, "aider");
        assert_eq!(convs.len(), 2);
        assert!(convs[0].created_at_us.is_some());
        assert_eq!(convs[0].messages.len(), 2);
        assert_eq!(convs[0].messages[0].role, "user");
        assert_eq!(
            convs[0].messages[0].text,
            "add a retry to the fetch call\nand log the failure"
        );
        assert_eq!(convs[0].messages[1].role, "assistant");
        assert!(convs[0].messages[1].text.contains("retry loop"));
        assert_eq!(convs[1].messages.len(), 2);
        assert!(convs[1].title.contains("2024-06-01"));
    }

    #[test]
    fn aider_without_sessions_is_an_error() {
        assert!(parse_aider_history("just some markdown\n").is_err());
    }

    #[test]
    fn user_only_mode_drops_assistant_text() {
        let mut convs = vec![
            ImportedConversation {
                source_id: None,
                title: "a".into(),
                created_at_us: None,
                messages: vec![
                    ImportedMessage { role: "user".into(), text: "mine".into(), ts_us: None },
                    ImportedMessage { role: "assistant".into(), text: "theirs".into(), ts_us: None },
                ],
            },
            ImportedConversation {
                source_id: None,
                title: "assistant-only".into(),
                created_at_us: None,
                messages: vec![ImportedMessage {
                    role: "assistant".into(),
                    text: "theirs".into(),
                    ts_us: None,
                }],
            },
        ];
        apply_user_only(&mut convs);
        assert_eq!(convs.len(), 1);
        assert_eq!(convs[0].messages.len(), 1);
        assert_eq!(convs[0].messages[0].text, "mine");
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
