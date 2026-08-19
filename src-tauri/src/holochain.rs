//! Multi-agent Holochain manager for Your Own AI.
//!
//! Each AI personality gets its own Holochain agent with its own keypair
//! and source chain. This module manages the lifecycle of multiple agents
//! within a single conductor instance.

use crate::conductor::{ConductorHandle, StartupResult};
use crate::transcript_crypto::RecoveryMaterial;
use crate::dna;
use holochain_client::AppWebsocket;
use holochain_types::prelude::AgentPubKey;
use lair_keystore_api::prelude::*;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// Represents a provisioned AI agent with its Holochain connection.
pub struct AiAgent {
    pub installed_app_id: String,
    pub agent_pub_key: AgentPubKey,
    pub app_client: AppWebsocket,
}

/// Manages multiple AI agents within a single Holochain conductor.
pub struct HolochainManager {
    pub lair_client: LairClient,
    pub handle: ConductorHandle,
    pub resource_dir: PathBuf,
    /// Map from agent pub key hex to its Holochain agent.
    pub agents: Mutex<HashMap<String, AiAgent>>,
    /// Serializes provisioning end-to-end. The frontend can invoke
    /// provisioning concurrently (eager startup + per-page paths); without
    /// this, two racers pass the already-provisioned check and collide on
    /// the same source chain ("head has moved") during the credential grant.
    provision_lock: Mutex<()>,
    /// One zome call at a time per agent chain. An agent turn's final
    /// answer write races the provider-side record of the same turn's last
    /// model call (same chain, milliseconds apart) - the loser gets
    /// "source chain head has moved" and its write is REJECTED, which
    /// surfaced as answers missing from resumed conversations.
    chain_locks: Mutex<HashMap<String, Arc<Mutex<()>>>>,
    /// User data key + per-user network seed (Phase A).
    pub recovery: RecoveryMaterial,
}

impl HolochainManager {
    /// Create a new manager from a startup result.
    pub fn new(startup: StartupResult, resource_dir: PathBuf, recovery: RecoveryMaterial) -> Self {
        Self {
            lair_client: startup.lair_client,
            handle: startup.handle,
            resource_dir,
            agents: Mutex::new(HashMap::new()),
            provision_lock: Mutex::new(()),
            chain_locks: Mutex::new(HashMap::new()),
            recovery,
        }
    }

    /// The user's transcript data key (Phase A encryption).
    pub fn data_key(&self) -> Result<[u8; 32], String> {
        self.recovery.data_key()
    }

