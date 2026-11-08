//! The move of a profile's transcript records from the 0.6 line (DNA v1) to
//! the 0.7 line (DNA v2), through flowsta-hc-lines' pass runner.
//!
//! - **Units** are this device's AI cells (the enabled v1 apps, each with
//!   its v2 app on the 0.7 line).
//! - **Each device carries the records it authored** while another device
//!   still needs the 0.6 line. A conversation started on one device and
//!   continued on another is carried in parts: the conversation by its
//!   author, each message by its own; the anchor derived from the
//!   conversation's id joins them.
//! - **Once no other device needs the 0.6 line, a pass carries every
//!   author's records** (a removed or reset device's, which no one else
//!   would): nothing is left behind when the 0.6 line stops. A copy there
//!   by any device counts as present, so nothing is written twice.
//! - **What v2 already holds is read from the links' keyed tags,** so a
//!   record carried before (by this pass or an interrupted one) is skipped
//!   without decrypting anything. Each kind names its records by the keyed
//!   hash of their id on both lines.
//! - **Deleted conversations are not carried,** nor their messages: those in
//!   the deleted ledger and those tombstoned on either line. Every 0.6
//!   tombstone, from any device, is carried to the 0.7 line on every pass,
//!   so a deletion made on a device still on 0.9.0 reaches the moved copy.
//! - **Memory rows** carry every revision (the history view shows them all),
//!   listed from the agent's own chain whatever their scope.

use crate::commands_holochain::HolochainState;
use crate::hc07::{Cells07, RecordMessage, StartConversation};
use crate::transcript_ids::{conversation_anchor, conversation_plaintext_v2, id_of_v1, message_key, message_plaintext_v2};
use flowsta_hc_lines::marker::MarkerSpec;
use flowsta_hc_lines::pass::{CopyUnit, CopyUnits, HookError, NoFlipHook, OwnKind, PassApp, PassCx, PassRunner, Version, VersionedKind};
use flowsta_hc_lines::Line;
use holochain_types::prelude::{ActionHash, AgentPubKey, ExternalHash};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;
use tauri::Manager as _;

/// The profile's move marker (`moved-07` in the profile's root).
pub const MARKER: MarkerSpec = MarkerSpec { file_name: "moved-07", old_line_key: "old_line_needed", previous: Line::V06, current: Line::V07 };

/// The move's own files in a profile's root, beside the two lines' folders:
/// a factory reset or a key swap, which remove those folders, removes these
/// too (a reset profile must not read as moved, nor keep another identity's
/// facts about its devices).
pub(crate) const MOVE_FILES: [&str; 7] = [
    "moved-07",
    "purged-ais-07.json",
    "coordinator-version",
    "line-facts.json",
    crate::conductor::COPY_NEEDED_FILE,
    crate::conductor::KEYS_07_ONLY_FILE,
    crate::line_facts::STOP_PRINT_FILE,
];

/// Remove [`MOVE_FILES`] from a profile's root.
pub(crate) fn remove_move_files(root: &std::path::Path) {
    for f in MOVE_FILES {
        let p = root.join(f);
        if p.exists() {
            if let Err(e) = std::fs::remove_file(&p) {
                log::warn!("[0.7] could not remove {f}: {e}");
            }
        }
    }
}

/// How long one read on either line may take: a large cell's first network
/// reads after a cold start take minutes.
const READ_TIMEOUT: Duration = Duration::from_secs(180);

/// How long a pass waits for the network's answer to a link read once this
/// conductor's own answer is in. The links at an AI's founding key hold what
/// the person's other devices wrote too, so the network's answer is added
/// when it comes; but a peer that cannot be reached after a cold start
/// would otherwise hold whole passes for minutes. Whatever it misses (another
/// device's tombstone not yet here) the next pass carries.
const NETWORK_GRACE: Duration = Duration::from_secs(30);

/// `into` plus what `more` adds: keys by (key, author), tombstoned anchors once.
fn merge_keys(into: &mut crate::hc07::ConversationKeys, more: crate::hc07::ConversationKeys) {
    for k in more.conversations {
        if !into.conversations.iter().any(|x| x.key == k.key && x.author == k.author) {
            into.conversations.push(k);
        }
    }
    for t in more.tombstoned {
        if !into.tombstoned.contains(&t) {
            into.tombstoned.push(t);
        }
    }
}

/// What the pass hooks share.
pub struct Move {
    pub hc: Arc<HolochainState>,
    pub data_key: [u8; 32],
    /// The app's own connections to its v2 cells: the move's writes and the
    /// app's take turns on each chain (two writers on one chain lose one to
    /// "source chain head has moved").
    pub cells: &'static Cells07,
    /// An AI cell's founding key (from `ai-data.json`), where every device
    /// lists its conversations; the cell's own key when it has none.
    pub founding: Box<dyn Fn(&str) -> String + Send + Sync>,
    /// The deleted ledger's conversation ids.
    pub deleted: Box<dyn Fn() -> HashSet<String> + Send + Sync>,
    /// Whether this session has had a pass over every AI.
    pub passed_in_full: std::sync::atomic::AtomicBool,
    /// What this device knows of the person's other devices (`line_facts`).
    pub known: Box<dyn Fn(&std::path::Path) -> crate::line_facts::Known + Send + Sync>,
    /// This pass carries every author's records (no other device needs the
    /// 0.6 line any more): set as each pass begins.
    pub full: std::sync::atomic::AtomicBool,
}

impl Move {
    /// The move as the app runs it.
    pub fn for_app(app: &tauri::AppHandle, data_key: [u8; 32]) -> Self {
        let (a, b, c) = (app.clone(), app.clone(), app.clone());
        Move {
            hc: app.state::<Arc<HolochainState>>().inner().clone(),
            data_key,
            cells: crate::hc07::app_cells(),
            founding: Box::new(move |agent| crate::commands_holochain::founding_key_for(&a, agent)),
            deleted: Box::new(move || crate::conversation_cache::deleted_ledger(&b).into_iter().map(|d| d.hash).collect()),
            passed_in_full: Default::default(),
            known: Box::new(move |root| crate::line_facts::known(&c, root)),
            full: Default::default(),
        }
    }

    fn agent_key(hex_key: &str) -> Result<AgentPubKey, HookError> {
        let raw = hex::decode(hex_key).map_err(|_| format!("bad agent key {hex_key}"))?;
        if raw.len() != 39 {
            return Err(format!("bad agent key length {hex_key}").into());
        }
        Ok(AgentPubKey::from_raw_39(raw))
    }

    /// The AI's founding key for this cell: where every device lists its
    /// conversations.
    fn founding(&self, agent: &str) -> String {
        (self.founding)(agent)
    }

    /// A v1 call on `agent`'s cell.
    async fn call_06<I: Serialize + std::fmt::Debug, O: serde::de::DeserializeOwned + std::fmt::Debug>(&self, agent: &str, fn_name: &str, input: I) -> Result<O, HookError> {
        use holochain_types::prelude::ExternIO;
        let manager = self.hc.get()?;
        let out = manager.call_zome_with_timeout(agent, "transcript", fn_name, ExternIO::encode(input)?, READ_TIMEOUT).await?;
        Ok(ExternIO::decode(&out)?)
    }

    /// The conversations tombstoned on the 0.6 line under the AI's founding
    /// key (by any device), as ids.
    async fn tombstones_06(&self, agent: &str) -> Result<HashSet<String>, HookError> {
        let anchor = Self::agent_key(&self.founding(agent))?;
        let hashes: Vec<ActionHash> = match self.call_06::<_, Vec<ActionHash>>(agent, "get_tombstones_local", anchor.clone()).await {
            Ok(mut local) => {
                match tokio::time::timeout(NETWORK_GRACE, self.call_06::<_, Vec<ActionHash>>(agent, "get_tombstones", anchor)).await {
                    Ok(Ok(network)) => local.extend(network),
                    Ok(Err(e)) => log::info!("[0.7] pass: the 0.6 tombstones' network read failed ({e}); this device's own copy stands"),
                    Err(_) => log::info!("[0.7] pass: the 0.6 tombstones' network read did not answer within {} s; this device's own copy stands", NETWORK_GRACE.as_secs()),
                }
                local
            }
            // A cell still on a coordinator before 13 has no local read: the
            // network read decides, with its full timeout, as before.
            Err(_) => self.call_06(agent, "get_tombstones", anchor).await?,
        };
        Ok(hashes.iter().map(|h| id_of_v1(h.get_raw_39())).collect())
    }

    /// The AI's conversation keys on the 0.7 line: this conductor's own at
    /// once, and the network's added when they come within NETWORK_GRACE.
    async fn keys_07(&self, agent: &str, founding: &str) -> Result<crate::hc07::ConversationKeys, HookError> {
        let anchor = Self::agent_key(founding)?;
        match self.cells.call::<_, crate::hc07::ConversationKeys>(agent, "get_conversation_keys_local", &anchor).await {
            Ok(mut keys) => {
                match tokio::time::timeout(NETWORK_GRACE, self.cells.call::<_, crate::hc07::ConversationKeys>(agent, "get_conversation_keys", &anchor)).await {
                    Ok(Ok(network)) => merge_keys(&mut keys, network),
                    Ok(Err(e)) => log::info!("[0.7] pass: the 0.7 conversation keys' network read failed ({e}); this device's own copy stands"),
                    Err(_) => log::info!("[0.7] pass: the 0.7 conversation keys' network read did not answer within {} s; this device's own copy stands", NETWORK_GRACE.as_secs()),
                }
                Ok(keys)
            }
            // A v2 cell installed before the local read existed.
            Err(_) => Ok(self.cells.call(agent, "get_conversation_keys", &anchor).await?),
        }
    }

    /// Conversation ids not to carry: the deleted ledger's and those
    /// tombstoned on the 0.6 line; and anchors tombstoned on the 0.7 line.
    async fn gone(&self, agent: &str) -> Result<(HashSet<String>, HashSet<[u8; 32]>), HookError> {
        let mut ids = (self.deleted)();
        ids.extend(self.tombstones_06(agent).await?);
        let keys = self.keys_07(agent, &self.founding(agent)).await?;
        let anchors = keys.tombstoned.iter().filter_map(|a| anchor_core(a)).collect();
        Ok((ids, anchors))
    }
}

