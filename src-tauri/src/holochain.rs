//! Multi-agent Holochain manager for Your Own AI.
//!
//! Each AI personality gets its own Holochain agent with its own keypair
//! and source chain. This module manages the lifecycle of multiple agents
//! within a single conductor instance.

use crate::transcript_crypto::RecoveryMaterial;
use crate::dna;
use holochain_client::AppWebsocket;
use holochain_types::prelude::AgentPubKey;
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
/// (total records, app-entry records) of a source chain, from an admin
/// `dump_state` answer, without assuming its exact framing (historically a
/// JSON array of [dump, summary-string]).
///
/// Chain LENGTH proves nothing about emptiness: every launch that connects
/// a cell writes a capability grant, so a never-used cell reaches 40-120
/// records in weeks (field-measured 08-24 - the 142 zombies carried 41-124
/// records, all Dna/AgentValidationPkg/InitZomesComplete/CapGrant). Real
/// data means APP ENTRIES: records whose action's entry_type is the
/// `{"App": ...}` object. Zero app entries = never written to.
fn chain_stats_from_dump(dump: &str) -> Option<(u64, u64)> {
    let v: serde_json::Value = serde_json::from_str(dump).ok()?;
    let scd = v
        .get(0)
        .and_then(|d| d.get("source_chain_dump"))
        .or_else(|| v.get("source_chain_dump"))?;
    let records = scd.get("records")?.as_array()?;
    let app_entries = records
        .iter()
        .filter(|r| {
            r.get("action")
                .and_then(|a| a.get("entry_type"))
                .map(|et| et.is_object() && et.get("App").is_some())
                .unwrap_or(false)
        })
        .count() as u64;
    Some((records.len() as u64, app_entries))
}

#[cfg(test)]
mod lineage_tests {
    use super::HolochainManager;

    #[test]
    fn the_suffix_reads_the_same_on_both_lines_and_the_oldest_name() {
        assert_eq!(HolochainManager::app_id_suffix("transcript_v1_abc"), "abc");
        assert_eq!(HolochainManager::app_id_suffix("transcript_v2_abc"), "abc", "not v2_abc");
        assert_eq!(HolochainManager::app_id_suffix("transcript_abc"), "abc");
        assert_eq!(HolochainManager::app_id_suffix("something_else"), "");
    }
}

#[cfg(test)]
mod census_tests {
    #[test]
    fn dump_parsing_counts_records_and_app_entries() {
        // The field shape of a zombie: genesis + init + grants, no app entries.
        let zombie = r#"[{"source_chain_dump":{"records":[
            {"action":{"type":"Dna"}},
            {"action":{"type":"AgentValidationPkg"}},
            {"action":{"type":"Create","entry_type":"AgentPubKey"}},
            {"action":{"type":"InitZomesComplete"}},
            {"action":{"type":"Create","entry_type":"CapGrant"}},
            {"action":{"type":"Create","entry_type":"CapGrant"}}
        ]}},"summary"]"#;
        assert_eq!(super::chain_stats_from_dump(zombie), Some((6, 0)));
        // A storied cell: app entries present (object-shaped entry_type).
        let storied = r#"{"source_chain_dump":{"records":[
            {"action":{"type":"Create","entry_type":"CapGrant"}},
            {"action":{"type":"Create","entry_type":{"App":{"entry_index":0,"zome_index":0,"visibility":"Private"}}}},
            {"action":{"type":"Update","entry_type":{"App":{"entry_index":0,"zome_index":0,"visibility":"Private"}}}}
        ]}}"#;
        assert_eq!(super::chain_stats_from_dump(storied), Some((3, 2)));
        assert_eq!(super::chain_stats_from_dump("not json"), None);
        assert_eq!(super::chain_stats_from_dump(r#"{"other":1}"#), None);
    }
}

