//! Replay-restore: bring the conversations held in the Vault "latest"
//! backup back onto this device.
//!
//! The full-data backup (vault_escrow) makes conversations recoverABLE;
//! this module makes them recoverED. After a device loss the user restores
//! the transcript key via the escrow conflict panel (which wipes the fresh
//! conductor and relaunches), then runs this restore, which:
//!
//! 1. Fetches the "latest" backup from Vault and verifies its app_keys
//!    match the local recovery material (raw cipher bytes only decrypt
//!    under the same key - a mismatch means "restore the key first").
//! 2. Merges the backup's ai_configs into the local ai-data.json store:
//!    missing AIs are added, and a still-unused fresh default (Veebo /
//!    Teresa / Reeves) is replaced by its backed-up counterpart. Restored
//!    entries carry `restoredFromId` so a re-run finds them again after
//!    the id has been re-keyed to a new agent.
//! 3. Provisions an agent per restored AI and re-keys the store entry to
//!    the new agent pub key - the same adoption the frontend performs.
//! 4. Replays the records through the normal zome functions. Conversations
//!    and messages re-commit their raw encrypted bytes verbatim (plaintext
//!    untouched, so started_at / sequence survive). New commits mint new
//!    action hashes, so grounding annotations - whose plaintext points at
//!    a message action hash - are decrypted, remapped old→new, and
//!    re-encrypted. Conversations whose `started_at` already exists
//!    locally are skipped, making the restore safe to re-run.
//! 5. Cells that match no AI (an AI deleted while its transcripts were
//!    kept) are preserved transcript-only under a headless agent - on the
//!    conductor and in future backups, but with no AI recreated in the
//!    app - so a device loss can never strand records the user kept.

use crate::commands_holochain::{EncryptedEntryRaw, HolochainState};
use crate::flowsta::{http, VAULT_ORIGIN, YOAI_HOLOCHAIN_CLIENT_ID};
use crate::holochain::HolochainManager;
use crate::vault_escrow;
use base64::Engine;
use holochain_types::prelude::{ActionHash, ExternIO};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_store::StoreExt;

const AI_STORE: &str = "ai-data.json";
const CUSTOM_AIS_KEY: &str = "custom-ais";
/// Archetypes of the AIs that ship with the app - a fresh, unused copy of
/// one of these may be replaced by its backed-up counterpart on restore.
const DEFAULT_ARCHETYPES: [&str; 3] = ["veebo", "teresa", "reeves"];

/// What the zome expects for start_conversation.
#[derive(Serialize, Debug)]
struct EncryptedInput {
    cipher: Vec<u8>,
    nonce: Vec<u8>,
}

/// What the zome expects for record_message.
#[derive(Serialize, Debug)]
struct RecordMessageInput {
    conversation_hash: ActionHash,
    cipher: Vec<u8>,
    nonce: Vec<u8>,
}

/// One backed-up conversation with its entries, ready to replay.
struct ConvGroup {
    old_hash: String,
    started_at: i64,
    ai_personality_id: String,
    ai_personality_name: String,
    conversation: serde_json::Value,
    /// Messages + annotations in createdAtMs order.
    entries: Vec<serde_json::Value>,
}

/// One backup cell's conversations, plus the old installed_app_id suffix
/// (the ai_id the old device provisioned that agent under).
struct CellGroups {
    role_suffix: String,
    groups: Vec<ConvGroup>,
    /// Entries whose conversation record wasn't in the backup (truncated).
    orphan_entries: usize,
}

