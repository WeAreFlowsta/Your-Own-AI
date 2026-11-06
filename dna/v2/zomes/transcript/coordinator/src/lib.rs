//! Transcript DNA v2 coordinator zome (Holochain 0.7.1).
//!
//! The zome never sees plaintext: the app seals each record with the
//! person's data key before calling in and opens it after reading. Every
//! record carries its id inside the plaintext; the app derives, with the
//! same key, the keyed hashes that address records here:
//!   - a conversation's anchor (an `ExternalHash`), where its messages hang
//!     and what its list link's tag names;
//!   - a message's key (32 bytes), its link's tag.
//! So the zome can hide tombstoned conversations, return one record where
//! two devices both copied it, and list ids and times without decrypting.

use hdk::prelude::*;
use std::collections::{HashMap, HashSet};
use transcript_integrity::*;

#[hdk_dependent_entry_types]
enum EntryZomes {
    IntegrityTranscript(transcript_integrity::EntryTypes),
}

fn seal(cipher: Vec<u8>, nonce: Vec<u8>) -> ExternResult<ActionHash> {
    create_entry(&EntryZomes::IntegrityTranscript(EntryTypes::EncryptedEntry(EncryptedEntry { cipher, nonce })))
}

fn conversation_tag(anchor: &ExternalHash) -> Vec<u8> {
    let mut t = vec![CONVERSATION_TAG];
    t.extend_from_slice(anchor.get_raw_39());
    t
}

fn message_tag(key: &[u8]) -> ExternResult<Vec<u8>> {
    if key.len() != 32 {
        return Err(wasm_error!(WasmErrorInner::Guest("A message key is 32 bytes".into())));
    }
    let mut t = vec![MESSAGE_TAG];
    t.extend_from_slice(key);
    Ok(t)
}

/// The conversation anchor a list link names, if it is a conversation's.
fn listed_anchor(link: &Link) -> Option<Vec<u8>> {
    match list_tag(&link.tag.0) {
        Some(ListTag::Conversation) => Some(link.tag.0[1..].to_vec()),
        _ => None,
    }
}

fn is_tombstone(link: &Link) -> bool {
    list_tag(&link.tag.0) == Some(ListTag::Tombstone)
}

/// The anchors tombstoned among a founding key's links.
fn tombstoned(links: &[Link]) -> HashSet<Vec<u8>> {
    links.iter().filter(|l| is_tombstone(l)).map(|l| l.target.get_raw_39().to_vec()).collect()
}

/// One link per key: where two devices both copied a record, the earliest
/// link stands for it (then the lowest target, so every device agrees).
fn one_per_key(mut links: Vec<(Vec<u8>, Link)>) -> Vec<(Vec<u8>, Link)> {
    links.sort_by(|a, b| a.1.timestamp.cmp(&b.1.timestamp).then_with(|| a.1.target.get_raw_39().cmp(b.1.target.get_raw_39())));
    let mut seen = HashSet::new();
    links.retain(|(key, _)| seen.insert(key.clone()));
    links
}

// ── Writes ──────────────────────────────────────────────────────────────

#[derive(Serialize, Deserialize, Debug)]
pub struct StartConversationInput {
    pub cipher: Vec<u8>,
    pub nonce: Vec<u8>,
    /// The AI's founding key: where its conversations are listed.
    pub anchor: AgentPubKey,
    /// The conversation's anchor, derived from its id.
    pub conversation_anchor: ExternalHash,
}

