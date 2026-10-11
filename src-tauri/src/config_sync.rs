//! AI configs across the person's devices (build-docs MULTI_DEVICE.md §3.4,
//! phase 4): each device's `manifest` backup already carries its
//! `ai-data.json`; the Vault copies every sibling's manifest here (1.6.0+).
//! A round reads each copy and merges the AIs by FOUNDING KEY - the newest
//! edit wins, whole config, by `updatedAt`; an AI this device does not have
//! yet is added and gets its own agent at the front end's next provisioning
//! pass. What is per device never travels: the model file an AI is pinned
//! to, this device's agent key, its starter slot.

use crate::flowsta::{self, http, VAULT_ORIGIN, YOAI_HOLOCHAIN_CLIENT_ID};
use serde::{Deserialize, Serialize};
use tauri::Emitter;

/// The editable, person-owned fields of an AI - what follows the identity.
const SYNCED_FIELDS: &[&str] = &[
    "name",
    "description",
    "baseArchetypeId",
    "systemPrompt",
    "emoji",
    "askBlurb",
    "status",
    "lengthDisposition",
    "defaultMode",
    "useEmojis",
    "skills",
    "mcp",
    "mcpOff",
    "skillsOff",
    "sampling",
    "archivedAt",
    "updatedAt",
];

fn founding_of(ai: &serde_json::Value) -> Option<String> {
    ai["foundingKey"]
        .as_str()
        .filter(|f| !f.is_empty())
        .or_else(|| ai["agentPubKey"].as_str().filter(|a| !a.is_empty()))
        .map(String::from)
}

fn updated_at(ai: &serde_json::Value) -> i64 {
    ai["updatedAt"].as_i64().unwrap_or(0)
}

/// Merge one sibling's AI list into ours. Returns the merged list, how many
/// were updated, how many added, and (sibling id → local id) for every AI
/// that was updated or added - the key the sibling's thumbnails are under.
pub(crate) fn merge_sibling_configs(
    local: Vec<serde_json::Value>,
    sibling: &[serde_json::Value],
) -> (Vec<serde_json::Value>, u32, u32, Vec<(String, String)>) {
    let mut merged = local;
    let (mut updated, mut added) = (0u32, 0u32);
    let mut touched: Vec<(String, String)> = Vec::new();
    for s in sibling {
        let Some(founding) = founding_of(s) else { continue };
        let s_id = s["id"].as_str().unwrap_or("").to_string();
        let pos = merged.iter().position(|l| {
            founding_of(l).as_deref() == Some(founding.as_str())
                || l["id"] == founding.as_str()
                || (!s_id.is_empty() && l["restoredFromId"] == s_id.as_str())
        });
        // A fresh device seeds its own three starters before it knows the
        // identity has them already: an untouched starter in the same slot
        // IS the identity's slot-N AI and folds into it (takes its founding
        // key and config; keeps this device's agent) instead of standing
        // beside it as a twin. An edited starter is this person's work and
        // stays.
        let fold = pos.is_none()
            && s["seedSlot"].as_u64().is_some()
            && merged.iter().position(|l| {
                l["seedSlot"] == s["seedSlot"]
                    && founding_of(l).as_deref() == l["agentPubKey"].as_str()
            }).map(|i| { merged[i]["foundingKey"] = serde_json::json!(founding); i }).is_some();
        let pos = if fold {
            merged.iter().position(|l| founding_of(l).as_deref() == Some(founding.as_str()))
        } else {
            pos
        };
        match pos {
            Some(i) => {
                // The newer edit wins the fields (and the picture); a fold
                // whose local side is newer takes only the key.
                let take_fields = updated_at(s) > updated_at(&merged[i]);
                if take_fields {
                    for f in SYNCED_FIELDS {
                        match s.get(*f) {
                            Some(v) => merged[i][*f] = v.clone(),
                            None => {
                                if let Some(obj) = merged[i].as_object_mut() {
                                    obj.remove(*f);
                                }
                            }
                        }
                    }
                    if let Some(local_id) = merged[i]["id"].as_str() {
                        touched.push((s_id.clone(), local_id.to_string()));
                    }
                }
                if take_fields || fold {
                    if merged[i]["foundingKey"].as_str().map(|f| f.is_empty()).unwrap_or(true) {
                        merged[i]["foundingKey"] = serde_json::json!(founding);
                    }
                    updated += 1;
                }
            }
            None => {
                // New here: the founding key stands in as the id until this
                // device provisions its own agent for it (the front end's
                // provisioning pass re-keys it, keeping foundingKey).
                let mut entry = serde_json::json!({ "id": founding, "foundingKey": founding, "model": "auto:offline" });
                for f in SYNCED_FIELDS {
                    if let Some(v) = s.get(*f) {
                        entry[*f] = v.clone();
                    }
                }
                if !s_id.is_empty() {
                    entry["restoredFromId"] = serde_json::json!(s_id);
                }
                if entry["status"].is_null() {
                    entry["status"] = serde_json::json!("active");
                }
                merged.push(entry);
                added += 1;
                touched.push((s_id.clone(), founding.clone()));
            }
        }
    }
    (merged, updated, added, touched)
}