/// Group a backup cell's flat record list into conversations. Entries are
/// attached via the `conversation_action_hash` injected into each entry's
/// human_readable at backup time.
fn group_cell(role_name: &str, records: &[serde_json::Value]) -> CellGroups {
    let role_suffix = role_name
        .strip_prefix("transcript_v1_")
        .or_else(|| role_name.strip_prefix("transcript_"))
        .unwrap_or(role_name)
        .to_string();

    let mut groups: Vec<ConvGroup> = Vec::new();
    let mut orphan_entries = 0usize;
    for rec in records {
        let human = &rec["human_readable"];
        if rec["entryType"] == "Conversation" {
            groups.push(ConvGroup {
                old_hash: rec["actionHash"].as_str().unwrap_or("").to_string(),
                started_at: human["started_at"].as_i64().unwrap_or(0),
                ai_personality_id: human["ai_personality_id"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                ai_personality_name: human["ai_personality_name"]
                    .as_str()
                    .unwrap_or("")
                    .to_string(),
                conversation: rec.clone(),
                entries: Vec::new(),
            });
        } else {
            let conv_hash = human["conversation_action_hash"].as_str().unwrap_or("");
            match groups.iter_mut().find(|g| g.old_hash == conv_hash) {
                Some(g) => g.entries.push(rec.clone()),
                None => orphan_entries += 1,
            }
        }
    }
    for g in &mut groups {
        g.entries
            .sort_by_key(|e| e["createdAtMs"].as_i64().unwrap_or(0));
    }
    groups.sort_by_key(|g| g.started_at);
    CellGroups { role_suffix, groups, orphan_entries }
}

/// Decide which local agent each backup cell replays into.
///
/// `alias_to_agent` seeds every known identity of every local AI (current
/// id, agentPubKey, restoredFromId) → its current agent key. A cell matches
/// when any of its aliases (role suffix + each conversation's
/// ai_personality_id) hits a known alias; its aliases then join the pool so
/// older agent generations chain through (an old app id's suffix is the
/// previous generation's id). Cells still unmatched fall back to the AI
/// name. Returns (cell index → agent key, unmatched cell indices).
fn assign_cells(
    cells: &[CellGroups],
    alias_to_agent: &HashMap<String, String>,
    name_to_agent: &HashMap<String, String>,
) -> (HashMap<usize, String>, Vec<usize>) {
    let mut known = alias_to_agent.clone();
    let mut assigned: HashMap<usize, String> = HashMap::new();

    loop {
        let mut progressed = false;
        for (i, cell) in cells.iter().enumerate() {
            if assigned.contains_key(&i) {
                continue;
            }
            let mut aliases: Vec<String> = vec![cell.role_suffix.clone()];
            aliases.extend(cell.groups.iter().map(|g| g.ai_personality_id.clone()));
            if let Some(agent) = aliases.iter().find_map(|a| known.get(a)).cloned() {
                for a in aliases {
                    known.entry(a).or_insert_with(|| agent.clone());
                }
                assigned.insert(i, agent);
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }

    let mut unmatched = Vec::new();
    for (i, cell) in cells.iter().enumerate() {
        if assigned.contains_key(&i) {
            continue;
        }
        let by_name = cell.groups.iter().find_map(|g| {
            name_to_agent.get(&g.ai_personality_name.to_lowercase())
        });
        match by_name {
            Some(agent) => {
                assigned.insert(i, agent.clone());
            }
            None => unmatched.push(i),
        }
    }
    (assigned, unmatched)
}

/// Merge the backup's AI list into the local one.
///
/// Per backup AI: already present (by id or restoredFromId) → untouched;
/// else if it's a shipped default whose fresh local copy has no
/// conversations yet → replace that copy (returning its old local id so a
/// custom thumbnail can follow); else → append. Added/replaced entries get
/// `restoredFromId` so re-runs recognize them after re-keying.
/// Returns (merged list, ids needing an agent, replaced-local ids by new id,
/// added count, replaced count).
fn merge_ai_lists(
    local: Vec<serde_json::Value>,
    backup: &[serde_json::Value],
    conv_counts: &HashMap<String, u64>,
) -> (Vec<serde_json::Value>, Vec<String>, HashMap<String, String>, usize, usize) {
    let mut merged = local;
    let mut to_provision: Vec<String> = Vec::new();
    let mut replaced_local: HashMap<String, String> = HashMap::new();
    let (mut added, mut replaced) = (0usize, 0usize);

    for b in backup {
        let b_id = b["id"].as_str().unwrap_or("");
        if b_id.is_empty() {
            continue;
        }
        if merged.iter().any(|l| {
            l["id"] == b_id || l["restoredFromId"] == b_id
        }) {
            continue; // already on this device (possibly under a re-keyed id)
        }

        let mut entry = b.clone();
        entry["restoredFromId"] = serde_json::json!(b_id);

        let b_arch = b["baseArchetypeId"].as_str().unwrap_or("");
        let replace_idx = if DEFAULT_ARCHETYPES.contains(&b_arch) {
            merged.iter().position(|l| {
                l["baseArchetypeId"] == b_arch
                    && l["restoredFromId"].is_null()
                    && l["agentPubKey"]
                        .as_str()
                        .map(|k| conv_counts.get(k).copied().unwrap_or(0) == 0)
                        .unwrap_or(true)
            })
        } else {
            None
        };

        match replace_idx {
            Some(i) => {
                if let Some(old_id) = merged[i]["id"].as_str() {
                    replaced_local.insert(b_id.to_string(), old_id.to_string());
                }
                merged[i] = entry;
                replaced += 1;
            }
            None => {
                merged.push(entry);
                added += 1;
            }
        }
        to_provision.push(b_id.to_string());
    }
    (merged, to_provision, replaced_local, added, replaced)
}

/// Rebuild the ciphertext for one backup record. Conversations and messages
/// replay their raw on-chain bytes verbatim; when the raw bytes are absent
/// (e.g. a hand-trimmed export) the human_readable view is re-encrypted,
/// minus the linkage field injected at backup time.
fn record_cipher(
    key: &[u8; 32],
    rec: &serde_json::Value,
) -> Result<(Vec<u8>, Vec<u8>), String> {
    if let Some(b64) = rec["raw_record"]["entry_b64"].as_str() {
        let raw = base64::engine::general_purpose::STANDARD
            .decode(b64)
            .map_err(|e| format!("bad entry_b64: {}", e))?;
        let ee: EncryptedEntryRaw =
            rmp_serde::from_slice(&raw).map_err(|e| format!("bad raw entry: {}", e))?;
        return Ok((ee.cipher, ee.nonce));
    }
    let mut human = rec["human_readable"].clone();
    if let Some(obj) = human.as_object_mut() {
        obj.remove("conversation_action_hash");
    }
    let plain = serde_json::to_vec(&human).map_err(|e| e.to_string())?;
    let (nonce, cipher) = crate::transcript_crypto::encrypt(key, &plain)?;
    Ok((cipher, nonce.to_vec()))
}

/// A grounding annotation's plaintext, with its target message hash
/// remapped to the hash the message was re-committed under.
fn remap_annotation(
    rec: &serde_json::Value,
    hash_map: &HashMap<String, String>,
) -> serde_json::Value {
    let mut human = rec["human_readable"].clone();
    if let Some(obj) = human.as_object_mut() {
        obj.remove("conversation_action_hash");
        if let Some(target) = obj.get("target_hash").and_then(|v| v.as_str()) {
            if let Some(new_target) = hash_map.get(target) {
                obj.insert("target_hash".into(), serde_json::json!(new_target));
            }
        }
    }
    human
}

/// Fetch the "latest" full-data backup from Vault.
/// Retrieve one labeled backup; the full IPC response (data / data_base64).
async fn retrieve_label(port: u16, label: &str) -> Result<serde_json::Value, String> {
    let resp = http()
        .post(format!("http://127.0.0.1:{}/backup/retrieve", port))
        .header("Origin", VAULT_ORIGIN)
        .json(&serde_json::json!({
            "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
            "label": label,
        }))
        .send()
        .await
        .map_err(|e| format!("vault unreachable: {}", e))?;
    match resp.status() {
        reqwest::StatusCode::NOT_FOUND => return Err("no_backup".into()),
        s if s.is_success() => {}
        _ => {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            return Err(body["error"].as_str().unwrap_or("retrieve_failed").to_string());
        }
    }
    resp.json()
        .await
        .map_err(|e| format!("bad retrieve response: {}", e))
}

/// Decode one fetched conversation object into its records.
fn decode_conv_object(label: &str, obj: &serde_json::Value) -> Result<Vec<serde_json::Value>, String> {
    let b64 = obj["data_base64"]
        .as_str()
        .ok_or_else(|| format!("conversation object {} has no bytes", label))?;
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64)
        .map_err(|_| format!("conversation object {} is corrupted", label))?;
    let plain = vault_escrow::gunzip_bytes(&bytes)
        .map_err(|e| format!("conversation object {}: {}", label, e))?;
    let part: serde_json::Value = serde_json::from_slice(&plain)
        .map_err(|e| format!("conversation object {}: {}", label, e))?;
    Ok(part["records"].as_array().cloned().unwrap_or_default())
}

/// The full backup for replay. Incremental manifests are reassembled into
/// the canonical monolith shape (cells[].records[]) by fetching each
/// conversation's gzipped object(s), so everything downstream - cell
/// assignment, replay, dedup - is untouched. Legacy "latest" monoliths
/// pass through as-is.
///
/// A PERMANENTLY unrecoverable object (absent from the Vault, or corrupted
/// past decoding) never aborts the walk: everything else is still
/// recoverable, and aborting used to turn one missing object into a 0%
/// restore reported as "no backup at all". Such objects are collected into
/// `_missing_objects` for the caller to surface. TRANSIENT failures (Vault
/// unreachable mid-walk) still abort - retrying later loses nothing.
async fn fetch_data_backup(port: u16) -> Result<serde_json::Value, String> {
    match retrieve_label(port, vault_escrow::MANIFEST_LABEL).await {
        Ok(v) => {
            let mut manifest = v["data"].clone();
            let index = manifest["conversations"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            // role_name -> records, in manifest (chronological) order.
            let mut cells: Vec<(String, Vec<serde_json::Value>)> = Vec::new();
            let mut missing: Vec<serde_json::Value> = Vec::new();
            for entry in &index {
                let role = entry["role_name"].as_str().unwrap_or("").to_string();
                let labels: Vec<String> = entry["labels"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|l| l.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default();
                for label in &labels {
                    let records = match retrieve_label(port, label).await {
                        Ok(obj) => match decode_conv_object(label, &obj) {
                            Ok(r) => r,
                            Err(e) => {
                                // Corrupted past decoding - permanent; keep walking.
                                log::warn!("[restore] {} - skipping this object", e);
                                missing.push(serde_json::json!({
                                    "label": label, "reason": "corrupted",
                                    "records": entry["records"].as_u64().unwrap_or(0),
                                }));
                                continue;
                            }
                        },
                        Err(e) if e == "no_backup" => {
                            // Absent from the Vault - permanent; keep walking.
                            log::warn!(
                                "[restore] conversation object {} is missing from the \
                                 Vault - skipping it and continuing",
                                label
                            );
                            missing.push(serde_json::json!({
                                "label": label, "reason": "missing",
                                "records": entry["records"].as_u64().unwrap_or(0),
                            }));
                            continue;
                        }
                        // Transient (unreachable / refused) - abort, retry later.
                        Err(e) => return Err(format!("conversation object {}: {}", label, e)),
                    };
                    match cells.iter_mut().find(|(r, _)| *r == role) {
                        Some((_, recs)) => recs.extend(records),
                        None => cells.push((role.clone(), records)),
                    }
                }
            }
            manifest["cells"] = serde_json::json!(cells
                .into_iter()
                .map(|(role_name, records)| serde_json::json!({
                    "role_name": role_name,
                    "records": records,
                }))
                .collect::<Vec<_>>());
            if !missing.is_empty() {
                manifest["_missing_objects"] = serde_json::json!(missing);
            }
            Ok(manifest)
        }
        Err(e) if e == "no_backup" => {
            let v = retrieve_label(port, vault_escrow::DATA_LABEL).await?;
            Ok(v["data"].clone())
        }
        Err(e) => Err(e),
    }
}

/// Everything already on this device, decrypted just enough to dedup:
/// the set of local conversation started_at values, and a per-agent
/// conversation count (drives the replace-a-fresh-default rule).
async fn local_conversation_index(
    manager: &HolochainManager,
    key: &[u8; 32],
) -> Result<(HashSet<i64>, HashMap<String, u64>), String> {
    let agent_keys: Vec<String> = {
        let agents = manager.agents.lock().await;
        agents.keys().cloned().collect()
    };
    let mut started = HashSet::new();
    let mut counts = HashMap::new();
    for agent in agent_keys {
        let payload = ExternIO::encode(()).map_err(|e| e.to_string())?;
        let result = manager
            .call_zome(&agent, "transcript", "get_all_conversations", payload)
            .await
            .map_err(|e| format!("get_all_conversations({}): {}", agent, e))?;
        let records: Vec<holochain_types::prelude::Record> =
            ExternIO::decode(&result).map_err(|e| e.to_string())?;
        counts.insert(agent, records.len() as u64);
        for rec in &records {
            if let Some((plain, _)) = vault_escrow::open_record(key, rec) {
                if let Some(s) = plain["started_at"].as_i64() {
                    started.insert(s);
                }
            }
        }
    }
    Ok((started, counts))
}

/// Replay one conversation into `agent_key`. Returns the number of records
/// committed.
async fn replay_group(
    manager: &HolochainManager,
    agent_key: &str,
    key: &[u8; 32],
    group: &ConvGroup,
) -> Result<u64, String> {
    let (cipher, nonce) = record_cipher(key, &group.conversation)?;
    let payload = ExternIO::encode(EncryptedInput { cipher, nonce })
        .map_err(|e| e.to_string())?;
    let result = manager
        .call_zome(agent_key, "transcript", "start_conversation", payload)
        .await?;
    let new_conv: ActionHash = ExternIO::decode(&result).map_err(|e| e.to_string())?;
    let mut committed = 1u64;

    // Old message action-hash hex → new, for annotation targets.
    let mut hash_map: HashMap<String, String> = HashMap::new();
    for entry in &group.entries {
        let (cipher, nonce) = if entry["entryType"] == "GroundingAnnotation" {
            let plain_val = remap_annotation(entry, &hash_map);
            let plain = serde_json::to_vec(&plain_val).map_err(|e| e.to_string())?;
            let (nonce, cipher) = crate::transcript_crypto::encrypt(key, &plain)?;
            (cipher, nonce.to_vec())
        } else {
            record_cipher(key, entry)?
        };
        let payload = ExternIO::encode(RecordMessageInput {
            conversation_hash: new_conv.clone(),
            cipher,
            nonce,
        })
        .map_err(|e| e.to_string())?;
        let result = manager
            .call_zome(agent_key, "transcript", "record_message", payload)
            .await?;
        let new_hash: ActionHash = ExternIO::decode(&result).map_err(|e| e.to_string())?;
        if let Some(old) = entry["actionHash"].as_str() {
            hash_map.insert(old.to_string(), hex::encode(new_hash.get_raw_39()));
        }
        committed += 1;
    }
    Ok(committed)
}

/// The device-local id a backed-up AI id maps to after the merge + re-key
/// (its own id if unchanged, or the id of the entry restored from it).
fn resolve_local_ai_id(merged: &[serde_json::Value], backup_id: &str) -> Option<String> {
    merged
        .iter()
        .find(|l| l["id"] == backup_id || l["restoredFromId"] == backup_id)
        .and_then(|l| l["id"].as_str().map(String::from))
}

/// Write the backup's Your Memory facts file - only when this device has
/// none (never clobber newer local facts), and only if it decrypts under
/// the local key.
fn restore_memory_facts(
    app: &tauri::AppHandle,
    key: &[u8; 32],
    backup: &serde_json::Value,
) -> bool {
    let Some(b64) = backup["memory_facts"]["raw_b64"].as_str() else { return false };
    let Ok(dir) = app.path().app_data_dir() else { return false };
    let path = dir.join(vault_escrow::FACTS_FILE);
    if path.exists() {
        return false;
    }
    let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) else {
        log::warn!("[restore] memory facts skipped: bad base64");
        return false;
    };
    if vault_escrow::facts_human_readable(key, &bytes).is_none() {
        log::warn!("[restore] memory facts skipped: backup copy does not decrypt under this key");
        return false;
    }
    match std::fs::write(&path, &bytes) {
        Ok(()) => {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
            }
            log::info!("[restore] Your Memory profile facts restored");
            true
        }
        Err(e) => {
            log::warn!("[restore] memory facts write failed: {}", e);
            false
        }
    }
}

/// Write the backup's thumbnails for AIs on this device, remapping each
/// backed-up AI id to its post-restore local id. Existing files win.
fn restore_thumbnails(
    app: &tauri::AppHandle,
    merged: &[serde_json::Value],
    backup: &serde_json::Value,
) -> u64 {
    let Some(map) = backup["thumbnails"]["data"].as_object() else { return 0 };
    let Ok(dir) = app.path().app_data_dir() else { return 0 };
    let thumb_dir = dir.join("thumbnails");
    let _ = std::fs::create_dir_all(&thumb_dir);
    let mut written = 0u64;
    for (backup_id, b64) in map {
        let Some(local_id) = resolve_local_ai_id(merged, backup_id) else { continue };
        let path = thumb_dir.join(format!("{}.jpg", local_id));
        if path.exists() {
            continue;
        }
        let Some(bytes) = b64
            .as_str()
            .and_then(|s| base64::engine::general_purpose::STANDARD.decode(s).ok())
        else { continue };
        if std::fs::write(&path, &bytes).is_ok() {
            written += 1;
        }
    }
    written
}

/// Append backed-up knowledge items not already present (by text) as
/// authored entries with EMPTY vectors - the frontend re-embed walker fills
/// them in once the embedding model is available. Returns how many were added.
fn merge_knowledge(
    entries: &mut Vec<crate::transcript_memory::TranscriptEmbedding>,
    items: &[serde_json::Value],
) -> u64 {
    let existing: HashSet<&str> = entries
        .iter()
        .filter(|e| e.kind == "authored")
        .map(|e| e.text.as_str())
        .collect();
    let mut fresh: Vec<crate::transcript_memory::TranscriptEmbedding> = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let Some(text) = item["text"].as_str().filter(|t| !t.is_empty()) else { continue };
        if existing.contains(text) || fresh.iter().any(|f| f.text == text) {
            continue;
        }
        let created_at = item["created_at"].as_i64().unwrap_or(0);
        fresh.push(crate::transcript_memory::TranscriptEmbedding {
            id: format!("restored-{}-{}", created_at, i),
            conversation_hash: "authored".into(),
            role: "authored".into(),
            text: text.to_string(),
            vector: Vec::new(),
            kind: "authored".into(),
            created_at,
            // Backup stores chunk text only (not the source grouping yet), so
            // restored document chunks retrieve but regroup as loose knowledge.
            source: None,
        });
    }
    let added = fresh.len() as u64;
    entries.extend(fresh);
    added
}

/// Restore each AI's authored knowledge (its Remembers tab) from the backup
/// into the transcript-embedding store, remapping backed-up AI ids to their
/// post-restore local ids. Existing entries win; only missing texts are added.
fn restore_ai_knowledge(
    app: &tauri::AppHandle,
    key: &[u8; 32],
    merged: &[serde_json::Value],
    backup: &serde_json::Value,
) -> u64 {
    let Some(map) = backup["ai_knowledge"]["data"].as_object() else { return 0 };
    let mut restored = 0u64;
    for (backup_id, arr) in map {
        let Some(local_id) = resolve_local_ai_id(merged, backup_id) else { continue };
        let Some(items) = arr.as_array() else { continue };
        let mut entries =
            crate::transcript_memory::read_for(app, key, &local_id).unwrap_or_default();
        let added = merge_knowledge(&mut entries, items);
        if added > 0 {
            match crate::transcript_memory::write_for(app, key, &local_id, &entries) {
                Ok(()) => restored += added,
                Err(e) => {
                    log::warn!("[restore] knowledge write failed for {}: {}", local_id, e)
                }
            }
        }
    }
    restored
}

/// Rename a thumbnail file from one AI id to another, if it exists.
fn move_thumbnail(app: &tauri::AppHandle, from_id: &str, to_id: &str) {
    let Ok(dir) = app.path().app_data_dir() else { return };
    let from = dir.join("thumbnails").join(format!("{}.jpg", from_id));
    let to = dir.join("thumbnails").join(format!("{}.jpg", to_id));
    if from.exists() && !to.exists() {
        if let Err(e) = std::fs::rename(&from, &to) {
            log::warn!("[restore] could not move thumbnail {} → {}: {}", from_id, to_id, e);
        }
    }
}

/// Whether a device restore is half-done: the key came back from Vault
/// but the conversations haven't been replayed yet. The frontend checks
/// this at startup to finish the job in the launch sequence.
#[tauri::command]
pub fn vault_restore_pending(app: tauri::AppHandle) -> bool {
    vault_escrow::restore_pending_path(&app)
        .map(|p| p.exists())
        .unwrap_or(false)
}

/// Restore conversations (and missing AI configs) from the Vault "latest"
/// backup onto this device. Idempotent: conversations already present are
/// skipped, and previously restored AIs are recognized by `restoredFromId`.
#[tauri::command]
pub async fn vault_restore_conversations(
    app: tauri::AppHandle,
) -> Result<serde_json::Value, String> {
    let port = match vault_escrow::escrow_port(&app, true).await {
        Ok(p) => p,
        Err(s) => return Err(s.error.unwrap_or(s.state)),
    };
    let backup = fetch_data_backup(port).await?;
    let missing_objects = backup["_missing_objects"]
        .as_array()
        .map(|a| a.len() as u64)
        .unwrap_or(0);
    let missing_records: u64 = backup["_missing_objects"]
        .as_array()
        .map(|a| a.iter().map(|m| m["records"].as_u64().unwrap_or(0)).sum())
        .unwrap_or(0);

    // The raw cipher bytes only decrypt under the key they were written
    // with - a different local key means "restore the key first" (the
    // escrow conflict panel), never a partial mixed-key restore.
    let local = vault_escrow::local_recovery(&app)?;
    let backup_keys = vault_escrow::parse_escrow(&backup).ok_or("backup_unreadable")?;
    if backup_keys != local {
        return Err("key_mismatch".into());
    }

    let hc = app
        .try_state::<Arc<HolochainState>>()
        .ok_or("holochain state missing")?;
    let manager = hc.get()?;
    let key = manager.data_key()?;

    let (existing_started, conv_counts) = local_conversation_index(manager, &key).await?;

    // 1. Merge the backup's AI configs into the local store.
    let store = app.store(AI_STORE).map_err(|e| e.to_string())?;
    let local_ais: Vec<serde_json::Value> = match store.get(CUSTOM_AIS_KEY) {
        Some(serde_json::Value::Array(arr)) => arr,
        _ => Vec::new(),
    };
    let backup_ais: Vec<serde_json::Value> =
        match &backup["ai_configs"]["data"][CUSTOM_AIS_KEY] {
            serde_json::Value::Array(arr) => arr.clone(),
            _ => Vec::new(),
        };
    let (mut merged, to_provision, replaced_local, ais_added, ais_replaced) =
        merge_ai_lists(local_ais, &backup_ais, &conv_counts);

    // 2. Provision an agent per restored AI and adopt its pub key as the
    // id - the same re-key the frontend does when it creates an AI.
    for backup_id in &to_provision {
        let new_key = manager.provision_agent(backup_id).await?;
        if let Some(entry) = merged.iter_mut().find(|l| l["restoredFromId"] == backup_id.as_str()) {
            if entry["id"] != new_key.as_str() {
                entry["id"] = serde_json::json!(new_key);
                entry["agentPubKey"] = serde_json::json!(new_key);
            }
            // A replaced fresh default's face carries over to the new id.
            if let Some(old_local_id) = replaced_local.get(backup_id) {
                move_thumbnail(&app, old_local_id, &new_key);
            }
        }
    }
    store.set(CUSTOM_AIS_KEY, serde_json::json!(merged.clone()));
    store.save().map_err(|e| e.to_string())?;

    // 3. Route each backup cell to a local agent.
    let cells: Vec<CellGroups> = backup["cells"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|c| {
                    group_cell(
                        c["role_name"].as_str().unwrap_or(""),
                        c["records"].as_array().map(|r| r.as_slice()).unwrap_or(&[]),
                    )
                })
                .collect()
        })
        .unwrap_or_default();

    let mut alias_to_agent: HashMap<String, String> = HashMap::new();
    let mut name_to_agent: HashMap<String, String> = HashMap::new();
    for ai in &merged {
        let Some(agent) = ai["agentPubKey"].as_str() else { continue };
        for alias_field in ["id", "agentPubKey", "restoredFromId"] {
            if let Some(alias) = ai[alias_field].as_str() {
                alias_to_agent.insert(alias.to_string(), agent.to_string());
            }
        }
        if let Some(name) = ai["name"].as_str() {
            name_to_agent.insert(name.to_lowercase(), agent.to_string());
        }
    }
    let (assigned, unmatched) = assign_cells(&cells, &alias_to_agent, &name_to_agent);

    // 4. Replay.
    let mut conversations_restored = 0u64;
    let mut records_restored = 0u64;
    let mut conversations_skipped = 0u64;
    let mut orphan_entries = 0u64;
    for (i, cell) in cells.iter().enumerate() {
        orphan_entries += cell.orphan_entries as u64;
        let Some(agent_key) = assigned.get(&i) else { continue };
        for group in &cell.groups {
            if existing_started.contains(&group.started_at) {
                conversations_skipped += 1;
                continue;
            }
            let n = replay_group(manager, agent_key, &key, group).await?;
            conversations_restored += 1;
            records_restored += n;
        }
    }

    // 5. Cells with no matching AI - e.g. an AI deleted while its
    // transcripts were kept - are preserved transcript-only: replayed
    // under a headless agent so they live on the conductor and keep
    // riding every future backup and export, exactly the state the old
    // device was in. The deleted AI is NOT recreated in the app. Without
    // this, the next overwrite of the "latest" backup - written from
    // this device's cells - would silently drop the very records the
    // user chose to keep.
    let mut conversations_preserved = 0u64;
    let mut records_preserved = 0u64;
    for &i in &unmatched {
        let cell = &cells[i];
        if cell.role_suffix.is_empty() || cell.groups.is_empty() {
            continue;
        }
        let agent_key = manager.provision_agent(&cell.role_suffix).await?;
        for group in &cell.groups {
            if existing_started.contains(&group.started_at) {
                conversations_skipped += 1;
                continue;
            }
            let n = replay_group(manager, &agent_key, &key, group).await?;
            conversations_preserved += 1;
            records_preserved += n;
        }
        log::info!(
            "[restore] cell {} has no matching AI - transcripts preserved without recreating the AI",
            cell.role_suffix
        );
    }

    log::info!(
        "[restore] {} conversation(s) restored ({} records), {} preserved transcript-only ({} records), {} already present, {} AI(s) added, {} default(s) replaced",
        conversations_restored, records_restored, conversations_preserved, records_preserved,
        conversations_skipped, ais_added, ais_replaced
    );

    // Your Memory profile facts + AI thumbnails + per-AI authored knowledge -
    // local files the backup carries alongside the Holochain records.
    let memory_facts_restored = restore_memory_facts(&app, &key, &backup);
    let thumbnails_restored = restore_thumbnails(&app, &merged, &backup);
    let knowledge_restored = restore_ai_knowledge(&app, &key, &merged, &backup);

    // The episodic recall index was wiped with the old key material, and the
    // restored knowledge above carries no vectors yet - leave a marker so the
    // frontend rebuilds the per-AI memory index once the embedding model is
    // available.
    if conversations_restored + conversations_preserved + conversations_skipped + knowledge_restored
        > 0
    {
        vault_escrow::set_reembed_pending(&app);
    }

    // The walk completed - this device now holds everything the backup
    // held, so automatic backups may write again.
    vault_escrow::clear_restore_pending(&app);

    // The replayed chain is content-identical to the backup, but refresh
    // the snapshot anyway so its cell layout reflects this device.
    if conversations_restored + conversations_preserved > 0 {
        vault_escrow::schedule_full_backup(&app);
    }

    Ok(serde_json::json!({
        "ais_added": ais_added,
        "ais_replaced": ais_replaced,
        "conversations_restored": conversations_restored,
        "records_restored": records_restored,
        "conversations_preserved": conversations_preserved,
        "records_preserved": records_preserved,
        "conversations_skipped": conversations_skipped,
        "orphan_entries": orphan_entries,
        "missing_objects": missing_objects,
        "missing_records": missing_records,
        "memory_facts_restored": memory_facts_restored,
        "thumbnails_restored": thumbnails_restored,
        "knowledge_restored": knowledge_restored,
    }))
}