pub struct HolochainManager {
    pub resource_dir: PathBuf,
    /// Map from agent pub key hex to its Holochain agent.
    pub agents: Mutex<HashMap<String, AiAgent>>,
    /// Lineage index: agent key hex -> the app-id suffix that generation
    /// was installed under (= the previous generation's key, or the root
    /// local id). Built from the conductor's `list_apps`, so the chain
    /// survives DISABLED members - the connected map only knows enabled
    /// cells, and severing a lineage at the first disabled link was the
    /// 08-19 tidy trap.
    lineage_index: Mutex<HashMap<String, String>>,
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
    /// A manager for the conductor that has just started.
    pub fn new(resource_dir: PathBuf, recovery: RecoveryMaterial) -> Self {
        Self {
            resource_dir,
            agents: Mutex::new(HashMap::new()),
            lineage_index: Mutex::new(HashMap::new()),
            provision_lock: Mutex::new(()),
            chain_locks: Mutex::new(HashMap::new()),
            recovery,
        }
    }

    /// The conductor's admin port (0 when it does not run: a connection
    /// then fails at once).
    pub fn admin_port(&self) -> u16 {
        crate::conductor::admin_port().unwrap_or(0)
    }

    /// The conductor's app interface port (0 when it does not run).
    pub fn app_port(&self) -> u16 {
        crate::conductor::app_port().unwrap_or(0)
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
    /// The first provisioned cell of an app - the one its source chain
    /// lives in (transcript apps have exactly one).
    fn first_cell_id(app: &holochain_client::AppInfo) -> Option<holochain_types::prelude::CellId> {
        app.cell_info.values().flatten().find_map(|ci| match ci {
            holochain_client::CellInfo::Provisioned(p) => Some(p.cell_id.clone()),
            _ => None,
        })
    }

    /// The suffix a transcript app was installed under, on either line
    /// (`transcript_v1_X`, `transcript_v2_X`, or the oldest `transcript_X`):
    /// X is the previous generation's key, or the root local id.
    fn app_id_suffix(installed_app_id: &str) -> String {
        installed_app_id
            .strip_prefix(dna::APP_ID_PREFIX)
            .or_else(|| installed_app_id.strip_prefix(crate::hc07::APP_ID_PREFIX))
            .or_else(|| installed_app_id.strip_prefix("transcript_"))
            .unwrap_or("")
            .to_string()
    }

    /// One random seed imported into both lines' key stores under `tag`: the
    /// same key on each. None when the 0.7 line is not up, its store's
    /// passphrase cannot be read, or the 0.6 store already holds the tag
    /// with another key (a provisioning that stopped part way): the caller
    /// then makes the key on the 0.6 line as before.
    async fn key_on_both_lines(&self, tag: &str) -> Option<[u8; 32]> {
        use flowsta_hc_lines::supervisor::KeySource;
        use flowsta_hc_lines::Line;
        crate::conductor::ports(Line::V07)?;
        let root = crate::conductor::profile_root()?;
        let pass_07 = crate::conductor::passphrase_of_store_07(root, &self.data_key().ok()?).ok()?;
        let mut seed = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::rngs::OsRng, &mut seed);
        let seed = seed;
        // The seed lives only in `source`, dropped when this returns.
        let source = KeySource::Seed { seed, tag: tag.to_string() };
        let sup = crate::conductor::supervisor();
        let k06 = match sup.add_agent(Line::V06, crate::conductor::PASSPHRASE_06, &source).await {
            Ok(k) => k,
            Err(e) => {
                log::info!("[keys] {tag}: one key for both lines was not possible on 0.6 ({e}); the 0.6 line makes it");
                return None;
            }
        };
        match sup.add_agent(Line::V07, &pass_07, &source).await {
            Ok(k07) if k07 == k06 => Some(k06),
            Ok(_) => {
                log::warn!("[keys] {tag}: the two stores gave different keys; the 0.6 one is used, its v2 app waits");
                None
            }
            Err(e) => {
                log::warn!("[keys] {tag}: the 0.7 store did not take the key ({e}); its v2 app waits for the next start");
                None
            }
        }
    }

