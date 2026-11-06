//! The move of a profile's transcript records from the 0.6 line (DNA v1) to
//! the 0.7 line (DNA v2), through flowsta-hc-lines' pass runner (build-docs
//! `apps/your-own-ai/planning/TRANSCRIPT_MOVE.md`).
//!
//! - **Units** are this device's AI cells (the enabled v1 apps, each with
//!   its v2 app on the 0.7 line).
//! - **Each device carries only the records it authored.** A conversation
//!   started on one device and continued on another is carried in parts:
//!   the conversation by its author, each message by its own; the anchor
//!   derived from the conversation's id joins them.
//! - **What v2 already holds is read from the links' keyed tags,** so a
//!   record carried before (by this pass or an interrupted one) is skipped
//!   without decrypting anything. Each kind names its records by the keyed
//!   hash of their id on both lines.
//! - **Deleted conversations are not carried,** nor their messages: those in
//!   the deleted ledger, and those tombstoned on the 0.7 line.

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

/// How long one read on either line may take: a large cell's first network
/// reads after a cold start take minutes.
const READ_TIMEOUT: Duration = Duration::from_secs(180);

/// What the pass hooks share.
pub struct Move {
    pub hc: Arc<HolochainState>,
    pub data_key: [u8; 32],
    pub cells: Cells07,
    /// An AI cell's founding key (from `ai-data.json`), where every device
    /// lists its conversations; the cell's own key when it has none.
    pub founding: Box<dyn Fn(&str) -> String + Send + Sync>,
    /// The deleted ledger's conversation ids.
    pub deleted: Box<dyn Fn() -> HashSet<String> + Send + Sync>,
}

