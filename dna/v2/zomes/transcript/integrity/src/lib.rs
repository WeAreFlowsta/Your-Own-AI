//! Transcript DNA v2 integrity zome (Holochain 0.7.1).
//!
//! Every record is an encrypted entry, sealed on the device with the
//! person's data key; this zome never sees plaintext. What a record is (a
//! conversation, a message, a memory row, a tombstone) is told by its link
//! type and tag only, and the network seed is per person, so only the
//! person's own devices ever see even the link graph.
//!
//! Records carry an id inside their plaintext. Each link carries the keyed
//! hash of its record's id in its tag (the key is the person's data key, so
//! the tag reveals nothing), which lets the coordinator hide, collapse and
//! list by id without decrypting:
//!   - a conversation's list link: `c` + its anchor (an `ExternalHash`
//!     derived from the conversation's id), from the AI's founding key;
//!   - a message's link: `m` + the keyed hash of its id, from its
//!     conversation's anchor;
//!   - a memory row's link: `mem`, from its scope anchor;
//!   - a tombstone: `tomb`, from the founding key to the conversation's
//!     anchor.
//!
//! Who may change what. Holochain's system validation checks no author on
//! a delete, an update or a link removal, so each is checked here:
//!   - nothing is ever updated: a revision is a new entry;
//!   - an entry is deleted by its author only;
//!   - a link is created by the author of the entry it points at (a
//!     tombstone points at an anchor and is any of the person's devices'),
//!     and removed by its own author only.

use hdi::prelude::*;

/// The largest ciphertext an entry may hold (1 MiB). Peers keep whatever
/// validation allows, so it is bounded.
pub const MAX_CIPHER_BYTES: usize = 1_048_576;

/// The nonce length of XSalsa20-Poly1305.
pub const NONCE_BYTES: usize = 24;

/// A list link's tag prefix: `c` + the conversation's 39-byte anchor.
pub const CONVERSATION_TAG: u8 = b'c';
/// A message link's tag prefix: `m` + the 32-byte keyed hash of its id.
pub const MESSAGE_TAG: u8 = b'm';
/// A memory row's link tag.
pub const MEMORY_TAG: &[u8] = b"mem";
/// A tombstone's link tag.
pub const TOMBSTONE_TAG: &[u8] = b"tomb";

/// One encrypted record: a conversation, a message or a memory row.
#[hdk_entry_helper]
#[derive(Clone, PartialEq)]
pub struct EncryptedEntry {
    /// XSalsa20-Poly1305 ciphertext under the person's data key.
    pub cipher: Vec<u8>,
    /// The 24-byte random nonce.
    pub nonce: Vec<u8>,
}

#[hdk_entry_types]
#[unit_enum(UnitEntryTypes)]
#[derive(Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum EntryTypes {
    EncryptedEntry(EncryptedEntry),
}

#[derive(Serialize, Deserialize)]
#[hdk_link_types]
pub enum LinkTypes {
    /// From an AI's founding key: its conversations (`c`) and tombstones
    /// (`tomb`). From a scope anchor: memory rows (`mem`).
    AllConversations,
    /// From a conversation's anchor: its messages (`m`).
    ConversationToEntries,
}

/// What a list link's tag says, if it is well formed.
#[derive(Debug, PartialEq, Eq)]
pub enum ListTag {
    Conversation,
    Memory,
    Tombstone,
}

/// Read a list link's tag: `c` + 39 bytes, `mem` or `tomb`.
pub fn list_tag(tag: &[u8]) -> Option<ListTag> {
    match tag {
        [CONVERSATION_TAG, anchor @ ..] if anchor.len() == 39 => Some(ListTag::Conversation),
        t if t == MEMORY_TAG => Some(ListTag::Memory),
        t if t == TOMBSTONE_TAG => Some(ListTag::Tombstone),
        _ => None,
    }
}

/// Whether a message link's tag is `m` + 32 bytes.
pub fn is_message_tag(tag: &[u8]) -> bool {
    matches!(tag, [MESSAGE_TAG, key @ ..] if key.len() == 32)
}

fn invalid(reason: impl Into<String>) -> ExternResult<ValidateCallbackResult> {
    Ok(ValidateCallbackResult::Invalid(reason.into()))
}

fn valid() -> ExternResult<ValidateCallbackResult> {
    Ok(ValidateCallbackResult::Valid)
}

