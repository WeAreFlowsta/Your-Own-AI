//! Conversation records read a page at a time.
//!
//! The unpaged reads (`get_all_conversations`, `get_conversation_entries`)
//! fetch every record inside ONE zome call, so a long list or a long
//! conversation is one call that either answers inside its timeout or is
//! lost whole. The paged reads fetch a window per call: each call is small,
//! and what was read before a failure is kept.
//!
//! A cell whose coordinator predates the paged reads answers "function does
//! not exist"; those fall back to the unpaged read, so nothing depends on
//! the startup coordinator sweep having reached a cell yet.

use crate::holochain::HolochainManager;
use holochain_types::prelude::{ActionHash, AgentPubKey, ExternIO, Record, Timestamp};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Conversations per call. On a warm 1,048-conversation cell a page answers
/// in under 2 s; the old single read did not answer in 60 s.
pub const PAGE: u32 = 100;
/// Messages per call. Every page call scans the chain's actions once
/// (~1.7 s on a 1,048-conversation cell), so a long conversation reads
/// faster in fewer, larger pages: 1,427 messages took 26 s in pages of 100.
pub const ENTRIES_PAGE: u32 = 500;

#[derive(Serialize, Debug)]
struct ConversationsPageInput {
    anchor: Option<AgentPubKey>,
    before: Option<Timestamp>,
    limit: u32,
}

#[derive(Serialize, Debug)]
struct EntriesPageInput {
    conversation_hash: ActionHash,
    after: Option<Timestamp>,
    limit: u32,
}

#[derive(Deserialize, Debug)]
struct RecordsPage {
    records: Vec<Record>,
    next: Option<Timestamp>,
    total: u32,
    missing: u32,
}

/// What a paged walk brought back.
#[derive(Debug, Default)]
pub struct PagedRead {
    pub records: Vec<Record>,
    /// False when a later page failed: `records` holds what came before it.
    pub complete: bool,
    /// Links in the whole list (0 when the unpaged fallback answered).
    pub total: u32,
    /// Links whose record could not be read.
    pub missing: u32,
    /// Why the walk stopped early, when it did.
    pub stopped_by: Option<String>,
}

impl PagedRead {
    /// The records only when every page answered. For callers where a part
    /// is worse than nothing (a backup must never hold half a conversation).
    pub fn whole(self) -> Result<Vec<Record>, String> {
        if self.complete {
            Ok(self.records)
        } else {
            Err(format!(
                "stopped after {} of {}: {}",
                self.records.len(),
                self.total,
                self.stopped_by.unwrap_or_else(|| "unknown".to_string())
            ))
        }
    }
}

/// True for the conductor's answer when a cell's coordinator has no such
/// function (it predates the paged reads).
pub fn is_missing_function(err: &str) -> bool {
    err.contains("ZomeFnNotExists") || err.contains("doesn't exist") || err.contains("does not exist")
}

async fn page<I: Serialize + std::fmt::Debug>(
    manager: &HolochainManager,
    agent: &str,
    fn_name: &str,
    input: &I,
    timeout: Duration,
) -> Result<RecordsPage, String> {
    let payload = ExternIO::encode(input).map_err(|e| format!("Failed to encode: {}", e))?;
    let t0 = std::time::Instant::now();
    let out = manager
        .call_zome_with_timeout(agent, "transcript", fn_name, payload, timeout)
        .await?;
    let decoded: RecordsPage =
        ExternIO::decode(&out).map_err(|e| format!("Failed to decode {}: {}", fn_name, e))?;
    // A slow page is worth a line in the log; a quick one is not.
    let level = if t0.elapsed().as_secs() >= 2 { log::Level::Info } else { log::Level::Debug };
    log::log!(
        level,
        "[records] {} {}..: {} of {} record(s), {} missing, {} ms",
        fn_name,
        &agent[..8.min(agent.len())],
        decoded.records.len(),
        decoded.total,
        decoded.missing,
        t0.elapsed().as_millis()
    );
    Ok(decoded)
}

async fn unpaged<I: Serialize + std::fmt::Debug>(
    manager: &HolochainManager,
    agent: &str,
    fn_name: &str,
    input: I,
    timeout: Duration,
) -> Result<PagedRead, String> {
    let payload = ExternIO::encode(input).map_err(|e| format!("Failed to encode: {}", e))?;
    let out = manager
        .call_zome_with_timeout(agent, "transcript", fn_name, payload, timeout)
        .await?;
    let records: Vec<Record> =
        ExternIO::decode(&out).map_err(|e| format!("Failed to decode {}: {}", fn_name, e))?;
    Ok(PagedRead { records, complete: true, ..Default::default() })
}

