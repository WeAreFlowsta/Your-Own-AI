//! The machine check every background job asks, so background work never
//! takes more of the person's computer than it can spare.
//!
//! It reads the WHOLE machine, other programs included: how busy the
//! processor is with programs other than ours, whether memory is short (the
//! give-back watch in `llm.rs`), whether the computer runs on battery, and
//! how long the person has been away from the keyboard and mouse anywhere on
//! the computer. One reading every 10 s, shared by every caller.
//!
//! - **Light** work (syncs, small backups, checks) runs unless memory is short.
//! - **Heavy** work (dreaming, folder indexing, re-embedding, the import
//!   learner, file hashing, document cards) runs only when the machine is
//!   calm: other programs under 30 % of the processor over two minutes,
//!   memory not short, mains power, and the person away for three minutes.
//!   A heavy job asks again between its units and stops at the next one
//!   when the answer changes.
//!
//! Settings › "Do background work only when my computer is idle" (on by
//! default). Off: heavy work waits only for our own app to be idle and the
//! computer to be on mains power, as before.

use std::collections::BTreeMap;
use std::sync::Mutex;
use std::time::Duration;
use tauri::AppHandle;

/// Other programs' share of the whole processor above which heavy work waits.
const HEAVY_OTHERS_CPU_PCT: f32 = 30.0;
/// How long the person must have been away before heavy work starts.
const HEAVY_AWAY_SECS: u64 = 180;
/// The window the processor figure is averaged over (12 readings, 10 s apart).
const CPU_WINDOW: usize = 12;
const TICK: Duration = Duration::from_secs(10);
/// The old rule, when the setting is off: our own app idle this long.
const OWN_IDLE_SECS: i64 = 10 * 60;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Work {
    /// Syncs, small backups, checks: runs unless memory is short.
    Light,
    /// Minutes of processor, graphics or disk: runs only on a calm machine.
    Heavy,
}