    /// An AI on the 0.7 line alone (no 0.6 line runs): an existing v2 app
    /// is adopted, by its key or by the AI id it was installed under; a new
    /// one is made only once the 0.6 line is recorded as stopped (until
    /// then it is restarting, and the caller tries again).
    async fn provision_on_07(&self, ai_id: &str) -> Result<String, String> {
        use flowsta_hc_lines::Line;
        if crate::conductor::ports(Line::V07).is_none() {
            return Err("Holochain conductor is still starting...".to_string());
        }
        let apps = crate::hc07::transcript_apps().await?;
        if let Some(app) = apps.iter().find(|a| a.agent == ai_id || a.app_id.ends_with(ai_id)) {
            self.lineage_index.lock().await.entry(app.agent.clone()).or_insert_with(|| Self::app_id_suffix(&app.app_id));
            return Ok(app.agent.clone());
        }
        crate::hc07::write_07_alone(ai_id).await?;
        let root = crate::conductor::profile_root().ok_or("no profile")?;
        let pass_07 = crate::conductor::passphrase_of_store_07(root, &self.data_key()?)?;
        let key = crate::conductor::supervisor()
            .add_agent(Line::V07, &pass_07, &flowsta_hc_lines::supervisor::KeySource::NewSeed { tag: format!("ai-{}", ai_id) })
            .await
            .map_err(|e| format!("Failed to create the key for AI {}: {}", ai_id, e))?;
        let agent = holochain_types::prelude::AgentPubKey::from_raw_32(key.to_vec());
        let v1_app_id = format!("{}{}", dna::APP_ID_PREFIX, ai_id);
        crate::hc07::ensure_apps(&self.resource_dir, &self.recovery.network_seed, &[(v1_app_id, agent.get_raw_39().to_vec())]).await?;
        let key_hex = hex::encode(agent.get_raw_39());
        self.lineage_index.lock().await.insert(key_hex.clone(), ai_id.to_string());
        log::info!("Agent provisioned on the 0.7 line for AI: {} (key: {})", ai_id, key_hex);
        Ok(key_hex)
    }

    /// Index the 0.7 line's v2 apps too (disabled ones included), so the
    /// lineage holds without the 0.6 line. A v2 app mirrors its v1 app's
    /// suffix, so where both exist they agree.
    pub async fn index_lineage_07(&self) -> Result<usize, String> {
        let apps = crate::hc07::transcript_apps().await?;
        let mut idx = self.lineage_index.lock().await;
        for a in &apps {
            idx.entry(a.agent.clone()).or_insert_with(|| Self::app_id_suffix(&a.app_id));
        }
        Ok(apps.len())
    }

    /// Every AI agent this device holds a cell for, on either line (keys,
    /// hex, sorted): the 0.6 line's connected agents and the 0.7 line's
    /// enabled v2 apps. A 0.7 line that does not answer adds nothing.
    pub async fn ai_agents(&self) -> Vec<String> {
        let mut keys: std::collections::BTreeSet<String> = self.agents.lock().await.keys().cloned().collect();
        if crate::conductor::ports(flowsta_hc_lines::Line::V07).is_some() {
            match crate::hc07::agents_with_apps().await {
                Ok(on_07) => keys.extend(on_07),
                Err(e) => log::warn!("[0.7] the AI list: {e}"),
            }
        }
        keys.into_iter().collect()
    }

    /// Rebuild the lineage index from a `list_apps` answer. Disabled apps
    /// are indexed too - that is the point.
    async fn rebuild_lineage_index(&self, apps: &[holochain_client::AppInfo]) {
        let mut idx = self.lineage_index.lock().await;
        for app in apps.iter().filter(|a| {
            a.installed_app_id.starts_with(dna::APP_ID_PREFIX)
                || a.installed_app_id.starts_with("transcript_")
        }) {
            idx.insert(
                hex::encode(app.agent_pub_key.get_raw_39()),
                Self::app_id_suffix(&app.installed_app_id),
            );
        }
    }

