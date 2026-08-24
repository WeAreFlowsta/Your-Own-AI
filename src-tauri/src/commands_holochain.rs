//! Tauri commands for Holochain transcript operations.
//!
//! These commands are called from the frontend to record and retrieve
//! conversation transcripts. All operations are fire-and-forget from
//! the UI perspective — chat works even if Holochain is down.
//!
//! Agent identification: the frontend passes an `agent_key` (hex-encoded
//! agent pub key) for all zome operations. This is the canonical identifier
//! returned by `provision_all_agents` at startup.

use crate::holochain::HolochainManager;
use holochain_types::prelude::{ActionHash, ExternIO};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

/// Wrapper that holds the HolochainManager once it's ready.
/// Registered in Tauri state at startup (before conductor starts).
pub struct HolochainState {
    pub manager: tokio::sync::OnceCell<HolochainManager>,
}

impl HolochainState {
    pub fn new() -> Self {
        Self {
            manager: tokio::sync::OnceCell::new(),
        }
    }

    pub fn get(&self) -> Result<&HolochainManager, String> {
        self.manager
            .get()
            .ok_or_else(|| "Holochain conductor is still starting...".to_string())
    }
}

/// Conversation record returned to the frontend.
#[derive(Serialize, Deserialize, Clone)]
pub struct ConversationInfo {
    pub hash: String,
    pub ai_personality_id: String,
    pub ai_personality_name: String,
    pub model_used: String,
    pub started_at: i64,
    /// When a turn was last recorded (micros) - what the conversation list
    /// orders by, so continuing an old conversation brings it back to the
    /// top. Kept in the local list cache (the records entry only knows
    /// when it started); None for conversations untouched since this
    /// existed, which fall back to started_at.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_active_at: Option<i64>,
    pub title: Option<String>,
    /// External app that drove this conversation over the API (None = in-app).
    pub source: Option<String>,
    /// The agent (generation) whose chain holds this conversation —
    /// transcript reads must be routed to this agent, not the AI's
    /// current one (see HolochainManager::agent_lineage).
    pub agent_key: String,
}

/// Token usage statistics.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TokenUsage {
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub total_tokens: u32,
    pub tokens_per_second: Option<f32>,
}

/// Transcript entry returned to the frontend.
#[derive(Serialize, Clone)]
pub struct TranscriptEntryInfo {
    pub hash: String,
    pub role: String,
    pub content: String,
    pub sequence: u32,
    pub timestamp: i64,
    pub model: String,
    pub model_hash: Option<String>,
    pub provider_fingerprint: Option<String>,
    pub thinking: Option<String>,
    pub tokens: Option<TokenUsage>,
    pub sources: Option<Vec<SourceRef>>,
    pub system_prompt: Option<String>,
    pub mode: Option<String>,
    pub attachments: Option<AttachmentInfo>,
    pub images: Option<Vec<ImageAttachmentInfo>>,
    pub grounded: Option<Vec<GroundedSource>>,
    pub runtime: Option<RuntimeInfo>,
    pub routing_reason: Option<String>,
    pub routing_task: Option<String>,
    /// The user stopped this reply; content is as far as it got.
    pub stopped: Option<bool>,
    pub agent_log: Option<serde_json::Value>,
    pub folder_path: Option<String>,
}

/// Encrypted entry as stored in the zome (cipher + nonce).
#[derive(Deserialize, Debug)]
pub(crate) struct EncryptedEntryRaw {
    pub cipher: Vec<u8>,
    pub nonce: Vec<u8>,
}

/// A cited source attached to an assistant message (web-search provenance).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SourceRef {
    pub url: String,
    pub title: String,
}

/// What the model actually received as attached context (the file content
/// folded into the prompt). Hash + byte size are always recorded; the content
/// itself is stored only when small enough to stay under the entry cipher cap.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct AttachmentInfo {
    pub bytes: u64,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// A recorded image attachment — what a vision model actually received this turn.
/// `sha256` + `bytes` + `mime` are always recorded (the provenance anchor that
/// grounded claims point back to); the `content` data-URL is kept only when small
/// enough to stay under the entry cipher cap, so a reloaded conversation can still
/// show the thumbnail. Large images record hash-only.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct ImageAttachmentInfo {
    pub filename: String,
    pub mime: String,
    pub bytes: u64,
    pub sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<String>,
}

/// A grounded source — a factual claim in an answer anchored to its supporting
/// verbatim quote + character span in an attached document (the verifiable chain
/// claim → quote+offset → doc SHA-256), or a coarse image-hash link. Typed
/// alongside the web `sources`; surfaced together in the "Sources" UI.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct GroundedSource {
    pub kind: String, // "document" | "image"
    pub doc_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub claim: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub quote: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub span: Option<Vec<u32>>, // [start, end] char offsets, or absent
}

/// An encrypted grounding annotation — links a set of grounded sources to an
/// already-recorded message (by its action-hash hex). Written as its own
/// `ConversationToEntries` entry via the existing `record_message` zome fn (so NO
/// integrity-zome change / DNA-hash churn), letting the on-demand "Verify sources"
/// button persist exactly like auto-grounding. Discriminated from a message on
/// read by the `annotation` marker; merged into its target on read.
#[derive(Serialize, Deserialize, Debug)]
struct GroundingAnnotationPlain {
    annotation: String, // always "grounding"
    target_hash: String,
    #[serde(default)]
    grounded: Option<Vec<GroundedSource>>,
    timestamp: i64,
}