/// Every conversation record of one agent, newest first. `timeout` is per
/// call. Err only when nothing at all could be read.
pub async fn all_conversations(
    manager: &HolochainManager,
    agent: &str,
    timeout: Duration,
) -> Result<PagedRead, String> {
    all_conversations_at(manager, agent, None, timeout).await
}

/// The conversations listed at `anchor` (an agent key as hex - an AI's
/// founding key, or an earlier generation of this AI), read through
/// `agent`'s cell. `None` = the cell's own key. Every device of the person
/// lists an AI's conversations at the same founding key (coordinator 9).
pub async fn all_conversations_at(
    manager: &HolochainManager,
    agent: &str,
    anchor: Option<&str>,
    timeout: Duration,
) -> Result<PagedRead, String> {
    let anchor_key = match anchor {
        Some(hex_key) if hex_key != agent => Some(
            hex::decode(hex_key)
                .ok()
                .filter(|b| b.len() == 39)
                .map(AgentPubKey::from_raw_39)
                .ok_or_else(|| format!("bad anchor key {}", hex_key))?,
        ),
        _ => None,
    };
    let mut read = PagedRead { complete: true, ..Default::default() };
    let mut before: Option<Timestamp> = None;
    loop {
        let input = ConversationsPageInput { anchor: anchor_key.clone(), before, limit: PAGE };
        match page(manager, agent, "get_conversations_page", &input, timeout).await {
            Ok(p) => {
                read.total = p.total;
                read.missing += p.missing;
                read.records.extend(p.records);
                match p.next {
                    Some(next) => before = Some(next),
                    None => return Ok(read),
                }
            }
            Err(e) if before.is_none() && is_missing_function(&e) => {
                return unpaged(manager, agent, "get_all_conversations", (), timeout).await;
            }
            Err(e) if before.is_none() => return Err(e),
            Err(e) => {
                read.complete = false;
                read.stopped_by = Some(e);
                return Ok(read);
            }
        }
    }
}

/// Every message record of one conversation, oldest first.
pub async fn all_entries(
    manager: &HolochainManager,
    agent: &str,
    conversation_hash: ActionHash,
    timeout: Duration,
) -> Result<PagedRead, String> {
    let mut read = PagedRead { complete: true, ..Default::default() };
    let mut after: Option<Timestamp> = None;
    loop {
        let input = EntriesPageInput { conversation_hash: conversation_hash.clone(), after, limit: ENTRIES_PAGE };
        match page(manager, agent, "get_conversation_entries_page", &input, timeout).await {
            Ok(p) => {
                read.total = p.total;
                read.missing += p.missing;
                read.records.extend(p.records);
                match p.next {
                    Some(next) => after = Some(next),
                    None => return Ok(read),
                }
            }
            Err(e) if after.is_none() && is_missing_function(&e) => {
                return unpaged(manager, agent, "get_conversation_entries", conversation_hash, timeout).await;
            }
            Err(e) if after.is_none() => return Err(e),
            Err(e) => {
                read.complete = false;
                read.stopped_by = Some(e);
                return Ok(read);
            }
        }
    }
}

#[derive(Serialize, Debug)]
struct MyEntriesPageInput {
    after_seq: Option<u32>,
    limit: u32,
}
#[derive(Deserialize, Debug)]
struct MyEntry {
    record: Record,
    conversation: ActionHash,
}
#[derive(Deserialize, Debug)]
struct MyEntriesPage {
    items: Vec<MyEntry>,
    next_seq: Option<u32>,
    total: u32,
}
/// Each page re-scans the chain once (~2-3 s on a 25,000-action chain) before
/// loading its records, so a bigger page is nearly free: 500 instead of 200
/// cuts the 11,768-message read from 59 scans to 24.
const MY_ENTRIES_PAGE: u32 = 500;

/// Every live message this agent recorded, with its conversation, read from
/// the agent's own chain a page at a time (`get_my_entries_page`). Err when
/// the cell's coordinator predates the function (callers fall back to the
/// per-conversation reads) or a page fails.
pub async fn all_my_entries(
    manager: &HolochainManager,
    agent: &str,
    timeout: Duration,
) -> Result<(Vec<(Record, ActionHash)>, u32), String> {
    let mut out = Vec::new();
    let mut after_seq: Option<u32> = None;
    let mut total = 0u32;
    loop {
        let input = MyEntriesPageInput { after_seq, limit: MY_ENTRIES_PAGE };
        let payload = ExternIO::encode(&input).map_err(|e| format!("Failed to encode: {}", e))?;
        let t0 = std::time::Instant::now();
        let raw = manager.call_zome_with_timeout(agent, "transcript", "get_my_entries_page", payload, timeout).await?;
        let page: MyEntriesPage = ExternIO::decode(&raw).map_err(|e| format!("Failed to decode get_my_entries_page: {}", e))?;
        log::info!(
            "[records] get_my_entries_page {}..: {} of {} message(s), {} ms",
            &agent[..8.min(agent.len())],
            page.items.len(),
            page.total,
            t0.elapsed().as_millis()
        );
        total = page.total;
        out.extend(page.items.into_iter().map(|i| (i.record, i.conversation)));
        match page.next_seq {
            Some(s) => after_seq = Some(s),
            None => return Ok((out, total)),
        }
    }
}

