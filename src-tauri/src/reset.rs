//! Factory reset — wipe local AIs, transcripts, and memory, then relaunch so the
//! default AIs are re-seeded from the current persona templates.
//!
//! Holochain transcripts are append-only and can't be deleted record-by-record,
//! so a reset stops the conductor + lair and removes their local databases.
//!
//! PRESERVED (deliberately untouched): the Flowsta account (`flowsta-auth.json`),
//! downloaded models (`models/`), the transcript encryption key
//! (`transcript-recovery.json`), and app settings (`settings.json`). The Flowsta
//! account is independent of the local Holochain identity, so it stays connected
//! across the reset (the Vault link auto-reconciles on the next launch).

use std::sync::Arc;
use tauri::{AppHandle, Manager};

/// Full factory reset: remove all AIs, conversations, and learned memory, then
/// restart. The frontend clears its own localStorage and confirms with the user
/// before calling this.
#[tauri::command]
pub async fn reset_to_defaults(app: AppHandle) -> Result<(), String> {
    let data_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("Could not find the app data directory: {e}"))?;

    // Stop the conductor + lair so their database files aren't held open when we
    // delete them. We only have the PIDs (the Child handles are shared, not
    // mutable), so signal by PID — matching the app's exit handler.
    if let Some(hc) = app.try_state::<Arc<crate::commands_holochain::HolochainState>>() {
        if let Some(manager) = hc.manager.get() {
            for pid in [
                manager.handle.conductor_child.id(),
                manager.handle.lair_child.id(),
            ] {
                crate::process_ext::stop_pid(pid);
            }
            log::info!("[reset] signalled conductor + lair to stop");
        }
    }
    // Give the OS a moment to reap the processes and release their file handles.
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;

    // Wipe local AIs, transcripts, memory, thumbnails, and UI sentinels.
    for dir in ["conductor", "lair", "thumbnails"] {
        let p = data_dir.join(dir);
        if p.exists() {
            if let Err(e) = std::fs::remove_dir_all(&p) {
                log::warn!("[reset] could not remove {dir}/: {e}");
            }
        }
    }
    for file in ["ai-data.json", "memory-facts.enc", "gpu-safety.json",
                 crate::vault_escrow::SYNC_STATE_FILE] {
        let _ = std::fs::remove_file(data_dir.join(file));
    }
    // The Vault may hold the only copy of the conversations this reset just
    // wiped. Without this marker, the first post-reset chat would arm a
    // backup that rewrites the Vault snapshot down to one conversation -
    // the same silent mirror-wipe the escrow guards exist to prevent. The
    // marker suspends automatic backups until the user either restores from
    // Vault or the Vault slot is empty; the Flowsta Account panel shows it.
    if let Err(e) = std::fs::write(
        data_dir.join(crate::vault_escrow::RESTORE_PENDING_FILE),
        b"",
    ) {
        log::warn!("[reset] could not write restore-pending marker: {e}");
    }
    // Per-AI episodic memory caches: transcript-emb-<id>.enc
    if let Ok(entries) = std::fs::read_dir(&data_dir) {
        for entry in entries.flatten() {
            if let Some(name) = entry.file_name().to_str() {
                if name.starts_with("transcript-emb-") && name.ends_with(".enc") {
                    let _ = std::fs::remove_file(entry.path());
                }
            }
        }
    }
    log::info!("[reset] local data wiped — relaunching to re-seed defaults");

    // Production: relaunch into a fresh app. The frontend is bundled into the
    // binary (frontendDist), so restart loads cleanly and first-run seeding
    // re-creates the default AIs; the Flowsta account + models are untouched.
    #[cfg(not(debug_assertions))]
    {
        app.restart()
    }

    // Dev: the webview is served by the external Vite dev server (devUrl), which
    // doesn't survive a programmatic restart — relaunching would land on a blank
    // "connection refused" screen. So exit cleanly instead and let the developer
    // re-run `npm run tauri dev` (the wipe has already completed).
    #[cfg(debug_assertions)]
    {
        log::info!("[reset] dev build — exiting; re-run `npm run tauri dev` to relaunch fresh");
        app.exit(0);
        Ok(())
    }
}
