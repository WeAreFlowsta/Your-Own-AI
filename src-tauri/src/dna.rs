//! DNA installation for Your Own AI.
//!
//! Installs the transcript hApp for individual AI personalities.
//! Each AI gets its own agent key and installed_app_id.

use holochain_client::{
    AdminWebsocket, AuthorizeSigningCredentialsPayload, ClientAgentSigner,
    InstallAppPayload,
};
use holochain_types::app::AppBundleSource;
use holochain_types::prelude::{
    AgentPubKey, CoordinatorBundle, CoordinatorManifest, CoordinatorSource,
    UpdateCoordinatorsPayload, ZomeDependency, ZomeManifest,
};
use std::path::Path;

/// The hApp bundle filename (in src-tauri/resources/).
const HAPP_FILENAME: &str = "yourown_ai_transcript_v1_happ.happ";

/// The coordinator zome version this build ships. Bump when the coordinator
/// zome gains/changes externs (rebuild via dna/v1/build-coordinator.sh); the
/// committed .happ (and with it the DNA hash) is never rebuilt. Two paths
/// carry it to cells: `install_transcript_app` applies it to every cell it
/// installs (the conductor keys ribosomes by CELL, so a cell installed from
/// the bundled .happ starts on the bundle's coordinator no matter what
/// earlier cells were swapped to), and the startup sweep hot-swaps every
/// pre-existing cell once per version.
/// v1 = original externs; v2 = + delete_conversation; v3 = same wasm as v2,
/// re-swept because cells installed after a v2 sweep (new AIs, Vault
/// restores, fresh installs whose default AIs came after the sweep) had kept
/// the bundle's v1.
pub const COORDINATOR_VERSION: u32 = 3;
/// The standalone coordinator wasm (in src-tauri/resources/), staged by
/// dna/v1/build-coordinator.sh.
const COORDINATOR_WASM_FILENAME: &str = "transcript_coordinator.wasm";
/// App-data marker recording the last version the sweep applied.
const COORDINATOR_MARKER_FILENAME: &str = "coordinator-version";

/// The role name inside the hApp bundle (must match happ.yaml).
pub const ROLE_NAME: &str = "transcript";

/// Versioned app-id prefix. Bump alongside DNA versions so future
/// versions install side-by-side during migration (ProofPoll pattern,
/// adopted pre-launch per TRANSCRIPT_PRIVACY_ROADMAP.md Phase A).
pub const APP_ID_PREFIX: &str = "transcript_v1_";

/// Construct an installed_app_id for an AI personality.
/// e.g., "transcript_v1_local-abc123"
pub fn make_app_id(ai_id: &str) -> String {
    format!("{}{}", APP_ID_PREFIX, ai_id)
}

/// Install the transcript hApp for one AI personality.
///
/// Returns the installed_app_id. Skips if already installed.
pub async fn install_transcript_app(
    admin_port: u16,
    resource_dir: &Path,
    agent_key: AgentPubKey,
    ai_id: &str,
    network_seed: &str,
) -> Result<String, String> {
    let app_id = make_app_id(ai_id);
    let happ_path = resource_dir.join(HAPP_FILENAME);

    if !happ_path.exists() {
        return Err(format!(
            "Transcript hApp not found at {:?}",
            happ_path
        ));
    }

    let admin_ws = AdminWebsocket::connect(
        format!("localhost:{}", admin_port),
        Some("your-own-ai".to_string()),
    )
    .await
    .map_err(|e| format!("Failed to connect to admin WebSocket: {}", e))?;

    // Check if already installed.
    let existing_apps = admin_ws
        .list_apps(None)
        .await
        .map_err(|e| format!("Failed to list apps: {}", e))?;

    if existing_apps.iter().any(|a| a.installed_app_id == app_id) {
        log::info!("Transcript app already installed for AI: {}", ai_id);
        return Ok(app_id);
    }

    // Install.
    log::info!("Installing transcript app for AI: {} ({:?})", ai_id, happ_path);
    let payload = InstallAppPayload {
        source: AppBundleSource::Path(happ_path),
        agent_key: Some(agent_key),
        installed_app_id: Some(app_id.clone()),
        // Per-user seed (Phase A): each user's transcript DHT is their
        // own network. The dna.yaml seed is a template, always overridden.
        network_seed: Some(network_seed.to_string()),
        roles_settings: None,
        ignore_genesis_failure: false,
    };

    let result = admin_ws.install_app(payload).await;
    match result {
        Ok(_) => {}
        Err(e) => {
            let err_str = format!("{}", e);
            if err_str.contains("AppAlreadyInstalled") {
                log::info!("App {} already installed (recovered)", app_id);
            } else {
                return Err(format!("Failed to install transcript app: {}", e));
            }
        }
    }

    // Enable.
    admin_ws
        .enable_app(app_id.clone())
        .await
        .map_err(|e| format!("Failed to enable transcript app: {}", e))?;

    // The bundle carries the original coordinator; bring this cell to the
    // version the build ships. Ribosomes are per cell in the conductor, so
    // the startup sweep having run before does nothing for a new cell.
    if COORDINATOR_VERSION > 1 {
        let bundle = coordinator_bundle(resource_dir)?;
        let apps = admin_ws
            .list_apps(None)
            .await
            .map_err(|e| format!("Failed to list apps: {}", e))?;
        if let Some(app) = apps.iter().find(|a| a.installed_app_id == app_id) {
            let n = apply_coordinator(&admin_ws, app, &bundle).await?;
            log::info!(
                "[dna] coordinator v{} applied to {} cell(s) of {}",
                COORDINATOR_VERSION,
                n,
                app_id
            );
        }
    }

    log::info!("Transcript app installed and enabled for AI: {}", ai_id);
    Ok(app_id)
}