    /// Provision a new Holochain agent for an AI personality.
    ///
    /// Creates a new lair seed (random keypair), installs the transcript
    /// hApp with that agent key, and connects an AppWebsocket.
    /// Idempotent — returns the agent pub key hex string.
    ///
    /// `ai_id` is only used as a stable seed tag in lair. The returned
    /// agent pub key hex is the canonical identifier for all subsequent
    /// Holochain operations.
    pub async fn provision_agent(&self, ai_id: &str) -> Result<String, String> {
        // Hold for the entire provision — makes concurrent calls for the
        // same AI idempotent instead of racing the source chain.
        let _provision_guard = self.provision_lock.lock().await;

        // Check if already provisioned (by scanning for matching lair tag).
        // We can't look up by ai_id anymore since the map is keyed by pub key hex.
        {
            let agents = self.agents.lock().await;

            // The frontend re-keys AIs to their agent pub key after first
            // provisioning — so the "ai_id" we receive is often an existing
            // agent's pub key hex. Resolve to that agent instead of minting
            // a new generation (the historic bug that stranded transcripts
            // on older agents run after run).
            if agents.contains_key(ai_id) {
                log::info!("AI id {} is an existing agent key, reusing", ai_id);
                return Ok(ai_id.to_string());
            }

            for (key_hex, _agent) in agents.iter() {
                // Check if this agent was created with the same lair tag
                // by looking at the installed_app_id suffix
                if _agent.installed_app_id.ends_with(ai_id) {
                    log::info!("Agent already provisioned for AI: {} (key: {})", ai_id, key_hex);
                    return Ok(key_hex.clone());
                }
            }
        }

        // The map above is incomplete whenever a reconnect lost the
        // CellDisabled race at startup, and silently minting a NEW
        // generation for an id the conductor already knows strands that
        // AI's records and grows the cell population every launch (the
        // 151-cells-for-12-AIs incident, 2026-08-19). So before minting,
        // ask the conductor itself.
        {
            let admin_ws = holochain_client::AdminWebsocket::connect(
                format!("localhost:{}", self.handle.admin_port),
                Some("your-own-ai".to_string()),
            )
            .await
            .map_err(|e| format!("Failed to connect to admin WebSocket: {}", e))?;
            let apps = admin_ws
                .list_apps(None)
                .await
                .map_err(|e| format!("Failed to list apps: {}", e))?;
            let known = apps.iter().find(|app| {
                app.installed_app_id.starts_with(dna::APP_ID_PREFIX)
                    && (hex::encode(app.agent_pub_key.get_raw_39()) == ai_id
                        || app.installed_app_id.ends_with(ai_id))
            });
            if let Some(app) = known {
                // The cell exists - adopt it. If its websocket can't
                // connect yet (enable wave still running), FAIL rather
                // than mint: the caller's retry ladder rides out
                // CellDisabled and calls again.
                let key_hex = hex::encode(app.agent_pub_key.get_raw_39());
                log::info!(
                    "AI id {} already installed on the conductor as {} (key: {}) - adopting, not minting",
                    ai_id,
                    app.installed_app_id,
                    key_hex
                );
                let app_client = dna::connect_app_websocket(
                    self.handle.admin_port,
                    self.handle.app_port,
                    &app.installed_app_id,
                )
                .await
                .map_err(|e| {
                    format!(
                        "AI {} exists on the conductor but isn't reachable yet: {}",
                        ai_id, e
                    )
                })?;
                let mut agents = self.agents.lock().await;
                agents.insert(
                    key_hex.clone(),
                    AiAgent {
                        installed_app_id: app.installed_app_id.clone(),
                        agent_pub_key: app.agent_pub_key.clone(),
                        app_client,
                    },
                );
                return Ok(key_hex);
            }
        }

        log::info!("Provisioning Holochain agent for AI: {}", ai_id);

        // 1. Get or create a lair seed for this AI personality.
        let tag: Arc<str> = format!("ai-{}", ai_id).into();
        let seed_info = match self
            .lair_client
            .new_seed(tag.clone(), None, false)
            .await
        {
            Ok(info) => {
                log::info!("Created new lair seed for AI: {}", ai_id);
                info
            }
            Err(e) => {
                let err_str = format!("{}", e);
                if err_str.contains("UNIQUE constraint") {
                    // Seed already exists from a previous session — look it up
                    log::info!("Lair seed already exists for AI: {}, reusing", ai_id);
                    let entries = self
                        .lair_client
                        .list_entries()
                        .await
                        .map_err(|e| format!("Failed to list lair entries: {}", e))?;

                    entries
                        .into_iter()
                        .find_map(|entry| {
                            if let lair_keystore_api::prelude::LairEntryInfo::Seed { tag: t, seed_info, .. } = entry {
                                if *t == *tag {
                                    return Some(seed_info);
                                }
                            }
                            None
                        })
                        .ok_or_else(|| format!("Seed exists but not found in lair entries for AI: {}", ai_id))?
                } else {
                    return Err(format!("Failed to create lair seed for AI {}: {}", ai_id, e));
                }
            }
        };

        let agent_pub_key = holochain_types::prelude::AgentPubKey::from_raw_32(
            seed_info.ed25519_pub_key.0.to_vec(),
        );

        log::info!(
            "Generated agent key for AI {}: {}",
            ai_id,
            hex::encode(&*seed_info.ed25519_pub_key.0)
        );

        // 2. Install the transcript hApp with this agent key.
        let app_id = dna::install_transcript_app(
            self.handle.admin_port,
            &self.resource_dir,
            agent_pub_key.clone(),
            ai_id,
            &self.recovery.network_seed,
        )
        .await?;

        // 3. Connect an AppWebsocket.
        let app_client = dna::connect_app_websocket(
            self.handle.admin_port,
            self.handle.app_port,
            &app_id,
        )
        .await?;

        // 4. Store the agent keyed by pub key hex.
        let key_hex = hex::encode(agent_pub_key.get_raw_39());
        {
            let mut agents = self.agents.lock().await;
            agents.insert(
                key_hex.clone(),
                AiAgent {
                    installed_app_id: app_id,
                    agent_pub_key,
                    app_client,
                },
            );
        }
        log::info!("Agent provisioned for AI: {} (key: {})", ai_id, key_hex);
        Ok(key_hex)
    }

