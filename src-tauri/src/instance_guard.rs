//! Single-instance enforcement that does not trust the platform plugin.
//!
//! tauri-plugin-single-instance's Windows implementation has a hole: the
//! losing process only exits when it can FIND the winner's hidden message
//! window. Launch two copies close together (a double-click while the first
//! is still initializing) or leave a hung instance whose window thread is
//! gone, and FindWindowW returns null - the "loser" then silently continues
//! as a full second instance. Two instances fight over ports 11435/8080/4477
//! and collide installing the same happ file ("access denied" every 2s);
//! seen live on a 4060 running 0.3.0-beta.1.
//!
//! The authoritative lock is therefore the inference port itself: exactly
//! one process can bind 11435, the OS releases it on process death (clean or
//! not), and a healthy owner identifies itself over /health. Holding the
//! lock also proves that any process still running from our install or
//! engines directories is an orphan from a dead session - so we sweep those
//! before starting our own sidecars (a leftover conductor on 4477 would
//! otherwise be adopted by the readiness probe and poison provisioning).

use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::time::Duration;

use tauri::{AppHandle, Manager};

const PROBE_TIMEOUT: Duration = Duration::from_millis(1500);

/// Claim the instance lock (the bound inference-server listener), or exit.
///
/// Must run before ANY subsystem spawns. Returns the listener to hand to
/// `inference_server::spawn` - or `None` in the one non-fatal case where a
/// program that is not ours squats on the port (the app then runs with the
/// external-apps API disabled rather than not at all).
pub fn acquire_or_exit(app: &AppHandle) -> Option<TcpListener> {
    let lan = crate::inference_server::lan_config(app).enabled;
    let addr = if lan {
        format!("0.0.0.0:{}", crate::inference_server::PORT)
    } else {
        format!("127.0.0.1:{}", crate::inference_server::PORT)
    };

    match TcpListener::bind(&addr) {
        Ok(l) => {
            sweep_orphans(app, false);
            return Some(l);
        }
        Err(e) => log::warn!("[instance] port {addr} taken at launch: {e}"),
    }

    // The port is taken. A holder that answers our health probe is a live
    // instance of this app - the user launched a second copy and the plugin
    // missed it. Tell them and bow out.
    if other_instance_is_healthy() {
        log::error!("[instance] another running instance answered the health probe - exiting");
        show_already_running(app);
        std::process::exit(0);
    }

    // The holder is not answering: a zombie session (hung window-less app,
    // crashed run's leftovers). Clear it and take over.
    log::warn!("[instance] port holder did not answer /health - clearing zombie session");
    sweep_orphans(app, true);
    for _ in 0..10 {
        std::thread::sleep(Duration::from_millis(500));
        if let Ok(l) = TcpListener::bind(&addr) {
            log::info!("[instance] zombie cleared, port {addr} acquired");
            return Some(l);
        }
    }

    // Still held after the sweep: some unrelated program owns the port.
    log::error!(
        "[instance] port {addr} is held by another program - external app connections disabled this session"
    );
    None
}

/// True if the port holder answers /health AND identifies as this app -
/// a plain web server on our port must not read as a second instance.
fn other_instance_is_healthy() -> bool {
    let addr = SocketAddr::from(([127, 0, 0, 1], crate::inference_server::PORT));
    let Ok(mut stream) = TcpStream::connect_timeout(&addr, PROBE_TIMEOUT) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(PROBE_TIMEOUT));
    let _ = stream.set_write_timeout(Some(PROBE_TIMEOUT));
    if stream
        .write_all(b"GET /health HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n")
        .is_err()
    {
        return false;
    }
    let mut buf = String::new();
    let _ = stream.take(4096).read_to_string(&mut buf);
    buf.contains("your-own-ai")
}

/// The bundled sidecar binary names (tauri.conf.json externalBin), matched
/// without extension. Deliberately a NAME list and not "anything in the exe
/// dir": on a Linux .deb the exe dir is /usr/bin, and a directory-only match
/// there would kill unrelated system processes.
const SIDECAR_NAMES: &[&str] = &["llama-server", "yourowai-holochain", "yourowai-lair-keystore"];

/// Kill leftover processes from a dead session. Only runs once the caller
/// holds (or is taking) the instance lock, which is what makes every match
/// an orphan by definition. Repeats while it finds something: kills take a
/// moment to land, and a launch that died mid-start may still have been
/// spawning its sidecars during the first pass (stacked half-launches left
/// several behind that a single pass missed).
fn sweep_orphans(app: &AppHandle, include_same_exe: bool) {
    for pass in 1..=3 {
        let killed = sweep_orphans_once(app, include_same_exe);
        if killed == 0 {
            break;
        }
        log::info!("[instance] sweep pass {pass}: cleared {killed} orphan process(es)");
        std::thread::sleep(Duration::from_millis(700));
    }
}