/// What a list must hide while both lines hold an AI's conversations, from
/// this device's own copies (instant; the network's arrive as they gossip):
/// ids deleted on the 0.6 line or in the deleted ledger, and anchors
/// tombstoned on the 0.7 line. A conversation deleted on one line stays
/// hidden even while its copy on the other line still lists.
#[derive(Default)]
pub(crate) struct Deleted {
    pub ids: HashSet<String>,
    pub anchors_07: HashSet<[u8; 32]>,
}

impl Deleted {
    pub fn hides(&self, data_key: &[u8; 32], id: &str) -> bool {
        self.ids.contains(id) || self.anchors_07.contains(&conversation_anchor(data_key, id))
    }
}

/// [`Deleted`] for `reader`'s cell of the AI founded at `founding` (hex).
pub(crate) async fn deleted_here(app: &tauri::AppHandle, manager: &crate::holochain::HolochainManager, reader: &str, founding: &str) -> Deleted {
    use holochain_types::prelude::ExternIO;
    let mut out = Deleted { ids: crate::conversation_cache::deleted_ledger(app).into_iter().map(|d| d.hash).collect(), ..Default::default() };
    let Ok(anchor) = Move::agent_key(founding) else { return out };
    let quick = Duration::from_secs(10);
    if crate::conductor::ports(Line::V06).is_some() && manager.is_provisioned(reader).await {
        if let Ok(payload) = ExternIO::encode(anchor.clone()) {
            // Coordinator 13 and later; an older cell has no local read and
            // its list hides its own tombstones already.
            if let Ok(io) = manager.call_zome_with_timeout(reader, "transcript", "get_tombstones_local", payload, quick).await {
                if let Ok(hashes) = ExternIO::decode::<Vec<ActionHash>>(&io) {
                    out.ids.extend(hashes.iter().map(|h| id_of_v1(h.get_raw_39())));
                }
            }
        }
    }
    if crate::conductor::ports(Line::V07).is_some() {
        if let Ok(Ok(keys)) = tokio::time::timeout(quick, crate::hc07::app_cells().call::<_, crate::hc07::ConversationKeys>(reader, "get_conversation_keys_local", &anchor)).await {
            out.anchors_07.extend(keys.tombstoned.iter().filter_map(|a| anchor_core(a)));
        }
    }
    out
}

/// The 32-byte core of a 39-byte `ExternalHash` as a link tag carries it.
fn anchor_core(raw_39: &[u8]) -> Option<[u8; 32]> {
    (raw_39.len() == 39).then(|| raw_39[3..35].try_into().ok()).flatten()
}

#[async_trait::async_trait]
impl PassApp for Move {
    fn line_up(&self, line: Line) -> bool {
        crate::conductor::ports(line).is_some()
    }

    /// The other devices, so a complete pass with none of them on 0.6
    /// records that nothing is left to carry (`carried_all_ms`). Asked once
    /// as each pass begins, which is also where the pass's mode is set:
    /// once no other device needs the 0.6 line, the pass carries every
    /// author's records, not only this device's (a removed or reset
    /// device's would vanish from view at the stop). That full pass reads
    /// every conversation, so it waits for the machine check's room for
    /// heavy work; until then the pass carries this device's own and says
    /// nothing here, so "nothing left" is only ever recorded by a full pass
    /// over every AI. Unknown facts say nothing.
    async fn siblings(app: &Arc<Self>, root: &std::path::Path) -> Option<flowsta_hc_lines::policy::Siblings> {
        use std::sync::atomic::Ordering;
        let known = (app.known)(root);
        let none_needs_06 = match &known {
            crate::line_facts::Known::Facts(f) => f.holding().is_empty(),
            crate::line_facts::Known::NeverShared => true,
            crate::line_facts::Known::Unknown => false,
        };
        let full = none_needs_06 && crate::machine::would_run(crate::machine::Work::Heavy);
        if app.full.swap(full, Ordering::SeqCst) != full {
            log::info!("[0.7] passes carry {}", if full { "every author's records (no other device needs the 0.6 line)" } else { "this device's own records" });
            if full {
                // The first full pass covers every AI, not only those a
                // write missed.
                app.passed_in_full.store(false, Ordering::SeqCst);
            }
        }
        if none_needs_06 && !full {
            return None;
        }
        match known {
            crate::line_facts::Known::Facts(f) => Some(f.siblings()),
            crate::line_facts::Known::NeverShared => Some(flowsta_hc_lines::policy::Siblings::NeverShared),
            crate::line_facts::Known::Unknown => None,
        }
    }
}

/// The pass's units: this device's AI cells. A session's first pass, and
/// any pass before the flip, copies every one; after that a pass copies only
/// the AIs a write to the 0.7 line missed (a large profile is not read in
/// full every few minutes), and the tombstones are carried for all.
pub struct Cells;

impl Cells {
    /// Every AI cell enabled on the 0.6 line. One the pass cannot read (its
    /// connection failed) or write (no v2 app yet) fails as a unit and is
    /// tried again, so the move is never counted done without it. (The tidy
    /// disables only cells it verified empty, so a disabled one has nothing
    /// to carry.)
    async fn all(_mv: &Move) -> Result<Vec<CopyUnit>, HookError> {
        let apps = crate::conductor::transcript_apps_06().await?;
        let mut ids: Vec<String> = apps.into_iter().map(|(_, key)| hex::encode(key)).collect();
        ids.sort();
        ids.dedup();
        Ok(ids.into_iter().map(|id| CopyUnit { id }).collect())
    }
}

#[async_trait::async_trait]
impl CopyUnits<Move> for Cells {
    async fn units(&self, cx: &PassCx<'_, Move>) -> Result<Vec<CopyUnit>, HookError> {
        let mv = cx.app.as_ref();
        let first = !mv.passed_in_full.swap(true, std::sync::atomic::Ordering::SeqCst);
        let all = Cells::all(mv).await?;
        if first || !cx.moved {
            return Ok(all);
        }
        let missed = crate::hc07::take_missed();
        Ok(all.into_iter().filter(|u| missed.contains(&u.id)).collect())
    }
}

/// What a v1 version carries to its write: the sealed entry, its id, and
/// for a message its conversation's.
#[derive(Serialize, Deserialize)]
struct Body {
    cipher: Vec<u8>,
    nonce: Vec<u8>,
    id: String,
    conversation_id: Option<String>,
}

fn version(logical: [u8; 32], created_us: i64, action_hash: Vec<u8>, body: Option<&Body>) -> Result<Version, HookError> {
    Ok(Version {
        logical_id: hex::encode(logical),
        // Conversations and messages never change: present is current.
        version_time: 0,
        created_at: created_us.max(0) as u64,
        deleted: false,
        action_hash,
        body: match body {
            Some(b) => rmp_serde::to_vec_named(b)?,
            None => Vec::new(),
        },
    })
}

/// The sealed entry of a v1 record.
fn sealed(record: &holochain_types::prelude::Record) -> Option<(Vec<u8>, Vec<u8>)> {
    let raw = record.entry().as_option()?.as_app_entry()?.as_ref().bytes().to_vec();
    let ee: crate::commands_holochain::EncryptedEntryRaw = rmp_serde::from_slice(&raw).ok()?;
    Some((ee.cipher, ee.nonce))
}

/// Open a v1 body, add its ids, seal it again with a new nonce.
fn reseal(key: &[u8; 32], b: &Body) -> Result<(Vec<u8>, Vec<u8>), HookError> {
    let plain = crate::transcript_crypto::decrypt(key, &b.nonce, &b.cipher)?;
    let value: serde_json::Value = serde_json::from_slice(&plain)?;
    let value = match &b.conversation_id {
        None => conversation_plaintext_v2(value, &b.id),
        Some(c) => message_plaintext_v2(value, &b.id, c),
    };
    let (nonce, cipher) = crate::transcript_crypto::encrypt(key, &serde_json::to_vec(&value)?)?;
    Ok((cipher, nonce.to_vec()))
}

// ── Conversations ───────────────────────────────────────────────────────

/// An AI's conversations on the 0.6 line, listed at the founding key and
/// the cell's own key: this device's own, or (`all_authors`) everyone's;
/// none that is deleted on either line.
async fn listed_06(mv: &Move, agent: &str, all_authors: bool) -> Result<Vec<holochain_types::prelude::Record>, HookError> {
    let manager = mv.hc.get()?;
    let author = hex::decode(agent)?;
    let founding = mv.founding(agent);
    let (gone_ids, gone_anchors) = mv.gone(agent).await?;
    let mut seen = HashSet::new();
    let mut out = Vec::new();
    // Listed at the founding key (since coordinator 9) and, for
    // conversations started before, at the cell's own key.
    let anchors: Vec<Option<&str>> = if founding == agent { vec![None] } else { vec![Some(founding.as_str()), None] };
    for anchor in anchors {
        let read = crate::transcript_pages::all_conversations_at(manager, agent, anchor, READ_TIMEOUT).await?;
        if !read.complete {
            return Err(format!("the conversation list was not read in full ({} unreadable)", read.missing).into());
        }
        // A listed record no peer holds: not this device's (its own records
        // are on its own chain), so it is left to the device that wrote it.
        if read.missing > 0 {
            log::info!("[0.7] pass: {} listed conversation(s) of {}.. are on no reachable peer: another device's, left to it", read.missing, &agent[..8.min(agent.len())]);
        }
        for r in read.records {
            if !all_authors && r.action().author().get_raw_39() != author.as_slice() {
                continue;
            }
            let hash = r.action_address().get_raw_39().to_vec();
            let id = id_of_v1(&hash);
            if !seen.insert(hash) || gone_ids.contains(&id) || gone_anchors.contains(&conversation_anchor(&mv.data_key, &id)) {
                continue;
            }
            out.push(r);
        }
    }
    Ok(out)
}

pub struct Conversations;