/// One sibling's manifest copy (Vault 1.6.1 reads the named device's; a
/// 1.6.0 Vault answers with the newest copy across devices instead).
async fn sibling_manifest(port: u16, device: &str) -> Result<Option<serde_json::Value>, String> {
    let resp = http()
        .post(format!("http://127.0.0.1:{}/backup/retrieve", port))
        .header("Origin", VAULT_ORIGIN)
        .json(&serde_json::json!({
            "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
            "label": crate::vault_escrow::MANIFEST_LABEL,
            "across": "devices",
            "device": device,
        }))
        .send()
        .await
        .map_err(|e| format!("vault unreachable: {}", e))?;
    match resp.status() {
        reqwest::StatusCode::NOT_FOUND => return Ok(None),
        s if s.is_success() => {}
        _ => {
            let body: serde_json::Value = resp.json().await.unwrap_or_default();
            return Err(body["error"].as_str().unwrap_or("retrieve_failed").to_string());
        }
    }
    let v: serde_json::Value = resp.json().await.map_err(|e| format!("bad retrieve response: {}", e))?;
    // Only a copy from another device counts; our own slot is not a sibling.
    if v["from_device"].is_null() {
        return Ok(None);
    }
    Ok(Some(v["data"].clone()))
}

fn ai_list(v: &serde_json::Value) -> Vec<serde_json::Value> {
    match &v["ai_configs"]["data"]["custom-ais"] {
        serde_json::Value::Array(a) => a.clone(),
        serde_json::Value::String(raw) => serde_json::from_str(raw).unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// What the last round found - the Settings line reads this.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ConfigSyncState {
    /// ms epoch of the last completed round.
    pub checked_at: u64,
    /// The other devices holding copies here, as the Vault names them.
    pub devices: Vec<SyncedDevice>,
    /// This round's changes.
    pub updated: u32,
    pub added: u32,
    /// Every round's changes on this profile, added up.
    #[serde(default)]
    pub total_updated: u32,
    #[serde(default)]
    pub total_added: u32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SyncedDevice {
    pub id: String,
    pub name: Option<String>,
    /// The device runs another Holochain generation: its configs still
    /// merge (they are JSON), but its conversations cannot reach this
    /// device until one of them updates - the Vault's own rule and words.
    pub needs_update: bool,
}

const STATE_FILE: &str = "config-sync.json";

fn state_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    crate::profile::root(app).ok().map(|r| r.join(STATE_FILE))
}

fn save_state(app: &tauri::AppHandle, state: &ConfigSyncState) {
    let mut state = state.clone();
    if let Some(prev) = config_sync_status(app.clone()) {
        state.total_updated = prev.total_updated + state.updated;
        state.total_added = prev.total_added + state.added;
    } else {
        state.total_updated = state.updated;
        state.total_added = state.added;
    }
    if let Some(p) = state_path(app) {
        if let Ok(json) = serde_json::to_vec(&state) {
            let _ = std::fs::write(p, json);
        }
    }
}

/// The last round's result, for Settings → Flowsta Identity. `None` until a
/// round has completed on this profile.
#[tauri::command]
pub fn config_sync_status(app: tauri::AppHandle) -> Option<ConfigSyncState> {
    let p = state_path(&app)?;
    serde_json::from_slice(&std::fs::read(p).ok()?).ok()
}

fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Whether a sibling's manifest comes from another network generation. A
/// manifest without the field is from a build before it: same generation.
fn sibling_needs_update(manifest: Option<&serde_json::Value>, device: &crate::vault_escrow::SiblingDevice) -> bool {
    if let Some(g) = manifest.and_then(|m| m["network_generation"].as_str()) {
        return g != crate::conductor::NETWORK_GENERATION;
    }
    matches!(device.generation.as_deref(), Some(g) if g != crate::conductor::NETWORK_GENERATION)
}

/// One round: every sibling's manifest copy, merged into this device's AI
/// list. Returns (updated, added). Silent when not signed in or no Vault.
pub(crate) async fn sync_from_siblings(app: &tauri::AppHandle) -> Result<(u32, u32), String> {
    use tauri_plugin_store::StoreExt;
    let port = crate::vault_escrow::escrow_port(app, true).await.map_err(|s| s.error.unwrap_or(s.state))?;
    let devices = crate::vault_escrow::sibling_devices(port).await?;
    if devices.is_empty() {
        save_state(app, &ConfigSyncState { checked_at: now_ms(), ..Default::default() });
        return Ok((0, 0));
    }
    let mut synced: Vec<SyncedDevice> = Vec::new();
    let store = app
        .store(crate::profile::store_path(app, "ai-data.json"))
        .map_err(|e| e.to_string())?;
    let mut local: Vec<serde_json::Value> = match store.get("custom-ais") {
        Some(serde_json::Value::Array(a)) => a,
        Some(serde_json::Value::String(raw)) => serde_json::from_str(&raw).unwrap_or_default(),
        _ => Vec::new(),
    };
    let (mut total_updated, mut total_added) = (0u32, 0u32);
    let thumb_dir = crate::profile::root(app).map_err(|e| e.to_string())?.join("thumbnails");
    for device in &devices {
        let manifest = match sibling_manifest(port, &device.id).await {
            Ok(m) => m,
            Err(e) => {
                log::warn!("[config-sync] manifest from {}: {}", &device.id[..device.id.len().min(8)], e);
                None
            }
        };
        synced.push(SyncedDevice {
            id: device.id.clone(),
            name: device.name.clone(),
            needs_update: sibling_needs_update(manifest.as_ref(), device),
        });
        let Some(manifest) = manifest else { continue };
        let sibling = ai_list(&manifest);
        if sibling.is_empty() {
            continue;
        }
        let (merged, updated, added, touched) = merge_sibling_configs(local, &sibling);
        local = merged;
        total_updated += updated;
        total_added += added;
        // The sibling's thumbnails for every AI that just took its config.
        if let Some(thumbs) = manifest["thumbnails"]["data"].as_object() {
            let _ = std::fs::create_dir_all(&thumb_dir);
            for (sibling_id, local_id) in &touched {
                let Some(b64) = thumbs.get(sibling_id).and_then(|v| v.as_str()) else { continue };
                use base64::Engine;
                if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
                    let _ = std::fs::write(thumb_dir.join(format!("{}.jpg", local_id)), bytes);
                }
            }
        }
    }
    save_state(app, &ConfigSyncState { checked_at: now_ms(), devices: synced, updated: total_updated, added: total_added, ..Default::default() });
    if total_updated + total_added > 0 {
        store.set("custom-ais", serde_json::json!(local));
        store.save().map_err(|e| e.to_string())?;
        log::info!("[config-sync] AIs from the other devices: {} updated, {} added", total_updated, total_added);
        let _ = app.emit("ai-configs-changed", serde_json::json!({ "updated": total_updated, "added": total_added }));
    }
    Ok((total_updated, total_added))
}

