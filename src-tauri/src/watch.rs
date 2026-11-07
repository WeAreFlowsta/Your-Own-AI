//! The lines' watch: every 30 s while the app runs, flowsta-hc-lines' own
//! decisions, the same as the Vault's:
//! - the primary (0.6) conductor died: the session starts again;
//! - the 0.7 conductor died: the 0.7 line is attached again;
//! - a session that started offline, without its rendezvous auth, starts
//!   again with it once the rendezvous answers;
//! - the 0.7 conductor holds more memory than it should (holochain#6064:
//!   every module it reloads stays in memory until it restarts): the 0.7
//!   line restarts once, when nothing is asked of it.
//!
//! At most three recoveries in 30 minutes; then it holds, and says so.

use crate::conductor::{self, supervisor, Line07};
use flowsta_hc_lines::watch::{conductor_memory, memory_restart_due, MemoryPolicy, WatchAction, WatchPolicy, Watchdog};
use flowsta_hc_lines::Line;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tauri::Manager;

const EVERY: Duration = Duration::from_secs(30);

const POLICY: WatchPolicy = WatchPolicy { current: Line::V07, limit: Some((3, Duration::from_secs(30 * 60))) };

/// The Vault's: over 1 GiB, after 20 minutes up, at most once in 6 hours.
const MEMORY: MemoryPolicy = MemoryPolicy { over_bytes: 1 << 30, min_uptime: Duration::from_secs(20 * 60), min_gap: Duration::from_secs(6 * 3600) };

static WATCHDOG: Watchdog = Watchdog::new();

/// What a 0.7 start needs, from the store as it stands.
fn line_07_now(app: &tauri::AppHandle, data_dir: &std::path::Path, resource_dir: &std::path::Path) -> Option<Line07> {
    let hc = app.state::<Arc<crate::commands_holochain::HolochainState>>();
    let manager = hc.get().ok()?;
    let data_key = manager.data_key().ok()?;
    let passphrase = conductor::passphrase_of_store_07(data_dir, &data_key).ok()?;
    Some(Line07 { passphrase, network_seed: manager.recovery.network_seed.clone(), resource_dir: resource_dir.to_path_buf() })
}

async fn restart_session(app: &tauri::AppHandle, data_dir: &std::path::Path, resource_dir: &std::path::Path, why: &str) {
    log::warn!("[watch] {why}: the lines start again");
    let hc = app.state::<Arc<crate::commands_holochain::HolochainState>>().inner().clone();
    let Ok(manager) = hc.get() else { return };
    // Both lines down first: the 0.7 key store is then refreshed from a quiet
    // 0.6 store, as at any start.
    let _ = tokio::task::spawn_blocking(|| supervisor().stop()).await;
    let line_07 = crate::move_07::line_07_for_start(data_dir, &manager.recovery, resource_dir).await;
    match conductor::start_holochain(app.clone(), data_dir.to_path_buf(), line_07).await {
        Ok(_) => {
            if let Err(e) = manager.reconnect_after_restart().await {
                log::warn!("[watch] the agents did not all reconnect: {e}");
            }
        }
        Err(e) => log::warn!("[watch] the lines did not start again: {e}"),
    }
}

/// Watch the lines for as long as the app runs (started once, after the
/// first start).
pub fn start(app: tauri::AppHandle, data_dir: PathBuf, resource_dir: PathBuf) {
    static STARTED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    if STARTED.swap(true, std::sync::atomic::Ordering::SeqCst) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let mut holding = false;
        let mut last_memory_restart: Option<Instant> = None;
        loop {
            tokio::time::sleep(EVERY).await;
            if conductor::is_closing() {
                return;
            }
            match WATCHDOG.action(supervisor(), &POLICY) {
                WatchAction::Nothing => holding = false,
                WatchAction::RestartSession => restart_session(&app, &data_dir, &resource_dir, "the 0.6 conductor is not running").await,
                WatchAction::Reattach(line) => {
                    log::warn!("[watch] the {line} conductor is not running: attaching it again");
                    supervisor().stop_line(line);
                    match line_07_now(&app, &data_dir, &resource_dir) {
                        Some(l07) => {
                            if let Err(e) = conductor::attach_07(app.clone(), data_dir.clone(), l07) {
                                log::warn!("[watch] {line} not attached: {e}");
                            }
                        }
                        None => log::warn!("[watch] the 0.7 key store's passphrase is not at hand: 0.7 waits for the next start"),
                    }
                }
                WatchAction::LetGo(line) => {
                    log::info!("[watch] the {line} conductor is not running and not needed this session: let go");
                    supervisor().stop_line(line);
                }
                WatchAction::Hold => {
                    if !holding {
                        log::error!("[watch] the lines were recovered 3 times in 30 minutes: holding until that eases");
                    }
                    holding = true;
                }
                #[allow(unreachable_patterns)]
                _ => {}
            }
            // A session that started offline uses its auth once it can.
            if conductor::started_offline() && conductor::rendezvous_answers().await {
                restart_session(&app, &data_dir, &resource_dir, "the rendezvous answers again").await;
                continue;
            }
            // The 0.7 conductor's memory.
            if let Some(m) = supervisor().conductor_pid(Line::V07).and_then(conductor_memory) {
                let quiet = flowsta_hc_lines::in_flight(Line::V07) == 0 && flowsta_hc_lines::in_flight(Line::V06) == 0;
                if memory_restart_due(&m, last_memory_restart.map(|t| t.elapsed()), quiet, &MEMORY) {
                    log::info!("[watch] the 0.7 conductor holds {} MB after {} min: restarting it once", m.resident_bytes >> 20, m.running.as_secs() / 60);
                    last_memory_restart = Some(Instant::now());
                    supervisor().stop_line(Line::V07);
                    if let Some(l07) = line_07_now(&app, &data_dir, &resource_dir) {
                        let _ = conductor::attach_07(app.clone(), data_dir.clone(), l07);
                    }
                }
            }
        }
    });
}