#[async_trait::async_trait]
impl VersionedKind<Move> for Conversations {
    fn name(&self) -> &'static str {
        "conversations"
    }

    async fn versions(&self, cx: &PassCx<'_, Move>, line: Line, unit: &CopyUnit) -> Result<Vec<Version>, HookError> {
        let mv = cx.app.as_ref();
        let agent = unit.id.as_str();
        let founding = mv.founding(agent);
        match line {
            Line::V06 => {
                let full = mv.full.load(std::sync::atomic::Ordering::SeqCst);
                let mut out = Vec::new();
                for r in listed_06(mv, agent, full).await? {
                    let hash = r.action_address().get_raw_39().to_vec();
                    let id = id_of_v1(&hash);
                    let anchor = conversation_anchor(&mv.data_key, &id);
                    let Some((cipher, nonce)) = sealed(&r) else { continue };
                    let body = Body { cipher, nonce, id, conversation_id: None };
                    out.push(version(anchor, r.action().timestamp().as_micros(), hash, Some(&body))?);
                }
                Ok(out)
            }
            // A copy there by any device counts: the key is the record's,
            // whoever wrote it, so nothing is ever written twice.
            Line::V07 => {
                let keys = mv.keys_07(agent, &founding).await?;
                keys.conversations
                    .iter()
                    .filter_map(|k| anchor_core(&k.key).map(|core| version(core, k.at.as_micros(), k.key.clone(), None)))
                    .collect()
            }
        }
    }

    async fn write(&self, cx: &PassCx<'_, Move>, line: Line, unit: &CopyUnit, v: &Version, _supersedes: Option<&[u8]>) -> Result<Vec<u8>, HookError> {
        if line != Line::V07 {
            return Err("conversations are carried to the 0.7 line only".into());
        }
        let mv = cx.app.as_ref();
        let body: Body = rmp_serde::from_slice(&v.body)?;
        let (cipher, nonce) = reseal(&mv.data_key, &body)?;
        let input = StartConversation {
            cipher,
            nonce,
            anchor: Move::agent_key(&mv.founding(&unit.id))?,
            conversation_anchor: ExternalHash::from_raw_32(conversation_anchor(&mv.data_key, &body.id).to_vec()),
        };
        let hash: ActionHash = mv.cells.write(&unit.id, "start_conversation", &input).await?;
        Ok(hash.get_raw_39().to_vec())
    }
}

// ── Messages and annotations ────────────────────────────────────────────

pub struct Messages;

#[async_trait::async_trait]
impl VersionedKind<Move> for Messages {
    fn name(&self) -> &'static str {
        "messages"
    }

    async fn versions(&self, cx: &PassCx<'_, Move>, line: Line, unit: &CopyUnit) -> Result<Vec<Version>, HookError> {
        let mv = cx.app.as_ref();
        let agent = unit.id.as_str();
        match line {
            Line::V06 if mv.full.load(std::sync::atomic::Ordering::SeqCst) => {
                // Every author's entries, conversation by conversation.
                let manager = mv.hc.get()?;
                let mut out = Vec::new();
                for c in listed_06(mv, agent, true).await? {
                    let conversation_id = id_of_v1(c.action_address().get_raw_39());
                    let read = crate::transcript_pages::all_entries(manager, agent, c.action_address().clone(), READ_TIMEOUT).await?;
                    if !read.complete {
                        return Err(format!("a conversation's entries were not read in full ({})", read.stopped_by.unwrap_or_default()).into());
                    }
                    for r in read.records {
                        let hash = r.action_address().get_raw_39().to_vec();
                        let id = id_of_v1(&hash);
                        let Some((cipher, nonce)) = sealed(&r) else { continue };
                        let key = message_key(&mv.data_key, &id);
                        let body = Body { cipher, nonce, id, conversation_id: Some(conversation_id.clone()) };
                        out.push(version(key, r.action().timestamp().as_micros(), hash, Some(&body))?);
                    }
                }
                Ok(out)
            }
            Line::V07 if mv.full.load(std::sync::atomic::Ordering::SeqCst) => {
                // Every author's message keys, for the same conversations.
                let mut out = Vec::new();
                for c in listed_06(mv, agent, true).await? {
                    let conversation_id = id_of_v1(c.action_address().get_raw_39());
                    let anchor = ExternalHash::from_raw_32(conversation_anchor(&mv.data_key, &conversation_id).to_vec());
                    let keys: Vec<crate::hc07::KeyItem> = mv.cells.call(agent, "get_message_keys", &anchor).await?;
                    for k in keys {
                        let Ok(core) = <[u8; 32]>::try_from(k.key.as_slice()) else { continue };
                        out.push(version(core, k.at.as_micros(), k.key.clone(), None)?);
                    }
                }
                Ok(out)
            }
            Line::V06 => {
                let manager = mv.hc.get()?;
                let (gone_ids, gone_anchors) = mv.gone(agent).await?;
                let (entries, _) = crate::transcript_pages::all_my_entries(manager, agent, READ_TIMEOUT).await?;
                let mut out = Vec::with_capacity(entries.len());
                for (r, conversation) in entries {
                    let conversation_id = id_of_v1(conversation.get_raw_39());
                    if gone_ids.contains(&conversation_id) || gone_anchors.contains(&conversation_anchor(&mv.data_key, &conversation_id)) {
                        continue;
                    }
                    let hash = r.action_address().get_raw_39().to_vec();
                    let id = id_of_v1(&hash);
                    let Some((cipher, nonce)) = sealed(&r) else { continue };
                    let key = message_key(&mv.data_key, &id);
                    let body = Body { cipher, nonce, id, conversation_id: Some(conversation_id) };
                    out.push(version(key, r.action().timestamp().as_micros(), hash, Some(&body))?);
                }
                Ok(out)
            }
            Line::V07 => {
                let founding = Move::agent_key(&mv.founding(agent))?;
                let mut out = Vec::new();
                let mut after_seq = None;
                loop {
                    let page = mv.cells.my_entries_page(agent, &crate::hc07::MyEntriesPage { anchor: founding.clone(), after_seq, limit: 500 }).await?;
                    for (record, _, key) in &page.items {
                        let Ok(core) = <[u8; 32]>::try_from(key.as_slice()) else { continue };
                        out.push(version(core, record.timestamp_us, record.action_hash.clone(), None)?);
                    }
                    match page.next_seq {
                        Some(s) => after_seq = Some(s),
                        None => return Ok(out),
                    }
                }
            }
        }
    }

    async fn write(&self, cx: &PassCx<'_, Move>, line: Line, unit: &CopyUnit, v: &Version, _supersedes: Option<&[u8]>) -> Result<Vec<u8>, HookError> {
        if line != Line::V07 {
            return Err("messages are carried to the 0.7 line only".into());
        }
        let mv = cx.app.as_ref();
        let body: Body = rmp_serde::from_slice(&v.body)?;
        let conversation_id = body.conversation_id.clone().ok_or("a message without its conversation")?;
        let (cipher, nonce) = reseal(&mv.data_key, &body)?;
        let input = RecordMessage {
            conversation_anchor: ExternalHash::from_raw_32(conversation_anchor(&mv.data_key, &conversation_id).to_vec()),
            message_key: message_key(&mv.data_key, &body.id).to_vec(),
            cipher,
            nonce,
        };
        let hash: ActionHash = mv.cells.write(&unit.id, "record_message", &input).await?;
        Ok(hash.get_raw_39().to_vec())
    }
}

// ── Memory rows ─────────────────────────────────────────────────────────

pub struct MemoryRows;

#[derive(Serialize, Deserialize)]
struct RowBody {
    cipher: Vec<u8>,
    nonce: Vec<u8>,
    anchor: Vec<u8>,
}

#[derive(Serialize, Debug)]
struct MyRowsPageInput {
    after_seq: Option<u32>,
    limit: u32,
}

#[derive(Deserialize, Debug)]
struct MyRow {
    record: holochain_types::prelude::Record,
    anchor: holochain_types::prelude::AnyLinkableHash,
}

#[derive(Deserialize, Debug)]
struct MyRowsPage {
    items: Vec<MyRow>,
    next_seq: Option<u32>,
}

/// A revision's name on both lines: its row's id and when it was written
/// (each revision is its own record; the reader reduces them by id).
fn revision_id(row: &crate::memory_rows::MemoryRow) -> String {
    format!("{}@{}", row.id, row.updated_at)
}

fn row_of(key: &[u8; 32], cipher: &[u8], nonce: &[u8]) -> Option<crate::memory_rows::MemoryRow> {
    let plain = crate::transcript_crypto::decrypt(key, nonce, cipher).ok()?;
    serde_json::from_slice(&plain).ok()
}

#[async_trait::async_trait]
impl VersionedKind<Move> for MemoryRows {
    fn name(&self) -> &'static str {
        "memory rows"
    }

    async fn versions(&self, cx: &PassCx<'_, Move>, line: Line, unit: &CopyUnit) -> Result<Vec<Version>, HookError> {
        let mv = cx.app.as_ref();
        let agent = unit.id.as_str();
        let mut rows = mine_06(mv, agent).await?;
        if mv.full.load(std::sync::atomic::Ordering::SeqCst) {
            rows.extend(others_06(mv, agent, &rows).await?);
        }
        match line {
            Line::V06 => Ok(rows
                .into_iter()
                .filter_map(|(r, anchor)| {
                    let (cipher, nonce) = sealed(&r)?;
                    let row = row_of(&mv.data_key, &cipher, &nonce)?;
                    let body = RowBody { cipher, nonce, anchor: anchor.get_raw_39().to_vec() };
                    Some(Version {
                        logical_id: revision_id(&row),
                        version_time: 0,
                        created_at: r.action().timestamp().as_micros().max(0) as u64,
                        deleted: false,
                        action_hash: r.action_address().get_raw_39().to_vec(),
                        body: rmp_serde::to_vec_named(&body).ok()?,
                    })
                })
                .collect()),
            Line::V07 => {
                // v2 lists rows by scope anchor: the anchors the rows above
                // are under on v1. A revision there by any device counts.
                let anchors: HashSet<Vec<u8>> = rows.iter().map(|(_, a)| a.get_raw_39().to_vec()).collect();
                let mut out = Vec::new();
                for anchor in anchors {
                    let views = mv.cells.memory_rows(agent, holochain_types::prelude::AnyLinkableHash::from_raw_39(anchor)).await?;
                    for view in views {
                        let Ok(ee) = view.entry_as::<crate::commands_holochain::EncryptedEntryRaw>() else { continue };
                        let Some(row) = row_of(&mv.data_key, &ee.cipher, &ee.nonce) else { continue };
                        out.push(Version {
                            logical_id: revision_id(&row),
                            version_time: 0,
                            created_at: view.timestamp_us.max(0) as u64,
                            deleted: false,
                            action_hash: view.action_hash.clone(),
                            body: Vec::new(),
                        });
                    }
                }
                Ok(out)
            }
        }
    }

    async fn write(&self, cx: &PassCx<'_, Move>, line: Line, unit: &CopyUnit, v: &Version, _supersedes: Option<&[u8]>) -> Result<Vec<u8>, HookError> {
        if line != Line::V07 {
            return Err("memory rows are carried to the 0.7 line only".into());
        }
        let mv = cx.app.as_ref();
        let b: RowBody = rmp_serde::from_slice(&v.body)?;
        // A row's plaintext already carries its id: the sealed bytes go
        // across as they are.
        let input = crate::hc07::SaveMemoryRow { cipher: b.cipher, nonce: b.nonce, anchor: holochain_types::prelude::AnyLinkableHash::from_raw_39(b.anchor) };
        let hash: ActionHash = mv.cells.write(&unit.id, "save_memory_row", &input).await?;
        Ok(hash.get_raw_39().to_vec())
    }
}

