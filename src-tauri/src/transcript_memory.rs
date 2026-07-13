//! Per-AI episodic memory — embedded conversation turns, for recall.
//!
//! Mirrors `memory.rs` (an encrypted blob under the user `data_key`) but stored
//! PER AI: each AI gets its own file, so the index stays bounded and an AI only
//! ever recalls its OWN conversations. The vectors are a rebuildable cache over
//! the canonical Holochain transcripts — safe to delete and re-derive.

use crate::commands_holochain::HolochainState;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{Manager, State};

/// One embedded entry in an AI's memory — a conversation turn (kind
/// "episodic") or a piece of knowledge the user authored for this AI (kind
/// "authored", the basis of lore/knowledge packs).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TranscriptEmbedding {
    pub id: String,
    /// Which conversation it belongs to — lets recall exclude the current one.
    /// "authored" for user-given knowledge (not a real conversation).
    pub conversation_hash: String,
    pub role: String, // "user" | "assistant" | "authored"
    pub text: String,
    pub vector: Vec<f32>,
    /// "episodic" (learned from chat) or "authored" (knowledge the user gave
    /// this AI). Legacy entries predate the field → default to "episodic".
    #[serde(default = "episodic_kind")]
    pub kind: String,
    pub created_at: i64,
}

fn episodic_kind() -> String {
    "episodic".to_string()
}

#[derive(Serialize, Deserialize)]
struct EncryptedEmbeddingsFile {
    version: u32,
    nonce: String,
    cipher: String,
}

/// Keep the filename safe + bounded: only alphanumerics from the AI id.
fn safe_id(ai_id: &str) -> String {
    ai_id
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .take(80)
        .collect()
}

fn file_for(app: &tauri::AppHandle, ai_id: &str) -> Result<std::path::PathBuf, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("No app data dir: {}", e))?;
    Ok(dir.join(format!("transcript-emb-{}.enc", safe_id(ai_id))))
}

/// Decrypt this AI's embedding store straight from disk - the non-command
/// path used by the Vault backup/restore, which already hold the data key.
pub(crate) fn read_for(
    app: &tauri::AppHandle,
    key: &[u8; 32],
    ai_id: &str,
) -> Result<Vec<TranscriptEmbedding>, String> {
    let path = file_for(app, ai_id)?;
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read transcript embeddings: {}", e))?;
    let file: EncryptedEmbeddingsFile =
        serde_json::from_str(&raw).map_err(|e| format!("Failed to parse: {}", e))?;
    let nonce = hex::decode(&file.nonce).map_err(|e| format!("Bad nonce: {}", e))?;
    let cipher = hex::decode(&file.cipher).map_err(|e| format!("Bad cipher: {}", e))?;
    let plain = crate::transcript_crypto::decrypt(key, &nonce, &cipher)?;
    serde_json::from_slice(&plain).map_err(|e| format!("Failed to deserialize: {}", e))
}

/// Load this AI's embedded turns (decrypted). Empty if none yet.
#[tauri::command]
pub fn get_transcript_embeddings(
    app: tauri::AppHandle,
    ai_id: String,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<Vec<TranscriptEmbedding>, String> {
    let key = hc_state.get()?.data_key()?;
    read_for(&app, &key, &ai_id)
}

/// Persist this AI's embedded turns (encrypted, 0600). The frontend owns the
/// set (append + cap); this overwrites the blob.
#[tauri::command]
pub fn save_transcript_embeddings(
    app: tauri::AppHandle,
    ai_id: String,
    items: Vec<TranscriptEmbedding>,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<(), String> {
    let key = hc_state.get()?.data_key()?;
    write_for(&app, &key, &ai_id, &items)
}

/// Encrypt + write this AI's embedding store - the non-command counterpart
/// of `read_for`, used by the Vault restore.
pub(crate) fn write_for(
    app: &tauri::AppHandle,
    key: &[u8; 32],
    ai_id: &str,
    items: &[TranscriptEmbedding],
) -> Result<(), String> {
    let plain = serde_json::to_vec(items).map_err(|e| format!("Failed to serialize: {}", e))?;
    let (nonce, cipher) = crate::transcript_crypto::encrypt(key, &plain)?;
    let file = EncryptedEmbeddingsFile {
        version: 1,
        nonce: hex::encode(nonce),
        cipher: hex::encode(cipher),
    };
    let path = file_for(app, ai_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create app data dir: {}", e))?;
    }
    let json = serde_json::to_string_pretty(&file)
        .map_err(|e| format!("Failed to serialize file: {}", e))?;
    std::fs::write(&path, json).map_err(|e| format!("Failed to write: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}