/// One reading of the machine.
#[derive(Clone, Debug, Default, PartialEq, serde::Serialize)]
pub struct Reading {
    /// Other programs' share of the whole processor, averaged over two
    /// minutes (0-100). None until the first two readings.
    pub others_cpu_pct: Option<f32>,
    /// The give-back watch's verdict (free memory or kernel pressure).
    pub memory_short: bool,
    pub on_battery: bool,
    /// Seconds since the person's last input anywhere on the computer.
    /// None where this system does not say (the check then falls back on our
    /// own app's idle).
    pub away_secs: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Verdict {
    Go,
    /// Wait, and why, in the words the Activity tray can show.
    Wait(&'static str),
}

/// The rule itself, apart from the readers so it can be tested.
pub fn decide(work: Work, r: &Reading, only_when_idle: bool, own_idle_secs: i64) -> Verdict {
    if r.memory_short {
        return Verdict::Wait("memory is short");
    }
    if work == Work::Light {
        return Verdict::Go;
    }
    if r.on_battery {
        return Verdict::Wait("on battery");
    }
    if !only_when_idle {
        return if own_idle_secs >= OWN_IDLE_SECS { Verdict::Go } else { Verdict::Wait("in use") };
    }
    if r.others_cpu_pct.is_some_and(|p| p >= HEAVY_OTHERS_CPU_PCT) {
        return Verdict::Wait("other programs are busy");
    }
    match r.away_secs {
        Some(s) if s < HEAVY_AWAY_SECS => Verdict::Wait("the computer is in use"),
        Some(_) => Verdict::Go,
        // Unknown on this system: our own app's idle stands in for it.
        None if own_idle_secs < HEAVY_AWAY_SECS as i64 => Verdict::Wait("the computer is in use"),
        None => Verdict::Go,
    }
}

static READING: Mutex<Option<Reading>> = Mutex::new(None);
/// Settings › "Do background work only when my computer is idle", read at
/// every tick so a job deep in any module can ask without an app handle.
static ONLY_WHEN_IDLE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);
/// Jobs held right now and why: what the Activity tray shows.
static HELD: Mutex<BTreeMap<String, &'static str>> = Mutex::new(BTreeMap::new());

/// The latest reading (a calm default until the first one: a job asked in
/// the first seconds of a session waits for the first tick instead).
pub fn reading() -> Option<Reading> {
    READING.lock().ok().and_then(|r| r.clone())
}

fn only_when_idle(app: &AppHandle) -> bool {
    use tauri_plugin_store::StoreExt;
    app.store("settings.json")
        .ok()
        .and_then(|s| s.get("backgroundOnlyWhenIdle"))
        .and_then(|v| v.as_bool())
        .unwrap_or(true)
}

/// May `job` run now? Records the answer so the Activity tray can say what
/// waits and why. Before the first reading, heavy work waits.
pub fn may_run(work: Work, job: &str) -> bool {
    let only = ONLY_WHEN_IDLE.load(std::sync::atomic::Ordering::Relaxed);
    let verdict = match reading() {
        Some(r) => decide(work, &r, only, crate::dreaming::own_idle_secs()),
        None if work == Work::Light => Verdict::Go,
        None => Verdict::Wait("checking the computer"),
    };
    let mut held = HELD.lock().unwrap_or_else(|e| e.into_inner());
    match verdict {
        Verdict::Go => {
            held.remove(job);
            true
        }
        Verdict::Wait(why) => {
            if held.insert(job.to_string(), why) != Some(why) {
                log::info!("[machine] {job} waits: {why}");
            }
            false
        }
    }
}

/// The same answer without recording the job as held: for upkeep the person
/// never sees (the 0.7 conductor's memory restart), which must not put a
/// line in the Activity tray.
pub fn would_run(work: Work) -> bool {
    let only = ONLY_WHEN_IDLE.load(std::sync::atomic::Ordering::Relaxed);
    match reading() {
        Some(r) => decide(work, &r, only, crate::dreaming::own_idle_secs()) == Verdict::Go,
        None => work == Work::Light,
    }
}

/// A job that finished or gave up is no longer held.
pub fn done(job: &str) {
    HELD.lock().unwrap_or_else(|e| e.into_inner()).remove(job);
}

/// The same question from the front end's own background jobs (the import
/// learner, document cards, memory from earlier conversations).
#[tauri::command]
pub fn machine_may_run(heavy: bool, job: String) -> bool {
    may_run(if heavy { Work::Heavy } else { Work::Light }, &job)
}

#[tauri::command]
pub fn machine_done(job: String) {
    done(&job);
}

#[derive(serde::Serialize)]
pub struct MachineStatus {
    pub reading: Option<Reading>,
    /// (job, why) for each job waiting for the computer.
    pub held: Vec<(String, String)>,
}

/// For the Activity tray: what waits for the computer, and the reading.
#[tauri::command]
pub fn machine_status() -> MachineStatus {
    let held = HELD.lock().unwrap_or_else(|e| e.into_inner()).iter().map(|(j, w)| (j.to_string(), w.to_string())).collect();
    MachineStatus { reading: reading(), held }
}

/// Start the reading loop (once, at launch).
pub fn start(app: &AppHandle) {
    // A session starts "in use": our own idle clock counts from launch, not
    // from the first chat (before, the app read as idle 30 min after launch
    // with the person reading or typing).
    crate::dreaming::note_activity();
    let app = app.clone();
    std::thread::Builder::new()
        .name("machine-check".into())
        .spawn(move || {
            let mut cpu = CpuSampler::new();
            let mut input = InputIdle::new();
            let mut battery_at: Option<std::time::Instant> = None;
            let mut battery = false;
            loop {
                let others = cpu.sample();
                // Battery spawns a program on macOS and Windows: once a minute.
                if battery_at.is_none_or(|t| t.elapsed() >= Duration::from_secs(60)) {
                    battery = crate::dreaming::on_battery();
                    battery_at = Some(std::time::Instant::now());
                }
                let r = Reading { others_cpu_pct: others, memory_short: crate::llm::machine_short_now(), on_battery: battery, away_secs: input.away_secs() };
                if let Ok(mut slot) = READING.lock() {
                    *slot = Some(r);
                }
                ONLY_WHEN_IDLE.store(only_when_idle(&app), std::sync::atomic::Ordering::Relaxed);
                std::thread::sleep(TICK);
            }
        })
        .ok();
}

/// Other programs' share of the whole processor: the machine's busy share
/// minus our own process tree's (the app, its engines, conductors and key
/// stores), averaged over `CPU_WINDOW` readings. Ours is left out so a heavy
/// job of ours never reads as "the machine is busy" and stops itself.
struct CpuSampler {
    sys: sysinfo::System,
    window: std::collections::VecDeque<f32>,
    primed: bool,
}

impl CpuSampler {
    fn new() -> Self {
        Self { sys: sysinfo::System::new(), window: Default::default(), primed: false }
    }

