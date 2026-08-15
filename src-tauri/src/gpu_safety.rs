//! GPU "safe mode" — crash-loop protection for GPU inference.
//!
//! Some setups (notably NVIDIA + Wayland + Vulkan compute) can HARD-HANG the
//! whole machine under GPU load, with no error and no recovery — the user's
//! system just freezes. They can't reach a setting to fix it. So we detect the
//! crash-loop and step down a LADDER automatically:
//!
//!   preferred engine (e.g. downloaded CUDA) → bundled GPU engine → CPU
//!
//!   - On launch, before GPU work, drop a `pending` sentinel; the spawn path
//!     records which backend the run used (`note_backend`).
//!   - On a CLEAN app exit, clear it.
//!   - If a launch finds the sentinel still set, the previous run never exited
//!     cleanly ⇒ it likely hard-crashed. Two such launches in a row step down
//!     one rung: a crashing CUDA engine falls back to the bundled engine
//!     (`cuda_disabled`), a crashing bundled engine falls back to CPU
//!     (`forced_cpu`). The frontend shows a notice with a retry action.
//!
//! Default is GPU; this is only the safety net. The dev/power-user override
//! `FLOWSTA_CPU_ONLY=1` short-circuits all of this (handled in llm.rs).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::OnceLock;
use tauri::{AppHandle, Manager};

const STATE_FILE: &str = "gpu-safety.json";
// Consecutive unclean GPU exits before stepping down a rung. macOS tolerates
// a longer streak: an unclean exit there is far more likely a force-quit than
// a driver crash (Metal on Apple Silicon doesn't take systems down the way
// desktop GPU drivers can), and a real M1 landed in false safe mode off two
// force-quits. A genuine repeat offender still ladders out.
const CRASH_THRESHOLD: u32 = if cfg!(target_os = "macos") { 4 } else { 2 };

#[derive(Serialize, Deserialize, Default)]
#[serde(default)]
struct State {
    /// A GPU run is in progress (set on launch, cleared on clean exit).
    /// Still set on the next launch ⇒ the previous run hard-crashed.
    pending: bool,
    /// Consecutive unclean GPU exits.
    crashes: u32,
    /// Safe mode engaged: stay on CPU until the user re-enables.
    forced_cpu: bool,
    /// True only on the launch a rung engaged (so the notice pops once).
    just_engaged: bool,
    /// Backend the pending run used ("cuda" | "bundled") — recorded by the
    /// spawn path so a crash steps down the RIGHT rung.
    backend: String,
    /// The CUDA rung tripped: stay on the bundled engine until retried.
    cuda_disabled: bool,
    /// The engine itself said this GPU cannot run models ("no kernel image",
    /// "does not support 16-bit storage", "Unsupported device") - a
    /// DETERMINISTIC verdict, unlike the crash ladder's statistics. Holds the
    /// reason ("vulkan-driver" | "cuda-arch") or "" when the device is fine.
    /// Cleared by gpu_retry (the user updated a driver and wants to try).
    device_unsupported: String,
}

#[derive(Serialize)]
pub struct SafeModeStatus {
    pub active: bool,
    pub just_engaged: bool,
    /// The CUDA engine was laddered out (bundled engine still runs the GPU).
    pub cuda_disabled: bool,
    /// Set when the engine reported the GPU cannot run models at all -
    /// "vulkan-driver" (a driver update may fix it) or "cuda-arch".
    pub device_unsupported: Option<String>,
}

// Evaluate the crash sentinel at most once per process — the model can be
// (re)loaded several times per session, and each reload asks for GPU args.
static SESSION_DECISION: OnceLock<bool> = OnceLock::new();

/// Production always; in debug only when FLOWSTA_GPU_SAFEMODE=1, so a dev's
/// Ctrl-C (an "unclean" exit) doesn't false-trip safe mode during work.
fn enabled() -> bool {
    !cfg!(debug_assertions)
        || std::env::var("FLOWSTA_GPU_SAFEMODE").map(|v| v != "0").unwrap_or(false)
}

fn state_path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(STATE_FILE))
}

