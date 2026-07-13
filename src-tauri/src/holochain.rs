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

        use holochain_client::ZomeCallTarget;
        use holochain_types::prelude::{FunctionName, RoleName, ZomeName};

        client
            .call_zome(
                ZomeCallTarget::RoleName(RoleName::from(dna::ROLE_NAME)),
                ZomeName::from(zome_name),
                FunctionName::from(fn_name),
                payload,
            )
            .await
            .map_err(|e| format!("Zome call {}/{} failed: {}", zome_name, fn_name, e))
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