/// Every memory row `agent` wrote on the 0.6 line, with its scope anchor.
/// Every other author's memory rows at the scopes this device knows: the
/// person's, this AI's, and those this agent's own rows are under (a
/// project another device alone knew has an anchor no one here can name).
async fn others_06(mv: &Move, agent: &str, mine: &[(holochain_types::prelude::Record, holochain_types::prelude::AnyLinkableHash)]) -> Result<Vec<(holochain_types::prelude::Record, holochain_types::prelude::AnyLinkableHash)>, HookError> {
    #[derive(Serialize, Debug)]
    struct RowsAt {
        anchor: holochain_types::prelude::AnyLinkableHash,
    }
    let author = hex::decode(agent)?;
    let mut anchors: Vec<holochain_types::prelude::AnyLinkableHash> = Vec::new();
    for scope in ["person".to_string(), format!("ai:{}", mv.founding(agent))] {
        anchors.push(crate::memory_rows::scope_anchor(&scope, &mv.data_key)?);
    }
    for (_, a) in mine {
        if !anchors.contains(a) {
            anchors.push(a.clone());
        }
    }
    let have: HashSet<Vec<u8>> = mine.iter().map(|(r, _)| r.action_address().get_raw_39().to_vec()).collect();
    let mut out = Vec::new();
    for anchor in anchors {
        let records: Vec<holochain_types::prelude::Record> = mv.call_06(agent, "get_memory_rows", RowsAt { anchor: anchor.clone() }).await?;
        for r in records {
            if r.action().author().get_raw_39() == author.as_slice() || have.contains(r.action_address().get_raw_39()) {
                continue;
            }
            out.push((r, anchor.clone()));
        }
    }
    Ok(out)
}

async fn mine_06(mv: &Move, agent: &str) -> Result<Vec<(holochain_types::prelude::Record, holochain_types::prelude::AnyLinkableHash)>, HookError> {
    let mut out = Vec::new();
    let mut after_seq = None;
    loop {
        let page: MyRowsPage = mv.call_06(agent, "get_my_memory_rows_page", MyRowsPageInput { after_seq, limit: 500 }).await?;
        out.extend(page.items.into_iter().map(|i| (i.record, i.anchor)));
        match page.next_seq {
            Some(s) => after_seq = Some(s),
            None => return Ok(out),
        }
    }
}

// ── Tombstones ──────────────────────────────────────────────────────────

/// Every conversation tombstoned on the 0.6 line, or in the deleted ledger,
/// is tombstoned on the 0.7 line too. On every pass: a device still on
/// 0.9.0 deletes on 0.6 only.
pub struct Tombstones;

#[async_trait::async_trait]
impl flowsta_hc_lines::pass::CarriedRecords<Move> for Tombstones {
    fn name(&self) -> &'static str {
        "tombstones"
    }

    fn blocks_flip(&self) -> bool {
        true
    }

    async fn carry(&self, cx: &PassCx<'_, Move>, report: &mut flowsta_hc_lines::pass::CarryReport) -> Result<flowsta_hc_lines::pass::Carried, HookError> {
        let mv = cx.app.as_ref();
        let units = Cells::all(mv).await?;
        let mut any = false;
        let mut done_founding = HashSet::new();
        for unit in units {
            let agent = unit.id.as_str();
            let founding = mv.founding(agent);
            if !done_founding.insert(founding.clone()) {
                continue;
            }
            let mut ids = mv.tombstones_06(agent).await?;
            ids.extend((mv.deleted)());
            let keys = mv.keys_07(agent, &founding).await?;
            let have: HashSet<[u8; 32]> = keys.tombstoned.iter().filter_map(|a| anchor_core(a)).collect();
            for id in ids {
                any = true;
                report.total += 1;
                let anchor = conversation_anchor(&mv.data_key, &id);
                if !have.contains(&anchor) {
                    let input = crate::hc07::DeleteConversation { anchor: Move::agent_key(&founding)?, conversation_anchor: ExternalHash::from_raw_32(anchor.to_vec()) };
                    let _: u32 = mv.cells.write(agent, "delete_conversation", &input).await?;
                    report.add("written", 1);
                }
                report.carried += 1;
            }
        }
        Ok(if any { flowsta_hc_lines::pass::Carried::Ran } else { flowsta_hc_lines::pass::Carried::Nothing })
    }
}

/// What a start's 0.7 line needs, when the profile is to run it: a profile
/// with a 0.6 key store (a new install's first start makes it, and moves
/// from the next). None when it does not, or when the 0.7 key store could
/// not be prepared: the app then runs on 0.6 alone this session, and the
/// move waits.
pub async fn line_07_for_start(data_dir: &std::path::Path, recovery: &crate::transcript_crypto::RecoveryMaterial, resource_dir: &std::path::Path, fresh_on_07: bool) -> Option<crate::conductor::Line07> {
    let data_key = match recovery.data_key() {
        Ok(k) => k,
        Err(e) => {
            log::warn!("[0.7] not started: {e}");
            return None;
        }
    };
    let (lair_06, _) = crate::conductor::line_dirs(data_dir, Line::V06);
    let moved = !matches!(flowsta_hc_lines::marker::Marker::load(data_dir, &MARKER), flowsta_hc_lines::marker::MarkerRead::Absent);
    if !moved && !flowsta_hc_lines::keystore::config_path(&lair_06).exists() {
        if !fresh_on_07 {
            log::info!("[0.7] no 0.6 key store yet: the move begins at the next start");
            return None;
        }
        // A fresh profile that starts on the 0.7 line alone.
        return match crate::conductor::fresh_store_07(data_dir, &data_key) {
            Ok(passphrase) => Some(crate::conductor::Line07 { passphrase, network_seed: recovery.network_seed.clone(), resource_dir: resource_dir.to_path_buf() }),
            Err(e) => {
                log::warn!("[0.7] the fresh 0.7 key store: {e}");
                None
            }
        };
    }
    match crate::conductor::prepare_store_07(data_dir, &data_key).await {
        Ok(passphrase) => Some(crate::conductor::Line07 { passphrase, network_seed: recovery.network_seed.clone(), resource_dir: resource_dir.to_path_buf() }),
        Err(e) => {
            log::warn!("[0.7] not started this session: {e}");
            None
        }
    }
}

/// The last pass, for Settings and diagnostics: what it did, and the AIs it
/// could not carry, with why.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct LastPass {
    pub at_ms: u64,
    pub summary: String,
    pub skipped: Option<String>,
    pub failing: Vec<(String, String)>,
}

static LAST_PASS: std::sync::Mutex<Option<LastPass>> = std::sync::Mutex::new(None);

pub fn last_pass() -> Option<LastPass> {
    LAST_PASS.lock().ok().and_then(|p| p.clone())
}

fn note_pass(p: LastPass) {
    if let Ok(mut last) = LAST_PASS.lock() {
        *last = Some(p);
    }
}

/// Free space a pass needs on the profile's disk: a pass copies records,
/// and a full disk stops every write (this app's and the person's others).
const PASS_MIN_FREE: u64 = 1024 * 1024 * 1024;

/// How often passes run while both lines do: what a device still on 0.9.0
/// writes to the 0.6 line (its deletions) reaches the 0.7 line within this.
const PASS_EVERY: Duration = Duration::from_secs(15 * 60);

/// One line per pass for the log.
fn describe(out: &flowsta_hc_lines::pass::PassOutcome) -> String {
    if let Some(why) = &out.skipped {
        return format!("skipped: {why}");
    }
    let kinds: Vec<String> = out.kinds.iter().map(|k| format!("{} {}/{} (+{}){}", k.name, k.present + k.written, k.total, k.written, if k.complete { "" } else { " incomplete" })).collect();
    format!("{}{}{} in {} ms", kinds.join(", "), if out.flipped { "; flipped" } else { "" }, out.error.as_ref().map(|e| format!("; {e}")).unwrap_or_default(), out.took_ms)
}

/// The move's passes, once the 0.7 line has joined: the first as soon as
/// the app's 0.6 agents are connected, then every [`PASS_EVERY`] while both
/// lines run. One runner for the process.
static RUNNER: std::sync::OnceLock<PassRunner<Move>> = std::sync::OnceLock::new();

/// Stop the 0.6 line by the facts: no device needs it and a full pass left
/// nothing behind. Only as a secondary line (0.7 leads), never during a
/// pass: the runner drains the calls in flight, stops the line and records
/// `old_line_needed: false`, which lets writes go to 0.7 alone. Its data
/// stays on disk.
pub async fn retire_06(app: &tauri::AppHandle, data_dir: &std::path::Path) -> bool {
    if crate::conductor::leading() != Line::V07 {
        return false;
    }
    let hc = app.state::<Arc<HolochainState>>().inner().clone();
    let Ok(data_key) = hc.get().and_then(|m| m.data_key()) else { return false };
    let m = Move::for_app(app, data_key);
    let retired = RUNNER.get_or_init(runner).retire_previous(&m, data_dir, crate::conductor::supervisor()).await;
    if retired {
        if let Ok(manager) = hc.get() {
            manager.forget_06_agents().await;
        }
        crate::line_facts::note_stopped(data_dir);
        log::info!("[0.6] stopped: no other device needs it and nothing is left on it to carry (its data stays on disk)");
    }
    retired
}

