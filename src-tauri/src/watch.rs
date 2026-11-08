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

/// The memory restart's line for this machine: 1 GiB, or 8 % of its memory
/// when that is less (holochain#6064 grows the 0.7 conductor with every idle
/// hour; 1 GiB is a lot on an 8 GB laptop, about 650 MB there).
fn memory_policy(total_bytes: u64) -> MemoryPolicy {
    let scaled = total_bytes / 100 * 8;
    MemoryPolicy { over_bytes: if total_bytes == 0 { MEMORY.over_bytes } else { MEMORY.over_bytes.min(scaled) }, ..MEMORY }
}

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
    let fresh_on_07 = crate::line_facts::fresh_starts_on_07(app, data_dir);
    let line_07 = crate::move_07::line_07_for_start(data_dir, &manager.recovery, resource_dir, fresh_on_07).await;
    match conductor::start_holochain(app.clone(), data_dir.to_path_buf(), line_07).await {
        // A 0.6 line beside 0.7 reconnects its agents when it joins.
        Ok(_) if conductor::leading() == Line::V06 => {
            if let Err(e) = manager.reconnect_after_restart().await {
                log::warn!("[watch] the agents did not all reconnect: {e}");
            }
        }
        Ok(_) => {}
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
        let mut last_06_attach: Option<Instant> = None;
        let total_memory = {
            let mut sys = sysinfo::System::new();
            sys.refresh_memory();
            sys.total_memory()
        };
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
            // The 0.6 line beside a session 0.7 leads: stopped once no device
            // needs it and nothing is left on it, started again when one
            // does (a device on 0.6 came back, or a wait resumed by itself).
            if conductor::leading() == Line::V07 && supervisor().session().is_some() {
                let d = crate::line_facts::decision(&app, &data_dir);
                let has_06 = supervisor().has(Line::V06);
                if d.decision.wanted() && !has_06 && last_06_attach.map(|t| t.elapsed() > Duration::from_secs(600)).unwrap_or(true) {
                    log::info!("[watch] the 0.6 line is wanted ({}): attaching it", d.why);
                    last_06_attach = Some(Instant::now());
                    if let Err(e) = conductor::attach_06(app.clone(), data_dir.clone()) {
                        log::warn!("[watch] the 0.6 line was not attached: {e}");
                    }
                } else if !d.decision.wanted() && has_06 {
                    crate::move_07::retire_06(&app, &data_dir).await;
                }
            }
            // A session that started offline uses its auth once it can.
            if conductor::started_offline() && conductor::rendezvous_answers().await {
                restart_session(&app, &data_dir, &resource_dir, "the rendezvous answers again").await;
                continue;
            }
            // The 0.7 conductor's memory.
            if let Some(m) = supervisor().conductor_pid(Line::V07).and_then(conductor_memory) {
                let quiet = flowsta_hc_lines::in_flight(Line::V07) == 0 && flowsta_hc_lines::in_flight(Line::V06) == 0;
                // While the person is away from the computer (machine.rs),
                // or at once when memory is short: the restart gives it back.
                let short = crate::machine::reading().is_some_and(|r| r.memory_short);
                let now_ok = short || crate::machine::would_run(crate::machine::Work::Heavy);
                if now_ok && memory_restart_due(&m, last_memory_restart.map(|t| t.elapsed()), quiet, &memory_policy(total_memory)) {
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

#[cfg(test)]
mod memory_policy_tests {
    use super::memory_policy;

    #[test]
    fn the_restart_line_scales_down_on_small_machines() {
        const GIB: u64 = 1 << 30;
        assert_eq!(memory_policy(32 * GIB).over_bytes, GIB, "1 GiB at most");
        assert_eq!(memory_policy(16 * GIB).over_bytes, GIB);
        let eight = memory_policy(8 * GIB).over_bytes;
        assert!(eight < GIB && eight > 600 << 20, "about 650 MB on 8 GB ({} MB)", eight >> 20);
        assert_eq!(memory_policy(0).over_bytes, GIB, "an unread total keeps the default");
    }
}