/// Whether the per-AI memory index needs a rebuild (marker left by a
/// conversation restore). The frontend walker checks this at startup and
/// after an embedding-model download.
#[tauri::command]
pub fn memory_reembed_pending(app: tauri::AppHandle) -> bool {
    vault_escrow::reembed_pending_path(&app)
        .map(|p| p.exists())
        .unwrap_or(false)
}

/// The frontend walker finished rebuilding the memory index.
#[tauri::command]
pub fn memory_reembed_done(app: tauri::AppHandle) {
    if let Some(p) = vault_escrow::reembed_pending_path(&app) {
        let _ = std::fs::remove_file(p);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gz_object(records: serde_json::Value) -> serde_json::Value {
        use base64::Engine;
        let payload = serde_json::json!({
            "version": 2, "role_name": "ai-1", "started_at": 100, "records": records,
        });
        let gz = vault_escrow::gzip_bytes(&serde_json::to_vec(&payload).unwrap()).unwrap();
        serde_json::json!({
            "data_base64": base64::engine::general_purpose::STANDARD.encode(gz),
        })
    }

    #[test]
    fn decode_conv_object_roundtrips() {
        let obj = gz_object(serde_json::json!([{ "entryType": "Message" }, { "entryType": "Message" }]));
        let records = decode_conv_object("conv-x", &obj).unwrap();
        assert_eq!(records.len(), 2);
    }

    #[test]
    fn decode_conv_object_reports_each_damage_mode() {
        // Missing bytes entirely.
        let err = decode_conv_object("conv-a", &serde_json::json!({})).unwrap_err();
        assert!(err.contains("no bytes"), "{err}");
        // Corrupted base64.
        let err = decode_conv_object(
            "conv-b",
            &serde_json::json!({ "data_base64": "!!!not-base64!!!" }),
        )
        .unwrap_err();
        assert!(err.contains("corrupted"), "{err}");
        // Valid base64, not gzip.
        use base64::Engine;
        let err = decode_conv_object(
            "conv-c",
            &serde_json::json!({
                "data_base64": base64::engine::general_purpose::STANDARD.encode(b"plainbytes"),
            }),
        )
        .unwrap_err();
        assert!(err.contains("conv-c"), "{err}");
    }

    #[test]
    fn decode_conv_object_empty_records_is_ok_not_error() {
        // A conversation object with zero records decodes to an empty list -
        // damage accounting must not confuse it with a decode failure.
        let obj = gz_object(serde_json::json!([]));
        assert_eq!(decode_conv_object("conv-e", &obj).unwrap().len(), 0);
    }

    fn conv_rec(hash: &str, started_at: i64, ai_id: &str, name: &str) -> serde_json::Value {
        serde_json::json!({
            "entryType": "Conversation",
            "actionHash": hash,
            "createdAtMs": started_at / 1000,
            "human_readable": {
                "started_at": started_at,
                "ai_personality_id": ai_id,
                "ai_personality_name": name,
            },
        })
    }

    fn msg_rec(hash: &str, conv: &str, ms: i64) -> serde_json::Value {
        serde_json::json!({
            "entryType": "Message",
            "actionHash": hash,
            "createdAtMs": ms,
            "human_readable": { "content": "hi", "conversation_action_hash": conv },
        })
    }

    fn ann_rec(hash: &str, conv: &str, target: &str, ms: i64) -> serde_json::Value {
        serde_json::json!({
            "entryType": "GroundingAnnotation",
            "actionHash": hash,
            "createdAtMs": ms,
            "human_readable": {
                "annotation": "grounding",
                "target_hash": target,
                "conversation_action_hash": conv,
            },
        })
    }

    #[test]
    fn grouping_attaches_entries_and_counts_orphans() {
        let records = vec![
            conv_rec("c1", 100, "K1", "Reeves"),
            msg_rec("m2", "c1", 20),
            msg_rec("m1", "c1", 10),
            conv_rec("c2", 200, "K1", "Reeves"),
            msg_rec("m3", "c2", 30),
            msg_rec("mX", "c-missing", 40),
        ];
        let cell = group_cell("transcript_v1_K1", &records);
        assert_eq!(cell.role_suffix, "K1");
        assert_eq!(cell.groups.len(), 2);
        assert_eq!(cell.orphan_entries, 1);
        // Entries sorted by createdAtMs within the group.
        let g1 = &cell.groups[0];
        assert_eq!(g1.old_hash, "c1");
        let order: Vec<&str> = g1.entries.iter().map(|e| e["actionHash"].as_str().unwrap()).collect();
        assert_eq!(order, vec!["m1", "m2"]);
    }

    #[test]
    fn cell_assignment_chains_generations_and_falls_back_to_name() {
        // Old device lineage: AI id K2; K2's cell is transcript_v1_K1
        // (provisioned under the previous generation's id), K1's cell is
        // transcript_v1_local-abc. A third cell matches only by name.
        let cells = vec![
            CellGroups {
                role_suffix: "K1".into(),
                groups: vec![ConvGroup {
                    old_hash: "c1".into(), started_at: 1,
                    ai_personality_id: "K2".into(), ai_personality_name: "Reeves".into(),
                    conversation: serde_json::json!({}), entries: vec![],
                }],
                orphan_entries: 0,
            },
            CellGroups {
                role_suffix: "local-abc".into(),
                groups: vec![ConvGroup {
                    old_hash: "c2".into(), started_at: 2,
                    ai_personality_id: "K1".into(), ai_personality_name: "Reeves".into(),
                    conversation: serde_json::json!({}), entries: vec![],
                }],
                orphan_entries: 0,
            },
            CellGroups {
                role_suffix: "stranger".into(),
                groups: vec![ConvGroup {
                    old_hash: "c3".into(), started_at: 3,
                    ai_personality_id: "K9".into(), ai_personality_name: "Teresa".into(),
                    conversation: serde_json::json!({}), entries: vec![],
                }],
                orphan_entries: 0,
            },
        ];
        let aliases = HashMap::from([("K2".to_string(), "NEW".to_string())]);
        let names = HashMap::from([("teresa".to_string(), "TNEW".to_string())]);
        let (assigned, unmatched) = assign_cells(&cells, &aliases, &names);
        assert_eq!(assigned[&0], "NEW"); // via ai_personality_id K2
        assert_eq!(assigned[&1], "NEW"); // chained via cell 0's suffix K1
        assert_eq!(assigned[&2], "TNEW"); // name fallback
        assert!(unmatched.is_empty());
    }

    #[test]
    fn merge_replaces_fresh_default_adds_custom_skips_present() {
        let local = vec![
            serde_json::json!({ "id": "local-1", "baseArchetypeId": "reeves",
                                "agentPubKey": "A1", "name": "Reeves" }),
            serde_json::json!({ "id": "N9", "restoredFromId": "K9",
                                "baseArchetypeId": "veebo", "agentPubKey": "A2" }),
        ];
        let backup = vec![
            serde_json::json!({ "id": "K2", "baseArchetypeId": "reeves", "name": "Reeves" }),
            serde_json::json!({ "id": "K5", "baseArchetypeId": "custom-x", "name": "Muse" }),
            serde_json::json!({ "id": "K9", "baseArchetypeId": "veebo", "name": "Veebo" }),
        ];
        let counts = HashMap::from([("A1".to_string(), 0u64)]);
        let (merged, to_provision, replaced_local, added, replaced) =
            merge_ai_lists(local, &backup, &counts);
        assert_eq!(replaced, 1); // fresh Reeves replaced by backed-up one
        assert_eq!(added, 1); // Muse appended
        assert_eq!(to_provision, vec!["K2".to_string(), "K5".to_string()]); // K9 already restored
        assert_eq!(replaced_local.get("K2").map(String::as_str), Some("local-1"));
        assert_eq!(merged.len(), 3);
        assert!(merged.iter().any(|a| a["restoredFromId"] == "K2"));
    }

    #[test]
    fn merge_keeps_default_with_conversations() {
        let local = vec![serde_json::json!({
            "id": "local-1", "baseArchetypeId": "reeves", "agentPubKey": "A1"
        })];
        let backup = vec![serde_json::json!({
            "id": "K2", "baseArchetypeId": "reeves", "name": "Reeves"
        })];
        let counts = HashMap::from([("A1".to_string(), 3u64)]);
        let (merged, _, _, added, replaced) = merge_ai_lists(local, &backup, &counts);
        assert_eq!(replaced, 0);
        assert_eq!(added, 1); // appended alongside, nothing overwritten
        assert_eq!(merged.len(), 2);
    }

    #[test]
    fn knowledge_merge_dedups_and_leaves_vectors_empty() {
        let mut entries = vec![crate::transcript_memory::TranscriptEmbedding {
            id: "k1".into(),
            conversation_hash: "authored".into(),
            role: "authored".into(),
            text: "The dragon's name is Ember".into(),
            vector: vec![0.1, 0.2],
            kind: "authored".into(),
            created_at: 10,
            source: None,
        }];
        let items = vec![
            serde_json::json!({ "text": "The dragon's name is Ember", "created_at": 10 }),
            serde_json::json!({ "text": "Ember guards the north gate", "created_at": 20 }),
            serde_json::json!({ "text": "Ember guards the north gate", "created_at": 20 }),
            serde_json::json!({ "text": "", "created_at": 30 }),
        ];
        let added = merge_knowledge(&mut entries, &items);
        assert_eq!(added, 1); // present + duplicate + empty all skipped
        assert_eq!(entries.len(), 2);
        let new = &entries[1];
        assert_eq!(new.text, "Ember guards the north gate");
        assert!(new.vector.is_empty()); // walker fills this in later
        assert_eq!(new.kind, "authored");
        assert_eq!(new.created_at, 20);
        // Existing entry untouched (its vector survives).
        assert_eq!(entries[0].vector.len(), 2);
    }

    #[test]
    fn thumbnail_ids_follow_the_rekey() {
        let merged = vec![
            serde_json::json!({ "id": "NEW1", "restoredFromId": "K2" }),
            serde_json::json!({ "id": "K5" }),
        ];
        assert_eq!(resolve_local_ai_id(&merged, "K2").as_deref(), Some("NEW1"));
        assert_eq!(resolve_local_ai_id(&merged, "K5").as_deref(), Some("K5"));
        assert!(resolve_local_ai_id(&merged, "gone").is_none());
    }

    #[test]
    fn annotation_target_remap_and_linkage_strip() {
        let rec = ann_rec("a1", "c1", "old-msg", 50);
        let map = HashMap::from([("old-msg".to_string(), "new-msg".to_string())]);
        let out = remap_annotation(&rec, &map);
        assert_eq!(out["target_hash"], "new-msg");
        assert!(out.get("conversation_action_hash").is_none());
        // Unmapped target stays as-is rather than corrupting the record.
        let out2 = remap_annotation(&ann_rec("a2", "c1", "gone", 60), &HashMap::new());
        assert_eq!(out2["target_hash"], "gone");
    }

    #[test]
    fn record_cipher_falls_back_to_reencrypting_human_readable() {
        let key = [7u8; 32];
        let rec = msg_rec("m1", "c1", 10); // no raw_record
        let (cipher, nonce) = record_cipher(&key, &rec).unwrap();
        let plain = crate::transcript_crypto::decrypt(&key, &nonce, &cipher).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&plain).unwrap();
        assert_eq!(v["content"], "hi");
        assert!(v.get("conversation_action_hash").is_none()); // linkage stripped
    }
}