/// One sweep pass. A process is an orphan when:
/// - `same_exe`: another copy of our own executable (only when taking over
///   from a zombie - a healthy second launch exits itself via the probe).
/// - release builds: a bundled sidecar (by name, next to the exe) or
///   anything under the downloaded engines dir (that dir is only ours).
///   Skipped in dev, where sidecars run from paths a dev controls.
/// - release builds: a sidecar-named process whose PARENT is a copy of our
///   executable that is not this process (a dead or dying earlier launch),
///   or - for the two binaries that carry our own name - whose parent is
///   gone. This is the case the path rules miss: the executable path of a
///   process mid-teardown or from another session can be unreadable.
fn sweep_orphans_once(app: &AppHandle, include_same_exe: bool) -> u32 {
    use sysinfo::{ProcessRefreshKind, RefreshKind, System, UpdateKind};

    let me = std::process::id();
    let my_exe = std::env::current_exe().ok();
    let my_exe_name = my_exe
        .as_ref()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_lowercase());
    let exe_dir = my_exe
        .as_ref()
        .and_then(|p| p.parent().map(|d| d.to_path_buf()));
    let engines_dir = app.path().app_data_dir().ok().map(|d| d.join("engines"));

    let sys = System::new_with_specifics(
        RefreshKind::nothing()
            .with_processes(ProcessRefreshKind::nothing().with_exe(UpdateKind::Always)),
    );
    let procs = sys.processes();
    // A process's name without the Windows extension, lower-case.
    let stem_of = |name: &std::ffi::OsStr| -> String {
        let n = name.to_string_lossy().to_lowercase();
        n.strip_suffix(".exe").map(str::to_string).unwrap_or(n)
    };
    let is_our_app_name = |pid: &sysinfo::Pid| -> bool {
        procs
            .get(pid)
            .map(|p| Some(p.name().to_string_lossy().to_lowercase()) == my_exe_name)
            .unwrap_or(false)
    };

    let mut killed = 0u32;
    let mut unreadable = 0u32;
    for (pid, proc_) in procs {
        if pid.as_u32() == me {
            continue;
        }
        let name_stem = stem_of(proc_.name());
        let sidecar_by_name = SIDECAR_NAMES.contains(&name_stem.as_str());
        // Parent state: our own exe (and not us) = a stacked launch's child;
        // missing = the launch that spawned it is gone.
        let parent = proc_.parent();
        let parent_is_other_launch = parent
            .map(|pp| pp.as_u32() != me && is_our_app_name(&pp))
            .unwrap_or(false);
        let parent_gone = parent.map(|pp| !procs.contains_key(&pp)).unwrap_or(true);
        let ours_by_name = name_stem.starts_with("yourowai-");

        let (same_exe, in_install, in_engines) = match proc_.exe() {
            Some(exe) => {
                let same_exe = my_exe.as_deref() == Some(exe);
                let sidecar_name = exe
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .map(|n| SIDECAR_NAMES.contains(&n))
                    .unwrap_or(false);
                let in_install = !cfg!(debug_assertions)
                    && sidecar_name
                    && exe_dir.as_deref().map(|d| exe.starts_with(d)).unwrap_or(false);
                let in_engines = !cfg!(debug_assertions)
                    && engines_dir
                        .as_deref()
                        .map(|d| exe.starts_with(d))
                        .unwrap_or(false);
                (same_exe, in_install, in_engines)
            }
            None => {
                if sidecar_by_name {
                    unreadable += 1;
                }
                (false, false, false)
            }
        };
        let stacked_sidecar = !cfg!(debug_assertions)
            && sidecar_by_name
            && (parent_is_other_launch || (ours_by_name && parent_gone));
        if (same_exe && include_same_exe) || in_install || in_engines || stacked_sidecar {
            log::warn!(
                "[instance] killing orphan process {} (pid {}{})",
                proc_
                    .exe()
                    .map(|e| e.display().to_string())
                    .unwrap_or_else(|| name_stem.clone()),
                pid,
                if stacked_sidecar { ", from an earlier launch" } else { "" }
            );
            proc_.kill();
            killed += 1;
        }
    }
    if unreadable > 0 {
        log::info!(
            "[instance] {unreadable} sidecar-named process(es) had no readable path; judged by parent"
        );
    }
    killed
}

fn show_already_running(app: &AppHandle) {
    use tauri_plugin_dialog::DialogExt;
    app.dialog()
        .message(
            "Your Own AI is already running. Look for it in your taskbar or system tray.\n\n\
             If you can't find it, restart your computer and open the app again.",
        )
        .title("Your Own AI is already running")
        .blocking_show();
}
