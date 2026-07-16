//! Phase A persistent memory — the user-level "remember me" profile.
//!
//! A single encrypted blob (XSalsa20-Poly1305 under the same user data key
//! the transcripts use) holding the derived profile facts. One key, one file
//! per install, so the store is naturally user-level. The index is a *cache*
//! rebuildable from the immutable transcripts; `owner_scope`/`key_scope` are
//! "personal" in Phase A — the hooks for group-scoped memory later.
//!
//! This module is a dumb encrypted store: the frontend owns extraction and
//! reconciliation; here we only load/persist the full fact set.

use crate::commands_holochain::HolochainState;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{Manager, State};

const FACTS_FILE: &str = "memory-facts.enc";

/// A durable fact/preference about the user (temporal, provenance-linked).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MemoryFact {
    pub id: String,
    pub subject: String,
    pub predicate: String,
    pub value: String,
    pub confidence: f32,
    pub valid_from: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<i64>,
    /// Provenance: the signed transcript entry this was learned from.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_action_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_ai_id: Option<String>,
    /// "personal" in Phase A; "group:<agreement_hash>" later.
    #[serde(default = "personal_scope")]
    pub owner_scope: String,
    /// Which key encrypts the item (personal now; group key later).
    #[serde(default = "personal_scope")]
    pub key_scope: String,
    /// "fact" (structured triple) or "note" (free-text, retrieved). Legacy
    /// records predate the field → default to "fact".
    #[serde(default = "fact_kind")]
    pub entry_kind: String,
    /// How this entry came to be: "extracted" (learned from chat), "signed"
    /// (extracted + linked to a signed transcript entry), or "authored" (the
    /// user added it directly). Legacy records → "extracted".
    #[serde(default = "extracted_provenance")]
    pub provenance: String,
    /// The Flowsta identity that signed an authored entry, once the optional
    /// Vault-signing upgrade lands (B3). Null until then.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub author_identity: Option<String>,
    /// Embedding vector for retrieval (notes/chunks). Facts that are always
    /// injected don't need one, so it's optional. Stored here so it survives
    /// the save round-trip (serde would otherwise drop an unknown field).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub embedding: Option<Vec<f32>>,
    /// Groups the chunk-notes of one long remembered passage so they can be
    /// forgotten together. Absent for everything else.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

fn personal_scope() -> String {
    "personal".to_string()
}

fn fact_kind() -> String {
    "fact".to_string()
}

fn extracted_provenance() -> String {
    "extracted".to_string()
}

/// On-disk envelope: hex-encoded nonce + ciphertext of the JSON `Vec<MemoryFact>`.
#[derive(Serialize, Deserialize)]
struct EncryptedFactsFile {
    version: u32,
    nonce: String,
    cipher: String,
}

/// Load all profile facts (decrypted). Returns empty if none recorded yet.
#[tauri::command]
pub fn get_memory_facts(
    app: tauri::AppHandle,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<Vec<MemoryFact>, String> {
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("No app data dir: {}", e))?;
    let path = dir.join(FACTS_FILE);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read memory facts: {}", e))?;
    let file: EncryptedFactsFile =
        serde_json::from_str(&raw).map_err(|e| format!("Failed to parse memory facts: {}", e))?;

    let key = hc_state.get()?.data_key()?;
    let nonce = hex::decode(&file.nonce).map_err(|e| format!("Bad nonce: {}", e))?;
    let cipher = hex::decode(&file.cipher).map_err(|e| format!("Bad cipher: {}", e))?;
    let plain = crate::transcript_crypto::decrypt(&key, &nonce, &cipher)?;
    serde_json::from_slice(&plain).map_err(|e| format!("Failed to deserialize facts: {}", e))
}

/// Persist the full fact set (encrypted, 0600). The frontend owns
/// reconciliation; this overwrites the blob with the supplied set.
#[tauri::command]
pub fn save_memory_facts(
    app: tauri::AppHandle,
    facts: Vec<MemoryFact>,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<(), String> {
    let key = hc_state.get()?.data_key()?;
    let plain =
        serde_json::to_vec(&facts).map_err(|e| format!("Failed to serialize facts: {}", e))?;
    let (nonce, cipher) = crate::transcript_crypto::encrypt(&key, &plain)?;
    let file = EncryptedFactsFile {
        version: 1,
        nonce: hex::encode(nonce),
        cipher: hex::encode(cipher),
    };

    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("No app data dir: {}", e))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("Failed to create app data dir: {}", e))?;
    let path = dir.join(FACTS_FILE);
    let json =
        serde_json::to_string_pretty(&file).map_err(|e| format!("Failed to serialize file: {}", e))?;
    std::fs::write(&path, json).map_err(|e| format!("Failed to write memory facts: {}", e))?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}