/// The coordinator bundle this build ships, assembled from the staged wasm.
fn coordinator_bundle(resource_dir: &Path) -> Result<CoordinatorBundle, String> {
    let wasm_path = resource_dir.join(COORDINATOR_WASM_FILENAME);
    let wasm = std::fs::read(&wasm_path)
        .map_err(|e| format!("Coordinator wasm missing at {:?}: {}", wasm_path, e))?;

    let manifest = CoordinatorManifest {
        zomes: vec![ZomeManifest {
            name: ROLE_NAME.into(),
            hash: None,
            path: COORDINATOR_WASM_FILENAME.to_string(),
            dependencies: Some(vec![ZomeDependency {
                name: "transcript_integrity".into(),
            }]),
        }],
    };
    Ok(mr_bundle::Bundle::new(
        manifest,
        [(COORDINATOR_WASM_FILENAME.to_string(), wasm.into())],
    )
    .map_err(|e| format!("Could not assemble coordinator bundle: {}", e))?
    .into())
}

/// Hot-swap the coordinator on every provisioned cell of one app. Returns
/// the number of cells updated.
async fn apply_coordinator(
    admin_ws: &AdminWebsocket,
    app: &holochain_client::AppInfo,
    bundle: &CoordinatorBundle,
) -> Result<u32, String> {
    let mut updated = 0u32;
    for cells in app.cell_info.values() {
        for cell in cells {
            if let holochain_client::CellInfo::Provisioned(p) = cell {
                admin_ws
                    .update_coordinators(UpdateCoordinatorsPayload {
                        cell_id: p.cell_id.clone(),
                        source: CoordinatorSource::Bundle(Box::new(bundle.clone())),
                    })
                    .await
                    .map_err(|e| {
                        format!(
                            "update_coordinators failed for {}: {:?}",
                            app.installed_app_id, e
                        )
                    })?;
                updated += 1;
            }
        }
    }
    Ok(updated)
}

/// Hot-swap the coordinator zome on every installed transcript cell to the
/// version this build ships. Runs at startup after the conductor is up;
/// no-ops via the marker once a version has been applied. Cells installed
/// later get the coordinator from `install_transcript_app`. The integrity
/// zome (and so the DNA hash) is never touched - existing cells keep their
/// data and gain the new externs live.
pub async fn update_coordinators_sweep(
    admin_port: u16,
    resource_dir: &Path,
    app_data_dir: &Path,
) -> Result<u32, String> {
    let marker = app_data_dir.join(COORDINATOR_MARKER_FILENAME);
    let applied: u32 = std::fs::read_to_string(&marker)
        .ok()
        .and_then(|s| s.trim().parse().ok())
        .unwrap_or(1);
    if applied >= COORDINATOR_VERSION {
        return Ok(0);
    }

    let bundle = coordinator_bundle(resource_dir)?;

    let admin_ws = AdminWebsocket::connect(
        format!("localhost:{}", admin_port),
        Some("your-own-ai".to_string()),
    )
    .await
    .map_err(|e| format!("Failed to connect to admin WebSocket: {}", e))?;

    let apps = admin_ws
        .list_apps(None)
        .await
        .map_err(|e| format!("Failed to list apps: {}", e))?;

    let mut updated = 0u32;
    for app in apps.iter().filter(|a| a.installed_app_id.starts_with(APP_ID_PREFIX)) {
        updated += apply_coordinator(&admin_ws, app, &bundle).await?;
    }

    std::fs::write(&marker, COORDINATOR_VERSION.to_string()).map_err(|e| e.to_string())?;
    log::info!(
        "[dna] coordinator sweep: {} cell(s) hot-swapped to v{}",
        updated,
        COORDINATOR_VERSION
    );
    Ok(updated)
}

