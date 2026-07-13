//! Transcript DNA integrity zome — encrypted entries (Phase A).
//!
//! All transcript content (conversation metadata AND messages) is
//! encrypted client-side with the user's data key (XSalsa20-Poly1305)
//! before committing. The entry body reveals nothing; object kind is
//! conveyed by which link type points at it. Because the network seed
//! is per-user (set at install time), the only peers that can ever see
//! even the link graph are the user's own devices/nodes.
//!
//! Validation applies the lessons from the ProofPoll encrypted-entry
//! audit (2026-06-11): ciphertext size cap, nonce length check, and
//! author-only deletes enforced at the integrity level.

use hdi::prelude::*;

/// Maximum ciphertext size per entry (1 MiB). Peers must replicate
/// whatever validation allows; long assistant messages with thinking
/// content can be large, but unbounded entries invite pathological
/// bloat.
pub const MAX_CIPHER_BYTES: usize = 1_048_576;

/// An encrypted transcript object — either conversation metadata or a
/// single message. Which one is conveyed by link type, never by the
/// entry itself.
#[hdk_entry_helper]
#[derive(Clone, PartialEq)]
pub struct EncryptedEntry {
    /// XSalsa20-Poly1305 ciphertext (user data key).
    pub cipher: Vec<u8>,
    /// 24-byte random nonce.
    pub nonce: Vec<u8>,
}

/// Migration mapping for future DNA versions (old hash → new hash).
/// Single-user simplification of ProofPoll's anchor-based pattern —
/// no cross-user gossip/pending machinery is needed because every
/// peer in this DHT is the same user.
#[hdk_entry_helper]
#[derive(Clone, PartialEq)]
pub struct MigratedEntry {
    /// Hex-encoded action hash on the previous DNA version.
    pub old_hash_hex: String,
    /// The recreated entry's hash on this version.
    pub new_hash: ActionHash,
    /// Version the entry came from, e.g. "v1".
    pub from_version: String,
}

#[hdk_entry_types]
#[unit_enum(UnitEntryTypes)]
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EntryTypes {
    EncryptedEntry(EncryptedEntry),
    MigratedEntry(MigratedEntry),
}

#[derive(Serialize, Deserialize)]
#[hdk_link_types]
pub enum LinkTypes {
    /// Agent pub key → encrypted conversation entries.
    AllConversations,
    /// Conversation action hash → encrypted message entries.
    ConversationToEntries,
    /// Agent pub key → MigratedEntry records (future versions).
    MigrationIndex,
}

#[hdk_extern]
pub fn validate(op: Op) -> ExternResult<ValidateCallbackResult> {
    // Author-only deletes, enforced where a modified client can't bypass
    // it. Transcripts have no delete path today; this is belt-and-braces
    // for future features and forks.
    if let Op::RegisterDelete(register_delete) = &op {
        let delete = &register_delete.delete;
        let original = must_get_action(delete.hashed.deletes_address.clone())?;
        if *original.hashed.author() != delete.hashed.author {
            return Ok(ValidateCallbackResult::Invalid(
                "Only the author can delete an entry".to_string(),
            ));
        }
    }

    match op.flattened::<EntryTypes, LinkTypes>()? {
        FlatOp::StoreEntry(store_entry) => match store_entry {
            OpEntry::CreateEntry { app_entry, .. } | OpEntry::UpdateEntry { app_entry, .. } => {
                match app_entry {
                    EntryTypes::EncryptedEntry(ee) => validate_encrypted_entry(&ee),
                    EntryTypes::MigratedEntry(_) => Ok(ValidateCallbackResult::Valid),
                }
            }
            _ => Ok(ValidateCallbackResult::Valid),
        },
        _ => Ok(ValidateCallbackResult::Valid),
    }
}

fn validate_encrypted_entry(ee: &EncryptedEntry) -> ExternResult<ValidateCallbackResult> {
    if ee.cipher.is_empty() {
        return Ok(ValidateCallbackResult::Invalid(
            "Cipher cannot be empty".to_string(),
        ));
    }
    if ee.cipher.len() > MAX_CIPHER_BYTES {
        return Ok(ValidateCallbackResult::Invalid(format!(
            "Cipher exceeds maximum size ({} > {} bytes)",
            ee.cipher.len(),
            MAX_CIPHER_BYTES
        )));
    }
    if ee.nonce.len() != 24 {
        return Ok(ValidateCallbackResult::Invalid(
            "Nonce must be 24 bytes".to_string(),
        ));
    }
    Ok(ValidateCallbackResult::Valid)
}