    fn sample(&mut self) -> Option<f32> {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate};
        self.sys.refresh_cpu_usage();
        self.sys.refresh_processes_specifics(ProcessesToUpdate::All, true, ProcessRefreshKind::nothing().with_cpu());
        if !self.primed {
            // The first refresh has no interval to measure over.
            self.primed = true;
            return None;
        }
        let cores = self.sys.cpus().len().max(1) as f32;
        let me = sysinfo::Pid::from_u32(std::process::id());
        let ours: f32 = self
            .sys
            .processes()
            .values()
            .filter(|p| p.pid() == me || descends_from(&self.sys, p, me))
            .map(|p| p.cpu_usage())
            .sum::<f32>()
            / cores;
        let others = (self.sys.global_cpu_usage() - ours).clamp(0.0, 100.0);
        self.window.push_back(others);
        while self.window.len() > CPU_WINDOW {
            self.window.pop_front();
        }
        Some(self.window.iter().sum::<f32>() / self.window.len() as f32)
    }
}

/// Whether `p` is a child, grandchild... of `root` (bounded: a cycle or a
/// deep tree stops the walk).
fn descends_from(sys: &sysinfo::System, p: &sysinfo::Process, root: sysinfo::Pid) -> bool {
    let mut cur = p.parent();
    for _ in 0..8 {
        match cur {
            Some(pid) if pid == root => return true,
            Some(pid) => cur = sys.process(pid).and_then(|q| q.parent()),
            None => return false,
        }
    }
    false
}

/// Seconds since the person's last keyboard or mouse input anywhere on the
/// computer, where the system says. Linux keeps one session-bus connection
/// across readings (made again after an error).
struct InputIdle {
    #[cfg(target_os = "linux")]
    bus: Option<zbus::blocking::Connection>,
}

impl InputIdle {
    fn new() -> Self {
        Self {
            #[cfg(target_os = "linux")]
            bus: None,
        }
    }