impl Move {
    /// The move as the app runs it.
    pub fn for_app(app: &tauri::AppHandle, data_key: [u8; 32]) -> Self {
        let (a, b) = (app.clone(), app.clone());
        Move {
            hc: app.state::<Arc<HolochainState>>().inner().clone(),
            data_key,
            cells: Cells07::default(),
            founding: Box::new(move |agent| crate::commands_holochain::founding_key_for(&a, agent)),
            deleted: Box::new(move || crate::conversation_cache::deleted_ledger(&b).into_iter().map(|d| d.hash).collect()),
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

    /// Conversation ids not to carry: the deleted ledger's, and those whose
    /// anchor is tombstoned on the 0.7 line.
    async fn gone(&self, agent: &str) -> Result<(HashSet<String>, HashSet<[u8; 32]>), HookError> {
        let ids = (self.deleted)();
        let keys: crate::hc07::ConversationKeys = self.cells.call(agent, "get_conversation_keys", &Self::agent_key(&self.founding(agent))?).await?;
        let anchors = keys.tombstoned.iter().filter_map(|a| anchor_core(a)).collect();
        Ok((ids, anchors))
    }
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
}

/// The pass's units: this device's AI cells.
pub struct Cells;

#[async_trait::async_trait]
impl CopyUnits<Move> for Cells {
    async fn units(&self, cx: &PassCx<'_, Move>) -> Result<Vec<CopyUnit>, HookError> {
        let manager = cx.app.hc.get()?;
        let mut ids: Vec<String> = manager.agents.lock().await.keys().cloned().collect();
        ids.sort();
        Ok(ids.into_iter().map(|id| CopyUnit { id }).collect())
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

pub struct Conversations;

#[async_trait::async_trait]
impl VersionedKind<Move> for Conversations {
    fn name(&self) -> &'static str {
        "conversations"
    }

    async fn versions(&self, cx: &PassCx<'_, Move>, line: Line, unit: &CopyUnit) -> Result<Vec<Version>, HookError> {
        let mv = cx.app.as_ref();
        let agent = unit.id.as_str();
        let author = hex::decode(agent)?;
        let founding = mv.founding(agent);
        match line {
            Line::V06 => {
                let manager = mv.hc.get()?;
                let (gone_ids, gone_anchors) = mv.gone(agent).await?;
                let mut seen = HashSet::new();
                let mut out = Vec::new();
                // Listed at the founding key (since coordinator 9) and, for
                // conversations started before, at the cell's own key.
                let anchors: Vec<Option<&str>> = if founding == agent { vec![None] } else { vec![Some(founding.as_str()), None] };
                for anchor in anchors {
                    let read = crate::transcript_pages::all_conversations_at(manager, agent, anchor, READ_TIMEOUT).await?;
                    if !read.complete || read.missing > 0 {
                        return Err(format!("the conversation list was not read in full ({} unreadable)", read.missing).into());
                    }
                    for r in read.records {
                        if r.action().author().get_raw_39() != author.as_slice() {
                            continue;
                        }
                        let hash = r.action_address().get_raw_39().to_vec();
                        let id = id_of_v1(&hash);
                        let anchor = conversation_anchor(&mv.data_key, &id);
                        if !seen.insert(hash.clone()) || gone_ids.contains(&id) || gone_anchors.contains(&anchor) {
                            continue;
                        }
                        let Some((cipher, nonce)) = sealed(&r) else { continue };
                        let body = Body { cipher, nonce, id, conversation_id: None };
                        out.push(version(anchor, r.action().timestamp().as_micros(), hash, Some(&body))?);
                    }
                }
                Ok(out)
            }
            Line::V07 => {
                let keys: crate::hc07::ConversationKeys = mv.cells.call(agent, "get_conversation_keys", &Move::agent_key(&founding)?).await?;
                keys.conversations
                    .iter()
                    .filter(|k| k.author.get_raw_39() == author.as_slice())
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
        let hash: ActionHash = mv.cells.call(&unit.id, "start_conversation", &input).await?;
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
        let hash: ActionHash = mv.cells.call(&unit.id, "record_message", &input).await?;
        Ok(hash.get_raw_39().to_vec())
    }
}

/// The profile's pass runner: conversations, then messages.
pub fn runner() -> PassRunner<Move> {
    PassRunner::new(
        MARKER,
        Arc::new(Cells),
        vec![OwnKind::Versioned(Arc::new(Conversations)), OwnKind::Versioned(Arc::new(Messages))],
        Vec::new(),
        Arc::new(NoFlipHook),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let seal = |v: serde_json::Value| encrypt(&data_key, &serde_json::to_vec(&v).unwrap()).unwrap();
        let (n, c) = seal(serde_json::json!({"title": "Plans", "started_at": 1, "ai_personality_id": agent}));
        let out = manager.call_zome(&agent, "transcript", "start_conversation", ExternIO::encode(StartV1 { cipher: c, nonce: n.to_vec(), anchor: None }).unwrap()).await.unwrap();
        let conversation: ActionHash = ExternIO::decode(&out).unwrap();
        for (i, text) in ["hello", "hi there"].iter().enumerate() {
            let (n, c) = seal(serde_json::json!({"role": "user", "content": text, "sequence": i, "timestamp": i}));
            manager.call_zome(&agent, "transcript", "record_message", ExternIO::encode(RecordV1 { conversation_hash: conversation.clone(), cipher: c, nonce: n.to_vec() }).unwrap()).await.unwrap();
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
        let mv = Arc::new(Move { hc, data_key, cells: Cells07::default(), founding: Box::new(|a| a.to_string()), deleted: Box::new(HashSet::new) });

        let first = runner().run(&mv, &profile, "live").await;
        assert_eq!(first.error, None, "{first:?}");
        assert_eq!(first.kind("conversations").map(|k| (k.total, k.written)), Some((1, 1)));
        assert_eq!(first.kind("messages").map(|k| (k.total, k.written)), Some((2, 2)));
        assert!(first.flipped, "the marker flips once every kind is complete");

        let second = runner().run(&mv, &profile, "live again").await;
        assert_eq!(second.error, None, "{second:?}");
        assert_eq!(second.kind("conversations").map(|k| (k.present, k.written)), Some((1, 0)), "skipped by key");
        assert_eq!(second.kind("messages").map(|k| (k.present, k.written)), Some((2, 0)));

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