/// How many messages a conversation lists on the 0.6 line, whether or not
/// their records are on this device.
pub async fn entries_listed(manager: &HolochainManager, agent: &str, conversation_hash: ActionHash, timeout: Duration) -> Result<u32, String> {
    let input = EntriesPageInput { conversation_hash, after: None, limit: 1 };
    page(manager, agent, "get_conversation_entries_page", &input, timeout).await.map(|p| p.total)
}

#[derive(Serialize, Debug)]
struct OthersEntriesInput {
    conversations: Vec<ActionHash>,
}
#[derive(Deserialize, Debug)]
struct OthersEntries {
    items: Vec<MyEntry>,
    incomplete: Vec<ActionHash>,
}
/// Conversations per `get_others_entries` call: each costs one local link
/// read inside the call, so a call stays well inside its timeout.
const OTHERS_PER_CALL: usize = 100;

/// The messages other agents recorded in `conversations`, each with its
/// conversation, from what this conductor holds (`get_others_entries`,
/// coordinator 14); and the conversations holding one it does not have,
/// which the caller reads in full. Err when the cell's coordinator predates
/// the function (callers fall back to the per-conversation reads) or a call
/// fails. `between` runs before each call (the move gives way to the person).
pub async fn others_entries<F: std::future::Future<Output = ()>>(
    manager: &HolochainManager,
    agent: &str,
    conversations: &[ActionHash],
    timeout: Duration,
    between: impl Fn() -> F,
) -> Result<(Vec<(Record, ActionHash)>, Vec<ActionHash>), String> {
    let mut out = Vec::new();
    let mut incomplete = Vec::new();
    let t0 = std::time::Instant::now();
    for chunk in conversations.chunks(OTHERS_PER_CALL) {
        between().await;
        let payload = ExternIO::encode(OthersEntriesInput { conversations: chunk.to_vec() }).map_err(|e| format!("Failed to encode: {}", e))?;
        let raw = manager.call_zome_with_timeout(agent, "transcript", "get_others_entries", payload, timeout).await?;
        let page: OthersEntries = ExternIO::decode(&raw).map_err(|e| format!("Failed to decode get_others_entries: {}", e))?;
        out.extend(page.items.into_iter().map(|i| (i.record, i.conversation)));
        incomplete.extend(page.incomplete);
    }
    log::info!(
        "[records] get_others_entries {}..: {} message(s) of other devices in {} conversation(s), {} to read in full, {} ms",
        &agent[..8.min(agent.len())],
        out.len(),
        conversations.len(),
        incomplete.len(),
        t0.elapsed().as_millis()
    );
    Ok((out, incomplete))
}

/// How many conversations an agent holds, without fetching their records.
/// How many conversations are listed at `anchor` (an agent key, hex; None =
/// the cell's own key), read through `agent`'s cell.
pub async fn conversation_count_at(manager: &HolochainManager, agent: &str, anchor: Option<&str>, timeout: Duration) -> Result<u64, String> {
    let Some(a) = anchor else { return conversation_count(manager, agent, timeout).await };
    let raw = hex::decode(a).ok().filter(|b| b.len() == 39).ok_or_else(|| format!("bad anchor key {a}"))?;
    let input = ConversationsPageInput { anchor: Some(AgentPubKey::from_raw_39(raw)), before: None, limit: 1 };
    page(manager, agent, "get_conversations_page", &input, timeout).await.map(|p| p.total as u64)
}

pub async fn conversation_count(manager: &HolochainManager, agent: &str, timeout: Duration) -> Result<u64, String> {
    let input = ConversationsPageInput { anchor: None, before: None, limit: 1 };
    match page(manager, agent, "get_conversations_page", &input, timeout).await {
        Ok(p) => Ok(p.total as u64),
        Err(e) if is_missing_function(&e) => unpaged(manager, agent, "get_all_conversations", (), timeout)
            .await
            .map(|r| r.records.len() as u64),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::is_missing_function;

    #[test]
    fn an_older_coordinator_is_recognized_and_a_timeout_is_not() {
        assert!(is_missing_function("RibosomeError(ZomeFnNotExists(ZomeName(\"transcript\"), FunctionName(\"get_conversations_page\")))"));
        assert!(is_missing_function("Attempted to call a zome function that doesn't exist: Zome: transcript Fn get_conversations_page"));
        assert!(!is_missing_function("zome call timed out after 60 s"));
        assert!(!is_missing_function("Agent not found"));
    }
}