fn load(app: &AppHandle) -> State {
    state_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save(app: &AppHandle, st: &State) {
    let Some(path) = state_path(app) else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string_pretty(st) {
        let _ = std::fs::write(path, json);
    }
}

/// Pure crash-loop decision: from the loaded state, return the next state to
/// persist and whether GPU is allowed this run. A crash streak on the CUDA
/// engine trips `cuda_disabled` (GPU stays allowed on the bundled engine);
/// a streak on the bundled engine trips `forced_cpu`. Kept separate from IO
/// so it can be unit-tested (the actual hang can't be).
fn decide(mut st: State) -> (State, bool) {
    st.just_engaged = false;
    let allow = if st.forced_cpu {
        false // already in safe mode from a prior session
    } else if st.pending {
        // Previous GPU run never exited cleanly — treat as a likely hard crash.
        st.crashes += 1;
        if st.crashes >= CRASH_THRESHOLD {
            if st.backend == "cuda" && !st.cuda_disabled {
                // Rung 1: the CUDA engine is out; the bundled engine still
                // gets the GPU. Streak restarts for the new rung.
                st.cuda_disabled = true;
                st.just_engaged = true;
                st.crashes = 0;
                st.pending = true;
                st.backend = "bundled".to_string();
                true
            } else {
                // Rung 2: the bundled engine crashed too — CPU only.
                st.forced_cpu = true;
                st.just_engaged = true;
                st.pending = false;
                false
            }
        } else {
            st.pending = true;
            true
        }
    } else {
        st.crashes = 0;
        st.pending = true;
        true
    };
    (st, allow)
}

/// Decide whether GPU inference is allowed this session. Records the sentinel so
/// a crash this run is detected next launch. Runs the evaluation once per
/// process; later calls return the same decision.
pub fn gpu_allowed(app: &AppHandle) -> bool {
    if !enabled() {
        return true;
    }
    if let Some(&decided) = SESSION_DECISION.get() {
        return decided;
    }

    let (st, decision) = decide(load(app));
    if st.crashes > 0 && !decision {
        log::warn!("[GPU] safe mode ENGAGED — forcing CPU after repeated unclean exits");
    } else if st.pending && st.crashes > 0 {
        log::warn!("[GPU] previous GPU run did not exit cleanly ({}/{})", st.crashes, CRASH_THRESHOLD);
    }
    save(app, &st);
    let _ = SESSION_DECISION.set(decision);
    decision
}

/// Clear the sentinel on a clean app exit so this run isn't counted as a crash.
pub fn mark_clean_exit(app: &AppHandle) {
    if !enabled() {
        return;
    }
    let mut st = load(app);
    if st.pending {
        st.pending = false;
        save(app, &st);
    }
}

/// Is the CUDA rung still available? False after a CUDA crash streak
/// laddered it out (until the user retries). Fresh read — a mid-session
/// engine download must take effect without a relaunch.
pub fn cuda_allowed(app: &AppHandle) -> bool {
    if !enabled() {
        return true;
    }
    !load(app).cuda_disabled
}

/// The spawn path records which backend this run's GPU work uses, so a
/// crash detected next launch steps down the right rung.
pub fn note_backend(app: &AppHandle, backend: &str) {
    if !enabled() {
        return;
    }
    let mut st = load(app);
    if st.backend != backend {
        st.backend = backend.to_string();
        save(app, &st);
    }
}

/// A GPU-backed llama-server crashed during a model load with no better
/// explanation (not OOM, not a file error, not a device verdict). Counts
/// toward the same ladder the launch sentinel feeds - a streak steps the
/// rung down exactly as if the whole app had died. This closes the gap
/// where a child process could crash on every load forever without the
/// ladder noticing (seen in the field: repeated engine crashes, ladder
/// still reading "CUDA laddered out: no").
pub fn note_gpu_child_crash(app: &AppHandle) {
    if !enabled() {
        return;
    }
    let mut st = load(app);
    st.crashes += 1;
    if st.crashes >= CRASH_THRESHOLD {
        if st.backend == "cuda" && !st.cuda_disabled {
            st.cuda_disabled = true;
            st.just_engaged = true;
            st.crashes = 0;
            st.backend = "bundled".to_string();
            log::warn!("[GPU] CUDA engine laddered out after repeated load crashes");
        } else {
            st.forced_cpu = true;
            st.just_engaged = true;
            log::warn!("[GPU] safe mode ENGAGED after repeated load crashes - CPU from next launch");
        }
    }
    save(app, &st);
}

/// The engine reported this GPU cannot run models. Persist the verdict so
/// every later spawn - this session and the next - goes straight to CPU
/// instead of re-crashing. Deterministic, so no threshold: one sighting is
/// the truth. NOT gated on `enabled()`: unlike the crash ladder (where a
/// dev's Ctrl-C looks like a crash), this only fires on the engine's own
/// explicit error text.
pub fn note_device_unsupported(app: &AppHandle, reason: &str) {
    let mut st = load(app);
    if st.device_unsupported != reason {
        st.device_unsupported = reason.to_string();
        save(app, &st);
        log::warn!("[GPU] device marked unsupported ({reason}) - models run on CPU until the user retries");
    }
}

/// Fresh read (deliberately NOT the once-per-session cached decision):
/// detection can happen mid-session, and the very next spawn must honor it.
pub fn device_unsupported(app: &AppHandle) -> Option<String> {
    let v = load(app).device_unsupported;
    (!v.is_empty()).then_some(v)
}

/// Frontend: is safe mode active, and did it just engage this launch?
#[tauri::command]
pub fn gpu_safe_mode_status(app: AppHandle) -> SafeModeStatus {
    let st = load(&app);
    SafeModeStatus {
        active: st.forced_cpu,
        just_engaged: st.just_engaged,
        cuda_disabled: st.cuda_disabled,
        device_unsupported: (!st.device_unsupported.is_empty())
            .then_some(st.device_unsupported),
    }
}

/// Frontend "Try GPU again": clear safe mode so the next launch uses the GPU.
#[tauri::command]
pub fn gpu_retry(app: AppHandle) {
    let st = State::default();
    save(&app, &st);
    log::info!("[GPU] safe mode cleared by user — GPU will be retried next launch");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_first_run_uses_gpu() {
        let (st, allow) = decide(State::default());
        assert!(allow, "fresh install should use GPU");
        assert!(st.pending, "sentinel set for this run");
        assert_eq!(st.crashes, 0);
        assert!(!st.forced_cpu);
    }

    #[test]
    fn one_unclean_exit_still_uses_gpu() {
        // last run set pending and never cleared it (one hard crash)
        let (st, allow) = decide(State { pending: true, crashes: 0, ..Default::default() });
        assert!(allow, "a single unclean exit shouldn't force CPU");
        assert_eq!(st.crashes, 1);
        assert!(!st.forced_cpu);
    }

    #[test]
    fn crash_streak_at_threshold_forces_cpu() {
        // crashes is threshold-relative so this stays true on macOS, where
        // CRASH_THRESHOLD is higher (force-quit tolerance).
        let (st, allow) =
            decide(State { pending: true, crashes: CRASH_THRESHOLD - 1, ..Default::default() });
        assert!(!allow, "crash at the threshold forces CPU");
        assert!(st.forced_cpu);
        assert!(st.just_engaged, "notice should pop this launch");
        assert!(!st.pending, "no GPU run this launch");
    }

    #[test]
    fn forced_cpu_stays_cpu() {
        let (st, allow) = decide(State { forced_cpu: true, ..Default::default() });
        assert!(!allow);
        assert!(st.forced_cpu);
        assert!(!st.just_engaged, "only pops the launch it engaged");
    }

    #[test]
    fn clean_exit_resets_crash_count() {
        // pending=false means last run exited cleanly
        let (st, allow) = decide(State { pending: false, crashes: 1, ..Default::default() });
        assert!(allow);
        assert_eq!(st.crashes, 0, "a clean run clears the crash streak");
        assert!(st.pending);
    }

    #[test]
    fn cuda_crash_streak_falls_back_to_bundled_gpu() {
        let (st, allow) = decide(State {
            pending: true,
            crashes: CRASH_THRESHOLD - 1,
            backend: "cuda".into(),
            ..Default::default()
        });
        assert!(allow, "GPU stays allowed - just not the CUDA engine");
        assert!(st.cuda_disabled);
        assert!(st.just_engaged, "notice should pop for the CUDA rung");
        assert_eq!(st.crashes, 0, "streak restarts on the new rung");
        assert_eq!(st.backend, "bundled");
        assert!(st.pending, "a bundled run starts this launch");
        assert!(!st.forced_cpu);
    }

    #[test]
    fn bundled_crash_streak_after_cuda_rung_forces_cpu() {
        let (st, allow) = decide(State {
            pending: true,
            crashes: CRASH_THRESHOLD - 1,
            backend: "bundled".into(),
            cuda_disabled: true,
            ..Default::default()
        });
        assert!(!allow, "second rung exhausted - CPU only");
        assert!(st.forced_cpu);
        assert!(st.cuda_disabled, "the CUDA rung stays out");
    }

    #[test]
    fn bundled_crash_streak_without_cuda_forces_cpu() {
        let (st, allow) = decide(State {
            pending: true,
            crashes: CRASH_THRESHOLD - 1,
            backend: "bundled".into(),
            ..Default::default()
        });
        assert!(!allow);
        assert!(st.forced_cpu);
        assert!(!st.cuda_disabled);
    }
}