    pub async fn provision_agent(&self, ai_id: &str) -> Result<String, String> {
        // Hold for the entire provision — makes concurrent calls for the
        // same AI idempotent instead of racing the source chain.
        let _provision_guard = self.provision_lock.lock().await;

        // No 0.6 line: the AI is on the 0.7 line alone.
        if crate::conductor::ports(flowsta_hc_lines::Line::V06).is_none() {
            return self.provision_on_07(ai_id).await;
        }

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
                format!("localhost:{}", self.admin_port()),
                Some(crate::conductor::origin()),
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
                // A tidied (disabled) cell that provisioning asks for by
                // id is wanted again - turn it back on first.
                if !matches!(
                    app.status,
                    holochain_types::app::AppStatus::Enabled
                ) {
                    admin_ws
                        .enable_app(app.installed_app_id.clone())
                        .await
                        .map_err(|e| {
                            format!(
                                "AI {} exists but couldn't be re-enabled: {}",
                                ai_id, e
                            )
                        })?;
                    log::info!(
                        "Re-enabled tidied cell {} for AI {}",
                        app.installed_app_id,
                        ai_id
                    );
                }
                log::info!(
                    "AI id {} already installed on the conductor as {} (key: {}) - adopting, not minting",
                    ai_id,
                    app.installed_app_id,
                    key_hex
                );
                let app_client = dna::connect_app_websocket(
                    self.admin_port(),
                    self.app_port(),
                    &app.installed_app_id,
                )
                .await
                .map_err(|e| {
                    format!(
                        "AI {} exists on the conductor but isn't reachable yet: {}",
                        ai_id, e
                    )
                })?;
                self.lineage_index.lock().await.insert(
                    key_hex.clone(),
                    Self::app_id_suffix(&app.installed_app_id),
                );
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

        // 1. This AI's key. With the 0.7 line up, one random seed goes into
        // both lines' key stores, so the AI has the same key on each (a
        // device still on 0.6 keeps seeing its chats) and the seed is kept
        // nowhere else. Otherwise a random seed made in the 0.6 store under
        // its tag, or the one already there from a previous session (looked
        // up by its tag, never by listing the store's entries: the key
        // store's connection carries at most 8 KiB per message, so listing a
        // large store aborts the connection).
        let tag = format!("ai-{}", ai_id);
        let (key, on_07) = match self.key_on_both_lines(&tag).await {
            Some(k) => (k, true),
            None => {
                let k = crate::conductor::supervisor()
                    .add_agent(flowsta_hc_lines::Line::V06, crate::conductor::PASSPHRASE_06, &flowsta_hc_lines::supervisor::KeySource::NewSeed { tag: tag.clone() })
                    .await
                    .map_err(|e| format!("Failed to create the key for AI {}: {}", ai_id, e))?;
                (k, false)
            }
        };
        let agent_pub_key = holochain_types::prelude::AgentPubKey::from_raw_32(key.to_vec());
        log::info!("Agent key for AI {}: {}", ai_id, hex::encode(key));

        // 2. Install the transcript hApp with this agent key.
        let app_id = dna::install_transcript_app(
            self.admin_port(),
            &self.resource_dir,
            agent_pub_key.clone(),
            ai_id,
            &self.recovery.network_seed,
        )
        .await?;

        // Its v2 app now, not at the next start, when its key is on both lines.
        if on_07 {
            match crate::hc07::ensure_apps(&self.resource_dir, &self.recovery.network_seed, &[(app_id.clone(), agent_pub_key.get_raw_39().to_vec())]).await {
                Ok(_) => log::info!("[0.7] {} has its v2 app from the start", ai_id),
                Err(e) => log::warn!("[0.7] the v2 app for {} waits for the next start: {e}", ai_id),
            }
        }

        // 3. Connect an AppWebsocket.
        let app_client = dna::connect_app_websocket(
            self.admin_port(),
            self.app_port(),
            &app_id,
        )
        .await?;

        // 4. Store the agent keyed by pub key hex.
        let key_hex = hex::encode(agent_pub_key.get_raw_39());
        self.lineage_index
            .lock()
            .await
            .insert(key_hex.clone(), Self::app_id_suffix(&app_id));
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
            format!("localhost:{}", self.admin_port()),
            Some(crate::conductor::origin()),
        )
        .await
        .map_err(|e| format!("Failed to connect to admin WebSocket: {}", e))?;
        let apps = admin_ws
            .list_apps(None)
            .await
            .map_err(|e| format!("Failed to list apps: {}", e))?;
        self.rebuild_lineage_index(&apps).await;

