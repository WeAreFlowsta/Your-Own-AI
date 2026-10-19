//! The one activity-tray notice that tells the person what syncing with
//! their other devices is doing (Eric 2026-10-16: "tell them what is
//! syncing, then it changes to the restart notice"). One event,
//! `device-sync`, with a stage; the front end keeps it as ONE tray row that
//! changes in place:
//!
//!   syncing   right after sign-in: the AIs, their pictures and the memory
//!             arrive from the Vault's copies (no restart)
//!   restart   they are here; the conversations need one restart (this
//!             device is on a network of its own - build-docs
//!             MULTI_DEVICE.md §5.1), with a Restart button
//!   bringing  after the restart: the conversations arrive in the background
//!   done      in sync
//!
//! A device with no other device sends nothing. Conflicts (conversations on
//! both sides) stay with Settings, where the person chooses.

use tauri::Emitter;

/// The other devices' names, joined for a title ("WhitedoveDM and MacBook").
pub(crate) async fn sibling_names(port: u16) -> Option<String> {
    let devices = crate::vault_escrow::sibling_devices(port).await.ok()?;
    if devices.is_empty() {
        return None;
    }
    let names: Vec<String> = devices
        .iter()
        .map(|d| d.name.clone().unwrap_or_else(|| "your other device".into()))
        .collect();
    Some(match names.len() {
        1 => names[0].clone(),
        2 => format!("{} and {}", names[0], names[1]),
        n => format!("{} and {} more", names[0], n - 1),
    })
}

pub(crate) fn emit(app: &tauri::AppHandle, stage: &str, devices: &str) {
    log::info!("[device-sync] {} ({})", stage, devices);
    let _ = app.emit("device-sync", serde_json::json!({ "stage": stage, "devices": devices }));
}

/// After an escrow sync: the notice for what it found.
pub(crate) fn report(app: &tauri::AppHandle, state: &str, devices: &str) {
    match state {
        "island_pending" | "island_deferred" => emit(app, "restart", devices),
        "synced" => emit(app, "done", devices),
        _ => {}
    }
}

/// Right after a sign-in: say what is syncing, take the AIs, pictures and
/// memory now (not at the next five-minute round), then either finish or
/// ask for the restart the conversations need.
pub(crate) async fn after_sign_in(app: tauri::AppHandle) {
    let Ok(port) = crate::vault_escrow::escrow_port(&app, true).await else { return };
    let Some(devices) = sibling_names(port).await else { return };
    emit(&app, "syncing", &devices);
    match crate::config_sync::sync_from_siblings(&app, true).await {
        Ok((u, a)) => log::info!("[device-sync] AIs from the other devices: {} updated, {} added", u, a),
        Err(e) => log::warn!("[device-sync] config round after sign-in: {}", e),
    }
    let status = crate::vault_escrow::vault_escrow_sync(app.clone()).await;
    report(&app, &status.state, &devices);
}