/// How long a pass waits for the app to reach the AIs on the 0.6 line.
const AGENTS_06_WAIT: Duration = Duration::from_secs(120);

/// Whether the app reaches every AI the 0.6 line holds. With 0.7 leading,
/// the 0.6 line joins first and the app connects its AIs a moment later; a
/// pass before that finds none of them.
async fn agents_06_connected(manager: &crate::holochain::HolochainManager) -> bool {
    let Ok(apps) = crate::conductor::transcript_apps_06().await else { return false };
    for (_, key) in apps {
        if !manager.is_provisioned(&hex::encode(key)).await {
            return false;
        }
    }
    true
}

/// Wait, at most [`AGENTS_06_WAIT`], for [`agents_06_connected`]. A pass
/// after the wait reports any AI it still cannot reach, and the next one
/// tries it again.
async fn wait_for_06_agents(hc: &Arc<HolochainState>) {
    let mut waited = Duration::ZERO;
    loop {
        if let Ok(manager) = hc.get() {
            if agents_06_connected(&manager).await {
                if !waited.is_zero() {
                    log::info!("[0.7] the AIs on the 0.6 line connected after {} s: the pass starts", waited.as_secs());
                }
                return;
            }
        }
        if waited >= AGENTS_06_WAIT || crate::conductor::ports(Line::V06).is_none() {
            log::warn!("[0.7] the app does not reach every AI on the 0.6 line after {} s: the pass starts anyway", waited.as_secs());
            return;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
        waited += Duration::from_secs(2);
    }
}

pub fn run_passes(app: tauri::AppHandle, data_dir: std::path::PathBuf) {
    static RUNNING: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if RUNNING.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let runner = RUNNER.get_or_init(runner);
        let hc = app.state::<Arc<HolochainState>>().inner().clone();
        let mut waited = Duration::ZERO;
        while hc.get().is_err() {
            if waited > Duration::from_secs(600) {
                log::warn!("[0.7] the app's 0.6 agents never connected: no pass this session");
                RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
                return;
            }
            tokio::time::sleep(Duration::from_secs(2)).await;
            waited += Duration::from_secs(2);
        }
        let data_key = match hc.get().and_then(|m| m.data_key()) {
            Ok(k) => k,
            Err(e) => {
                log::warn!("[0.7] no pass: {e}");
                RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
                return;
            }
        };
        // The lineage holds on the 0.7 line's own apps too.
        if let Ok(manager) = hc.get() {
            if let Err(e) = manager.index_lineage_07().await {
                log::warn!("[0.7] lineage from the 0.7 apps: {e}");
            }
        }
        // The connections follow a 0.7 restart by themselves (`Cells07`).
        let m = Arc::new(Move::for_app(&app, data_key));
        loop {
            if crate::conductor::ports(Line::V07).is_none() || crate::conductor::ports(Line::V06).is_none() {
                break;
            }
            let now_ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
            if let Some((free, _)) = crate::llm::disk_space_for(&data_dir).filter(|(free, _)| *free < PASS_MIN_FREE) {
                let why = format!("the disk is almost full ({} MB free)", free / (1024 * 1024));
                log::warn!("[0.7] pass skipped: {why}");
                note_pass(LastPass { at_ms: now_ms, summary: String::new(), skipped: Some(why), failing: Vec::new() });
                tokio::time::sleep(PASS_EVERY).await;
                continue;
            }
            wait_for_06_agents(&hc).await;
            let out = runner.run(&m, &data_dir, "the 0.7 line is up").await;
            log::info!("[0.7] pass: {}", describe(&out));
            // An AI whose copy failed is copied again by the next pass.
            let mut failing: Vec<(String, String)> = Vec::new();
            for kind in &out.kinds {
                for (unit, why) in &kind.failed_units {
                    crate::hc07::note_missed(unit);
                    if !failing.iter().any(|(u, _)| u == unit) {
                        failing.push((unit.clone(), format!("{}: {why}", kind.name)));
                    }
                }
            }
            note_pass(LastPass { at_ms: now_ms, summary: describe(&out), skipped: out.skipped.clone(), failing });
            tokio::time::sleep(PASS_EVERY).await;
        }
        RUNNING.store(false, std::sync::atomic::Ordering::SeqCst);
    });
}

/// The profile's pass runner: conversations, messages, memory rows, then
/// the tombstones.
pub fn runner() -> PassRunner<Move> {
    PassRunner::new(
        MARKER,
        Arc::new(Cells),
        vec![OwnKind::Versioned(Arc::new(Conversations)), OwnKind::Versioned(Arc::new(Messages)), OwnKind::Versioned(Arc::new(MemoryRows))],
        vec![Arc::new(Tombstones)],
        Arc::new(NoFlipHook),
    )
}

#[cfg(test)]
mod deleted_tests {
    use super::*;