        struct Row {
            app_id: String,
            key_hex: String,
            parent: String,
            status: String,
            connected: bool,
            conversations: Option<u64>,
            /// Source-chain record count from an admin dump - the on-disk
            /// truth, immune to the records-warmup window that made the
            /// 08-19 census call storied cells "empty".
            chain_records: Option<u64>,
            /// How many of those records are APP ENTRIES (real writes).
            /// Grants and genesis don't count - see chain_stats_from_dump.
            app_entries: Option<u64>,
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

            let enabled = matches!(
                app.status,
                holochain_types::app::AppStatus::Enabled
            );
            // Reach the cell: reuse an existing connection, else connect
            // and adopt (same as the reconnect sweep). Disabled cells are
            // never dialed - each attempt is a slow authorize failure.
            let already = { self.agents.lock().await.contains_key(&key_hex) };
            let connected = if already {
                true
            } else if !enabled {
                false
            } else {
                match dna::connect_app_websocket(
                    self.admin_port(),
                    self.app_port(),
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
                // The link count alone - no record is fetched for it.
                crate::transcript_pages::conversation_count(
                    self,
                    &key_hex,
                    std::time::Duration::from_secs(60),
                )
                .await
                .ok()
            } else {
                None
            };

            // WARM verification: a conversation count of zero (or an
            // unreachable cell) proves nothing - cells answer empty while
            // records load from disk, and disabled cells cannot be asked.
            // The source chain on disk is the truth: probe it whenever the
            // zome count did not already prove data.
            // Disabled cells are classified by status alone - their dump
            // would fail anyway, and 142 of them warn-spam the log.
            let chain_stats = if enabled && conversations.unwrap_or(0) == 0 {
                match Self::first_cell_id(app) {
                    Some(cell_id) => match admin_ws.dump_state(cell_id).await {
                        Ok(dump) => chain_stats_from_dump(&dump),
                        Err(e) => {
                            log::warn!("[cells] dump_state failed for {}: {e:?}", app.installed_app_id);
                            None
                        }
                    },
                    None => None,
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
                chain_records: chain_stats.map(|(r, _)| r),
                app_entries: chain_stats.map(|(_, a)| a),
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
        let mut n_unverified = 0u64;
        let mut n_disabled = 0u64;
        let mut would_disable: Vec<String> = Vec::new();
        let cells: Vec<serde_json::Value> = rows
            .iter()
            .map(|r| {
                let is_live = live_keys.contains(r.key_hex.as_str());
                // Data is proven by EITHER a warm conversation count or a
                // storied source chain; emptiness ONLY by a verified
                // genesis-length chain. "Couldn't verify" is its own class
                // and is never tidyable - the 08-19 rule.
                let has_data = r.conversations.unwrap_or(0) > 0
                    || r.app_entries.map(|n| n > 0).unwrap_or(false);
                let verified_empty = r.app_entries == Some(0);
                let class = if is_live {
                    n_live += 1;
                    "live"
                } else if r.status.starts_with("Disabled") {
                    n_disabled += 1;
                    "disabled"
                } else if has_data {
                    n_data += 1;
                    "stranded_data"
                } else if verified_empty && on_live_chain.contains(&r.key_hex) {
                    n_link += 1;
                    would_disable.push(r.app_id.clone());
                    "empty_link_verified"
                } else if verified_empty {
                    n_orphan_empty += 1;
                    would_disable.push(r.app_id.clone());
                    "empty_orphan_verified"
                } else {
                    n_unverified += 1;
                    "unverified"
                };
                serde_json::json!({
                    "app_id": r.app_id,
                    "agent_key": r.key_hex,
                    "parent_id": r.parent,
                    "status": r.status,
                    "connected": r.connected,
                    "conversations": r.conversations,
                    "chain_records": r.chain_records,
                    "app_entries": r.app_entries,
                    "class": class,
                })
            })
            .collect();

        Ok(serde_json::json!({
            "census_version": 3,
            "empty_rule": "zero app-entry records on the on-disk source chain",
            "summary": {
                "total_cells": rows.len(),
                "live": n_live,
                "stranded_data": n_data,
                "empty_link_verified": n_link,
                "empty_orphan_verified": n_orphan_empty,
                "unverified": n_unverified,
                "disabled": n_disabled,
                "live_ais": live.len(),
                "would_disable": would_disable.len(),
            },
            "would_disable": would_disable,
            "chains": chains,
            "cells": cells,
        }))
    }

    /// Tidy v2: turn off exactly the cells a FRESH census just verified
    /// as empty (zero app entries, not live, not already disabled).
    /// `disable_app` only - reversible, nothing deleted, and the adopt
    /// path re-enables a disabled cell if its id is ever asked for again.
    /// The lineage index keeps disabled generations as links, so history
    /// reads stay whole - the 08-19 severing cannot recur.
    pub async fn tidy_cells(&self, live: &[(String, String)]) -> Result<serde_json::Value, String> {
        let report = self.cell_lineage_report(live).await?;
        let planned: Vec<String> = report["would_disable"]
            .as_array()
            .map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect())
            .unwrap_or_default();
        let key_by_app: HashMap<String, String> = report["cells"]
            .as_array()
            .map(|cells| {
                cells
                    .iter()
                    .filter_map(|c| {
                        Some((
                            c["app_id"].as_str()?.to_string(),
                            c["agent_key"].as_str()?.to_string(),
                        ))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let admin_ws = holochain_client::AdminWebsocket::connect(
            format!("localhost:{}", self.admin_port()),
            Some(crate::conductor::origin()),
        )
        .await
        .map_err(|e| format!("Failed to connect to admin WebSocket: {}", e))?;
        let mut disabled = 0u64;
        let mut errors: Vec<String> = Vec::new();
        for app_id in &planned {
            match admin_ws.disable_app(app_id.clone()).await {
                Ok(_) => {
                    disabled += 1;
                    // A just-disabled cell must not be dialed again this
                    // session; the lineage index still links through it.
                    if let Some(key) = key_by_app.get(app_id) {
                        self.agents.lock().await.remove(key);
                    }
                }
                Err(e) => errors.push(format!("{app_id}: {e:?}")),
            }
        }
        crate::hc07::disable_for(&planned).await;
        log::info!(
            "[cells] tidy v2: {} of {} verified-empty cells disabled, {} errors",
            disabled,
            planned.len(),
            errors.len()
        );
        Ok(serde_json::json!({
            "planned": planned.len(),
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
            format!("localhost:{}", self.admin_port()),
            Some(crate::conductor::origin()),
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
        {
            let mut agents = self.agents.lock().await;
            agents.retain(|_, a| !a.installed_app_id.ends_with(ai_id));
        }
        // Its copies on the 0.7 line go too.
        crate::hc07::purge(ai_id).await;

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
        self.call_zome_with_timeout(agent_key, zome_name, fn_name, payload, std::time::Duration::from_secs(60)).await
    }

    /// `call_zome` with a caller-chosen wait. The backup reads a whole
    /// cell in one call, and a thousand-conversation cell takes longer
    /// than the sixty seconds a chat turn may wait (the 32-days-old-backup
    /// finding, 2026-09-06). The app websocket's own request timeout is
    /// set to match in dna.rs.
    pub async fn call_zome_with_timeout(
        &self,
        agent_key: &str,
        zome_name: &str,
        fn_name: &str,
        payload: holochain_types::prelude::ExternIO,
        timeout: std::time::Duration,
    ) -> Result<holochain_types::prelude::ExternIO, String> {
        // Extract the app_client reference — but we can't hold the lock across await.
        // AppWebsocket is behind Arc internally, so we need to get a reference.
        // The trick: lock, get a reference to the agent, unlock, then call.
        // Since AppWebsocket uses interior mutability (Arc<RwLock>), we can
        // call methods on it after dropping the outer lock.

        // Counted on the 0.6 line from here, queued behind the chain's lock
        // included: a retirement of the line drains these first, and the
        // watch's memory restart waits for them.
        let _in_flight = flowsta_hc_lines::calling(flowsta_hc_lines::Line::V06);

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
            // Bounded wait: a hung conductor websocket must surface as an
            // error, not freeze every reader queued behind this chain's
            // lock (and the UI above them) forever.
            let call = client.call_zome(
                ZomeCallTarget::RoleName(RoleName::from(dna::ROLE_NAME)),
                ZomeName::from(zome_name),
                FunctionName::from(fn_name),
                payload.clone(),
            );
            match tokio::time::timeout(timeout, call).await {
                Err(_) => {
                    last_err = format!("no answer from the conductor within {}s", timeout.as_secs());
                    break;
                }
                Ok(Ok(result)) => return Ok(result),
                Ok(Err(e)) => {
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
        // Walk the list_apps-based index: every generation is a link
        // REGARDLESS of enable state, so a disabled middle cell no longer
        // hides everything older. Readers skip members they cannot reach
        // (call_zome fails, they warn + continue) - skip, never sever.
        {
            let idx = self.lineage_index.lock().await;
            if idx.contains_key(agent_key_hex) {
                let mut lineage: Vec<String> = Vec::new();
                let mut cur = agent_key_hex.to_string();
                while idx.contains_key(&cur) {
                    lineage.push(cur.clone());
                    let suffix = idx.get(&cur).cloned().unwrap_or_default();
                    if suffix.is_empty() || suffix == cur || lineage.contains(&suffix) {
                        break;
                    }
                    cur = suffix;
                }
                return lineage;
            }
        }
        // Not indexed yet (call before the first reconnect finished):
        // the connected-map walk still answers.
        let agents = self.agents.lock().await;
        let mut lineage: Vec<String> = Vec::new();
        let mut cur = agent_key_hex.to_string();
        while let Some(agent) = agents.get(&cur) {
            lineage.push(cur.clone());
            let suffix = Self::app_id_suffix(&agent.installed_app_id);
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
    /// After the lines restarted (new ports, a new origin): every agent's
    /// connection is made again.
    pub async fn reconnect_after_restart(&self) -> Result<(), String> {
        self.agents.lock().await.clear();
        self.reconnect_existing_agents().await
    }

    pub async fn reconnect_existing_agents(&self) -> Result<(), String> {
        let admin_ws = holochain_client::AdminWebsocket::connect(
            format!("localhost:{}", self.admin_port()),
            Some(crate::conductor::origin()),
        )
        .await
        .map_err(|e| format!("Failed to connect to admin WebSocket: {}", e))?;

        let apps = admin_ws
            .list_apps(None)
            .await
            .map_err(|e| format!("Failed to list apps: {}", e))?;

        self.rebuild_lineage_index(&apps).await;

        let mut reconnected = 0;
        let mut skipped_disabled = 0u32;
        let mut failed: Vec<(String, String, holochain_types::prelude::AgentPubKey)> =
            Vec::new();
        for app in &apps {
            if app.installed_app_id.starts_with(dna::APP_ID_PREFIX) {
                // Tidied (disabled) cells stay installed but must not be
                // dialed - every attempt is a slow authorize failure.
                if !matches!(
                    app.status,
                    holochain_types::app::AppStatus::Enabled
                ) {
                    skipped_disabled += 1;
                    continue;
                }
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
                    self.admin_port(),
                    self.app_port(),
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
                    self.admin_port(),
                    self.app_port(),
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
        if skipped_disabled > 0 {
            log::info!(
                "Skipped {} disabled (tidied) transcript cells",
                skipped_disabled
            );
        }

        Ok(())
    }
}