    /// Phase A of cell-lineage recovery (CELL_LINEAGE_RECOVERY.md): a
    /// read-only census of every transcript cell on the conductor.
    /// Connects - and adopts into the live map - any cell it can reach,
    /// which is exactly what startup reconnect does; it changes no cell
    /// state and writes nothing to any chain.
    ///
    /// `live` = (agent key hex, AI name) for every AI in the store.
    pub async fn cell_lineage_report(
        &self,
        live: &[(String, String)],
    ) -> Result<serde_json::Value, String> {
        let admin_ws = holochain_client::AdminWebsocket::connect(
            format!("localhost:{}", self.handle.admin_port),
            Some("your-own-ai".to_string()),
        )
        .await
        .map_err(|e| format!("Failed to connect to admin WebSocket: {}", e))?;
        let apps = admin_ws
            .list_apps(None)
            .await
            .map_err(|e| format!("Failed to list apps: {}", e))?;

        struct Row {
            app_id: String,
            key_hex: String,
            parent: String,
            status: String,
            connected: bool,
            conversations: Option<u64>,
        }
        let mut rows: Vec<Row> = Vec::new();

        for app in apps
            .iter()
            .filter(|a| a.installed_app_id.starts_with(dna::APP_ID_PREFIX))
        {
            let key_hex = hex::encode(app.agent_pub_key.get_raw_39());
            let parent = app
                .installed_app_id
                .strip_prefix(dna::APP_ID_PREFIX)
                .unwrap_or("")
                .to_string();

            // Reach the cell: reuse an existing connection, else connect
            // and adopt (same as the reconnect sweep).
            let already = { self.agents.lock().await.contains_key(&key_hex) };
            let connected = if already {
                true
            } else {
                match dna::connect_app_websocket(
                    self.handle.admin_port,
                    self.handle.app_port,
                    &app.installed_app_id,
                )
                .await
                {
                    Ok(app_client) => {
                        self.agents.lock().await.insert(
                            key_hex.clone(),
                            AiAgent {
                                installed_app_id: app.installed_app_id.clone(),
                                agent_pub_key: app.agent_pub_key.clone(),
                                app_client,
                            },
                        );
                        true
                    }
                    Err(_) => false,
                }
            };

            // Count conversations - a decode-only read, no decryption.
            let conversations = if connected {
                let payload = holochain_types::prelude::ExternIO::encode(())
                    .map_err(|e| format!("Failed to encode: {}", e))?;
                match self
                    .call_zome(&key_hex, "transcript", "get_all_conversations", payload)
                    .await
                {
                    Ok(r) => holochain_types::prelude::ExternIO::decode::<
                        Vec<holochain_types::prelude::Record>,
                    >(&r)
                    .ok()
                    .map(|v| v.len() as u64),
                    Err(_) => None,
                }
            } else {
                None
            };

            rows.push(Row {
                app_id: app.installed_app_id.clone(),
                key_hex,
                parent,
                status: format!("{:?}", app.status),
                connected,
                conversations,
            });
        }

        // Walk each live AI's chain: a generation's app-id suffix is the
        // id it was installed under = the previous generation's key (or
        // the original local id at the root). Every generation - even an
        // empty one - is a LINK; severing one hides everything older.
        use std::collections::{HashMap, HashSet};
        let by_key: HashMap<String, usize> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| (r.key_hex.clone(), i))
            .collect();
        let live_keys: HashSet<&str> = live.iter().map(|(k, _)| k.as_str()).collect();
        let mut on_live_chain: HashSet<String> = HashSet::new();
        let mut chains = serde_json::Map::new();
        for (key, name) in live {
            let mut chain: Vec<String> = Vec::new();
            let mut cur = key.clone();
            while let Some(&i) = by_key.get(&cur) {
                if chain.contains(&rows[i].key_hex) {
                    break; // cycle guard
                }
                chain.push(rows[i].key_hex.clone());
                on_live_chain.insert(rows[i].key_hex.clone());
                cur = rows[i].parent.clone();
            }
            chains.insert(
                name.clone(),
                serde_json::json!({ "generations": chain.len(), "chain": chain }),
            );
        }