    fn away_secs(&mut self) -> Option<u64> {
        #[cfg(target_os = "linux")]
        {
            if self.bus.is_none() {
                self.bus = zbus::blocking::Connection::session().ok();
            }
            let got = self.bus.as_ref().and_then(linux_away_secs);
            if got.is_none() {
                self.bus = None;
            }
            got
        }
        #[cfg(not(target_os = "linux"))]
        {
            away_secs()
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn away_secs() -> Option<u64> {
    #[cfg(target_os = "macos")]
    {
        macos_away_secs()
    }
    #[cfg(target_os = "windows")]
    {
        windows_away_secs()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        None
    }
}

/// GNOME (Wayland or X11) answers on its idle monitor, in milliseconds; KDE
/// and others answer on the freedesktop screensaver interface, in seconds.
#[cfg(target_os = "linux")]
fn linux_away_secs(conn: &zbus::blocking::Connection) -> Option<u64> {
    if let Ok(reply) = conn.call_method(
        Some("org.gnome.Mutter.IdleMonitor"),
        "/org/gnome/Mutter/IdleMonitor/Core",
        Some("org.gnome.Mutter.IdleMonitor"),
        "GetIdletime",
        &(),
    ) {
        if let Ok(ms) = reply.body().deserialize::<u64>() {
            return Some(ms / 1000);
        }
    }
    let reply = conn
        .call_method(Some("org.freedesktop.ScreenSaver"), "/org/freedesktop/ScreenSaver", Some("org.freedesktop.ScreenSaver"), "GetSessionIdleTime", &())
        .ok()?;
    reply.body().deserialize::<u32>().ok().map(u64::from)
}

#[cfg(target_os = "macos")]
fn macos_away_secs() -> Option<u64> {
    #[link(name = "CoreGraphics", kind = "framework")]
    extern "C" {
        fn CGEventSourceSecondsSinceLastEventType(state: i32, event_type: u32) -> f64;
    }
    // kCGEventSourceStateHIDSystemState = 1, kCGAnyInputEventType = !0.
    // SAFETY: a pure query with plain arguments.
    let s = unsafe { CGEventSourceSecondsSinceLastEventType(1, u32::MAX) };
    (s.is_finite() && s >= 0.0).then_some(s as u64)
}

#[cfg(target_os = "windows")]
fn windows_away_secs() -> Option<u64> {
    use windows_sys::Win32::System::SystemInformation::GetTickCount;
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO};
    let mut info = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    // SAFETY: the struct is sized as the call requires.
    if unsafe { GetLastInputInfo(&mut info) } == 0 {
        return None;
    }
    // Both are milliseconds since boot, wrapping together at 49.7 days.
    let now = unsafe { GetTickCount() };
    Some(u64::from(now.wrapping_sub(info.dwTime)) / 1000)
}

#[cfg(test)]
mod tests {
    use super::{decide, Reading, Verdict, Work};

    fn calm() -> Reading {
        Reading { others_cpu_pct: Some(5.0), memory_short: false, on_battery: false, away_secs: Some(600) }
    }

    #[test]
    fn a_calm_machine_runs_heavy_work() {
        assert_eq!(decide(Work::Heavy, &calm(), true, 0), Verdict::Go);
    }

    #[test]
    fn other_programs_hold_heavy_work_but_not_light() {
        let r = Reading { others_cpu_pct: Some(45.0), ..calm() };
        assert_eq!(decide(Work::Heavy, &r, true, 3600), Verdict::Wait("other programs are busy"));
        assert_eq!(decide(Work::Light, &r, true, 0), Verdict::Go);
    }

    #[test]
    fn the_person_at_the_computer_holds_heavy_work_even_when_our_app_is_idle() {
        let r = Reading { away_secs: Some(20), ..calm() };
        assert_eq!(decide(Work::Heavy, &r, true, 3600), Verdict::Wait("the computer is in use"));
    }

    #[test]
    fn short_memory_holds_everything() {
        let r = Reading { memory_short: true, ..calm() };
        assert_eq!(decide(Work::Light, &r, true, 3600), Verdict::Wait("memory is short"));
        assert_eq!(decide(Work::Heavy, &r, false, 3600), Verdict::Wait("memory is short"));
    }

    #[test]
    fn battery_holds_heavy_work() {
        let r = Reading { on_battery: true, ..calm() };
        assert_eq!(decide(Work::Heavy, &r, true, 3600), Verdict::Wait("on battery"));
        assert_eq!(decide(Work::Light, &r, true, 0), Verdict::Go);
    }

    #[test]
    fn unknown_input_falls_back_on_our_own_idle() {
        let r = Reading { away_secs: None, ..calm() };
        assert_eq!(decide(Work::Heavy, &r, true, 30), Verdict::Wait("the computer is in use"));
        assert_eq!(decide(Work::Heavy, &r, true, 600), Verdict::Go);
    }

    #[test]
    fn with_the_setting_off_only_our_app_and_power_count() {
        let busy = Reading { others_cpu_pct: Some(90.0), away_secs: Some(1), ..calm() };
        assert_eq!(decide(Work::Heavy, &busy, false, 11 * 60), Verdict::Go);
        assert_eq!(decide(Work::Heavy, &busy, false, 60), Verdict::Wait("in use"));
    }

    /// This computer's own figures (run by hand: `cargo test --lib -- --ignored live_machine --nocapture`).
    #[test]
    #[ignore]
    fn live_machine_reads_this_computer() {
        let mut cpu = super::CpuSampler::new();
        let mut input = super::InputIdle::new();
        assert_eq!(cpu.sample(), None, "the first reading has no interval yet");
        std::thread::sleep(std::time::Duration::from_secs(2));
        let others = cpu.sample();
        let away = input.away_secs();
        println!("others' processor {others:?} %, away {away:?} s, battery {}", crate::dreaming::on_battery());
        assert!(others.is_some_and(|p| (0.0..=100.0).contains(&p)));
        if cfg!(target_os = "linux") && std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.contains("GNOME")) {
            assert!(away.is_some(), "GNOME's idle monitor answers");
        }
    }

    /// Another program busy on half the processor holds heavy work; the same
    /// load inside our own process tree does not (run by hand:
    /// `cargo test --lib -- --ignored live_machine_tells --nocapture`).
    #[cfg(unix)]
    #[test]
    #[ignore]
    fn live_machine_tells_other_programs_from_ours() {
        let cores = std::thread::available_parallelism().map(|n| n.get()).unwrap_or(4);
        let n = cores.div_ceil(2) + 1;
        let read = |s: &mut super::CpuSampler| {
            let mut last = None;
            for _ in 0..4 {
                std::thread::sleep(std::time::Duration::from_secs(2));
                last = s.sample();
            }
            last.unwrap_or(0.0)
        };
        // Detached (the shell exits, the loops are adopted by init): another program.
        let _ = std::process::Command::new("bash").arg("-c").arg(format!("for i in $(seq {n}); do (exec -a yoai-test-hog bash -c 'while :; do :; done' &) ; done")).status();
        let mut s = super::CpuSampler::new();
        s.sample();
        let others = read(&mut s);
        // Anchored: only the loops' own command lines start with the name.
        let _ = std::process::Command::new("pkill").args(["-f", "^yoai-test-hog"]).status();
        println!("other programs busy on {n} of {cores} threads: others {others:.1} %");
        assert!(others >= super::HEAVY_OTHERS_CPU_PCT, "another program's load counts ({others:.1} %)");
        let r = Reading { others_cpu_pct: Some(others), ..calm() };
        assert_eq!(decide(Work::Heavy, &r, true, 3600), Verdict::Wait("other programs are busy"));
        // The same load as our own children: left out.
        let mut ours: Vec<std::process::Child> = (0..n).map(|_| std::process::Command::new("sh").arg("-c").arg("while :; do :; done").spawn().expect("a loop")).collect();
        let mut s = super::CpuSampler::new();
        s.sample();
        let others = read(&mut s);
        for c in ours.iter_mut() {
            let _ = c.kill();
        }
        println!("our own children busy on {n} of {cores} threads: others {others:.1} %");
        assert!(others < super::HEAVY_OTHERS_CPU_PCT, "our own load is left out ({others:.1} %)");
    }

    #[test]
    fn the_processor_figure_starts_unknown_and_does_not_hold() {
        let r = Reading { others_cpu_pct: None, ..calm() };
        assert_eq!(decide(Work::Heavy, &r, true, 0), Verdict::Go);
    }
}
