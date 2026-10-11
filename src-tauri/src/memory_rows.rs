//! Memory rows (build-docs apps/your-own-ai/planning/MEMORY_REDESIGN.md §3,
//! §11): one typed, dated, sealed record for everything an AI or the person
//! remembers - a rule, a command, a location, a decision, a fact, an
//! episode, a note - in a scope (the person, one AI, a project, a
//! conversation). Rows are records in the person's own transcript network
//! (coordinator 10: a `mem` link at a scope anchor), so they follow the
//! identity across devices by construction. A correction or a deletion is
//! a NEW row with the same `id`; readers reduce to the newest per id, the
//! same way on every device.

use crate::commands_holochain::HolochainState;
use holochain_types::prelude::{ActionHash, AgentPubKey, AnyLinkableHash, ExternIO, ExternalHash, Record};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::State;

/// What a row is. A `rule` is the person's words verbatim and may be
/// pinned; extraction may propose but never save one (decision 2).
pub const KINDS: &[&str] = &["rule", "command", "location", "decision", "fact", "episode", "note"];

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MemoryRow {
    /// Logical id - the same across revisions and devices.
    pub id: String,
    /// `person` | `ai:<founding key>` | `project:<folder identity>` | `conversation:<hash>`.
    pub scope: String,
    pub kind: String,
    /// The statement, Markdown.
    pub text: String,
    /// `fact {subject, predicate, value}`, `decision {decided_by, plan_ref}`, `command {cmd, cwd}`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structured: Option<serde_json::Value>,
    /// ms epoch the statement holds from (absolute).
    pub as_of: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_to: Option<i64>,
    /// `person` | `ai:<founding key>`.
    pub saved_by: String,
    /// `authored` | `extracted:<action hash>` | `distilled:<conversation hash>` | `dream:<ids>` | `migrated:<store>`.
    pub provenance: String,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub topics: Vec<String>,
    /// Ids this revision merges or corrects.
    #[serde(default)]
    pub replaces: Vec<String>,
    /// A deletion is a record: the newest revision says so.
    #[serde(default)]
    pub deleted: bool,
    /// ms epoch of this revision.
    pub updated_at: i64,
    /// The record's action hash (hex) once read back; not part of the
    /// sealed payload's meaning, set by the reader.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_hash: Option<String>,
}

/// Reduce every revision to the newest per `id`: newer `updated_at`, then a
/// deletion over a live version, then the larger action hash (the Vault's
/// rule - the same answer on every device). Deleted ids are dropped; rows
/// another row `replaces` are dropped too. Pinned rules first, then newest.
pub fn reduce(rows: Vec<MemoryRow>) -> Vec<MemoryRow> {
    use std::collections::HashMap;
    let mut newest: HashMap<String, MemoryRow> = HashMap::new();
    for r in rows {
        match newest.get(&r.id) {
            Some(cur) if !newer(&r, cur) => {}
            _ => {
                newest.insert(r.id.clone(), r);
            }
        }
    }
    let replaced: std::collections::HashSet<String> = newest.values().flat_map(|r| r.replaces.iter().cloned()).collect();
    let mut live: Vec<MemoryRow> = newest
        .into_values()
        .filter(|r| !r.deleted && !replaced.contains(&r.id))
        .collect();
    live.sort_by(|a, b| {
        b.pinned
            .cmp(&a.pinned)
            .then(b.updated_at.cmp(&a.updated_at))
            .then(a.id.cmp(&b.id))
    });
    live
}

fn newer(a: &MemoryRow, b: &MemoryRow) -> bool {
    (a.updated_at, a.deleted, a.action_hash.as_deref().unwrap_or(""))
        > (b.updated_at, b.deleted, b.action_hash.as_deref().unwrap_or(""))
}

/// The anchor a scope's rows hang from. An `ai:` scope hangs from the AI's
/// founding key (the same address its conversations list at); every other
/// scope from an external hash over the scope string under this network's
/// data key, so two people with the same folder path never share an anchor
/// and the anchor says nothing about the folder to a peer.
pub fn scope_anchor(scope: &str, data_key: &[u8; 32]) -> Result<AnyLinkableHash, String> {
    if let Some(founding_hex) = scope.strip_prefix("ai:") {
        let raw = hex::decode(founding_hex).map_err(|_| "bad founding key".to_string())?;
        if raw.len() != 39 {
            return Err("bad founding key length".into());
        }
        return Ok(AgentPubKey::from_raw_39(raw).into());
    }
    use sha2::Digest;
    let mut h = sha2::Sha256::new();
    h.update(b"yoai-memory-anchor-v1\0");
    h.update(data_key);
    h.update(scope.as_bytes());
    let digest = h.finalize();
    Ok(ExternalHash::from_raw_32(digest.to_vec()).into())
}

#[derive(Serialize, Debug)]
struct SaveInput {
    cipher: Vec<u8>,
    nonce: Vec<u8>,
    anchor: AnyLinkableHash,
}

#[derive(Serialize, Debug)]
struct RowsInput {
    anchor: AnyLinkableHash,
}