        let mut n_live = 0u64;
        let mut n_link = 0u64;
        let mut n_data = 0u64;
        let mut n_orphan_empty = 0u64;
        let mut n_unreachable = 0u64;
        let cells: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| {
                let is_live = live_keys.contains(r.key_hex.as_str());
                let has_data = r.conversations.unwrap_or(0) > 0;
                let class = if is_live {
                    n_live += 1;
                    "live"
                } else if !r.connected {
                    n_unreachable += 1;
                    "unreachable"
                } else if has_data {
                    n_data += 1;
                    "stranded_data"
                } else if on_live_chain.contains(&r.key_hex) {
                    n_link += 1;
                    "empty_link_on_live_chain"
                } else {
                    n_orphan_empty += 1;
                    "empty_orphan"
                };
                serde_json::json!({
                    "app_id": r.app_id,
                    "agent_key": r.key_hex,
                    "parent_id": r.parent,
                    "status": r.status,
                    "connected": r.connected,
                    "conversations": r.conversations,
                    "class": class,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "summary": {
                "total_cells": rows.len(),
                "live": n_live,
                "stranded_data": n_data,
                "empty_link_on_live_chain": n_link,
                "empty_orphan": n_orphan_empty,
                "unreachable": n_unreachable,
                "live_ais": live.len(),
            },
            "chains": chains,
            "cells": cells,
        }))
    }

    /// Phase B of cell-lineage recovery: disable (never uninstall) the
    /// empty zombie generations the census identifies. Data-safety rule:
    /// a cell stays enabled if it is live, holds conversations, or is a
    /// LINK between a live head and a deeper data-bearing cell on the
    /// same chain (the lineage walker needs every link connected).
    /// Unverified cells (unreachable during the census) are never
    /// touched. Reversible via enable_app.
    pub async fn cell_cleanup(
        &self,
        live: &[(String, String)],
    ) -> Result<serde_json::Value, String> {
        let report = self.cell_lineage_report(live).await?;
        let cells = report["cells"]
            .as_array()
            .ok_or("census produced no cells")?;

        use std::collections::HashSet;
        let mut keep: HashSet<String> = HashSet::new();

        // Live + data-bearing cells always stay.
        for c in cells {
            let class = c["class"].as_str().unwrap_or("");
            if class == "live" || class == "stranded_data" || class == "unreachable" {
                keep.insert(c["agent_key"].as_str().unwrap_or("").to_string());
            }
        }

        // Chain links shallower than the deepest data-bearing cell stay:
        // chains are ordered head (live) -> root, and the walker stops at
        // the first missing link.
        if let Some(chains) = report["chains"].as_object() {
            for (_name, chain) in chains {
                let Some(list) = chain["chain"].as_array() else { continue };
                let keys: Vec<&str> =
                    list.iter().filter_map(|k| k.as_str()).collect();
                let deepest_data = keys.iter().rposition(|k| {
                    cells.iter().any(|c| {
                        c["agent_key"].as_str() == Some(k)
                            && c["conversations"].as_u64().unwrap_or(0) > 0
                    })
                });
                if let Some(d) = deepest_data {
                    for k in &keys[..=d] {
                        keep.insert((*k).to_string());
                    }
                }
            }
        }

        let admin_ws = holochain_client::AdminWebsocket::connect(
            format!("localhost:{}", self.handle.admin_port),
            Some("your-own-ai".to_string()),
        )
        .await
        .map_err(|e| format!("Failed to connect to admin WebSocket: {}", e))?;

        let mut disabled: Vec<String> = Vec::new();
        let mut errors: Vec<String> = Vec::new();
        for c in cells {
            let key = c["agent_key"].as_str().unwrap_or("");
            let app_id = c["app_id"].as_str().unwrap_or("");
            if keep.contains(key) || app_id.is_empty() {
                continue;
            }
            match admin_ws.disable_app(app_id.to_string()).await {
                Ok(_) => {
                    disabled.push(app_id.to_string());
                    // Drop its websocket from the map - a disabled cell
                    // rejects zome calls, and lineage walks should stop
                    // treating it as a link.
                    self.agents.lock().await.remove(key);
                }
                Err(e) => errors.push(format!("{}: {}", app_id, e)),
            }
        }
        log::info!(
            "[cells] cleanup: {} cells disabled, {} kept, {} errors",
            disabled.len(),
            keep.len(),
            errors.len()
        );

        Ok(serde_json::json!({
            "summary": {
                "disabled": disabled.len(),
                "kept": keep.len(),
                "errors": errors.len(),
            },
            "disabled": disabled,
            "errors": errors,
            "census_before": report["summary"],
        }))
    }

    /// Check if an agent is provisioned (by pub key hex).
    pub async fn is_provisioned(&self, agent_key: &str) -> bool {
        let agents = self.agents.lock().await;
        agents.contains_key(agent_key)
    }

    /// Permanently uninstall an AI's transcript hApp(s) from the conductor —
    /// the irreversible half of "purge" (vs. archive, which keeps everything).
    /// Matches every installed transcript app whose id ends with `ai_id`
    /// (covers the AI's lineage of agent generations) and drops it, then
    /// forgets the agent in-memory. Returns the number of hApps removed.
    pub async fn purge_agent(&self, ai_id: &str) -> Result<u32, String> {
        let admin_ws = holochain_client::AdminWebsocket::connect(
            format!("localhost:{}", self.handle.admin_port),
            Some("your-own-ai".to_string()),
        )
        .await
        .map_err(|e| format!("Failed to connect to admin WebSocket: {}", e))?;

        let apps = admin_ws
            .list_apps(None)
            .await
            .map_err(|e| format!("Failed to list apps: {}", e))?;

        let mut removed = 0;
        for app in &apps {
            let id = &app.installed_app_id;
            if id.starts_with(dna::APP_ID_PREFIX) && id.ends_with(ai_id) {
                match admin_ws.uninstall_app(id.clone(), false).await {
                    Ok(_) => {
                        log::info!("Purged transcript app: {}", id);
                        removed += 1;
                    }
                    Err(e) => log::warn!("Failed to purge app {}: {}", id, e),
                }
            }
        }

        // Drop any in-memory agents whose app was removed.
        let mut agents = self.agents.lock().await;
        agents.retain(|_, a| !a.installed_app_id.ends_with(ai_id));

        Ok(removed)
    }

    /// Call a zome function for a specific agent (identified by pub key hex).
    /// Handles locking and unlocking the agents map correctly (no MutexGuard across await).
    pub async fn call_zome(
        &self,
        agent_key: &str,
        zome_name: &str,
        fn_name: &str,
        payload: holochain_types::prelude::ExternIO,
    ) -> Result<holochain_types::prelude::ExternIO, String> {
        // Extract the app_client reference — but we can't hold the lock across await.
        // AppWebsocket is behind Arc internally, so we need to get a reference.
        // The trick: lock, get a reference to the agent, unlock, then call.
        // Since AppWebsocket uses interior mutability (Arc<RwLock>), we can
        // call methods on it after dropping the outer lock.

        // We need to get a raw pointer or clone. Let's check if we can just
        // call from within the lock scope by using block_in_place...
        // Actually, the simplest fix: store AppWebsocket in an Arc so we can clone the Arc.

        let agents = self.agents.lock().await;
        let agent = agents
            .get(agent_key)
            .ok_or_else(|| format!("Agent not found for key: {}", agent_key))?;

        // AppWebsocket internally uses Arc, so clone is cheap.
        let client = agent.app_client.clone();
        drop(agents); // Release lock before await!

        // One call at a time per chain: the answer write and the provider-
        // side record of the same turn land milliseconds apart, and the
        // conductor rejects the second committer ("source chain head has
        // moved") - a lost transcript entry. Serialize, and retry the rare
        // conflict that still slips through (e.g. conductor-internal
        // writes moving the head).
        let chain_lock = {
            let mut locks = self.chain_locks.lock().await;
            locks
                .entry(agent_key.to_string())
                .or_insert_with(|| Arc::new(Mutex::new(())))
                .clone()
        };
        let _chain_guard = chain_lock.lock().await;

        use holochain_client::ZomeCallTarget;
        use holochain_types::prelude::{FunctionName, RoleName, ZomeName};

        let mut last_err = String::new();
        for attempt in 0..3 {
            match client
                .call_zome(
                    ZomeCallTarget::RoleName(RoleName::from(dna::ROLE_NAME)),
                    ZomeName::from(zome_name),
                    FunctionName::from(fn_name),
                    payload.clone(),
                )
                .await
            {
                Ok(result) => return Ok(result),
                Err(e) => {
                    last_err = format!("{}", e);
                    if attempt < 2 && last_err.contains("head has moved") {
                        log::warn!(
                            "[holochain] head-moved conflict on {}/{} - retrying ({})",
                            zome_name,
                            fn_name,
                            attempt + 1
                        );
                        tokio::time::sleep(std::time::Duration::from_millis(50 * (attempt + 1))).await;
                        continue;
                    }
                    break;
                }
            }
        }
        Err(format!("Zome call {}/{} failed: {}", zome_name, fn_name, last_err))
    }

    /// Walk the agent generations behind a given agent key, newest first.
    ///
    /// Historic bug: each run could mint a fresh agent for an AI whose id
    /// had been re-keyed to its previous agent's pub key, stranding earlier
    /// transcripts on older agents. The chain is recoverable from
    /// installed_app_ids: `transcript_<X>` where X is either the original
    /// local AI id (the root) or the pub key hex of the previous
    /// generation's agent. Readers merge across the whole lineage so no
    /// conversation is orphaned.
    pub async fn agent_lineage(&self, agent_key_hex: &str) -> Vec<String> {
        let agents = self.agents.lock().await;
        let mut lineage: Vec<String> = Vec::new();
        let mut cur = agent_key_hex.to_string();
        while let Some(agent) = agents.get(&cur) {
            lineage.push(cur.clone());
            let suffix = agent
                .installed_app_id
                .strip_prefix(dna::APP_ID_PREFIX)
                .or_else(|| agent.installed_app_id.strip_prefix("transcript_"))
                .unwrap_or("")
                .to_string();
            // Root apps end in the original local AI id (not in the map);
            // guard against self-reference and cycles.
            if suffix.is_empty() || suffix == cur || lineage.contains(&suffix) {
                break;
            }
            cur = suffix;
        }
        lineage
    }

    /// Reconnect AppWebsockets for already-installed apps on startup.
    ///
    /// Called after conductor restart to reconnect to apps that were
    /// installed in a previous session.
    pub async fn reconnect_existing_agents(&self) -> Result<(), String> {
        let admin_ws = holochain_client::AdminWebsocket::connect(
            format!("localhost:{}", self.handle.admin_port),
            Some("your-own-ai".to_string()),
        )
        .await
        .map_err(|e| format!("Failed to connect to admin WebSocket: {}", e))?;

        let apps = admin_ws
            .list_apps(None)
            .await
            .map_err(|e| format!("Failed to list apps: {}", e))?;

        let mut reconnected = 0;
        let mut failed: Vec<(String, String, holochain_types::prelude::AgentPubKey)> =
            Vec::new();
        for app in &apps {
            if app.installed_app_id.starts_with(dna::APP_ID_PREFIX) {
                let agent_pub_key = app.agent_pub_key.clone();
                let key_hex = hex::encode(agent_pub_key.get_raw_39());

                // Skip if already in our map.
                {
                    let agents = self.agents.lock().await;
                    if agents.contains_key(&key_hex) {
                        continue;
                    }
                }

                // Connect AppWebsocket.
                match dna::connect_app_websocket(
                    self.handle.admin_port,
                    self.handle.app_port,
                    &app.installed_app_id,
                )
                .await
                {
                    Ok(app_client) => {
                        let mut agents = self.agents.lock().await;
                        agents.insert(
                            key_hex.clone(),
                            AiAgent {
                                installed_app_id: app.installed_app_id.clone(),
                                agent_pub_key,
                                app_client,
                            },
                        );
                        reconnected += 1;
                        log::info!("Reconnected agent (key: {})", key_hex);
                    }
                    Err(e) => {
                        log::warn!("Failed to reconnect agent {}: {}", key_hex, e);
                        failed.push((
                            app.installed_app_id.clone(),
                            key_hex.clone(),
                            agent_pub_key.clone(),
                        ));
                    }
                }
            }
        }

        // Second sweep: a connect that failed early in the pass almost
        // always lost the CellDisabled enable-wave race - and the wave has
        // had the whole (long) sequential pass to catch up. One retry per
        // failure recovers most of them; whatever still fails is protected
        // from generation-forking by the adopt-don't-mint guard in
        // provision_agent.
        if !failed.is_empty() {
            log::info!(
                "Retrying {} agent reconnects that lost the enable race",
                failed.len()
            );
            for app_id in failed {
                match dna::connect_app_websocket(
                    self.handle.admin_port,
                    self.handle.app_port,
                    &app_id.0,
                )
                .await
                {
                    Ok(app_client) => {
                        let mut agents = self.agents.lock().await;
                        agents.insert(
                            app_id.1.clone(),
                            AiAgent {
                                installed_app_id: app_id.0.clone(),
                                agent_pub_key: app_id.2.clone(),
                                app_client,
                            },
                        );
                        reconnected += 1;
                        log::info!("Reconnected agent on retry (key: {})", app_id.1);
                    }
                    Err(e) => {
                        log::warn!(
                            "Agent {} still unreachable after retry: {}",
                            app_id.1,
                            e
                        );
                    }
                }
            }
        }

        if reconnected > 0 {
            log::info!("Reconnected {} existing AI agents", reconnected);
        }

        Ok(())
    }
}