    #[test]
    fn a_conversation_deleted_on_either_line_is_hidden_from_both() {
        let key = [7u8; 32];
        let mut d = Deleted::default();
        d.ids.insert("v1-deleted".into());
        d.anchors_07.insert(conversation_anchor(&key, "v2-deleted"));
        assert!(d.hides(&key, "v1-deleted"), "deleted on 0.6 (or here): its 0.7 copy hides too");
        assert!(d.hides(&key, "v2-deleted"), "deleted on 0.7: its 0.6 copy hides too");
        assert!(!d.hides(&key, "kept"));
        assert!(!d.hides(&[8u8; 32], "v2-deleted"), "anchors follow the data key");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize, Debug)]
    struct StartV1 {
        cipher: Vec<u8>,
        nonce: Vec<u8>,
        anchor: Option<AgentPubKey>,
    }

    #[derive(Serialize, Debug)]
    struct RecordV1 {
        conversation_hash: ActionHash,
        cipher: Vec<u8>,
        nonce: Vec<u8>,
    }

    /// Kill this process's sidecars (both lines' conductors and key stores)
    /// with SIGKILL, as a power-off leaves them. Returns how many.
    #[cfg(target_os = "linux")]
    fn kill_sidecars() -> usize {
        let me = std::process::id().to_string();
        let mut killed = 0;
        for e in std::fs::read_dir("/proc").unwrap().flatten() {
            let pid = e.file_name().to_string_lossy().to_string();
            if !pid.chars().all(|c| c.is_ascii_digit()) {
                continue;
            }
            let Ok(stat) = std::fs::read_to_string(e.path().join("stat")) else { continue };
            // "pid (comm) state ppid ..."
            let ppid = stat.rsplit_once(") ").and_then(|(_, rest)| rest.split_whitespace().nth(1).map(str::to_string));
            let comm = std::fs::read_to_string(e.path().join("comm")).unwrap_or_default();
            if ppid.as_deref() == Some(me.as_str()) && comm.starts_with("yourowai-") {
                let _ = std::process::Command::new("kill").args(["-9", &pid]).status();
                killed += 1;
            }
        }
        killed
    }

    /// The real sidecars: an AI writes a conversation and two messages on
    /// 0.6; with both lines up, one pass carries them to 0.7 with their
    /// ids and flips the marker; a second pass writes nothing.
    /// Run with the app closed: `cargo test --lib -- --ignored live_move`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn live_move_one_pass_carries_an_ais_records_and_a_second_writes_nothing() {
        use crate::conductor::{prepare_store_07, start_for_test, supervisor, Line07};
        use crate::holochain::HolochainManager;
        use crate::transcript_crypto::{decrypt, encrypt, RecoveryMaterial};
        use holochain_types::prelude::ExternIO;
        let tmp = tempfile::tempdir().unwrap();
        let profile = tmp.path().join("profile");
        let resource_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        let data_key = [6u8; 32];
        let material = || RecoveryMaterial { network_seed: format!("live-move-{}", std::process::id()), data_key_hex: hex::encode(data_key), version: 1 };

        start_for_test(profile.clone(), None).await.expect("0.6 starts");
        let manager = HolochainManager::new(resource_dir.clone(), material());
        let agent = manager.provision_agent("live-move").await.expect("an AI");
        let seal = |v: serde_json::Value| encrypt(&data_key, &serde_json::to_vec(&v).unwrap()).unwrap();
        let (n, c) = seal(serde_json::json!({"title": "Plans", "started_at": 1, "ai_personality_id": agent}));
        let out = manager.call_zome(&agent, "transcript", "start_conversation", ExternIO::encode(StartV1 { cipher: c, nonce: n.to_vec(), anchor: None }).unwrap()).await.unwrap();
        let conversation: ActionHash = ExternIO::decode(&out).unwrap();
        for (i, text) in ["hello", "hi there"].iter().enumerate() {
            let (n, c) = seal(serde_json::json!({"role": "user", "content": text, "sequence": i, "timestamp": i}));
            manager.call_zome(&agent, "transcript", "record_message", ExternIO::encode(RecordV1 { conversation_hash: conversation.clone(), cipher: c, nonce: n.to_vec() }).unwrap()).await.unwrap();
        }
        // A second conversation, deleted on 0.6: not carried, its tombstone is.
        let (n, c) = seal(serde_json::json!({"title": "Gone", "started_at": 2, "ai_personality_id": agent}));
        let out = manager.call_zome(&agent, "transcript", "start_conversation", ExternIO::encode(StartV1 { cipher: c, nonce: n.to_vec(), anchor: None }).unwrap()).await.unwrap();
        let gone: ActionHash = ExternIO::decode(&out).unwrap();
        let (n, c) = seal(serde_json::json!({"role": "user", "content": "forget this", "sequence": 0, "timestamp": 5}));
        manager.call_zome(&agent, "transcript", "record_message", ExternIO::encode(RecordV1 { conversation_hash: gone.clone(), cipher: c, nonce: n.to_vec() }).unwrap()).await.unwrap();
        #[derive(Serialize, Debug)]
        struct DeleteV1 {
            conversation_hash: ActionHash,
            anchor: Option<AgentPubKey>,
        }
        manager.call_zome(&agent, "transcript", "delete_conversation_at", ExternIO::encode(DeleteV1 { conversation_hash: gone.clone(), anchor: Some(Move::agent_key(&agent).unwrap()) }).unwrap()).await.unwrap();
        // Two revisions of one memory row in the person's scope.
        #[derive(Serialize, Debug)]
        struct SaveRowV1 {
            cipher: Vec<u8>,
            nonce: Vec<u8>,
            anchor: holochain_types::prelude::AnyLinkableHash,
        }
        let scope_anchor = crate::memory_rows::scope_anchor("person", &data_key).unwrap();
        for (updated_at, text) in [(10, "likes tea"), (20, "likes green tea")] {
            let row = serde_json::json!({"id": "r1", "scope": "person", "kind": "fact", "text": text, "as_of": updated_at, "saved_by": "person", "provenance": "test", "updated_at": updated_at});
            let (n, c) = seal(row);
            manager.call_zome(&agent, "transcript", "save_memory_row", ExternIO::encode(SaveRowV1 { cipher: c, nonce: n.to_vec(), anchor: scope_anchor.clone() }).unwrap()).await.unwrap();
        }
        drop(manager);
        supervisor().stop();

        let passphrase = prepare_store_07(&profile, &data_key).await.expect("prepared");
        start_for_test(profile.clone(), Some(Line07 { passphrase, network_seed: material().network_seed, resource_dir: resource_dir.clone() })).await.expect("both lines");
        let until = std::time::Instant::now() + Duration::from_secs(240);
        while !supervisor().has(Line::V07) {
            assert!(std::time::Instant::now() < until, "the 0.7 line did not join");
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let manager = HolochainManager::new(resource_dir, material());
        manager.reconnect_existing_agents().await.expect("the AI reconnects");
        let hc = Arc::new(HolochainState::new());
        let _ = hc.manager.set(manager);
        let mv = Arc::new(Move { hc, data_key, cells: crate::hc07::app_cells(), founding: Box::new(|a| a.to_string()), deleted: Box::new(HashSet::new), passed_in_full: Default::default(), known: Box::new(|_| crate::line_facts::Known::Unknown), full: Default::default() });

        let first = runner().run(&mv, &profile, "live").await;
        assert_eq!(first.error, None, "{first:?}");
        assert_eq!(first.kind("conversations").map(|k| (k.total, k.written)), Some((1, 1)), "the deleted one is not carried");
        assert_eq!(first.kind("messages").map(|k| (k.total, k.written)), Some((2, 2)), "nor its message");
        assert_eq!(first.kind("memory rows").map(|k| (k.total, k.written)), Some((2, 2)), "both revisions");
        assert_eq!(first.kind("tombstones").map(|k| (k.total, k.written)), Some((1, 1)), "the deletion reaches 0.7");
        assert!(first.flipped, "the marker flips once every kind is complete");

        // A later pass in the session copies only AIs a write missed: none.
        let second = runner().run(&mv, &profile, "live again").await;
        assert_eq!(second.error, None, "{second:?}");
        assert_eq!(second.kind("conversations").map(|k| (k.total, k.written)), Some((0, 0)), "no AI to copy");

        // A new session's first pass reads every AI again and finds it all there.
        let fresh = Arc::new(Move { hc: mv.hc.clone(), data_key, cells: crate::hc07::app_cells(), founding: Box::new(|a| a.to_string()), deleted: Box::new(HashSet::new), passed_in_full: Default::default(), known: Box::new(|_| crate::line_facts::Known::Unknown), full: Default::default() });
        let again = runner().run(&fresh, &profile, "next session").await;
        assert_eq!(again.error, None, "{again:?}");
        assert_eq!(again.kind("conversations").map(|k| (k.present, k.written)), Some((1, 0)), "skipped by key");
        assert_eq!(again.kind("messages").map(|k| (k.present, k.written)), Some((2, 0)));
        assert_eq!(again.kind("memory rows").map(|k| (k.present, k.written)), Some((2, 0)));
        assert_eq!(second.kind("tombstones").map(|k| k.details.clone()), Some(Vec::new()), "no tombstone written twice");

        let conversation_id = id_of_v1(conversation.get_raw_39());
        let entries = mv
            .cells
            .entries_page(&agent, &crate::hc07::EntriesPage { conversation_anchor: ExternalHash::from_raw_32(conversation_anchor(&data_key, &conversation_id).to_vec()), after: None, limit: 10 })
            .await
            .expect("entries on v2");
        assert_eq!(entries.items.len(), 2, "both messages under the conversation's anchor");
        for (view, key) in &entries.items {
            let ee: crate::commands_holochain::EncryptedEntryRaw = view.entry_as().unwrap();
            let plain: serde_json::Value = serde_json::from_slice(&decrypt(&data_key, &ee.nonce, &ee.cipher).unwrap()).unwrap();
            assert_eq!(plain["conversation_id"].as_str(), Some(conversation_id.as_str()));
            assert_eq!(key.as_slice(), message_key(&data_key, plain["id"].as_str().unwrap()), "each listed under its own id's key");
        }
        supervisor().stop();
    }

    /// Nothing left behind: another device's conversation, at the AI's
    /// founding key, whose own cell is gone (disabled here). A pass in its
    /// own mode carries this device's records only and says nothing about
    /// the other devices; once no device needs the 0.6 line (and the machine
    /// has room), a full pass carries the other device's conversation and
    /// message too, over every AI, and only then records that nothing is
    /// left, so the 0.6 line is no longer wanted.
    /// Run with the app closed: `cargo test --lib -- --ignored live_move --test-threads 1`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn live_move_a_full_pass_carries_a_gone_devices_records_before_nothing_is_left() {
        use crate::conductor::{prepare_store_07, start_for_test, supervisor, Line07};
        use crate::holochain::HolochainManager;
        use crate::transcript_crypto::{encrypt, RecoveryMaterial};
        use holochain_types::prelude::ExternIO;
        let tmp = tempfile::tempdir().unwrap();
        let profile = tmp.path().join("profile");
        let resource_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        let data_key = [8u8; 32];
        let material = || RecoveryMaterial { network_seed: format!("live-full-{}", std::process::id()), data_key_hex: hex::encode(data_key), version: 1 };
        let seal = |v: serde_json::Value| encrypt(&data_key, &serde_json::to_vec(&v).unwrap()).unwrap();

        start_for_test(profile.clone(), None).await.expect("0.6 starts");
        let manager = HolochainManager::new(resource_dir.clone(), material());
        let a = manager.provision_agent("live-full-a").await.expect("the AI's founding cell");
        let b = manager.provision_agent("live-full-b").await.expect("a second cell, standing for another device's");
        let founding = Move::agent_key(&a).unwrap();
        // This device's own conversation, and the other device's, both at the founding key.
        let (n, c) = seal(serde_json::json!({"title": "Mine", "started_at": 1}));
        manager.call_zome(&a, "transcript", "start_conversation", ExternIO::encode(StartV1 { cipher: c, nonce: n.to_vec(), anchor: None }).unwrap()).await.unwrap();
        let (n, c) = seal(serde_json::json!({"title": "From the laptop", "started_at": 2}));
        let out = manager.call_zome(&b, "transcript", "start_conversation", ExternIO::encode(StartV1 { cipher: c, nonce: n.to_vec(), anchor: Some(founding.clone()) }).unwrap()).await.unwrap();
        let theirs: ActionHash = ExternIO::decode(&out).unwrap();
        let (n, c) = seal(serde_json::json!({"role": "user", "content": "written on the laptop", "sequence": 0, "timestamp": 3}));
        manager.call_zome(&b, "transcript", "record_message", ExternIO::encode(RecordV1 { conversation_hash: theirs.clone(), cipher: c, nonce: n.to_vec() }).unwrap()).await.unwrap();
        // The other device is gone: its cell no longer runs here.
        let b_app = manager.agents.lock().await.get(&b).map(|x| x.installed_app_id.clone()).unwrap();
        let admin = holochain_client::AdminWebsocket::connect(format!("localhost:{}", manager.admin_port()), Some(crate::conductor::origin())).await.unwrap();
        admin.disable_app(b_app).await.unwrap();
        drop(admin);
        drop(manager);
        supervisor().stop();

        let passphrase = prepare_store_07(&profile, &data_key).await.expect("prepared");
        start_for_test(profile.clone(), Some(Line07 { passphrase, network_seed: material().network_seed, resource_dir: resource_dir.clone() })).await.expect("both lines");
        let until = std::time::Instant::now() + Duration::from_secs(240);
        while !supervisor().has(Line::V07) {
            assert!(std::time::Instant::now() < until, "the 0.7 line did not join");
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        let manager = HolochainManager::new(resource_dir, material());
        manager.reconnect_existing_agents().await.expect("the AI reconnects");
        let hc = Arc::new(HolochainState::new());
        let _ = hc.manager.set(manager);
        let facts = std::sync::Arc::new(std::sync::Mutex::new(crate::line_facts::Known::Unknown));
        let f2 = facts.clone();
        let mv = Arc::new(Move {
            hc,
            data_key,
            cells: crate::hc07::app_cells(),
            founding: Box::new(|x| x.to_string()),
            deleted: Box::new(HashSet::new),
            passed_in_full: Default::default(),
            known: Box::new(move |_| f2.lock().unwrap().clone()),
            full: Default::default(),
        });
        let keys_at_founding = || async { mv.cells.call::<_, crate::hc07::ConversationKeys>(&a, "get_conversation_keys_local", &founding).await.unwrap().conversations.len() };

        // Own mode: the facts unknown, so this device's own records only.
        let own = runner().run(&mv, &profile, "own").await;
        assert_eq!(own.error, None, "{own:?}");
        assert_eq!(own.kind("conversations").map(|k| (k.total, k.written)), Some((1, 1)), "this device's own conversation only");
        assert!(own.flipped, "its own records are carried: the marker flips");
        assert_eq!(keys_at_founding().await, 1, "the gone device's conversation is not on 0.7 yet");
        let carried_all = |p: &std::path::Path| match flowsta_hc_lines::marker::Marker::load(p, &MARKER) {
            flowsta_hc_lines::marker::MarkerRead::Present(m) => m.carried_all_ms,
            _ => None,
        };
        assert_eq!(carried_all(&profile), None, "nothing is recorded as left from a pass that did not read every author");

        // No device needs the 0.6 line, the machine has room: a full pass.
        *facts.lock().unwrap() = crate::line_facts::Known::Facts(crate::line_facts::LineFacts { checked_at_ms: 1, ..Default::default() });
        crate::machine::set_reading_for_test(Some(crate::machine::Reading { others_cpu_pct: Some(0.0), memory_short: false, on_battery: false, away_secs: Some(3600) }));
        let full = runner().run(&mv, &profile, "full").await;
        crate::machine::set_reading_for_test(None);
        assert_eq!(full.error, None, "{full:?}");
        assert_eq!(full.kind("conversations").map(|k| (k.total, k.written)), Some((2, 1)), "every author's, the other device's written now (every AI read again)");
        assert_eq!(full.kind("messages").map(|k| k.written), Some(1), "and its message");
        assert_eq!(keys_at_founding().await, 2, "both on 0.7 at the founding key");
        assert!(carried_all(&profile).is_some(), "a full pass over every AI records that nothing is left");
        let decided = crate::line_facts::decide(&*facts.lock().unwrap(), flowsta_hc_lines::marker::Marker::load(&profile, &MARKER), true, 10);
        assert!(!decided.decision.wanted(), "the 0.6 line is no longer wanted: {decided:?}");

        // Another full pass writes nothing: present by key, whoever wrote it.
        crate::machine::set_reading_for_test(Some(crate::machine::Reading { others_cpu_pct: Some(0.0), memory_short: false, on_battery: false, away_secs: Some(3600) }));
        mv.passed_in_full.store(false, std::sync::atomic::Ordering::SeqCst);
        let again = runner().run(&mv, &profile, "full again").await;
        crate::machine::set_reading_for_test(None);
        assert_eq!(again.kind("conversations").map(|k| (k.present, k.written)), Some((2, 0)), "nothing twice");
        assert_eq!(again.kind("messages").map(|k| k.written), Some(0));
        supervisor().stop();
    }

    /// A profile from an older release moves whole. Every release since
    /// 0.1.0 ships the same transcript bundle; what differs is the
    /// coordinator its cells were brought to (none past the bundle's own in
    /// 0.1.0, 3 in 0.5-0.7, 8 in 0.8), and the records it wrote with it
    /// (conversations at the cell's own key, before coordinator 9's founding
    /// key). Each profile here is made with its release's own coordinator,
    /// taken from that release's tag; then this build's start sweeps the
    /// coordinator, and the move carries everything and flips.
    /// Run with the app closed: `cargo test --lib -- --ignored live_move_older --test-threads 1`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn live_move_older_release_profiles_move_whole() {
        use crate::conductor::{prepare_store_07, start_for_test, supervisor, Line07};
        use crate::holochain::HolochainManager;
        use crate::transcript_crypto::{encrypt, RecoveryMaterial};
        use holochain_types::prelude::ExternIO;
        let resources = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        let repo = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..");
        for (tag, coordinator_wasm) in [("v0.1.0", None), ("v0.5.1", Some("v0.5.1")), ("v0.8.1", Some("v0.8.1"))] {
            let tmp = tempfile::tempdir().unwrap();
            let profile = tmp.path().join("profile");
            // That release's resources: the shared bundle and its coordinator.
            let old_res = tmp.path().join("resources");
            std::fs::create_dir_all(&old_res).unwrap();
            std::fs::copy(resources.join(crate::dna::HAPP_FILENAME), old_res.join(crate::dna::HAPP_FILENAME)).unwrap();
            if let Some(t) = coordinator_wasm {
                let out = std::process::Command::new("git").current_dir(&repo).args(["show", &format!("{t}:src-tauri/resources/transcript_coordinator.wasm")]).output().unwrap();
                assert!(out.status.success(), "{tag}'s coordinator from its tag");
                std::fs::write(old_res.join("transcript_coordinator.wasm"), out.stdout).unwrap();
            }
            let data_key = [9u8; 32];
            let material = || RecoveryMaterial { network_seed: format!("live-older-{tag}-{}", std::process::id()), data_key_hex: hex::encode(data_key), version: 1 };
            let seal = |v: serde_json::Value| encrypt(&data_key, &serde_json::to_vec(&v).unwrap()).unwrap();

            start_for_test(profile.clone(), None).await.expect("0.6 starts");
            let old = HolochainManager::new(old_res.clone(), material());
            // 0.1.0 shipped no coordinator beside the bundle: its install
            // stops before the swap, leaving the bundle's own, as 0.1.0 did.
            let _ = old.provision_agent(&format!("older-{tag}")).await;
            old.reconnect_existing_agents().await.expect("the old cell connects");
            let agent = old.agents.lock().await.keys().next().cloned().expect("an AI from that release");
            for c in 0..3 {
                let (n, ci) = seal(serde_json::json!({"title": format!("{tag} #{c}"), "started_at": c + 1}));
                let out = old.call_zome(&agent, "transcript", "start_conversation", ExternIO::encode(StartV1 { cipher: ci, nonce: n.to_vec(), anchor: None }).unwrap()).await.unwrap();
                let conversation: ActionHash = ExternIO::decode(&out).unwrap();
                for m in 0..2 {
                    let (n, ci) = seal(serde_json::json!({"role": "user", "content": format!("{tag} {c}.{m}"), "sequence": m, "timestamp": m}));
                    old.call_zome(&agent, "transcript", "record_message", ExternIO::encode(RecordV1 { conversation_hash: conversation.clone(), cipher: ci, nonce: n.to_vec() }).unwrap()).await.unwrap();
                }
            }
            drop(old);
            supervisor().stop();
            let _ = std::fs::remove_file(profile.join("coordinator-version"));

            // This build's first start: its coordinator swept onto the old
            // cell, both lines up, one pass.
            let passphrase = prepare_store_07(&profile, &data_key).await.expect("prepared");
            start_for_test(profile.clone(), Some(Line07 { passphrase, network_seed: material().network_seed, resource_dir: resources.clone() })).await.expect("both lines");
            let until = std::time::Instant::now() + Duration::from_secs(240);
            while !supervisor().has(Line::V07) {
                assert!(std::time::Instant::now() < until, "the 0.7 line did not join");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            let manager = HolochainManager::new(resources.clone(), material());
            crate::dna::update_coordinators_sweep(manager.admin_port(), &resources, &profile).await.expect("the coordinator sweep");
            manager.reconnect_existing_agents().await.expect("the AI reconnects");
            let hc = Arc::new(HolochainState::new());
            let _ = hc.manager.set(manager);
            let mv = Arc::new(Move { hc, data_key, cells: crate::hc07::app_cells(), founding: Box::new(|x| x.to_string()), deleted: Box::new(HashSet::new), passed_in_full: Default::default(), known: Box::new(|_| crate::line_facts::Known::Unknown), full: Default::default() });
            let out = runner().run(&mv, &profile, tag).await;
            assert_eq!(out.error, None, "{tag}: {out:?}");
            assert_eq!(out.kind("conversations").map(|k| (k.total, k.written)), Some((3, 3)), "{tag}: every conversation");
            assert_eq!(out.kind("messages").map(|k| (k.total, k.written)), Some((6, 6)), "{tag}: every message");
            assert!(out.flipped, "{tag}: the marker flips");
            let again = runner().run(&mv, &profile, tag).await;
            assert_eq!(again.kind("conversations").map(|k| k.written), Some(0), "{tag}: nothing twice");
            supervisor().stop();
        }
    }

    /// A power-off mid-move: the pass is cut off with part of an AI's
    /// messages on 0.7 (every sidecar killed, the pass abandoned); the next
    /// start's pass carries exactly the rest, nothing twice, and only then
    /// flips the marker.
    /// Run with the app closed: `cargo test --lib -- --ignored live_move --test-threads 1`
    #[cfg(target_os = "linux")]
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn live_move_a_power_off_mid_pass_resumes_without_loss_or_duplicates() {
        use crate::conductor::{prepare_store_07, start_for_test, supervisor, Line07};
        use crate::holochain::HolochainManager;
        use crate::transcript_crypto::{encrypt, RecoveryMaterial};
        use flowsta_hc_lines::marker::{Marker, MarkerRead};
        use holochain_types::prelude::ExternIO;
        const N: usize = 400;
        let tmp = tempfile::tempdir().unwrap();
        let profile = tmp.path().join("profile");
        let resource_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        let data_key = [7u8; 32];
        let material = || RecoveryMaterial { network_seed: format!("live-move-cut-{}", std::process::id()), data_key_hex: hex::encode(data_key), version: 1 };
        let both_lines = || async {
            let passphrase = prepare_store_07(&profile, &data_key).await.expect("prepared");
            start_for_test(profile.clone(), Some(Line07 { passphrase, network_seed: material().network_seed, resource_dir: resource_dir.clone() })).await.expect("both lines");
            let until = std::time::Instant::now() + Duration::from_secs(240);
            while !supervisor().has(Line::V07) {
                assert!(std::time::Instant::now() < until, "the 0.7 line did not join");
                tokio::time::sleep(Duration::from_millis(500)).await;
            }
            let manager = HolochainManager::new(resource_dir.clone(), material());
            manager.reconnect_existing_agents().await.expect("the AI reconnects");
            let hc = Arc::new(HolochainState::new());
            let _ = hc.manager.set(manager);
            Arc::new(Move { hc, data_key, cells: crate::hc07::app_cells(), founding: Box::new(|a| a.to_string()), deleted: Box::new(HashSet::new), passed_in_full: Default::default(), known: Box::new(|_| crate::line_facts::Known::Unknown), full: Default::default() })
        };

        start_for_test(profile.clone(), None).await.expect("0.6 starts");
        let manager = HolochainManager::new(resource_dir.clone(), material());
        let agent = manager.provision_agent("live-move-cut").await.expect("an AI");
        let seal = |v: serde_json::Value| encrypt(&data_key, &serde_json::to_vec(&v).unwrap()).unwrap();
        let (n, c) = seal(serde_json::json!({"title": "Long", "started_at": 1, "ai_personality_id": agent}));
        let out = manager.call_zome(&agent, "transcript", "start_conversation", ExternIO::encode(StartV1 { cipher: c, nonce: n.to_vec(), anchor: None }).unwrap()).await.unwrap();
        let conversation: ActionHash = ExternIO::decode(&out).unwrap();
        for i in 0..N {
            let (n, c) = seal(serde_json::json!({"role": if i % 2 == 0 { "user" } else { "assistant" }, "content": format!("message {i}"), "sequence": i, "timestamp": i}));
            manager.call_zome(&agent, "transcript", "record_message", ExternIO::encode(RecordV1 { conversation_hash: conversation.clone(), cipher: c, nonce: n.to_vec() }).unwrap()).await.unwrap();
        }
        drop(manager);
        supervisor().stop();

        let mv = both_lines().await;
        // The offline leg runs this with auth material and a rendezvous that
        // does not answer (`FLOWSTA_AUTH_MATERIAL`, `FLOWSTA_BOOTSTRAP_URL`,
        // `FLOWSTA_BOOTSTRAP_URL_07`): the lines start without their auth.
        eprintln!("both lines up; started offline: {}", crate::conductor::started_offline());
        let anchor = ExternalHash::from_raw_32(conversation_anchor(&data_key, &id_of_v1(conversation.get_raw_39())).to_vec());
        let on_07 = |mv: Arc<Move>| {
            let (agent, anchor) = (agent.clone(), anchor.clone());
            async move { mv.cells.entries_page(&agent, &crate::hc07::EntriesPage { conversation_anchor: anchor, after: None, limit: 1 }).await.map(|p| p.total as usize) }
        };
        let pass = tokio::spawn({
            let (mv, profile) = (mv.clone(), profile.clone());
            async move { runner().run(&mv, &profile, "to be cut").await }
        });
        let mut cut_at = 0;
        let until = std::time::Instant::now() + Duration::from_secs(600);
        while cut_at < N / 10 {
            if pass.is_finished() {
                panic!("the pass finished before it could be cut ({cut_at} seen): {:?}", pass.await);
            }
            assert!(std::time::Instant::now() < until, "the pass carried {cut_at} messages in 10 min");
            match on_07(mv.clone()).await {
                Ok(n) => cut_at = n,
                Err(e) => eprintln!("reading 0.7: {e}"),
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        assert!(cut_at < N, "cut with part of the messages carried");
        let killed = kill_sidecars();
        pass.abort();
        assert!(killed >= 2, "both conductors killed (killed {killed})");
        supervisor().stop();
        assert_eq!(Marker::load(&profile, &MARKER), MarkerRead::Absent, "an interrupted pass flips nothing");
        drop(mv);

        let mv = both_lines().await;
        let out = runner().run(&mv, &profile, "after the power-off").await;
        assert_eq!(out.error, None, "{out:?}");
        assert_eq!(out.kind("conversations").map(|k| (k.present, k.written)), Some((1, 0)), "the conversation was carried before the cut");
        let messages = out.kind("messages").expect("messages");
        eprintln!("cut with {cut_at} of {N} messages on 0.7; the next pass found {} and wrote {}", messages.present, messages.written);
        assert_eq!(messages.present + messages.written, N, "{messages:?}");
        assert!(messages.present >= cut_at, "what was on 0.7 at the cut is still there ({} < {cut_at})", messages.present);
        assert!(out.flipped, "the marker flips once every kind is complete");

        assert_eq!(on_07(mv.clone()).await.unwrap(), N, "every message under the conversation's anchor");
        let mine = mv.cells.my_entries_page(&agent, &crate::hc07::MyEntriesPage { anchor: Move::agent_key(&agent).unwrap(), after_seq: None, limit: (2 * N) as u32 }).await.expect("own entries");
        let keys: HashSet<Vec<u8>> = mine.items.iter().map(|(_, _, k)| k.clone()).collect();
        assert_eq!((mine.total as usize, mine.items.len(), keys.len()), (N, N, N), "each message written once on the 0.7 chain");
        supervisor().stop();
    }

    /// A large profile: twenty AIs, each with a conversation of twenty
    /// messages and a memory row, carried by one pass that flips the marker.
    /// Run with the app closed: `cargo test --lib -- --ignored live_move --test-threads 1`
    #[tokio::test(flavor = "multi_thread")]
    #[ignore]
    async fn live_move_a_large_profile_moves_in_one_pass() {
        use crate::conductor::{prepare_store_07, start_for_test, supervisor, Line07};
        use crate::holochain::HolochainManager;
        use crate::transcript_crypto::{encrypt, RecoveryMaterial};
        use holochain_types::prelude::ExternIO;
        const AIS: usize = 20;
        const MESSAGES: usize = 20;
        #[derive(Serialize, Debug)]
        struct SaveRowV1 {
            cipher: Vec<u8>,
            nonce: Vec<u8>,
            anchor: holochain_types::prelude::AnyLinkableHash,
        }
        let tmp = tempfile::tempdir().unwrap();
        let profile = tmp.path().join("profile");
        let resource_dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("resources");
        let data_key = [8u8; 32];
        let material = || RecoveryMaterial { network_seed: format!("live-move-large-{}", std::process::id()), data_key_hex: hex::encode(data_key), version: 1 };
        let seal = |v: serde_json::Value| encrypt(&data_key, &serde_json::to_vec(&v).unwrap()).unwrap();

        start_for_test(profile.clone(), None).await.expect("0.6 starts");
        let manager = HolochainManager::new(resource_dir.clone(), material());
        let started = std::time::Instant::now();
        let mut agents = Vec::new();
        for a in 0..AIS {
            let agent = manager.provision_agent(&format!("live-move-large-{a}")).await.expect("an AI");
            let (n, c) = seal(serde_json::json!({"title": format!("Conversation {a}"), "started_at": a, "ai_personality_id": agent}));
            let out = manager.call_zome(&agent, "transcript", "start_conversation", ExternIO::encode(StartV1 { cipher: c, nonce: n.to_vec(), anchor: None }).unwrap()).await.unwrap();
            let conversation: ActionHash = ExternIO::decode(&out).unwrap();
            for i in 0..MESSAGES {
                let (n, c) = seal(serde_json::json!({"role": "user", "content": format!("{a}.{i}"), "sequence": i, "timestamp": i}));
                manager.call_zome(&agent, "transcript", "record_message", ExternIO::encode(RecordV1 { conversation_hash: conversation.clone(), cipher: c, nonce: n.to_vec() }).unwrap()).await.unwrap();
            }
            let scope_anchor = crate::memory_rows::scope_anchor("person", &data_key).unwrap();
            let row = serde_json::json!({"id": format!("r{a}"), "scope": "person", "kind": "fact", "text": "likes tea", "as_of": 1, "saved_by": "person", "provenance": "test", "updated_at": 1});
            let (n, c) = seal(row);
            manager.call_zome(&agent, "transcript", "save_memory_row", ExternIO::encode(SaveRowV1 { cipher: c, nonce: n.to_vec(), anchor: scope_anchor }).unwrap()).await.unwrap();
            agents.push(agent);
        }
        eprintln!("{AIS} AIs written on 0.6 in {} s", started.elapsed().as_secs());
        drop(manager);
        supervisor().stop();

        let started = std::time::Instant::now();
        let passphrase = prepare_store_07(&profile, &data_key).await.expect("prepared");
        start_for_test(profile.clone(), Some(Line07 { passphrase, network_seed: material().network_seed, resource_dir: resource_dir.clone() })).await.expect("both lines");
        let until = std::time::Instant::now() + Duration::from_secs(600);
        while !supervisor().has(Line::V07) {
            assert!(std::time::Instant::now() < until, "the 0.7 line did not join");
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
        eprintln!("both lines up, {AIS} v2 apps installed, in {} s", started.elapsed().as_secs());
        let manager = HolochainManager::new(resource_dir, material());
        manager.reconnect_existing_agents().await.expect("the AIs reconnect");
        let hc = Arc::new(HolochainState::new());
        let _ = hc.manager.set(manager);
        let mv = Arc::new(Move { hc, data_key, cells: crate::hc07::app_cells(), founding: Box::new(|a| a.to_string()), deleted: Box::new(HashSet::new), passed_in_full: Default::default(), known: Box::new(|_| crate::line_facts::Known::Unknown), full: Default::default() });

        let out = runner().run(&mv, &profile, "large").await;
        eprintln!("the pass took {} s", out.took_ms / 1000);
        assert_eq!(out.error, None, "{out:?}");
        assert_eq!(out.kind("conversations").map(|k| (k.total, k.written)), Some((AIS, AIS)));
        assert_eq!(out.kind("messages").map(|k| (k.total, k.written)), Some((AIS * MESSAGES, AIS * MESSAGES)));
        assert_eq!(out.kind("memory rows").map(|k| (k.total, k.written)), Some((AIS, AIS)));
        assert!(out.flipped, "the marker flips once every kind is complete");
        for (a, agent) in agents.iter().enumerate() {
            let mine = mv.cells.my_entries_page(agent, &crate::hc07::MyEntriesPage { anchor: Move::agent_key(agent).unwrap(), after_seq: None, limit: 100 }).await.expect("own entries");
            assert_eq!(mine.total as usize, MESSAGES, "AI {a}: its messages on its own 0.7 chain, once each");
        }
        supervisor().stop();
    }

    #[test]
    fn a_tag_anchor_gives_the_core_the_app_derives() {
        let core = [9u8; 32];
        let raw = ExternalHash::from_raw_32(core.to_vec()).get_raw_39().to_vec();
        assert_eq!(anchor_core(&raw), Some(core), "the v2 tag's anchor and the app's derivation meet");
        assert_eq!(anchor_core(&raw[..20]), None);
    }

    #[test]
    fn a_resealed_body_is_v1_with_its_ids_under_a_new_nonce() {
        let key = [3u8; 32];
        let plain = serde_json::json!({"role": "user", "content": "hi", "sequence": 1, "timestamp": 2});
        let (nonce, cipher) = crate::transcript_crypto::encrypt(&key, &serde_json::to_vec(&plain).unwrap()).unwrap();
        let body = Body { cipher: cipher.clone(), nonce: nonce.to_vec(), id: "m1".into(), conversation_id: Some("c1".into()) };
        let (c2, n2) = reseal(&key, &body).unwrap();
        assert_ne!(n2, nonce.to_vec(), "a new nonce");
        let back: serde_json::Value = serde_json::from_slice(&crate::transcript_crypto::decrypt(&key, &n2, &c2).unwrap()).unwrap();
        assert_eq!((back["id"].as_str(), back["conversation_id"].as_str(), back["content"].as_str()), (Some("m1"), Some("c1"), Some("hi")));
    }
}
