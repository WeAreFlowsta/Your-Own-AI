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
#[derive(Serialize, Clone)]
pub struct ConversationInfo {
    pub hash: String,
    pub ai_personality_id: String,
    pub ai_personality_name: String,
    pub model_used: String,
    pub started_at: i64,
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
    /// The classified routing task for this turn ("code" | "math" |
    /// "reasoning" | "general").
    #[serde(default)]
    pub routing_task: Option<String>,
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
#[tauri::command]
pub async fn provision_all_agents(
    ai_ids: Vec<String>,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<std::collections::HashMap<String, String>, String> {
    let manager = hc_state.get()?;
    let mut results = std::collections::HashMap::new();

    for ai_id in ai_ids {
        match manager.provision_agent(&ai_id).await {
            Ok(key) => {
                results.insert(ai_id, key);
            }
            Err(e) => {
                log::warn!("Failed to provision agent for AI {}: {}", ai_id, e);
            }
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
    let plain = serde_json::json!({
        "ai_personality_id": agent_key,
        "ai_personality_name": ai_name,
        "model_used": model,
        "started_at": now_micros(),
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
    Ok(hex::encode(raw))
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
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<String, String> {
    // Decode the conversation hash from raw hex (39 bytes)
    let raw_bytes = hex::decode(&conversation_hash)
        .map_err(|e| format!("Invalid conversation hash hex: {}", e))?;
    let conv_hash = ActionHash::from_raw_39(raw_bytes);

    let manager = hc_state.get()?;

    // Phase A: message content is encrypted with the user data key.
    let prov = provenance.unwrap_or_default();
    let plain = MessagePlain {
        role,
        content,
        sequence,
        timestamp: now_micros(),
        model,
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

/// Get all conversations for an AI agent.
/// `agent_key` is the hex-encoded agent pub key.
#[tauri::command]
pub async fn get_conversations(
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
                        title: conv["title"].as_str().map(|s| s.to_string()),
                        source: conv["source"].as_str().map(|s| s.to_string()),
                        agent_key: key.clone(),
                    });
                }
            }
        }
    }

    // Newest first across all generations.
    conversations.sort_by(|a, b| b.started_at.cmp(&a.started_at));

    Ok(conversations)
}

/// Get all messages in a conversation.
/// `agent_key` is the hex-encoded agent pub key.
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
                let Ok(e) = serde_json::from_slice::<MessagePlain>(&plain_bytes) else { continue };
                entries.push(TranscriptEntryInfo {
                    hash: hex::encode(record.action_address().get_raw_39()),
                    role: e.role,
                    content: e.content,
                    sequence: e.sequence,
                    timestamp: e.timestamp,
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

/// Export the transcript recovery material (data key + network seed) to
/// the user's Downloads folder. Standalone users MUST export this (or
/// later escrow it via Vault) — without it, the encrypted transcripts
/// are unrecoverable after device loss, even from their own nodes.
#[tauri::command]
pub async fn export_transcript_recovery(
    app: tauri::AppHandle,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<String, String> {
    use tauri::Manager;
    let material = hc_state.get()?.recovery.clone();
    let json = serde_json::to_string_pretty(&serde_json::json!({
        "_readme": "Your Own AI transcript recovery material. Keep this file safe and private — anyone with it can decrypt your AI conversation transcripts, and without it your transcripts cannot be recovered after device loss.",
        "version": material.version,
        "network_seed": material.network_seed,
        "data_key_hex": material.data_key_hex,
    }))
    .map_err(|e| format!("Failed to serialize: {}", e))?;

    let dir = app
        .path()
        .download_dir()
        .map_err(|e| format!("No downloads directory: {}", e))?;
    let path = dir.join("yoai-transcript-recovery.json");
    std::fs::write(&path, json).map_err(|e| format!("Failed to write: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }

    Ok(path.display().to_string())
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