/// Write one row (a new row, a revision, or a deletion) through this
/// device's agent for the AI (`agent_key`); any provisioned agent can
/// write a person- or project-scope row, since the anchor is the scope's.
#[tauri::command]
pub async fn save_memory_row(
    agent_key: String,
    row: MemoryRow,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<String, String> {
    if !KINDS.contains(&row.kind.as_str()) {
        return Err(format!("unknown memory kind {}", row.kind));
    }
    let manager = hc_state.get()?;
    let key = manager.data_key()?;
    let mut sealed = row.clone();
    sealed.action_hash = None;
    let plain = serde_json::to_vec(&sealed).map_err(|e| e.to_string())?;
    let (nonce, cipher) = crate::transcript_crypto::encrypt(&key, &plain)?;
    let anchor = scope_anchor(&row.scope, &key)?;
    let payload = ExternIO::encode(SaveInput { cipher, nonce: nonce.to_vec(), anchor }).map_err(|e| e.to_string())?;
    let out = manager.call_zome(&agent_key, "transcript", "save_memory_row", payload).await?;
    let hash: ActionHash = ExternIO::decode(&out).map_err(|e| e.to_string())?;
    Ok(hex::encode(hash.get_raw_39()))
}

/// Every live row in a scope, reduced, pinned rules first.
#[tauri::command]
pub async fn list_memory_rows(
    agent_key: String,
    scope: String,
    hc_state: State<'_, Arc<HolochainState>>,
) -> Result<Vec<MemoryRow>, String> {
    let manager = hc_state.get()?;
    let key = manager.data_key()?;
    let anchor = scope_anchor(&scope, &key)?;
    let payload = ExternIO::encode(RowsInput { anchor }).map_err(|e| e.to_string())?;
    let out = manager.call_zome(&agent_key, "transcript", "get_memory_rows", payload).await?;
    let records: Vec<Record> = ExternIO::decode(&out).map_err(|e| e.to_string())?;
    let mut rows = Vec::with_capacity(records.len());
    for record in &records {
        let Some((plain, _)) = crate::vault_escrow::open_record(&key, record) else { continue };
        let Ok(mut row) = serde_json::from_value::<MemoryRow>(plain) else { continue };
        row.action_hash = Some(hex::encode(record.action_address().get_raw_39()));
        rows.push(row);
    }
    Ok(reduce(rows))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(id: &str, text: &str, at: i64) -> MemoryRow {
        MemoryRow {
            id: id.into(), scope: "person".into(), kind: "note".into(), text: text.into(), structured: None,
            as_of: at, valid_to: None, saved_by: "person".into(), provenance: "authored".into(), pinned: false,
            topics: vec![], replaces: vec![], deleted: false, updated_at: at, action_hash: Some(format!("h{at}")),
        }
    }

    #[test]
    fn the_newest_revision_per_id_wins_a_deletion_is_a_record_and_pinned_rules_come_first() {
        let mut deleted = row("b", "gone", 30);
        deleted.deleted = true;
        let mut rule = row("r", "always ask before sending", 5);
        rule.kind = "rule".into();
        rule.pinned = true;
        let rows = vec![row("a", "old", 10), row("a", "new", 20), row("b", "live", 20), deleted, rule, row("c", "c", 15)];
        let out = reduce(rows);
        let ids: Vec<&str> = out.iter().map(|r| r.id.as_str()).collect();
        assert_eq!(ids, vec!["r", "a", "c"], "pinned first, then newest; b is deleted");
        assert_eq!(out[1].text, "new");
    }

    #[test]
    fn a_same_time_tie_prefers_the_deletion_then_the_larger_hash_the_same_on_every_device() {
        let mut d = row("a", "x", 10);
        d.deleted = true;
        d.action_hash = Some("h1".into());
        let mut l = row("a", "y", 10);
        l.action_hash = Some("h9".into());
        assert!(reduce(vec![l.clone(), d.clone()]).is_empty());
        assert!(reduce(vec![d, l]).is_empty(), "order of arrival does not matter");
        let mut p = row("a", "p", 10);
        p.action_hash = Some("h1".into());
        let mut q = row("a", "q", 10);
        q.action_hash = Some("h9".into());
        assert_eq!(reduce(vec![p.clone(), q.clone()])[0].text, "q");
        assert_eq!(reduce(vec![q, p])[0].text, "q");
    }

    #[test]
    fn a_merge_replaces_its_sources() {
        let mut merged = row("m", "one row for both", 50);
        merged.replaces = vec!["a".into(), "b".into()];
        let out = reduce(vec![row("a", "a", 10), row("b", "b", 20), merged]);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].id, "m");
    }

    #[test]
    fn scope_anchors_are_stable_per_key_and_distinct_per_scope_and_an_ai_scope_is_its_founding_key() {
        let k1 = [1u8; 32];
        let k2 = [2u8; 32];
        let a = scope_anchor("person", &k1).unwrap();
        assert_eq!(a, scope_anchor("person", &k1).unwrap());
        assert_ne!(a, scope_anchor("person", &k2).unwrap(), "another network, another anchor");
        assert_ne!(a, scope_anchor("project:abc", &k1).unwrap());
        let founding = hex::encode([0x84u8, 0x20, 0x24].iter().chain([7u8; 36].iter()).copied().collect::<Vec<u8>>());
        let ai = scope_anchor(&format!("ai:{founding}"), &k1).unwrap();
        assert_eq!(ai, AnyLinkableHash::from(AgentPubKey::from_raw_39(hex::decode(&founding).unwrap())));
        assert!(scope_anchor("ai:zz", &k1).is_err());
    }
}