/// One-time fresh start (Phase A): uninstall pre-versioning transcript
/// apps ("transcript_<x>" without the "_v1_" tag). These are dev-era
/// installs — the plaintext DNA and the orphan generations from the
/// re-keying bug (holochain-integration.md Issue 9). Deliberate data
/// reset agreed pre-launch; the only content ever recorded was a
/// single April test conversation.
pub async fn cleanup_legacy_apps(admin_port: u16) -> Result<u32, String> {
    let admin_ws = AdminWebsocket::connect(
        format!("localhost:{}", admin_port),
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
        if id.starts_with("transcript_") && !id.starts_with(APP_ID_PREFIX) {
            match admin_ws.uninstall_app(id.clone(), false).await {
                Ok(_) => {
                    log::info!("Removed legacy transcript app: {}", id);
                    removed += 1;
                }
                Err(e) => log::warn!("Failed to remove legacy app {}: {}", id, e),
            }
        }
    }
    if removed > 0 {
        log::info!("Legacy cleanup removed {} pre-v1 transcript apps", removed);
    }
    Ok(removed)
}

/// Set up an authenticated AppWebsocket for a specific installed app.
pub async fn connect_app_websocket(
    admin_port: u16,
    app_port: u16,
    app_id: &str,
) -> Result<holochain_client::AppWebsocket, String> {
    let admin_ws = AdminWebsocket::connect(
        format!("localhost:{}", admin_port),
        Some("your-own-ai".to_string()),
    )
    .await
    .map_err(|e| format!("Failed to connect to admin WebSocket: {}", e))?;

    // Get cell info and authorize signing credentials.
    let app_info = admin_ws
        .list_apps(None)
        .await
        .map_err(|e| format!("Failed to list apps: {}", e))?;

    let app = app_info
        .iter()
        .find(|a| a.installed_app_id == app_id)
        .ok_or_else(|| format!("App {} not found", app_id))?;

    let cell_info = app
        .cell_info
        .get(ROLE_NAME)
        .ok_or_else(|| format!("Role {} not found in app {}", ROLE_NAME, app_id))?;

    let provisioned_cell = cell_info
        .iter()
        .find(|c| matches!(c, holochain_client::CellInfo::Provisioned(_)))
        .ok_or_else(|| format!("No provisioned cell found for {}", app_id))?;

    let cell_id = match provisioned_cell {
        holochain_client::CellInfo::Provisioned(p) => &p.cell_id,
        _ => unreachable!(),
    };

    // Authorize signing credentials, retrying two transient failures:
    // - CellDisabled: after a conductor restart/upgrade, cells can lag the
    //   app's Enabled status. Re-issue enable_app and retry — the same
    //   recovery pattern as ProofPoll/Vault's ensure_apps_enabled.
    // - "source chain head has moved": a concurrent commit raced this
    //   grant. Provisioning is serialized app-side now (HolochainManager
    //   provision_lock), but retry as belt-and-braces.
    let signer = ClientAgentSigner::default();
    let mut attempt = 0;
    let creds = loop {
        attempt += 1;
        match admin_ws
            .authorize_signing_credentials(AuthorizeSigningCredentialsPayload {
                cell_id: cell_id.clone(),
                functions: None,
            })
            .await
        {
            Ok(creds) => break creds,
            Err(e) if attempt < 6 => {
                let err = format!("{}", e);
                if err.contains("CellDisabled") {
                    log::warn!(
                        "Cell disabled for {} (attempt {}), re-enabling and retrying",
                        app_id, attempt
                    );
                    let _ = admin_ws.enable_app(app_id.to_string()).await;
                    tokio::time::sleep(std::time::Duration::from_secs(3)).await;
                } else if err.contains("head has moved") {
                    log::warn!(
                        "Source chain head moved for {} (attempt {}), retrying",
                        app_id, attempt
                    );
                    tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                } else {
                    return Err(format!("Failed to authorize signing credentials: {}", e));
                }
            }
            Err(e) => return Err(format!("Failed to authorize signing credentials: {}", e)),
        }
    };
    signer.add_credentials(cell_id.clone(), creds);

    let signer_arc: std::sync::Arc<dyn holochain_client::AgentSigner + Send + Sync> =
        signer.into();

    // Issue app auth token.
    let token = admin_ws
        .issue_app_auth_token(
            holochain_client::IssueAppAuthenticationTokenPayload::for_installed_app_id(
                app_id.to_string().into(),
            )
            .expiry_seconds(0)
            .single_use(false),
        )
        .await
        .map_err(|e| format!("Failed to issue app auth token: {}", e))?;

    // Connect app websocket.
    let app_ws = holochain_client::AppWebsocket::connect(
        format!("localhost:{}", app_port),
        token.token,
        signer_arc,
        Some("your-own-ai".to_string()),
    )
    .await
    .map_err(|e| format!("Failed to connect app WebSocket: {}", e))?;

    log::info!("App WebSocket connected for {}", app_id);
    Ok(app_ws)
}
