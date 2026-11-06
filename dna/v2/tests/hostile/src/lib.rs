//! Test only. What another agent on the network can attempt with a
//! coordinator of its own against the transcript integrity zome: Holochain
//! checks no author on a delete, an update or a link removal, and none on
//! what is filed where. The sweettests run this beside the real coordinator;
//! the integrity rules refuse each write. `own_entry` is the one valid write
//! here: an entry of the caller's own, to point links at.

use hdk::prelude::*;
use transcript_integrity::*;

#[hdk_dependent_entry_types]
enum EntryZomes {
    Integrity(transcript_integrity::EntryTypes),
}

fn sealed() -> EntryZomes {
    EntryZomes::Integrity(EntryTypes::EncryptedEntry(EncryptedEntry { cipher: vec![9; 48], nonce: vec![1; 24] }))
}

/// An entry of the caller's own.
#[hdk_extern]
pub fn own_entry(_: ()) -> ExternResult<ActionHash> {
    create_entry(&sealed())
}

/// Delete someone else's record.
#[hdk_extern]
pub fn delete_record(hash: ActionHash) -> ExternResult<ActionHash> {
    delete_entry(hash)
}

/// Rewrite a record by updating it (anyone's, the caller's own included).
#[hdk_extern]
pub fn update_record(hash: ActionHash) -> ExternResult<ActionHash> {
    update_entry(hash, &sealed())
}

#[derive(Serialize, Deserialize, Debug)]
pub struct LinkInput {
    pub base: AnyLinkableHash,
    pub target: AnyLinkableHash,
    pub tag: Vec<u8>,
}

/// A list link (conversation, memory row or tombstone) of any shape.
#[hdk_extern]
pub fn list_link(input: LinkInput) -> ExternResult<ActionHash> {
    create_link(input.base, input.target, LinkTypes::AllConversations, input.tag)
}

/// A message link of any shape.
#[hdk_extern]
pub fn message_link(input: LinkInput) -> ExternResult<ActionHash> {
    create_link(input.base, input.target, LinkTypes::ConversationToEntries, input.tag)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RemoveInput {
    pub base: AnyLinkableHash,
    /// `list` or `message`.
    pub kind: String,
}

/// Remove every link of a kind at a base, whoever made it.
#[hdk_extern]
pub fn remove_links(input: RemoveInput) -> ExternResult<u32> {
    let link_type = if input.kind == "list" { LinkTypes::AllConversations } else { LinkTypes::ConversationToEntries };
    let mut removed = 0;
    for link in get_links(LinkQuery::try_new(input.base, link_type)?, GetStrategy::default())? {
        delete_link(link.create_link_hash, GetOptions::default())?;
        removed += 1;
    }
    Ok(removed)
}
