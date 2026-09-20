//! Transcript DNA coordinator zome — encrypted entries (Phase A).
//!
//! The zome never sees plaintext: the Tauri layer encrypts conversation
//! metadata and messages with the user's data key before calling in,
//! and decrypts after reading. Ordering uses action timestamps (content
//! ordering by `sequence` happens client-side after decryption).

use hdk::prelude::*;
use std::collections::{HashMap, HashSet};
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

/// A window onto a list of links, so a long list is read a page at a time.
/// Listing links is cheap; the cost of the unpaged reads is one `get` per
/// link, so a page fetches records for its own window only.
#[derive(Serialize, Deserialize, Debug)]
pub struct ConversationsPageInput {
    /// Whose list to read. None = this cell's own agent.
    pub anchor: Option<AgentPubKey>,
    /// Only conversations started before this time. None = from the newest.
    pub before: Option<Timestamp>,
    pub limit: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct EntriesPageInput {
    pub conversation_hash: ActionHash,
    /// Only messages recorded after this time. None = from the first.
    pub after: Option<Timestamp>,
    pub limit: u32,
}

#[derive(Serialize, Deserialize, Debug)]
pub struct RecordsPage {
    pub records: Vec<Record>,
    /// Pass back as `before` / `after` for the next page. None = no more.
    pub next: Option<Timestamp>,
    /// Links in the whole list, all pages.
    pub total: u32,
    /// Links in this window whose record could not be read.
    pub missing: u32,
}

/// Take one window from links already sorted in reading order. Links that
/// share the window's last timestamp ride along, so the timestamp cursor
/// never skips one.
fn take_window(sorted: Vec<Link>, limit: u32) -> (Vec<Link>, bool) {
    let stamps: Vec<Timestamp> = sorted.iter().map(|l| l.timestamp).collect();
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
    stamps[limit..]
        .iter()
        .position(|s| s != edge)
        .map(|p| limit + p)
        .unwrap_or(stamps.len())
}

/// Windows up to this many records are read with `get`; larger ones with the
/// chain queries (one scan ≈ six gets on a large cell).
const CHAIN_SCAN_ABOVE: usize = 6;

/// The records of a window that sit on THIS agent's own chain, read with two
/// chain queries instead of one `get` per record. A `get` costs about 0.3 s
/// on a large cell whatever its strategy (measured: 1,427 messages, 450 s by
/// network-first gets, 488 s by local-first gets); a person's records are
/// written on their own device, so the chain has them.
fn from_own_chain(targets: &[ActionHash]) -> ExternResult<HashMap<ActionHash, Record>> {
    // The two queries scan the chain once whatever the window's size (~1.7 s
    // on a 1,048-conversation cell), so they only beat `get` above a handful
    // of records. A short conversation is read record by record: measured,
    // a 1-message conversation took ~2 s through the scan.
    if targets.len() <= CHAIN_SCAN_ABOVE {
        return Ok(HashMap::new());
    }
    let wanted: HashSet<&ActionHash> = targets.iter().collect();
    // Actions only (no entry is loaded): which of the wanted records are on
    // this chain, and at which sequence numbers.
    let actions = query(ChainQueryFilter::new().action_type(ActionType::Create).include_entries(false))?;
    let mut seqs: Vec<u32> = actions
        .iter()
        .filter(|r| wanted.contains(r.action_address()))
        .map(|r| r.action().action_seq())
        .collect();
    drop(actions);
    // Full records for those sequence numbers only. NEVER ask for entries
    // without a sequence range: the conductor applies `entry_hashes` AFTER
    // loading every entry of the chain, so an unranged query reads the whole
    // chain's contents into memory on every call (on a 1,059-conversation
    // cell that froze a 16 GB machine). The sequence range is applied in the
    // database, so only these rows' entries are loaded.
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

/// Sequence numbers this close together are fetched as one range: a
/// conversation's messages sit side by side on the chain, so one range covers
/// a run of them, and the few records loaded in between are thrown away.
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

fn read_window(window: Vec<Link>, more: bool, total: u32) -> ExternResult<RecordsPage> {
    let next = if more { window.last().map(|l| l.timestamp) } else { None };
    let mut targets = Vec::with_capacity(window.len());
    for link in window {
        targets.push(
            ActionHash::try_from(link.target).map_err(|_| wasm_error!("Invalid record hash"))?,
        );
    }
    let mut own = from_own_chain(&targets)?;
    let mut records = Vec::new();
    let mut missing = 0u32;
    for hash in targets {
        // Not on this chain (another agent's list): the store, then the network.
        let found = match own.remove(&hash) {
            Some(record) => Some(record),
            None => match get(hash.clone(), GetOptions::local())? {
                Some(record) => Some(record),
                None => get(hash, GetOptions::default())?,
            },
        };
        match found {
            Some(record) => records.push(record),
            None => missing += 1,
        }
    }
    Ok(RecordsPage { records, next, total, missing })
}

/// One page of conversations (encrypted records), newest first.
#[hdk_extern]
pub fn get_conversations_page(input: ConversationsPageInput) -> ExternResult<RecordsPage> {
    let anchor = match input.anchor {
        Some(a) => a,
        None => agent_anchor()?,
    };
    let mut links = get_links(
        LinkQuery::try_new(anchor, LinkTypes::AllConversations)?,
        GetStrategy::default(),
    )?;
    let total = links.len() as u32;
    links.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    if let Some(before) = input.before {
        links.retain(|l| l.timestamp < before);
    }
    let (window, more) = take_window(links, input.limit);
    read_window(window, more, total)
}

/// One page of a conversation's (encrypted) messages, oldest first.
#[hdk_extern]
pub fn get_conversation_entries_page(input: EntriesPageInput) -> ExternResult<RecordsPage> {
    let mut links = get_links(
        LinkQuery::try_new(input.conversation_hash, LinkTypes::ConversationToEntries)?,
        GetStrategy::default(),
    )?;
    let total = links.len() as u32;
    links.sort_by(|a, b| a.timestamp.cmp(&b.timestamp));
    if let Some(after) = input.after {
        links.retain(|l| l.timestamp > after);
    }
    let (window, more) = take_window(links, input.limit);
    let mut page = read_window(window, more, total)?;
    page.records.sort_by_key(|record| record.action().timestamp());
    Ok(page)
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

#[cfg(test)]
mod paging_tests {
    use super::window_end;

    /// Walk a newest-first list by the `before` cursor the way a caller does.
    fn walk(stamps: &[i64], limit: u32) -> Vec<i64> {
        let mut seen = Vec::new();
        let mut before: Option<i64> = None;
        loop {
            let rest: Vec<i64> = stamps.iter().copied().filter(|s| before.map_or(true, |b| *s < b)).collect();
            let end = window_end(&rest, limit);
            seen.extend_from_slice(&rest[..end]);
            if end >= rest.len() {
                return seen;
            }
            before = Some(rest[end - 1]);
        }
    }

    #[test]
    fn every_item_is_read_once_even_when_stamps_tie_at_a_page_edge() {
        let stamps = vec![90, 80, 80, 80, 70, 60, 60, 50];
        for limit in [0, 1, 2, 3, 5, 8, 100] {
            assert_eq!(walk(&stamps, limit), stamps, "limit {limit}");
        }
        assert_eq!(walk(&[], 10), Vec::<i64>::new());
    }

    #[test]
    fn wanted_records_are_fetched_in_tight_ranges_never_the_whole_chain() {
        use super::seq_ranges;
        // A conversation's messages: side by side, with another chat's in between.
        let mut run = vec![100, 101, 103, 104, 110, 111];
        assert_eq!(seq_ranges(&mut run, 16), vec![(100, 111)]);
        // A list page: conversation starts scattered across the chain.
        let mut scattered = vec![9_000, 12, 4_500, 13];
        assert_eq!(seq_ranges(&mut scattered, 16), vec![(12, 13), (4_500, 4_500), (9_000, 9_000)]);
        let covered: u32 = seq_ranges(&mut vec![12, 13, 4_500, 9_000], 16).iter().map(|(a, b)| b - a + 1).sum();
        assert_eq!(covered, 4, "four records wanted, four rows loaded - not nine thousand");
        assert!(seq_ranges(&mut Vec::new(), 16).is_empty());
        assert_eq!(seq_ranges(&mut vec![7, 7, 7], 16), vec![(7, 7)]);
    }

    #[test]
    fn a_short_list_is_one_page() {
        assert_eq!(window_end(&[3, 2, 1], 50), 3);
        assert_eq!(window_end(&[3, 2, 1], 3), 3);
        assert_eq!(window_end(&[3, 2, 1], 2), 2);
    }
}