/// One round now (Settings, tests). The same answer the background round
/// records.
#[tauri::command]
pub async fn config_sync_now(app: tauri::AppHandle) -> Result<ConfigSyncState, String> {
    sync_from_siblings(&app).await?;
    Ok(config_sync_status(app).unwrap_or_default())
}

/// Rounds like the Vault's: the first shortly after start, then every five
/// minutes while the app is open.
pub fn start_config_sync_rounds(app: &tauri::AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let mut wait = std::time::Duration::from_secs(40);
        loop {
            tokio::time::sleep(wait).await;
            wait = std::time::Duration::from_secs(300);
            if flowsta::find_vault().await.port.is_none() {
                continue;
            }
            match sync_from_siblings(&app).await {
                Ok((0, 0)) => {}
                Ok((u, a)) => log::info!("[config-sync] round: {} updated, {} added", u, a),
                Err(e) if e == "unlinked" || e == "vault_unavailable" || e == "vault_locked" || e == "identity_switched" => {}
                Err(e) => log::warn!("[config-sync] round failed: {}", e),
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{merge_sibling_configs, sibling_needs_update};
    use crate::vault_escrow::SiblingDevice;

    #[test]
    fn another_generation_needs_the_update_and_a_manifest_without_the_field_does_not() {
        let dev = |g: Option<&str>| SiblingDevice { id: "d".into(), generation: g.map(String::from), ..Default::default() };
        let same = serde_json::json!({ "network_generation": crate::conductor::NETWORK_GENERATION });
        let other = serde_json::json!({ "network_generation": "holochain-0.7" });
        let older = serde_json::json!({ "version": 1 });
        assert!(!sibling_needs_update(Some(&same), &dev(None)));
        assert!(sibling_needs_update(Some(&other), &dev(None)));
        assert!(!sibling_needs_update(Some(&older), &dev(None)), "a build before the field is this generation");
        assert!(sibling_needs_update(None, &dev(Some("holochain-0.7"))), "no manifest yet: the registry's word");
        assert!(!sibling_needs_update(None, &dev(None)));
    }

    fn ai(id: &str, founding: &str, name: &str, updated: i64) -> serde_json::Value {
        serde_json::json!({ "id": id, "agentPubKey": id, "foundingKey": founding, "name": name, "model": "LFM.gguf", "seedSlot": 1, "updatedAt": updated, "status": "active" })
    }

    #[test]
    fn the_newest_edit_wins_by_founding_key_and_never_the_model_or_the_keys() {
        let local = vec![ai("dev-b", "found-1", "Teresa", 10), ai("dev-b2", "found-2", "Max", 50)];
        let sibling = vec![
            { let mut s = ai("dev-a", "found-1", "Tessa", 20); s["model"] = serde_json::json!("other.gguf"); s },
            ai("dev-a2", "found-2", "Maximilian", 40),
        ];
        let (merged, updated, added, touched) = merge_sibling_configs(local, &sibling);
        assert_eq!((updated, added), (1, 0));
        let t = &merged[0];
        assert_eq!(t["name"], "Tessa", "the sibling's newer edit wins");
        assert_eq!(t["id"], "dev-b", "this device's agent stays");
        assert_eq!(t["agentPubKey"], "dev-b");
        assert_eq!(t["model"], "LFM.gguf", "the model file is per device");
        assert_eq!(t["seedSlot"], 1);
        assert_eq!(merged[1]["name"], "Max", "an older sibling edit loses");
        assert_eq!(touched, vec![("dev-a".to_string(), "dev-b".to_string())]);
    }

    #[test]
    fn an_ai_made_elsewhere_arrives_without_an_agent_and_a_seeded_starter_takes_an_edit() {
        let mut seeded = ai("dev-b", "found-1", "Veebo", 0);
        seeded.as_object_mut().unwrap().remove("updatedAt");
        let local = vec![seeded];
        let sibling = vec![ai("dev-a", "found-1", "Vee", 5), ai("dev-a3", "found-3", "Luna", 7)];
        let (merged, updated, added, _) = merge_sibling_configs(local, &sibling);
        assert_eq!((updated, added), (1, 1));
        assert_eq!(merged[0]["name"], "Vee", "a never-edited starter takes any edit");
        let luna = &merged[1];
        assert_eq!(luna["id"], "found-3", "the founding key stands in as the id until provisioned");
        assert!(luna["agentPubKey"].is_null(), "no agent yet on this device");
        assert_eq!(luna["model"], "auto:offline");
        assert_eq!(luna["restoredFromId"], "dev-a3");
        // Again: nothing changes.
        let (again, u2, a2, _) = merge_sibling_configs(merged.clone(), &sibling);
        assert_eq!((u2, a2), (0, 0));
        assert_eq!(again.len(), 2);
    }

    #[test]
    fn a_fresh_devices_untouched_starter_folds_into_the_identitys_ai_in_that_slot() {
        // B seeded its own Teresa (slot 2, never edited); A's Teresa was
        // renamed on A. B's starter takes A's founding key and name, keeps
        // its own agent; an edited starter on B stays beside the sibling's.
        let mut b_teresa = ai("b-agent-2", "b-agent-2", "Teresa", 0);
        b_teresa.as_object_mut().unwrap().remove("updatedAt");
        b_teresa["seedSlot"] = serde_json::json!(2);
        let mut b_veebo = ai("b-agent-1", "b-agent-1", "Veebo the Great", 9);
        b_veebo["seedSlot"] = serde_json::json!(1);
        let mut a_teresa = ai("a-agent-2", "a-agent-2", "Teresa Two", 30);
        a_teresa["seedSlot"] = serde_json::json!(2);
        let mut a_veebo = ai("a-agent-1", "a-agent-1", "Veebo", 0);
        a_veebo.as_object_mut().unwrap().remove("updatedAt");
        a_veebo["seedSlot"] = serde_json::json!(1);
        let (merged, updated, added, touched) = merge_sibling_configs(vec![b_veebo, b_teresa], &[a_veebo, a_teresa]);
        assert_eq!((updated, added), (2, 0), "both slots fold, nothing is added");
        assert_eq!(merged.len(), 2);
        let t = merged.iter().find(|l| l["seedSlot"] == 2).unwrap();
        assert_eq!(t["name"], "Teresa Two");
        assert_eq!(t["foundingKey"], "a-agent-2", "the identity's founding key");
        assert_eq!(t["agentPubKey"], "b-agent-2", "this device's agent stays");
        let v = merged.iter().find(|l| l["seedSlot"] == 1).unwrap();
        assert_eq!(v["foundingKey"], "a-agent-1", "an edited starter still folds its KEY (one AI per slot)");
        assert_eq!(v["name"], "Veebo the Great", "but keeps its newer edit");
        assert_eq!(touched.len(), 1, "only the slot that took A's fields takes A's picture");
    }
}