/// Runtime provenance — which build/mode produced the generation.
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct RuntimeInfo {
    pub app_version: String,
    pub online: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

/// Optional provenance bundle passed in alongside a recorded message.
#[derive(Deserialize, Default, Debug)]
pub struct Provenance {
    #[serde(default)]
    pub sources: Option<Vec<SourceRef>>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub mode: Option<String>,
    #[serde(default)]
    pub attachments: Option<AttachmentInfo>,
    #[serde(default)]
    pub images: Option<Vec<ImageAttachmentInfo>>,
    #[serde(default)]
    pub grounded: Option<Vec<GroundedSource>>,
    #[serde(default)]
    pub runtime: Option<RuntimeInfo>,
    /// Why the router picked this turn's model (Auto modes; None = the user
    /// picked the model). Human-readable, shown in receipts.
    #[serde(default)]
    pub routing_reason: Option<String>,
    /// Online turns: the provider's own fingerprint for the backend
    /// configuration that answered (`system_fingerprint`), when it sent one.
    /// The provider's claim - the online sibling of `model_hash`.
    #[serde(default)]
    pub provider_fingerprint: Option<String>,
    /// The classified routing task for this turn ("code" | "math" |
    /// "reasoning" | "general").
    #[serde(default)]
    pub routing_task: Option<String>,
    /// The user stopped this reply; content is as far as it got.
    #[serde(default)]
    pub stopped: Option<bool>,
}

/// Plaintext message payload (lives INSIDE the ciphertext). Provenance fields
/// are all optional and `skip_serializing_if` empty, so pre-provenance entries
/// deserialize cleanly (missing → None) and new entries stay compact.
#[derive(Serialize, Deserialize, Debug)]
struct MessagePlain {
    pub role: String,
    pub content: String,
    pub sequence: u32,
    pub timestamp: i64,
    pub model: String,
    /// sha256 of the exact model artifact that answered - provenance a
    /// filename alone cannot prove. Absent for online models and files
    /// not yet hashed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model_hash: Option<String>,
    /// Online turns: the provider's backend fingerprint, as it reported it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provider_fingerprint: Option<String>,
    pub thinking: Option<String>,
    pub tokens: Option<TokenUsage>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<SourceRef>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub system_prompt: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub attachments: Option<AttachmentInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub images: Option<Vec<ImageAttachmentInfo>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grounded: Option<Vec<GroundedSource>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime: Option<RuntimeInfo>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub routing_task: Option<String>,
    /// The user stopped this reply; content is as far as it got.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stopped: Option<bool>,
    /// The agent turn's working log (steps/narration/thoughts/permission
    /// receipts + stats), opaque JSON owned by the frontend. The plaintext
    /// schema is client-side and encrypted before the zome sees it, so this
    /// needs NO DNA change and old entries read back as None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent_log: Option<serde_json::Value>,
    /// Workspace folder the turn worked in.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder_path: Option<String>,
}

/// Ciphertext payload sent to the zome.
#[derive(Serialize, Debug)]
struct EncryptedInput {
    cipher: Vec<u8>,
    nonce: Vec<u8>,
}

fn now_micros() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

/// Provision a Holochain agent for an AI personality.
/// `ai_id` is the stable local ID used as the lair seed tag.
/// Returns the agent pub key as a hex string.
#[tauri::command]
pub async fn provision_ai_agent(
    ai_id: String,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<String, String> {
    hc_state.get()?.provision_agent(&ai_id).await
}

/// Provision agents for multiple AIs at once (used at startup).
/// `ai_ids` are the stable local IDs used as lair seed tags.
/// Returns a map of AI ID → agent pub key hex.
///
/// Error contract for the caller's retry logic: "still starting" (from
/// `get()`) means try again shortly; any other error means every install
/// failed the same way (e.g. the happ file is unreadable) and retrying
/// won't change the answer - surface it instead.
#[tauri::command]
pub async fn provision_all_agents(
    ai_ids: Vec<String>,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<std::collections::HashMap<String, String>, String> {
    let manager = hc_state.get()?;
    let mut results = std::collections::HashMap::new();
    let total = ai_ids.len();
    let mut last_err: Option<String> = None;

    for ai_id in ai_ids {
        match manager.provision_agent(&ai_id).await {
            Ok(key) => {
                results.insert(ai_id, key);
            }
            Err(e) => {
                log::warn!("Failed to provision agent for AI {}: {}", ai_id, e);
                last_err = Some(e);
            }
        }
    }

    if results.is_empty() {
        if let Some(e) = last_err {
            return Err(format!("Provisioning failed for all {} AIs: {}", total, e));
        }
    }

    Ok(results)
}

/// Start a new conversation for an AI agent.
/// `agent_key` is the hex-encoded agent pub key.
#[tauri::command]
pub async fn start_conversation(
    app: tauri::AppHandle,
    agent_key: String,
    ai_name: String,
    model: String,
    title: Option<String>,
    source: Option<String>,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<String, String> {
    let manager = hc_state.get()?;

    // Phase A: conversation metadata is encrypted with the user data key
    // before it ever reaches the zome. `source` names the external app for
    // API-driven conversations (None = the in-app chat).
    let started_at = now_micros();
    let plain = serde_json::json!({
        "ai_personality_id": agent_key,
        "ai_personality_name": ai_name,
        "model_used": model,
        "started_at": started_at,
        "title": title,
        "source": source,
    });
    let plain_bytes = serde_json::to_vec(&plain)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    let key = manager.data_key()?;
    let (nonce, cipher) = crate::transcript_crypto::encrypt(&key, &plain_bytes)?;

    let payload = ExternIO::encode(EncryptedInput { cipher, nonce: nonce.to_vec() })
        .map_err(|e| format!("Failed to encode input: {}", e))?;

    let result = manager
        .call_zome(&agent_key, "transcript", "start_conversation", payload)
        .await?;

    let hash: ActionHash = ExternIO::decode(&result)
        .map_err(|e| format!("Failed to decode conversation hash: {}", e))?;

    crate::vault_escrow::schedule_full_backup(&app);
    let raw = hash.get_raw_39();
    let hex_hash = hex::encode(raw);
    // Write-through: the new conversation is visible in cached lists
    // immediately, even if full reads are struggling on a loaded box.
    crate::conversation_cache::append_to_cache(
        &app,
        &agent_key,
        ConversationInfo {
            hash: hex_hash.clone(),
            ai_personality_id: agent_key.clone(),
            ai_personality_name: ai_name.clone(),
            model_used: model.clone(),
            started_at,
            last_active_at: Some(started_at),
            title: title.clone(),
            source: source.clone(),
            agent_key: agent_key.clone(),
        },
    );
    if source.as_deref() == Some(WORKSPACE_MEMORY_SOURCE) {
        if let Some(folder) = title.as_deref() {
            project_memory_index_add(&app, folder, &agent_key, &hex_hash);
        }
        project_memory_cache_clear();
    }
    Ok(hex_hash)
}

/// Serialized-plaintext budget: the zome caps CIPHERTEXT at 1 MiB
/// (`MAX_CIPHER_BYTES` in the integrity zome) and encryption adds only a
/// small constant, so staying under 1,000,000 plaintext bytes is safe.
const PLAIN_BUDGET: usize = 1_000_000;

/// Shrink an oversized entry until it fits the zome's size cap - losing
/// detail beats losing the turn. Rungs mirror the frontend ladder: drop
/// thoughts, cap step details, keep the log's ends with a gap marker, drop
/// the log, and as a last resort truncate the content itself.
fn shrink_to_budget(plain: &mut MessagePlain) {
    fn size(p: &MessagePlain) -> usize {
        serde_json::to_vec(p).map(|v| v.len()).unwrap_or(usize::MAX)
    }
    fn items(p: &mut MessagePlain) -> Option<&mut Vec<serde_json::Value>> {
        p.agent_log.as_mut()?.get_mut("items")?.as_array_mut()
    }
    if size(plain) <= PLAIN_BUDGET {
        return;
    }
    // Rung 1: thoughts are the bulkiest optional detail.
    if let Some(list) = items(plain) {
        list.retain(|i| i["type"] != "thought");
    }
    if size(plain) <= PLAIN_BUDGET {
        return;
    }
    // Rung 2: cap per-step detail text.
    if let Some(list) = items(plain) {
        for i in list.iter_mut() {
            if let Some(d) = i["action"]["detail"].as_str() {
                if d.len() > 200 {
                    let capped = format!("{}..", d.chars().take(200).collect::<String>());
                    i["action"]["detail"] = serde_json::Value::String(capped);
                }
            }
        }
    }
    if size(plain) <= PLAIN_BUDGET {
        return;
    }
    // Rung 3: keep the story's ends with an honest gap marker.
    if let Some(list) = items(plain) {
        if list.len() > 80 {
            let trimmed = list.len() - 80;
            let mut kept: Vec<serde_json::Value> = list[..40].to_vec();
            kept.push(serde_json::json!({
                "id": "log-trimmed",
                "type": "narration",
                "text": format!(".. {} steps trimmed to fit the transcript ..", trimmed),
            }));
            kept.extend_from_slice(&list[list.len() - 40..]);
            *list = kept;
        }
    }
    if size(plain) <= PLAIN_BUDGET {
        return;
    }
    // Rung 4: the log goes entirely before the words do.
    plain.agent_log = None;
    if size(plain) <= PLAIN_BUDGET {
        return;
    }
    // Rung 5: truncate the content itself (safe on a char boundary).
    let over = size(plain).saturating_sub(PLAIN_BUDGET) + 64;
    let keep = plain.content.len().saturating_sub(over);
    let mut cut = keep;
    while cut > 0 && !plain.content.is_char_boundary(cut) {
        cut -= 1;
    }
    plain.content.truncate(cut);
    plain.content.push_str(" ..[trimmed to fit the transcript]");
}

/// Record a message in a conversation.
/// `agent_key` is the hex-encoded agent pub key.
#[tauri::command]
pub async fn record_transcript_entry(
    app: tauri::AppHandle,
    agent_key: String,
    conversation_hash: String,
    role: String,
    content: String,
    sequence: u32,
    model: String,
    thinking: Option<String>,
    tokens: Option<TokenUsage>,
    provenance: Option<Provenance>,
    agent_log: Option<serde_json::Value>,
    folder_path: Option<String>,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<String, String> {
    // Decode the conversation hash from raw hex (39 bytes)
    let raw_bytes = hex::decode(&conversation_hash)
        .map_err(|e| format!("Invalid conversation hash hex: {}", e))?;
    let conv_hash = ActionHash::from_raw_39(raw_bytes);

    let manager = hc_state.get()?;

    // A project-memory revision (from the app's Remember/curate paths or the
    // MCP tool) makes every cached read stale - drop the cache before the
    // write so a concurrent reader can never see the old content as fresh.
    if model == "workspace-memory" {
        project_memory_cache_clear();
    }

    // Phase A: message content is encrypted with the user data key.
    let prov = provenance.unwrap_or_default();
    let model_hash = crate::model_hash::get(&app, &model);
    let plain = MessagePlain {
        role,
        content,
        sequence,
        timestamp: now_micros(),
        model,
        model_hash,
        provider_fingerprint: prov.provider_fingerprint,
        thinking,
        tokens,
        sources: prov.sources,
        system_prompt: prov.system_prompt,
        mode: prov.mode,
        attachments: prov.attachments,
        images: prov.images,
        grounded: prov.grounded,
        runtime: prov.runtime,
        routing_reason: prov.routing_reason,
        routing_task: prov.routing_task,
        stopped: prov.stopped,
        agent_log,
        folder_path,
    };
    // Final size guard: the integrity zome rejects entries over 1 MiB of
    // ciphertext, and a rejected write means a LOST turn. The frontend
    // slims agent logs first; this backstop makes overflow impossible for
    // every caller.
    let mut plain = plain;
    shrink_to_budget(&mut plain);
    let plain_bytes = serde_json::to_vec(&plain)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    let key = manager.data_key()?;
    let (nonce, cipher) = crate::transcript_crypto::encrypt(&key, &plain_bytes)?;

    #[derive(serde::Serialize, Debug)]
    struct RecordMessageInput {
        conversation_hash: ActionHash,
        cipher: Vec<u8>,
        nonce: Vec<u8>,
    }

    let payload = ExternIO::encode(RecordMessageInput {
        conversation_hash: conv_hash,
        cipher,
        nonce: nonce.to_vec(),
    })
    .map_err(|e| format!("Failed to encode input: {}", e))?;

    let result = manager
        .call_zome(&agent_key, "transcript", "record_message", payload)
        .await?;

    let hash: ActionHash = ExternIO::decode(&result)
        .map_err(|e| format!("Failed to decode entry hash: {}", e))?;

    crate::vault_escrow::schedule_full_backup(&app);
    // The conversation was just continued: it moves to the top of the list.
    crate::conversation_cache::touch(&app, &conversation_hash, now_micros());
    Ok(hex::encode(hash.get_raw_39()))
}

/// Persist an on-demand grounding result, linked to the message it annotates.
/// Encrypted and written as its own conversation entry via the existing
/// `record_message` zome fn (no zome change), so reopening the conversation shows
/// the grounding — just like auto-grounding, which rides with the message itself.
/// `target_hash` is the annotated message's action-hash hex.
#[tauri::command]
pub async fn record_grounding_annotation(
    app: tauri::AppHandle,
    agent_key: String,
    conversation_hash: String,
    target_hash: String,
    grounded: Vec<GroundedSource>,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<String, String> {
    let raw_bytes = hex::decode(&conversation_hash)
        .map_err(|e| format!("Invalid conversation hash hex: {}", e))?;
    let conv_hash = ActionHash::from_raw_39(raw_bytes);

    let manager = hc_state.get()?;
    let plain = GroundingAnnotationPlain {
        annotation: "grounding".to_string(),
        target_hash,
        grounded: Some(grounded),
        timestamp: now_micros(),
    };
    let plain_bytes = serde_json::to_vec(&plain)
        .map_err(|e| format!("Failed to serialize: {}", e))?;
    let key = manager.data_key()?;
    let (nonce, cipher) = crate::transcript_crypto::encrypt(&key, &plain_bytes)?;

    #[derive(serde::Serialize, Debug)]
    struct RecordMessageInput {
        conversation_hash: ActionHash,
        cipher: Vec<u8>,
        nonce: Vec<u8>,
    }

    let payload = ExternIO::encode(RecordMessageInput {
        conversation_hash: conv_hash,
        cipher,
        nonce: nonce.to_vec(),
    })
    .map_err(|e| format!("Failed to encode input: {}", e))?;

    let result = manager
        .call_zome(&agent_key, "transcript", "record_message", payload)
        .await?;

    let hash: ActionHash = ExternIO::decode(&result)
        .map_err(|e| format!("Failed to decode entry hash: {}", e))?;

    crate::vault_escrow::schedule_full_backup(&app);
    Ok(hex::encode(hash.get_raw_39()))
}

/// Project memory, Rust side - the agent's deliberate-memory MCP tools
/// write through the SAME chain paths as the UI. Mirrors the frontend's
/// merge-on-read rule: the newest revision across every provisioned
/// agent's chain wins; notes append to the writer's own conversation.
pub const WORKSPACE_MEMORY_SOURCE: &str = "workspace-memory";
pub const WORKSPACE_MEMORY_MAX_LINES: usize = 60;

// ── Project-memory read path: cache + index ─────────────────────────────────
// Reading a folder's memory used to walk EVERY AI's conversations and then
// every entry of every memory conversation - minutes on a busy machine, and
// the agent's read tool blocked on it. Two fixes, both invalidated by the only
// writers there are (the Rust append below and the frontend's recordMessage /
// startConversation, which both pass through this file):
//  - an in-process cache of the latest content per folder;
//  - a persisted index folder -> [(agent_key, conversation_hash)] so a cold
//    read touches only the memory conversations, never the whole catalogue.
static PROJECT_MEMORY_CACHE: std::sync::LazyLock<tokio::sync::Mutex<std::collections::HashMap<String, String>>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(std::collections::HashMap::new()));

fn project_memory_index_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    app.path().app_data_dir().ok().map(|d| d.join("project-memory-index.json"))
}

fn project_memory_index_load(app: &tauri::AppHandle) -> std::collections::HashMap<String, Vec<(String, String)>> {
    project_memory_index_path(app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn project_memory_index_save(app: &tauri::AppHandle, idx: &std::collections::HashMap<String, Vec<(String, String)>>) {
    if let Some(p) = project_memory_index_path(app) {
        if let Ok(b) = serde_json::to_vec(idx) {
            let _ = std::fs::write(p, b);
        }
    }
}

/// A memory conversation now exists for `folder` on `agent_key` - remember
/// where, so later reads go straight to it.
pub(crate) fn project_memory_index_add(app: &tauri::AppHandle, folder: &str, agent_key: &str, hash: &str) {
    let mut idx = project_memory_index_load(app);
    let list = idx.entry(folder.to_string()).or_default();
    if !list.iter().any(|(a, h)| a == agent_key && h == hash) {
        list.push((agent_key.to_string(), hash.to_string()));
        project_memory_index_save(app, &idx);
    }
}

/// Something wrote project memory (any folder) - drop cached content.
pub(crate) fn project_memory_cache_clear() {
    tauri::async_runtime::spawn(async {
        PROJECT_MEMORY_CACHE.lock().await.clear();
    });
}

/// The current memory for a folder: newest revision across all agents.
/// Returns (content, writer_hint) where writer_hint is the conversation
/// owner of the newest revision (unused by callers today, kept for parity).
pub async fn project_memory_read(
    app: &tauri::AppHandle,
    folder: &str,
) -> Result<String, String> {
    use tauri::Manager;
    if let Some(hit) = PROJECT_MEMORY_CACHE.lock().await.get(folder) {
        return Ok(hit.clone());
    }
    let started = std::time::Instant::now();
    let hc_state = app.state::<Arc<HolochainState>>();
    let manager = hc_state.get()?;
    // Fast path: the index names the memory conversations - read just those.
    let indexed = project_memory_index_load(app).remove(folder).unwrap_or_default();
    if !indexed.is_empty() {
        let mut best_ts = 0i64;
        let mut best = String::new();
        let mut ok = true;
        for (agent, hash) in &indexed {
            match get_conversation_transcript(agent.clone(), hash.clone(), app.state()).await {
                Ok(entries) => {
                    for e in entries {
                        if e.timestamp > best_ts {
                            best_ts = e.timestamp;
                            best = e.content;
                        }
                    }
                }
                Err(_) => { ok = false; break; }
            }
        }
        if ok {
            log::info!(
                "[project-memory] read {} via index ({} conv, {} ms)",
                folder, indexed.len(), started.elapsed().as_millis()
            );
            PROJECT_MEMORY_CACHE.lock().await.insert(folder.to_string(), best.clone());
            return Ok(best);
        }
        // An indexed conversation could not be read (rare: cell not up yet) -
        // fall through to the full walk, which rebuilds the index.
    }
    let agent_keys: Vec<String> = {
        let agents = manager.agents.lock().await;
        agents.keys().cloned().collect()
    };
    let mut best_ts = 0i64;
    let mut best = String::new();
    let mut found: Vec<(String, String)> = Vec::new();
    for key in agent_keys {
        let Ok(conversations) = get_conversations(app.clone(), key.clone(), app.state()).await else {
            continue;
        };
        for c in conversations {
            if c.source.as_deref() != Some(WORKSPACE_MEMORY_SOURCE) {
                continue;
            }
            if c.title.as_deref() != Some(folder) {
                continue;
            }
            found.push((c.agent_key.clone(), c.hash.clone()));
            let Ok(entries) = get_conversation_transcript(c.agent_key.clone(), c.hash.clone(), app.state()).await
            else {
                continue;
            };
            for e in entries {
                if e.timestamp > best_ts {
                    best_ts = e.timestamp;
                    best = e.content;
                }
            }
        }
    }
    log::info!(
        "[project-memory] read {} via full walk ({} conv found, {} ms)",
        folder, found.len(), started.elapsed().as_millis()
    );
    if !found.is_empty() {
        let mut idx = project_memory_index_load(app);
        idx.insert(folder.to_string(), found);
        project_memory_index_save(app, &idx);
    }
    PROJECT_MEMORY_CACHE.lock().await.insert(folder.to_string(), best.clone());
    Ok(best)
}

/// Append a note row to the folder's memory as a new revision on the
/// writer's chain (trimmed oldest-first under the cap - the next automatic
/// distillation curates it into shape).
pub async fn project_memory_append_note(
    app: &tauri::AppHandle,
    writer_key: &str,
    writer_label: &str,
    folder: &str,
    note: &str,
) -> Result<(), String> {
    use tauri::Manager;
    let capped: String = note
        .lines()
        .take(12)
        .collect::<Vec<_>>()
        .join(" / ")
        .chars()
        .take(1000)
        .collect();
    let capped = capped.trim();
    if capped.is_empty() {
        return Err("empty note".to_string());
    }
    let current = project_memory_read(app, folder).await?;
    let row = format!("- [saved by {}] {}", writer_label, capped);
    let mut lines: Vec<String> = current
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(str::to_string)
        .collect();
    lines.push(row);
    if lines.len() > WORKSPACE_MEMORY_MAX_LINES {
        let cut = lines.len() - WORKSPACE_MEMORY_MAX_LINES;
        lines.drain(0..cut);
    }
    let content = lines.join("\n");

    // Reuse the writer's memory conversation for this folder, else start one.
    let conversations = get_conversations(app.clone(), writer_key.to_string(), app.state()).await?;
    let existing = conversations.into_iter().find(|c| {
        c.source.as_deref() == Some(WORKSPACE_MEMORY_SOURCE) && c.title.as_deref() == Some(folder)
    });
    let (conv_key, hash, seq) = match existing {
        Some(c) => {
            let entries = get_conversation_transcript(c.agent_key.clone(), c.hash.clone(), app.state()).await?;
            let seq = entries.iter().map(|e| e.sequence).max().map(|s| s + 1).unwrap_or(0);
            (c.agent_key, c.hash, seq)
        }
        None => {
            let hash = start_conversation(
                app.clone(),
                writer_key.to_string(),
                writer_label.to_string(),
                "workspace-memory".to_string(),
                Some(folder.to_string()),
                Some(WORKSPACE_MEMORY_SOURCE.to_string()),
                app.state(),
            )
            .await?;
            (writer_key.to_string(), hash, 0)
        }
    };
    let content_for_cache = content.clone();
    record_transcript_entry(
        app.clone(),
        conv_key,
        hash,
        "assistant".to_string(),
        content,
        seq,
        "workspace-memory".to_string(),
        None,
        None,
        None,
        None,
        None,
        app.state(),
    )
    .await?;
    // record_transcript_entry cleared the cache (any workspace-memory write
    // does); put the content we just wrote back so the next read is instant.
    PROJECT_MEMORY_CACHE.lock().await.insert(folder.to_string(), content_for_cache);
    Ok(())
}

/// Tidy v2 (cell-lineage recovery Phase B, reworked): disable exactly the
/// cells a fresh census verifies as empty. Manual, reversible, beta-gated
/// in the UI; writes cell-tidy-log.json beside the census report.
#[tauri::command]
pub async fn cell_tidy(
    app: tauri::AppHandle,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    let manager = hc_state.get()?;
    let live = live_ais(&app);
    let result = manager.tidy_cells(&live).await?;
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = std::fs::write(
            dir.join("cell-tidy-log.json"),
            serde_json::to_string_pretty(&result).unwrap_or_default(),
        );
    }
    Ok(result)
}

/// Live AIs = the store's custom AIs that carry an agent key.
fn live_ais(app: &tauri::AppHandle) -> Vec<(String, String)> {
    use tauri_plugin_store::StoreExt;
    let mut live: Vec<(String, String)> = Vec::new();
    if let Ok(store) = app.store("ai-data.json") {
        if let Some(serde_json::Value::Array(arr)) = store.get("custom-ais") {
            for ai in arr {
                let key = ai.get("agentPubKey").and_then(|v| v.as_str()).unwrap_or("");
                let name = ai.get("name").and_then(|v| v.as_str()).unwrap_or("?");
                if !key.is_empty() {
                    live.push((key.to_string(), name.to_string()));
                }
            }
        }
    }
    live
}

/// Phase A of cell-lineage recovery: build the read-only cell census
/// (CELL_LINEAGE_RECOVERY.md) and save it next to the app data so it can
/// be reviewed. Changes no cell state.
#[tauri::command]
pub async fn cell_lineage_report(
    app: tauri::AppHandle,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<serde_json::Value, String> {
    use tauri::Manager;
    let manager = hc_state.get()?;
    let live = live_ais(&app);
    let report = manager.cell_lineage_report(&live).await?;

    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("No app data dir: {}", e))?;
    let path = dir.join("cell-lineage-report.json");
    std::fs::write(
        &path,
        serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("Failed to write report: {}", e))?;
    log::info!(
        "[cells] lineage report written to {} - {}",
        path.display(),
        report["summary"].to_string()
    );

    Ok(serde_json::json!({
        "summary": report["summary"],
        "path": path.to_string_lossy(),
    }))
}

/// Is the conductor up? The transcript surface reads empty during startup,
/// which is indistinguishable from "no conversations" - callers that would
/// show an empty state poll this first and keep their spinner up instead.
#[tauri::command]
pub fn holochain_ready(hc_state: State<'_, Arc<HolochainState>>) -> bool {
    hc_state.manager.get().is_some()
}

/// Get all conversations for an AI agent.
/// `agent_key` is the hex-encoded agent pub key.
#[tauri::command]
pub async fn get_conversations(
    app: tauri::AppHandle,
    agent_key: String,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<Vec<ConversationInfo>, String> {
    let manager = hc_state.get()?;

    // Merge conversations across the AI's whole agent lineage — earlier
    // generations hold conversations recorded before the AI was re-keyed
    // (see HolochainManager::agent_lineage). Without this, old transcripts
    // are invisible even though they're intact on disk.
    let lineage = manager.agent_lineage(&agent_key).await;
    if lineage.is_empty() {
        return Ok(vec![]);
    }

    let data_key = manager.data_key()?;
    let mut conversations = Vec::new();
    for key in &lineage {
        let payload = ExternIO::encode(())
            .map_err(|e| format!("Failed to encode: {}", e))?;

        let result = match manager
            .call_zome(key, "transcript", "get_all_conversations", payload)
            .await
        {
            Ok(r) => r,
            Err(e) => {
                log::warn!("get_all_conversations failed for lineage agent {}: {}", key, e);
                continue;
            }
        };

        let records: Vec<holochain_types::prelude::Record> = match ExternIO::decode(&result) {
            Ok(r) => r,
            Err(e) => {
                log::warn!("Failed to decode conversations for lineage agent {}: {}", key, e);
                continue;
            }
        };

        for record in &records {
            if let Some(entry) = record.entry().as_option() {
                if let Some(app_bytes) = entry.as_app_entry() {
                    let Ok(ee) = rmp_serde::from_slice::<EncryptedEntryRaw>(app_bytes.as_ref().bytes()) else { continue };
                    let Ok(plain_bytes) = crate::transcript_crypto::decrypt(&data_key, &ee.nonce, &ee.cipher) else {
                        log::warn!("Failed to decrypt a conversation entry (wrong key?)");
                        continue;
                    };
                    let Ok(conv) = serde_json::from_slice::<serde_json::Value>(&plain_bytes) else { continue };
                    let hash_encoded = hex::encode(record.action_address().get_raw_39());
                    conversations.push(ConversationInfo {
                        hash: hash_encoded,
                        ai_personality_id: conv["ai_personality_id"].as_str().unwrap_or("").to_string(),
                        ai_personality_name: conv["ai_personality_name"].as_str().unwrap_or("").to_string(),
                        model_used: conv["model_used"].as_str().unwrap_or("").to_string(),
                        started_at: conv["started_at"].as_i64().unwrap_or(0),
                        last_active_at: None,
                        title: conv["title"].as_str().map(|s| s.to_string()),
                        source: conv["source"].as_str().map(|s| s.to_string()),
                        agent_key: key.clone(),
                    });
                }
            }
        }
    }

    // Last activity lives only in the local cache (the records entry knows
    // when a conversation started, not when it was last continued) - carry
    // it over so a live refresh never resets the order.
    let known: std::collections::HashMap<String, i64> =
        crate::conversation_cache::read_cache(&app, &agent_key)
            .unwrap_or_default()
            .into_iter()
            .filter_map(|c| c.last_active_at.map(|t| (c.hash, t)))
            .collect();
    for c in conversations.iter_mut() {
        c.last_active_at = known.get(&c.hash).copied();
    }
    // Most recently active first, across all generations.
    conversations.sort_by_key(|c| std::cmp::Reverse(c.last_active_at.unwrap_or(c.started_at)));

    // Last-known-good cache: served instantly by get_conversations_cached
    // while live reads are slow or the conductor is still starting.
    if let Err(e) = crate::conversation_cache::write_cache(&app, &agent_key, &conversations) {
        log::warn!("[conv-cache] write failed: {}", e);
    }

    Ok(conversations)
}

/// The cached (last-known-good) conversation list for an AI - instant,
/// works even while the conductor starts. Empty when nothing cached yet.
#[tauri::command]
pub fn get_conversations_cached(
    app: tauri::AppHandle,
    agent_key: String,
) -> Vec<ConversationInfo> {
    crate::conversation_cache::read_cache(&app, &agent_key).unwrap_or_default()
}

/// Get all messages in a conversation.
/// `agent_key` is the hex-encoded agent pub key.
/// Delete a conversation from an AI's chain: tombstones the list link,
/// every message entry, and the conversation-metadata entry (author-only,
/// enforced by the integrity zome). The deletion itself is a signed chain
/// action - the record OF deleting remains; the content leaves all queries.
/// Returns the number of records tombstoned.
#[tauri::command]
pub async fn delete_conversation(
    app: tauri::AppHandle,
    agent_key: String,
    conversation_hash: String,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<u32, String> {
    let raw_bytes = hex::decode(&conversation_hash)
        .map_err(|e| format!("Invalid conversation hash hex: {}", e))?;
    let conv_hash = ActionHash::from_raw_39(raw_bytes);

    let payload = ExternIO::encode(conv_hash).map_err(|e| format!("Failed to encode: {}", e))?;
    let result = hc_state
        .get()?
        .call_zome(&agent_key, "transcript", "delete_conversation", payload)
        .await
        .map_err(|e| {
            // Frontend-only errors were invisible in every diagnostic; a
            // failed delete must leave a line here.
            log::warn!(
                "[transcripts] delete conversation {}… failed: {}",
                &conversation_hash[..12.min(conversation_hash.len())],
                e
            );
            e
        })?;
    let deleted: u32 =
        ExternIO::decode(&result).map_err(|e| format!("Failed to decode: {}", e))?;

    log::info!(
        "[transcripts] deleted conversation {}… ({} records tombstoned)",
        &conversation_hash[..12.min(conversation_hash.len())],
        deleted
    );
    crate::conversation_cache::remove_from_cache(&app, &agent_key, &conversation_hash);
    // The chain changed shape - refresh the escrow backup like other writes.
    crate::vault_escrow::schedule_full_backup(&app);
    Ok(deleted)
}

#[tauri::command]
pub async fn get_conversation_transcript(
    agent_key: String,
    conversation_hash: String,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<Vec<TranscriptEntryInfo>, String> {
    if !hc_state.get()?.is_provisioned(&agent_key).await {
        return Ok(vec![]);
    }

    let raw_bytes = hex::decode(&conversation_hash)
        .map_err(|e| format!("Invalid conversation hash hex: {}", e))?;
    let conv_hash = ActionHash::from_raw_39(raw_bytes);

    let payload = ExternIO::encode(conv_hash)
        .map_err(|e| format!("Failed to encode: {}", e))?;

    let result = hc_state.get()?
        .call_zome(&agent_key, "transcript", "get_conversation_entries", payload)
        .await?;

    let records: Vec<holochain_types::prelude::Record> = ExternIO::decode(&result)
        .map_err(|e| format!("Failed to decode transcript: {}", e))?;

    let data_key = hc_state.get()?.data_key()?;
    let mut entries = Vec::new();
    // Grounding annotations recorded after their message (on-demand "Verify
    // sources"), merged into the target message below. Records arrive
    // timestamp-sorted, so applying in order leaves the latest annotation.
    let mut annotations: Vec<(String, Option<Vec<GroundedSource>>)> = Vec::new();
    for record in records.iter() {
        if let Some(entry) = record.entry().as_option() {
            if let Some(app_bytes) = entry.as_app_entry() {
                let Ok(ee) = rmp_serde::from_slice::<EncryptedEntryRaw>(app_bytes.as_ref().bytes()) else { continue };
                let Ok(plain_bytes) = crate::transcript_crypto::decrypt(&data_key, &ee.nonce, &ee.cipher) else {
                    log::warn!("Failed to decrypt a transcript entry (wrong key?)");
                    continue;
                };
                // A grounding annotation (not a message) carries the marker.
                if let Ok(ann) = serde_json::from_slice::<GroundingAnnotationPlain>(&plain_bytes) {
                    if ann.annotation == "grounding" {
                        annotations.push((ann.target_hash, ann.grounded));
                        continue;
                    }
                }
                let e = match serde_json::from_slice::<MessagePlain>(&plain_bytes) {
                    Ok(e) => e,
                    Err(err) => {
                        // NEVER drop entries silently - a parse failure here
                        // reads as "the answer vanished" in the app.
                        log::warn!(
                            "Transcript entry failed to parse (dropped): {} - first 120 bytes: {}",
                            err,
                            String::from_utf8_lossy(&plain_bytes[..plain_bytes.len().min(120)])
                        );
                        continue;
                    }
                };
                entries.push(TranscriptEntryInfo {
                    hash: hex::encode(record.action_address().get_raw_39()),
                    role: e.role,
                    content: e.content,
                    sequence: e.sequence,
                    timestamp: e.timestamp,
                    model_hash: e.model_hash,
                    provider_fingerprint: e.provider_fingerprint,
                    model: e.model,
                    thinking: e.thinking,
                    tokens: e.tokens,
                    sources: e.sources,
                    system_prompt: e.system_prompt,
                    mode: e.mode,
                    attachments: e.attachments,
                    images: e.images,
                    grounded: e.grounded,
                    runtime: e.runtime,
                    routing_reason: e.routing_reason,
                    routing_task: e.routing_task,
                    stopped: e.stopped,
                    agent_log: e.agent_log,
                    folder_path: e.folder_path,
                });
            }
        }
    }

    // Fold grounding annotations into the messages they target (latest wins).
    for (target_hash, grounded) in annotations {
        if grounded.is_none() {
            continue;
        }
        if let Some(e) = entries.iter_mut().find(|e| e.hash == target_hash) {
            e.grounded = grounded;
        }
    }

    // Content-level ordering (sequence lives inside the ciphertext).
    entries.sort_by_key(|e| e.sequence);

    Ok(entries)
}


/// Check if an agent is provisioned on the Holochain conductor.
/// `ai_id` is checked by provisioning (which uses it as lair seed tag).
#[tauri::command]
pub async fn get_ai_holochain_status(
    ai_id: String,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<bool, String> {
    // This checks by AI id via provision_agent's scan
    let manager = hc_state.get()?;
    let agents = manager.agents.lock().await;
    Ok(agents.values().any(|a| a.installed_app_id.ends_with(&ai_id)))
}

/// Permanently uninstall an AI's transcript hApp(s) — the conductor half of
/// "purge". The frontend still removes the local config + memory facts; this
/// drops the signed transcripts from the conductor. Best-effort: callers
/// should proceed with local cleanup even if this errors (conductor may be
/// down). `ai_id` is the AI's current id (its agent pub key hex).
#[tauri::command]
pub async fn purge_ai_app(
    app: tauri::AppHandle,
    ai_id: String,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<u32, String> {
    let removed = hc_state.get()?.purge_agent(&ai_id).await?;
    log::info!("Purged AI {}: removed {} hApp(s)", ai_id, removed);
    crate::vault_escrow::schedule_full_backup(&app);
    Ok(removed)
}

/// Write a caller-supplied text file (e.g. an exported conversation
/// transcript) to the user's Downloads folder and return the path.
/// The filename is sanitised to a single, safe path component.
#[tauri::command]
pub fn save_text_download(
    app: tauri::AppHandle,
    filename: String,
    content: String,
) -> Result<String, String> {
    use tauri::Manager;
    let mut safe: String = filename
        .chars()
        .map(|c| if c.is_alphanumeric() || matches!(c, '-' | '_' | '.' | ' ') { c } else { '_' })
        .collect();
    safe = safe.trim().to_string();
    if safe.is_empty() {
        safe = "export.txt".to_string();
    }
    let dir = app
        .path()
        .download_dir()
        .map_err(|e| format!("No downloads directory: {}", e))?;
    let path = dir.join(safe);
    std::fs::write(&path, content).map_err(|e| format!("Failed to write: {}", e))?;
    Ok(path.display().to_string())
}

#[cfg(test)]
mod size_guard_tests {
    use super::*;

    fn plain_with(content: &str, log: Option<serde_json::Value>) -> MessagePlain {
        MessagePlain {
            role: "assistant".into(),
            model_hash: None,
            provider_fingerprint: None,
            content: content.into(),
            sequence: 1,
            timestamp: 0,
            model: "gpt-5.6-sol".into(),
            thinking: None,
            tokens: None,
            sources: None,
            system_prompt: None,
            mode: None,
            attachments: None,
            images: None,
            grounded: None,
            runtime: None,
            routing_reason: None,
            routing_task: None,
            stopped: None,
            agent_log: log,
            folder_path: None,
        }
    }

    fn size(p: &MessagePlain) -> usize {
        serde_json::to_vec(p).unwrap().len()
    }

    #[test]
    fn small_entries_pass_untouched() {
        let mut p = plain_with(
            "a normal answer",
            Some(serde_json::json!({"items": [{"type": "thought", "text": "hi"}]})),
        );
        let before = serde_json::to_vec(&p).unwrap();
        shrink_to_budget(&mut p);
        assert_eq!(serde_json::to_vec(&p).unwrap(), before);
    }

    #[test]
    fn huge_log_fits_and_keeps_the_words() {
        // 99-tool-session shape: many bulky items. Must end under budget
        // with content intact.
        let big_text = "x".repeat(8_000);
        let items: Vec<serde_json::Value> = (0..300)
            .map(|i| {
                serde_json::json!({
                    "id": format!("a{i}"),
                    "type": if i % 3 == 0 { "thought" } else { "action" },
                    "text": big_text,
                    "action": {"label": "Running a command", "detail": big_text, "status": "completed"},
                })
            })
            .collect();
        let mut p = plain_with("the real answer", Some(serde_json::json!({"items": items})));
        assert!(size(&p) > PLAIN_BUDGET);
        shrink_to_budget(&mut p);
        assert!(size(&p) <= PLAIN_BUDGET);
        assert_eq!(p.content, "the real answer");
        // Thoughts went first.
        let items = p.agent_log.as_ref().unwrap()["items"].as_array().unwrap();
        assert!(items.iter().all(|i| i["type"] != "thought"));
    }

    #[test]
    fn giant_content_truncates_with_marker() {
        let mut p = plain_with(&"y".repeat(2_000_000), None);
        shrink_to_budget(&mut p);
        assert!(size(&p) <= PLAIN_BUDGET);
        assert!(p.content.ends_with("..[trimmed to fit the transcript]"));
    }

    #[test]
    fn budget_stays_under_the_zome_cap() {
        // The zome rejects cipher over 1 MiB; encryption overhead is a small
        // constant, so the plaintext budget must sit safely below it.
        assert!(PLAIN_BUDGET + 1024 < 1_048_576);
    }
}
