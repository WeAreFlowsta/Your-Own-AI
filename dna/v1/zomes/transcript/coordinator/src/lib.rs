//! Transcript DNA coordinator zome — encrypted entries (Phase A).
//!
//! The zome never sees plaintext: the Tauri layer encrypts conversation
//! metadata and messages with the user's data key before calling in,
//! and decrypts after reading. Ordering uses action timestamps (content
//! ordering by `sequence` happens client-side after decryption).

use hdk::prelude::*;
use transcript_integrity::*;

#[hdk_dependent_entry_types]
enum EntryZomes {
    IntegrityTranscript(transcript_integrity::EntryTypes),
}

/// Use the agent's public key as the anchor base for conversation links.
/// Each AI personality has its own agent, so this naturally scopes
/// conversations per AI.
fn agent_anchor() -> ExternResult<AgentPubKey> {
    Ok(agent_info()?.agent_initial_pubkey)
}

/// Ciphertext payload for creating an encrypted object.
#[derive(Serialize, Deserialize, Debug)]
pub struct EncryptedInput {
    pub cipher: Vec<u8>,
    pub nonce: Vec<u8>,
}

/// Start a new conversation (encrypted metadata). Returns the action hash.
#[hdk_extern]
pub fn start_conversation(input: EncryptedInput) -> ExternResult<ActionHash> {
    let entry = EncryptedEntry {
        cipher: input.cipher,
        nonce: input.nonce,
    };
    let hash = create_entry(&EntryZomes::IntegrityTranscript(
        EntryTypes::EncryptedEntry(entry),
    ))?;

    let agent = agent_anchor()?;
    create_link(agent, hash.clone(), LinkTypes::AllConversations, ())?;

    Ok(hash)
}

/// Input for recording an encrypted message.
#[derive(Serialize, Deserialize, Debug)]
pub struct RecordMessageInput {
    pub conversation_hash: ActionHash,
    pub cipher: Vec<u8>,
    pub nonce: Vec<u8>,
}

/// Record a single (encrypted) message in a conversation.
#[hdk_extern]
pub fn record_message(input: RecordMessageInput) -> ExternResult<ActionHash> {
    let entry = EncryptedEntry {
        cipher: input.cipher,
        nonce: input.nonce,
    };
    let hash = create_entry(&EntryZomes::IntegrityTranscript(
        EntryTypes::EncryptedEntry(entry),
    ))?;

    create_link(
        input.conversation_hash,
        hash.clone(),
        LinkTypes::ConversationToEntries,
        (),
    )?;

    Ok(hash)
}

/// Get all conversations for this agent (encrypted records).
#[hdk_extern]
pub fn get_all_conversations(_: ()) -> ExternResult<Vec<Record>> {
    let agent = agent_anchor()?;
    let links = get_links(
        LinkQuery::try_new(agent, LinkTypes::AllConversations)?,
        GetStrategy::default(),
    )?;

    let mut conversations = Vec::new();
    for link in links {
        let hash = ActionHash::try_from(link.target)
            .map_err(|_| wasm_error!("Invalid conversation hash"))?;
        if let Some(record) = get(hash, GetOptions::default())? {
            conversations.push(record);
        }
    }

    Ok(conversations)
}

/// Get all (encrypted) messages in a conversation, ordered by action
/// timestamp. Content-level ordering by `sequence` happens client-side
/// after decryption.
#[hdk_extern]
pub fn get_conversation_entries(conversation_hash: ActionHash) -> ExternResult<Vec<Record>> {
    let links = get_links(
        LinkQuery::try_new(conversation_hash, LinkTypes::ConversationToEntries)?,
        GetStrategy::default(),
    )?;

    let mut entries = Vec::new();
    for link in links {
        let hash = ActionHash::try_from(link.target)
            .map_err(|_| wasm_error!("Invalid entry hash"))?;
        if let Some(record) = get(hash, GetOptions::default())? {
            entries.push(record);
        }
    }

    entries.sort_by_key(|record| record.action().timestamp());

    Ok(entries)
}

/// Get a single conversation record.
#[hdk_extern]
pub fn get_conversation(action_hash: ActionHash) -> ExternResult<Option<Record>> {
    get(action_hash, GetOptions::default())
}

/// Delete a conversation from this agent's chain: tombstone the list link,
/// every message link and entry, and the conversation-metadata entry itself.
/// A link only exists while its create has no matching delete, so the
/// conversation vanishes from `get_all_conversations` and its messages from
/// `get_conversation_entries`. Author-only deletion is enforced by the
/// integrity zome's `RegisterDelete` validation. Returns the number of
/// records tombstoned. Deletes are themselves signed chain actions - the
/// record OF the deletion remains, the content does not.
#[hdk_extern]
pub fn delete_conversation(conversation_hash: ActionHash) -> ExternResult<u32> {
    let mut deleted: u32 = 0;

    // Message entries + their links off the conversation.
    let entry_links = get_links(
        LinkQuery::try_new(conversation_hash.clone(), LinkTypes::ConversationToEntries)?,
        GetStrategy::default(),
    )?;
    for link in entry_links {
        if let Ok(entry_hash) = ActionHash::try_from(link.target.clone()) {
            // Best-effort per entry: a missing/already-deleted entry must
            // not strand the rest of the conversation undeleted.
            if delete_entry(entry_hash).is_ok() {
                deleted += 1;
            }
        }
        delete_link(link.create_link_hash, GetOptions::default())?;
    }

    // The agent-list link that makes it appear in get_all_conversations.
    let agent = agent_anchor()?;
    let conv_links = get_links(
        LinkQuery::try_new(agent, LinkTypes::AllConversations)?,
        GetStrategy::default(),
    )?;
    for link in conv_links {
        if ActionHash::try_from(link.target.clone()).ok().as_ref() == Some(&conversation_hash) {
            delete_link(link.create_link_hash, GetOptions::default())?;
        }
    }

    // The conversation-metadata entry itself.
    if delete_entry(conversation_hash).is_ok() {
        deleted += 1;
    }

    Ok(deleted)
}

// ── Migration scaffolding (future DNA versions) ─────────────────────────
//
// Single-user simplification of ProofPoll's anchor-based pattern: when a
// future vN+1 migrates data from vN, it records old→new hash mappings so
// references (e.g. a conversation_hash inside an encrypted message) can
// be remapped. No cross-user pending/retry machinery is needed — every
// peer in this DHT is the same user.

/// Register a migration mapping on this version.
#[hdk_extern]
pub fn register_migrated_entry(entry: MigratedEntry) -> ExternResult<ActionHash> {
    let hash = create_entry(&EntryZomes::IntegrityTranscript(
        EntryTypes::MigratedEntry(entry),
    ))?;
    let agent = agent_anchor()?;
    create_link(agent, hash.clone(), LinkTypes::MigrationIndex, ())?;
    Ok(hash)
}

/// Get all migration mappings recorded by this agent.
#[hdk_extern]
pub fn get_migration_mappings(_: ()) -> ExternResult<Vec<Record>> {
    let agent = agent_anchor()?;
    let links = get_links(
        LinkQuery::try_new(agent, LinkTypes::MigrationIndex)?,
        GetStrategy::default(),
    )?;

    let mut mappings = Vec::new();
    for link in links {
        let hash = ActionHash::try_from(link.target)
            .map_err(|_| wasm_error!("Invalid mapping hash"))?;
        if let Some(record) = get(hash, GetOptions::default())? {
            mappings.push(record);
        }
    }
    Ok(mappings)
}