/// A conversation, listed under the AI's founding key with its anchor.
#[hdk_extern]
pub fn start_conversation(input: StartConversationInput) -> ExternResult<ActionHash> {
    let hash = seal(input.cipher, input.nonce)?;
    create_link(input.anchor, hash.clone(), LinkTypes::AllConversations, conversation_tag(&input.conversation_anchor))?;
    Ok(hash)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RecordMessageInput {
    pub conversation_anchor: ExternalHash,
    /// The keyed hash of the message's id (32 bytes).
    pub message_key: Vec<u8>,
    pub cipher: Vec<u8>,
    pub nonce: Vec<u8>,
}

/// A message, filed under its conversation's anchor with its key.
#[hdk_extern]
pub fn record_message(input: RecordMessageInput) -> ExternResult<ActionHash> {
    let tag = message_tag(&input.message_key)?;
    let hash = seal(input.cipher, input.nonce)?;
    create_link(input.conversation_anchor, hash.clone(), LinkTypes::ConversationToEntries, tag)?;
    Ok(hash)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct SaveMemoryRowInput {
    pub cipher: Vec<u8>,
    pub nonce: Vec<u8>,
    /// The AI's founding key for an `ai:` scope, a hashed external anchor
    /// for a person or project scope.
    pub anchor: AnyLinkableHash,
}

/// A memory row (a logical id and `updated_at` inside the plaintext; every
/// revision is a new row, reduced by the reader).
#[hdk_extern]
pub fn save_memory_row(input: SaveMemoryRowInput) -> ExternResult<ActionHash> {
    let hash = seal(input.cipher, input.nonce)?;
    create_link(input.anchor, hash.clone(), LinkTypes::AllConversations, MEMORY_TAG.to_vec())?;
    Ok(hash)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct DeleteConversationInput {
    pub anchor: AgentPubKey,
    pub conversation_anchor: ExternalHash,
}

fn authored_by_me(hash: &ActionHash, me: &AgentPubKey) -> bool {
    matches!(get(hash.clone(), GetOptions::local()), Ok(Some(r)) if r.action().author() == me)
}

/// Delete a conversation from any of the person's devices: a tombstone at
/// the founding key hides it everywhere, and this device removes what it
/// authored (its entries and links; only an author may). Returns how many
/// entries this call deleted.
#[hdk_extern]
pub fn delete_conversation(input: DeleteConversationInput) -> ExternResult<u32> {
    let me = agent_info()?.agent_initial_pubkey;
    let target = AnyLinkableHash::from(input.conversation_anchor.clone());
    let mut deleted = 0u32;
    for link in get_links(LinkQuery::try_new(input.conversation_anchor.clone(), LinkTypes::ConversationToEntries)?, GetStrategy::default())? {
        if link.author == me {
            if let Some(entry) = link.target.clone().into_action_hash() {
                if authored_by_me(&entry, &me) && delete_entry(entry).is_ok() {
                    deleted += 1;
                }
            }
            let _ = delete_link(link.create_link_hash, GetOptions::default());
        }
    }
    let listed = get_links(LinkQuery::try_new(input.anchor.clone(), LinkTypes::AllConversations)?, GetStrategy::default())?;
    let anchor_bytes = target.get_raw_39().to_vec();
    let mut has_tombstone = false;
    for link in listed {
        if is_tombstone(&link) && link.target.get_raw_39() == anchor_bytes.as_slice() {
            has_tombstone = true;
        } else if listed_anchor(&link).as_deref() == Some(anchor_bytes.as_slice()) && link.author == me {
            if let Some(entry) = link.target.clone().into_action_hash() {
                if authored_by_me(&entry, &me) && delete_entry(entry).is_ok() {
                    deleted += 1;
                }
            }
            let _ = delete_link(link.create_link_hash, GetOptions::default());
        }
    }
    if !has_tombstone {
        create_link(input.anchor, target, LinkTypes::AllConversations, TOMBSTONE_TAG.to_vec())?;
    }
    Ok(deleted)
}

// ── Reads ───────────────────────────────────────────────────────────────

/// A record with the key its link carries (a conversation's anchor, a
/// message's key).
#[derive(Serialize, Deserialize, Debug)]
pub struct PageItem {
    pub record: Record,
    pub key: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RecordsPage {
    pub items: Vec<PageItem>,
    /// Pass back as `before` / `after` for the next page. None = no more.
    pub next: Option<Timestamp>,
    /// Records in the whole list (one per key), all pages.
    pub total: u32,
    /// Records in this window that could not be read.
    pub missing: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ConversationsPageInput {
    pub anchor: AgentPubKey,
    /// Only conversations listed before this time. None = from the newest.
    pub before: Option<Timestamp>,
    pub limit: u32,
}

/// One page of conversations, newest first: one per anchor, tombstoned
/// ones hidden.
#[hdk_extern]
pub fn get_conversations_page(input: ConversationsPageInput) -> ExternResult<RecordsPage> {
    let all = get_links(LinkQuery::try_new(input.anchor, LinkTypes::AllConversations)?, GetStrategy::default())?;
    let gone = tombstoned(&all);
    let keyed: Vec<(Vec<u8>, Link)> = all.into_iter().filter_map(|l| listed_anchor(&l).map(|a| (a, l))).filter(|(a, _)| !gone.contains(a)).collect();
    let mut links = one_per_key(keyed);
    let total = links.len() as u32;
    links.sort_by(|a, b| b.1.timestamp.cmp(&a.1.timestamp));
    if let Some(before) = input.before {
        links.retain(|(_, l)| l.timestamp < before);
    }
    let (window, more) = take_window(links, input.limit);
    read_window(window, more, total)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EntriesPageInput {
    pub conversation_anchor: ExternalHash,
    /// Only messages filed after this time. None = from the first.
    pub after: Option<Timestamp>,
    pub limit: u32,
}

/// One page of a conversation's messages, oldest first, one per key.
#[hdk_extern]
pub fn get_conversation_entries_page(input: EntriesPageInput) -> ExternResult<RecordsPage> {
    let links = get_links(LinkQuery::try_new(input.conversation_anchor, LinkTypes::ConversationToEntries)?, GetStrategy::default())?;
    let keyed: Vec<(Vec<u8>, Link)> = links.into_iter().filter(|l| is_message_tag(&l.tag.0)).map(|l| (l.tag.0[1..].to_vec(), l)).collect();
    let mut links = one_per_key(keyed);
    let total = links.len() as u32;
    links.sort_by(|a, b| a.1.timestamp.cmp(&b.1.timestamp));
    if let Some(after) = input.after {
        links.retain(|(_, l)| l.timestamp > after);
    }
    let (window, more) = take_window(links, input.limit);
    let mut page = read_window(window, more, total)?;
    page.items.sort_by_key(|i| i.record.action().timestamp());
    Ok(page)
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MemoryRowsInput {
    pub anchor: AnyLinkableHash,
}

/// Every memory row at the anchor, oldest first. Reduction (the latest
/// revision per id, a deletion over a live row) is the reader's.
#[hdk_extern]
pub fn get_memory_rows(input: MemoryRowsInput) -> ExternResult<Vec<Record>> {
    let mut links: Vec<Link> = get_links(LinkQuery::try_new(input.anchor, LinkTypes::AllConversations)?, GetStrategy::default())?
        .into_iter()
        .filter(|l| list_tag(&l.tag.0) == Some(ListTag::Memory))
        .collect();
    links.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    let mut rows = Vec::with_capacity(links.len());
    for link in links {
        let Some(hash) = link.target.into_action_hash() else { continue };
        if let Some(record) = get(hash, GetOptions::default())? {
            rows.push(record);
        }
    }
    Ok(rows)
}

/// A single record by its action hash.
#[hdk_extern]
pub fn get_conversation(action_hash: ActionHash) -> ExternResult<Option<Record>> {
    get(action_hash, GetOptions::default())
}

// ── Keys only (the move's listing): links, never a record ──────────────

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct KeyItem {
    pub key: Vec<u8>,
    pub at: Timestamp,
    pub author: AgentPubKey,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct ConversationKeys {
    /// One per anchor, tombstoned ones included (see `tombstoned`).
    pub conversations: Vec<KeyItem>,
    pub tombstoned: Vec<Vec<u8>>,
}

/// Every conversation anchor listed under the founding key, and the
/// tombstoned ones, from links alone.
#[hdk_extern]
pub fn get_conversation_keys(anchor: AgentPubKey) -> ExternResult<ConversationKeys> {
    let all = get_links(LinkQuery::try_new(anchor, LinkTypes::AllConversations)?, GetStrategy::default())?;
    let gone: Vec<Vec<u8>> = tombstoned(&all).into_iter().collect();
    let keyed: Vec<(Vec<u8>, Link)> = all.into_iter().filter_map(|l| listed_anchor(&l).map(|a| (a, l))).collect();
    let conversations = one_per_key(keyed).into_iter().map(|(key, l)| KeyItem { key, at: l.timestamp, author: l.author }).collect();
    Ok(ConversationKeys { conversations, tombstoned: gone })
}

/// Every message key under a conversation's anchor, from links alone.
#[hdk_extern]
pub fn get_message_keys(conversation_anchor: ExternalHash) -> ExternResult<Vec<KeyItem>> {
    let links = get_links(LinkQuery::try_new(conversation_anchor, LinkTypes::ConversationToEntries)?, GetStrategy::default())?;
    let keyed: Vec<(Vec<u8>, Link)> = links.into_iter().filter(|l| is_message_tag(&l.tag.0)).map(|l| (l.tag.0[1..].to_vec(), l)).collect();
    Ok(one_per_key(keyed).into_iter().map(|(key, l)| KeyItem { key, at: l.timestamp, author: l.author }).collect())
}

// ── This device's own messages, from its own chain ─────────────────────

#[derive(Serialize, Deserialize, Debug)]
pub struct MyEntriesPageInput {
    /// The AI's founding key, whose tombstones (from any device) hide a
    /// conversation's messages.
    pub anchor: AgentPubKey,
    /// Only messages after this chain sequence number. None = from the first.
    pub after_seq: Option<u32>,
    pub limit: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MyEntry {
    pub record: Record,
    pub conversation_anchor: ExternalHash,
    pub key: Vec<u8>,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct MyEntriesPage {
    pub items: Vec<MyEntry>,
    /// Pass back as `after_seq` for the next page. None = no more.
    pub next_seq: Option<u32>,
    /// Live messages on this chain, all pages.
    pub total: u32,
}

/// Every message this device recorded, read from its own chain and paged
/// by sequence number (the memory index rebuild reads it). One scan without
/// entries finds the message creates, their links, the deletes and this
/// device's tombstones; the founding key's links add other devices'
/// tombstones; records come in tight sequence ranges (never an unranged
/// entry query, see `from_own_chain`).
#[hdk_extern]
pub fn get_my_entries_page(input: MyEntriesPageInput) -> ExternResult<MyEntriesPage> {
    let entries_lt = ScopedLinkType::try_from(LinkTypes::ConversationToEntries)?;
    let list_lt = ScopedLinkType::try_from(LinkTypes::AllConversations)?;
    let mut gone: HashSet<Vec<u8>> = tombstoned(&get_links(LinkQuery::try_new(input.anchor, LinkTypes::AllConversations)?, GetStrategy::default())?);
    let actions = query(ChainQueryFilter::new().include_entries(false))?;
    let mut seq_of: HashMap<ActionHash, u32> = HashMap::new();
    let mut filed: HashMap<ActionHash, (ExternalHash, Vec<u8>)> = HashMap::new();
    let mut deleted: HashSet<ActionHash> = HashSet::new();
    for r in actions.iter() {
        let action = r.action();
        match &action.data {
            ActionData::Create(_) => {
                seq_of.insert(r.action_address().clone(), action.action_seq());
            }
            ActionData::CreateLink(l) if l.zome_index == entries_lt.zome_index && l.link_type == entries_lt.zome_type => {
                if let (Some(base), Some(target)) = (l.base_address.clone().into_external_hash(), l.target_address.clone().into_action_hash()) {
                    if is_message_tag(&l.tag.0) {
                        filed.insert(target, (base, l.tag.0[1..].to_vec()));
                    }
                }
            }
            ActionData::CreateLink(l) if l.zome_index == list_lt.zome_index && l.link_type == list_lt.zome_type && l.tag.0 == TOMBSTONE_TAG => {
                gone.insert(l.target_address.get_raw_39().to_vec());
            }
            ActionData::Delete(d) => {
                deleted.insert(d.deletes_address.clone());
            }
            _ => {}
        }
    }
    drop(actions);
    let mut wanted: Vec<(u32, ActionHash)> = filed
        .iter()
        .filter(|(msg, (anchor, _))| !deleted.contains(*msg) && !gone.contains(anchor.get_raw_39()))
        .filter_map(|(msg, _)| seq_of.get(msg).map(|&s| (s, msg.clone())))
        .collect();
    wanted.sort_by_key(|w| w.0);
    let total = wanted.len() as u32;
    if let Some(after) = input.after_seq {
        wanted.retain(|w| w.0 > after);
    }
    let limit = input.limit.max(1) as usize;
    let more = wanted.len() > limit;
    wanted.truncate(limit);
    let next_seq = if more { wanted.last().map(|w| w.0) } else { None };
    let mut seqs: Vec<u32> = wanted.iter().map(|w| w.0).collect();
    let in_page: HashSet<ActionHash> = wanted.into_iter().map(|w| w.1).collect();
    let mut items = Vec::with_capacity(in_page.len());
    for (from, to) in seq_ranges(&mut seqs, RANGE_GAP) {
        let records = query(
            ChainQueryFilter::new()
                .sequence_range(ChainQueryFilterRange::ActionSeqRange(from, to))
                .action_type(ActionType::Create)
                .include_entries(true),
        )?;
        for r in records {
            if in_page.contains(r.action_address()) {
                if let Some((anchor, key)) = filed.get(r.action_address()) {
                    items.push(MyEntry { conversation_anchor: anchor.clone(), key: key.clone(), record: r });
                }
            }
        }
    }
    items.sort_by_key(|i| i.record.action().action_seq());
    Ok(MyEntriesPage { items, next_seq, total })
}

// ── Paging (as v1, measured on large cells) ────────────────────────────

/// Take one window from keyed links already sorted in reading order. Links
/// that share the window's last timestamp ride along, so the timestamp
/// cursor never skips one.
fn take_window(sorted: Vec<(Vec<u8>, Link)>, limit: u32) -> (Vec<(Vec<u8>, Link)>, bool) {
    let stamps: Vec<Timestamp> = sorted.iter().map(|(_, l)| l.timestamp).collect();
    let end = window_end(&stamps, limit);
    let more = end < sorted.len();
    (sorted.into_iter().take(end).collect(), more)
}

/// How many items of an ordered list one page takes: `limit` (at least 1),
/// plus any that share the last one's stamp.
fn window_end<T: PartialEq>(stamps: &[T], limit: u32) -> usize {
    let limit = limit.max(1) as usize;
    if stamps.len() <= limit {
        return stamps.len();
    }
    let edge = &stamps[limit - 1];
    stamps[limit..].iter().position(|s| s != edge).map(|p| limit + p).unwrap_or(stamps.len())
}

/// Windows up to this many records are read with `get`; larger ones with
/// the chain queries (one scan costs about six gets on a large cell).
const CHAIN_SCAN_ABOVE: usize = 6;

/// The records of a window that sit on this device's own chain, read with
/// two chain queries instead of one `get` per record (about 0.3 s each on a
/// large cell). The first query loads actions only; the second loads
/// entries by sequence range alone: an unranged entry query reads the whole
/// chain into memory (it froze a 16 GB machine on a 1,059-conversation
/// cell).
fn from_own_chain(targets: &[ActionHash]) -> ExternResult<HashMap<ActionHash, Record>> {
    if targets.len() <= CHAIN_SCAN_ABOVE {
        return Ok(HashMap::new());
    }
    let wanted: HashSet<&ActionHash> = targets.iter().collect();
    let actions = query(ChainQueryFilter::new().action_type(ActionType::Create).include_entries(false))?;
    let mut seqs: Vec<u32> = actions.iter().filter(|r| wanted.contains(r.action_address())).map(|r| r.action().action_seq()).collect();
    drop(actions);
    let mut out = HashMap::new();
    for (from, to) in seq_ranges(&mut seqs, RANGE_GAP) {
        let records = query(
            ChainQueryFilter::new()
                .sequence_range(ChainQueryFilterRange::ActionSeqRange(from, to))
                .action_type(ActionType::Create)
                .include_entries(true),
        )?;
        for r in records {
            if wanted.contains(r.action_address()) {
                out.insert(r.action_address().clone(), r);
            }
        }
    }
    Ok(out)
}

/// Sequence numbers this close together are fetched as one range.
const RANGE_GAP: u32 = 16;

/// Sorted, merged (from, to) ranges covering every number, joining numbers
/// no further apart than `gap`.
fn seq_ranges(seqs: &mut Vec<u32>, gap: u32) -> Vec<(u32, u32)> {
    seqs.sort_unstable();
    seqs.dedup();
    let mut out: Vec<(u32, u32)> = Vec::new();
    for &s in seqs.iter() {
        match out.last_mut() {
            Some((_, to)) if s - *to <= gap => *to = s,
            _ => out.push((s, s)),
        }
    }
    out
}

fn read_window(window: Vec<(Vec<u8>, Link)>, more: bool, total: u32) -> ExternResult<RecordsPage> {
    let next = if more { window.last().map(|(_, l)| l.timestamp) } else { None };
    let mut targets = Vec::with_capacity(window.len());
    for (key, link) in window {
        let hash = link.target.into_action_hash().ok_or_else(|| wasm_error!(WasmErrorInner::Guest("Invalid record hash".into())))?;
        targets.push((key, hash));
    }
    let hashes: Vec<ActionHash> = targets.iter().map(|(_, h)| h.clone()).collect();
    let mut own = from_own_chain(&hashes)?;
    let mut items = Vec::new();
    let mut missing = 0u32;
    for (key, hash) in targets {
        // Not on this chain (another device's record): the store, then the network.
        let found = match own.remove(&hash) {
            Some(record) => Some(record),
            None => match get(hash.clone(), GetOptions::local())? {
                Some(record) => Some(record),
                None => get(hash, GetOptions::default())?,
            },
        };
        match found {
            Some(record) => items.push(PageItem { record, key }),
            None => missing += 1,
        }
    }
    Ok(RecordsPage { items, next, total, missing })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_page_takes_its_limit_and_any_that_share_the_last_stamp() {
        assert_eq!(window_end(&[1, 2, 3, 4], 2), 2);
        assert_eq!(window_end(&[1, 2, 2, 2, 3], 2), 4);
        assert_eq!(window_end(&[1, 2], 5), 2);
        assert_eq!(window_end::<u8>(&[], 3), 0);
        assert_eq!(window_end(&[7, 8], 0), 1, "a limit of 0 reads one");
    }

    #[test]
    fn close_sequence_numbers_are_read_as_one_range() {
        assert_eq!(seq_ranges(&mut vec![40, 3, 5, 3, 21], 16), vec![(3, 21), (40, 40)]);
        assert_eq!(seq_ranges(&mut vec![], 16), vec![]);
    }
}