#[hdk_extern]
pub fn validate(op: Op) -> ExternResult<ValidateCallbackResult> {
    match op.flattened::<EntryTypes, LinkTypes>()? {
        FlatOp::CreateEntry(op_entry) => match op_entry {
            OpEntry::CreateEntry { app_entry, .. } => match app_entry {
                EntryTypes::EncryptedEntry(ee) => validate_encrypted_entry(&ee),
            },
            OpEntry::UpdateEntry { .. } => invalid("Records here are never updated"),
            _ => valid(),
        },
        FlatOp::CreateRecord(OpRecord::UpdateEntry { .. }) | FlatOp::Update(OpUpdate::Entry { .. }) => {
            invalid("Records here are never updated")
        }
        FlatOp::CreateRecord(OpRecord::DeleteEntry { action }) | FlatOp::Delete(OpDelete { action }) => {
            let original = must_get_action(action.data.deletes_address.clone())?;
            if original.action().author() == action.author() {
                valid()
            } else {
                invalid("Only a record's author can delete it")
            }
        }
        FlatOp::Link(OpLink::CreateLink { link_type, action }) => {
            let (base, target, tag, author) = (&action.data.base_address, &action.data.target_address, &action.data.tag, action.author());
            match link_type {
                LinkTypes::AllConversations => match list_tag(&tag.0) {
                    Some(ListTag::Conversation) => {
                        if base.clone().into_agent_pub_key().is_none() {
                            return invalid("A conversation is listed under the AI's founding key");
                        }
                        filed_by_its_author(target, author)
                    }
                    Some(ListTag::Memory) => {
                        if base.clone().into_agent_pub_key().is_none() && base.clone().into_external_hash().is_none() {
                            return invalid("A memory row is listed under a scope anchor");
                        }
                        filed_by_its_author(target, author)
                    }
                    Some(ListTag::Tombstone) => {
                        if base.clone().into_agent_pub_key().is_none() {
                            return invalid("A tombstone is listed under the AI's founding key");
                        }
                        if target.clone().into_external_hash().is_none() {
                            return invalid("A tombstone names a conversation's anchor");
                        }
                        valid()
                    }
                    None => invalid("A list link's tag is c + an anchor, mem or tomb"),
                },
                LinkTypes::ConversationToEntries => {
                    if base.clone().into_external_hash().is_none() {
                        return invalid("A message is filed under its conversation's anchor");
                    }
                    if !is_message_tag(&tag.0) {
                        return invalid("A message link's tag is m + the keyed hash of its id");
                    }
                    filed_by_its_author(target, author)
                }
            }
        }
        FlatOp::Link(OpLink::DeleteLink { original_action, action, .. }) => {
            if original_action.author() == action.author() {
                valid()
            } else {
                invalid("Only a link's author can remove it")
            }
        }
        _ => valid(),
    }
}

/// A link to an encrypted entry, made by that entry's author.
fn filed_by_its_author(target: &AnyLinkableHash, author: &AgentPubKey) -> ExternResult<ValidateCallbackResult> {
    let Some(hash) = target.clone().into_action_hash() else {
        return invalid("This link points at a record");
    };
    let record = must_get_valid_record(hash)?;
    let (Some(EntryType::App(def)), Some(entry)) = (record.action().entry_type(), record.entry().as_option()) else {
        return invalid("This link points at an encrypted entry");
    };
    match EntryTypes::deserialize_from_type(def.zome_index, def.entry_index, entry)? {
        Some(EntryTypes::EncryptedEntry(_)) if record.action().author() == author => valid(),
        Some(EntryTypes::EncryptedEntry(_)) => invalid("A record is filed by its own author"),
        None => invalid("This link points at an encrypted entry"),
    }
}

fn validate_encrypted_entry(ee: &EncryptedEntry) -> ExternResult<ValidateCallbackResult> {
    if ee.cipher.is_empty() {
        return invalid("Cipher cannot be empty");
    }
    if ee.cipher.len() > MAX_CIPHER_BYTES {
        return invalid(format!("Cipher exceeds maximum size ({} > {} bytes)", ee.cipher.len(), MAX_CIPHER_BYTES));
    }
    if ee.nonce.len() != NONCE_BYTES {
        return invalid("Nonce must be 24 bytes");
    }
    valid()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_tags_are_c_and_an_anchor_mem_or_tomb() {
        let mut c = vec![CONVERSATION_TAG];
        c.extend([1u8; 39]);
        assert_eq!(list_tag(&c), Some(ListTag::Conversation));
        assert_eq!(list_tag(b"mem"), Some(ListTag::Memory));
        assert_eq!(list_tag(b"tomb"), Some(ListTag::Tombstone));
        assert_eq!(list_tag(&c[..30]), None, "a short anchor");
        assert_eq!(list_tag(b""), None, "v1's untagged list link is not a v2 tag");
        assert_eq!(list_tag(b"c"), None);
    }

    #[test]
    fn message_tags_are_m_and_32_bytes() {
        let mut m = vec![MESSAGE_TAG];
        m.extend([2u8; 32]);
        assert!(is_message_tag(&m));
        assert!(!is_message_tag(&m[..20]));
        assert!(!is_message_tag(b""));
    }
}
