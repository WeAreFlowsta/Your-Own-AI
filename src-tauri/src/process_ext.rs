//! Cross-platform helpers for spawning child processes that are tied to the
//! lifetime of the parent vault process and (on Windows) don't pop up a
//! console window.
//!
//! Without `tie_to_parent`, lair-keystore and the holochain conductor outlive
//! the vault when the vault is killed (SIGKILL, OOM, dev-mode reload, dpkg
//! upgrade, …) and hold the conductor admin-WS port, blocking the next
//! launch.
//!
//! Without `spawn_hidden`, on Windows every Vault launch flashes two terminal
//! windows (lair-keystore.exe + holochain.exe) - visually unprofessional and
//! confusing for end users.
//!
//! Linux: `prctl(PR_SET_PDEATHSIG, SIGTERM)`.
//! Windows: post-spawn `EnumWindows` + `ShowWindow(SW_HIDE)` for the new
//! process's top-level console windows.
//! macOS: no-op for now. Clean exits work via `RunEvent::Exit`; abnormal
//! terminations on macOS / Windows can still leak children. Job Objects
//! (Windows) and kqueue (macOS) remain TODO.
//!
//! ## Why post-spawn hide instead of `CREATE_NO_WINDOW`
//!
//! v0.5.0 set `CREATE_NO_WINDOW` (0x08000000) on Windows to suppress the
//! console windows. That flag does more than hide the window - it prevents
//! Windows from allocating a console handle at all. `holochain.exe` then
//! crashed with `0xc0000005` access violation in `MSVCP140.dll` during
//! signing-DNA WASM compilation, because LLVM/cranelift's stdio path
//! dereferences the (null) console handle. v0.5.1 reverted to default flags
//! to restore reliable installs but at the cost of visible terminals.
//!
//! v0.5.2's approach: spawn normally so the console is allocated and
//! handles work, then enumerate the new process's top-level windows and
//! `ShowWindow(SW_HIDE)` each one. There's a brief flash (~50ms) while we
//! find and hide the window, but no crashes. The polled hide thread retries
//! for up to 2 seconds, so even if the window appears late we still catch
//! it. To eliminate the flash entirely, a future change could spawn with
//! `CREATE_SUSPENDED` via raw `CreateProcessW`, hide the window, then
//! `ResumeThread` - much more invasive and not necessary for the MVP.

use std::io;
use std::process::{Child, Command};

pub trait CommandExt {
    /// Configure the child to be managed as a vault sidecar:
    /// - Linux: kernel sends `SIGTERM` when the parent dies.
    /// - Windows / macOS: no-op for now.
    fn tie_to_parent(&mut self) -> &mut Self;

    /// Spawn the child, then on Windows asynchronously hide its console
    /// window. On other platforms, identical to `spawn()`.
    fn spawn_hidden(&mut self) -> io::Result<Child>;
}

impl CommandExt for Command {
    #[cfg(target_os = "linux")]
    fn tie_to_parent(&mut self) -> &mut Self {
        use std::os::unix::process::CommandExt as _;
        // SAFETY: prctl with PR_SET_PDEATHSIG is async-signal-safe and only
        // touches the calling thread's signal disposition; safe to invoke
        // between fork and exec.
        unsafe {
            self.pre_exec(|| {
                if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGTERM, 0, 0, 0) == -1 {
                    let err = std::io::Error::last_os_error();
                    eprintln!("warning: prctl(PR_SET_PDEATHSIG) failed: {err}");
                }
                Ok(())
            });
        }
        self
    }

    #[cfg(not(target_os = "linux"))]
    fn tie_to_parent(&mut self) -> &mut Self {
        self
    }

    #[cfg(target_os = "windows")]
    fn spawn_hidden(&mut self) -> io::Result<Child> {
        let child = self.spawn()?;
        let pid = child.id();
        // Tie the sidecar to a kill-on-close Job Object so it dies when THIS
        // app process exits - clean quit, crash, or force-kill. Windows has no
        // PR_SET_PDEATHSIG equivalent, so without this the conductor/lair
        // children orphan on app close, leaving their console windows open and
        // their admin ports + lair sockets locked. For the Vault this matters
        // more than for client apps: the conductor watchdog restarts on
        // failure, so orphans multiply during instability and can crash-loop
        // the conductor.
        win_job::assign_to_kill_on_close_job(&child);
        log::info!("[hide] spawned child pid {pid}, dispatching async hide thread");
        windows_hide::hide_console_for_pid_async(pid);
        // Hide our sidecars' console-host windows (Windows Terminal / conhost)
        // by title, and log the result so we can confirm they're hidden.
        windows_hide::start_window_manager_once();
        Ok(child)
    }

    #[cfg(not(target_os = "windows"))]
    fn spawn_hidden(&mut self) -> io::Result<Child> {
        self.spawn()
    }
}

#[cfg(target_os = "windows")]
mod win_job {
    //! Kill-on-close Job Object: the Windows stand-in for Linux's
    //! `PR_SET_PDEATHSIG`. Every sidecar (`yourowai-holochain`,
    //! `yourowai-lair-keystore`, `llama-server`) is assigned to one process-wide job that has
    //! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. The app process owns the only
    //! handle to that job, so when it exits - gracefully, by crash, or by
    //! Task Manager - Windows closes the handle and terminates every process
    //! still in the job. No more orphaned conductors holding ports/sockets and
    //! leaving console windows open.
    use std::os::windows::io::AsRawHandle;
    use std::process::Child;
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    // Stored as isize so the HANDLE is Send + Sync inside the static. Created
    // once, lazily; reused for every sidecar.
    static JOB: OnceLock<isize> = OnceLock::new();

    fn job_handle() -> HANDLE {
        let h = *JOB.get_or_init(|| unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                log::error!("[job] CreateJobObjectW failed - sidecars won't auto-kill on exit");
                return 0;
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                log::error!("[job] SetInformationJobObject(KILL_ON_JOB_CLOSE) failed");
            }
            job as isize
        });
        h as HANDLE
    }

    pub(super) fn assign_to_kill_on_close_job(child: &Child) {
        let job = job_handle();
        if job.is_null() {
            return;
        }
        unsafe {
            if AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) == 0 {
                log::warn!(
                    "[job] failed to assign pid {} to kill-on-close job (orphan possible)",
                    child.id(),
                );
            }
        }
    }
}

#[cfg(target_os = "windows")]
mod windows_hide {
    //! Find any top-level windows owned by `pid` and hide them. Used to
    //! suppress the console windows that pop up when we spawn console-mode
    //! sidecars on Windows.
    //!
    //! ## Why always-hide on every poll
    //!
    //! The previous implementation exited early when it found a matching
    //! window that wasn't currently visible (`IsWindowVisible == 0`),
    //! reasoning the child was using a hidden IPC window we shouldn't
    //! flap. That was wrong - Windows console hosts (conhost) create
    //! their window with WS_VISIBLE off and toggle it on later, often
    //! after our few-poll budget had already elapsed. Result: terminal
    //! windows reliably appeared after we'd "given up".
    //!
    //! Now we poll for the full 3 s budget at 50 ms intervals and call
    //! `ShowWindow(SW_HIDE)` on every match every iteration regardless of
    //! current visibility. `SW_HIDE` is idempotent on already-hidden
    //! windows, so the worst case is a 50 ms visibility flicker between
    //! the OS toggling `WS_VISIBLE` on and our next poll catching it.
    //!
    //! We also log the window class on the first match per pid so we can
    //! verify we're finding the actual `ConsoleWindowClass` window vs.
    //! some unrelated internal window.
    use std::collections::HashMap;
    use std::time::{Duration, Instant};
    use windows_sys::Win32::Foundation::{BOOL, CloseHandle, HWND, LPARAM, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::Diagnostics::ToolHelp::{
        CreateToolhelp32Snapshot, Process32FirstW, Process32NextW, PROCESSENTRY32W,
        TH32CS_SNAPPROCESS,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        EnumWindows, GetClassNameW, GetWindowTextLengthW, GetWindowTextW, GetWindowThreadProcessId,
        IsWindowVisible, ShowWindow, SW_HIDE,
    };

    /// Window classes used by Windows' console host.
    ///
    /// - `ConsoleWindowClass`     - classic conhost window
    /// - `OpenConsoleWindow`      - Windows Terminal's underlying conhost
    /// - `PseudoConsoleWindow`    - ConPTY infrastructure window owned by the
    ///                              attached process (always invisible, but
    ///                              we still hide it for completeness)
    /// - `CASCADIA_HOSTING_WINDOW_CLASS` - Windows Terminal hosting frame
    fn is_console_class(class: &str) -> bool {
        matches!(
            class,
            "ConsoleWindowClass"
                | "OpenConsoleWindow"
                | "PseudoConsoleWindow"
                | "CASCADIA_HOSTING_WINDOW_CLASS"
        )
    }

    /// One-shot Toolhelp32 snapshot returning a `pid → parent_pid` map for
    /// every process currently running. Used by the hide thread to find
    /// conhost.exe children whose parent is our spawned binary.
    fn build_parent_map() -> HashMap<u32, u32> {
        let mut map: HashMap<u32, u32> = HashMap::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap.is_null() || snap == INVALID_HANDLE_VALUE {
                return map;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            if Process32FirstW(snap, &mut entry) != 0 {
                loop {
                    map.insert(entry.th32ProcessID, entry.th32ParentProcessID);
                    if Process32NextW(snap, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
        }
        map
    }

    /// Candidate window discovered during a single EnumWindows pass.
    /// Hide decisions happen outside the callback so we can consult the
    /// parent-PID map.
    #[derive(Clone)]
    struct Candidate {
        hwnd: usize,
        owner_pid: u32,
        class: String,
        was_visible: bool,
    }

    struct EnumState {
        target_pid: u32,
        candidates: Vec<Candidate>,
    }

    struct PassResult {
        hides: u32,
        hides_while_visible: u32,
        first_class: Option<String>,
        first_via_parent_class: Option<String>,
    }

    fn get_window_class(hwnd: HWND) -> String {
        let mut buf = [0u16; 256];
        let len = unsafe { GetClassNameW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        if len > 0 {
            String::from_utf16_lossy(&buf[..len as usize])
        } else {
            String::new()
        }
    }

    // ── Diagnostics (logging only - never hides or changes anything) ────────
    //
    // A full, evidence-first dump of every console-related window: class,
    // title, visibility, owning process (name + pid) and that process's
    // parent. Sampled several times across the first ~15s because the
    // conductor's window appears seconds into startup. Lets us see EXACTLY
    // what the leaked windows are and who owns them, so the real hide can be
    // designed from fact rather than guessed.

    /// pid → (exe_name, parent_pid) for every running process.
    fn build_process_info() -> HashMap<u32, (String, u32)> {
        let mut map: HashMap<u32, (String, u32)> = HashMap::new();
        unsafe {
            let snap = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0);
            if snap.is_null() || snap == INVALID_HANDLE_VALUE {
                return map;
            }
            let mut entry: PROCESSENTRY32W = std::mem::zeroed();
            entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
            if Process32FirstW(snap, &mut entry) != 0 {
                loop {
                    let end = entry
                        .szExeFile
                        .iter()
                        .position(|&c| c == 0)
                        .unwrap_or(entry.szExeFile.len());
                    let name = String::from_utf16_lossy(&entry.szExeFile[..end]);
                    map.insert(entry.th32ProcessID, (name, entry.th32ParentProcessID));
                    if Process32NextW(snap, &mut entry) == 0 {
                        break;
                    }
                }
            }
            CloseHandle(snap);
        }
        map
    }

    fn get_window_text(hwnd: HWND) -> String {
        unsafe {
            let len = GetWindowTextLengthW(hwnd);
            if len <= 0 {
                return String::new();
            }
            let mut buf = vec![0u16; len as usize + 1];
            let n = GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32);
            String::from_utf16_lossy(&buf[..n.max(0) as usize])
        }
    }

    unsafe extern "system" fn collect_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let v = unsafe { &mut *(lparam as *mut Vec<usize>) };
        v.push(hwnd as usize);
        1
    }

    /// Dump every console-related window (console-class, OR owned by a process
    /// whose name contains holochain/lair, OR conhost.exe) with full
    /// attribution, so we can see what is leaking and how it's owned.
    fn dump_console_windows(tag: &str) {
        let procs = build_process_info();
        let mut hwnds: Vec<usize> = Vec::new();
        unsafe {
            EnumWindows(Some(collect_proc), &mut hwnds as *mut Vec<usize> as LPARAM);
        }
        let mut count = 0u32;
        for h in hwnds {
            let hwnd = h as HWND;
            let class = get_window_class(hwnd);
            let mut pid: u32 = 0;
            unsafe {
                GetWindowThreadProcessId(hwnd, &mut pid);
            }
            let (pname, ppid) = procs
                .get(&pid)
                .cloned()
                .unwrap_or_else(|| ("?".to_string(), 0));
            let lname = pname.to_ascii_lowercase();
            let relevant = is_console_class(&class)
                || lname.contains("holochain")
                || lname.contains("lair")
                || lname == "conhost.exe";
            if !relevant {
                continue;
            }
            let visible = unsafe { IsWindowVisible(hwnd) } != 0;
            let title = get_window_text(hwnd);
            let gpname = procs
                .get(&ppid)
                .map(|(n, _)| n.clone())
                .unwrap_or_else(|| "?".to_string());
            log::info!(
                "[windiag {tag}] class='{class}' visible={visible} owner={pname}(pid {pid}) parent={gpname}(pid {ppid}) title='{title}'",
            );
            count += 1;
        }
        log::info!("[windiag {tag}] {count} console-related window(s) total");
    }

    /// Substrings (lower-case) identifying THIS app's sidecar host windows.
    /// On Windows 11 with Windows Terminal as the default terminal, our console
    /// sidecars are hosted by `WindowsTerminal.exe` in a visible
    /// `CASCADIA_HOSTING_WINDOW_CLASS` window whose title is the full path to
    /// the sidecar binary - so the title (not the owning process) is how we
    /// find them. The classic `conhost` window titles the same way.
    const SIDECAR_TITLE_MARKERS: &[&str] = &["yourowai-holochain", "yourowai-lair-keystore", "llama-server"];

    /// Hide every console-host window whose title names one of our sidecars,
    /// regardless of which process owns it (it's usually `WindowsTerminal.exe`,
    /// not us). `ShowWindow` works cross-process. Returns how many we hid.
    fn hide_sidecar_terminals() -> u32 {
        let mut hwnds: Vec<usize> = Vec::new();
        unsafe {
            EnumWindows(Some(collect_proc), &mut hwnds as *mut Vec<usize> as LPARAM);
        }
        let mut hidden = 0u32;
        for h in hwnds {
            let hwnd = h as HWND;
            let class = get_window_class(hwnd);
            if !is_console_class(&class) {
                continue;
            }
            let title = get_window_text(hwnd).to_ascii_lowercase();
            if SIDECAR_TITLE_MARKERS.iter().any(|m| title.contains(m)) {
                unsafe {
                    ShowWindow(hwnd, SW_HIDE);
                }
                hidden += 1;
            }
        }
        hidden
    }

    /// One process-wide background thread (runs once): hides our sidecars'
    /// console-host windows by title. Frequent during startup to catch windows
    /// as they appear, then a slow heartbeat forever to re-hide anything the
    /// terminal host ever re-shows. Logs at INFO only when the hidden count
    /// changes; the full per-window dump is available at DEBUG level.
    pub(super) fn start_window_manager_once() {
        use std::sync::atomic::{AtomicBool, Ordering};
        static STARTED: AtomicBool = AtomicBool::new(false);
        if STARTED.swap(true, Ordering::SeqCst) {
            return;
        }
        std::thread::spawn(|| {
            let startup = [300u64, 300, 400, 500, 500, 1000, 1500, 2000, 3000, 5000, 5000];
            let mut i = 0usize;
            let mut last_hidden = u32::MAX;
            loop {
                let delta = startup.get(i).copied().unwrap_or(60_000);
                std::thread::sleep(Duration::from_millis(delta));
                i += 1;
                let hidden = hide_sidecar_terminals();
                if hidden != last_hidden {
                    log::info!("[windows] hid {hidden} sidecar console window(s)");
                    last_hidden = hidden;
                }
                if log::log_enabled!(log::Level::Debug) {
                    dump_console_windows("debug");
                }
            }
        });
    }

    /// EnumWindows callback. Collects every window whose owning PID matches
    /// our target *or* whose class is one of the known console classes.
    /// Hide decisions are made outside the callback (we consult the parent-PID
    /// map there).
    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let state = unsafe { &mut *(lparam as *mut EnumState) };
        let mut wnd_pid: u32 = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut wnd_pid);
        }

        let direct = wnd_pid == state.target_pid;
        // Cheap gate: only fetch the class for non-direct candidates, where
        // we need it to recognise console-host windows. Direct matches we
        // hide regardless of class.
        let class = if direct {
            String::new()
        } else {
            get_window_class(hwnd)
        };
        let console_class = !direct && is_console_class(&class);

        if direct || console_class {
            let was_visible = unsafe { IsWindowVisible(hwnd) } != 0;
            // Capture the direct-match class lazily - we only need it for
            // the first one we see (for diagnostic logging).
            let class_str = if direct { get_window_class(hwnd) } else { class };
            state.candidates.push(Candidate {
                hwnd: hwnd as usize,
                owner_pid: wnd_pid,
                class: class_str,
                was_visible,
            });
        }
        // Continue enumeration - multiple matches per process are possible.
        1
    }

    /// One pass: enumerate windows, then hide every direct-match window plus
    /// every console-class window whose owning process's parent is our target.
    fn try_hide_once(target_pid: u32, parent_map: &HashMap<u32, u32>) -> PassResult {
        let mut state = EnumState {
            target_pid,
            candidates: Vec::new(),
        };
        unsafe {
            EnumWindows(
                Some(enum_proc),
                &mut state as *mut EnumState as LPARAM,
            );
        }

        let mut hides: u32 = 0;
        let mut hides_while_visible: u32 = 0;
        let mut first_class: Option<String> = None;
        let mut first_via_parent_class: Option<String> = None;

        for c in &state.candidates {
            let direct = c.owner_pid == target_pid;
            let via_parent = !direct
                && parent_map.get(&c.owner_pid) == Some(&target_pid)
                && is_console_class(&c.class);

            if !(direct || via_parent) {
                continue;
            }

            if direct && first_class.is_none() {
                first_class = Some(c.class.clone());
            }
            if via_parent && first_via_parent_class.is_none() {
                first_via_parent_class = Some(c.class.clone());
            }

            unsafe {
                ShowWindow(c.hwnd as HWND, SW_HIDE);
            }
            hides += 1;
            if c.was_visible {
                hides_while_visible += 1;
            }
        }

        PassResult {
            hides,
            hides_while_visible,
            first_class,
            first_via_parent_class,
        }
    }

    pub(super) fn hide_console_for_pid_async(pid: u32) {
        std::thread::spawn(move || {
            let started = Instant::now();
            let deadline = started + Duration::from_secs(3);
            log::info!("[hide:{pid}] thread started");
            let mut iter: u32 = 0;
            let mut total_hides: u32 = 0;
            let mut total_visible_hides: u32 = 0;
            let mut logged_direct_class = false;
            let mut logged_via_parent_class = false;
            while Instant::now() < deadline {
                iter += 1;
                // Re-snapshot the parent-PID map every iteration. Conhost can
                // be spawned with a delay after our spawn returns, so an
                // earlier snapshot might miss it.
                let parent_map = build_parent_map();
                let pass = try_hide_once(pid, &parent_map);
                total_hides += pass.hides;
                total_visible_hides += pass.hides_while_visible;

                if !logged_direct_class {
                    if let Some(class) = pass.first_class.as_ref() {
                        log::info!(
                            "[hide:{pid}] first direct-match class: '{}' (iter {}, {}ms)",
                            class,
                            iter,
                            started.elapsed().as_millis(),
                        );
                        logged_direct_class = true;
                    }
                }
                if !logged_via_parent_class {
                    if let Some(class) = pass.first_via_parent_class.as_ref() {
                        log::info!(
                            "[hide:{pid}] first conhost-child match class: '{}' (iter {}, {}ms)",
                            class,
                            iter,
                            started.elapsed().as_millis(),
                        );
                        logged_via_parent_class = true;
                    }
                }
                if pass.hides_while_visible > 0 {
                    log::info!(
                        "[hide:{pid}] hid {} visible window(s) on iter {} ({}ms elapsed)",
                        pass.hides_while_visible,
                        iter,
                        started.elapsed().as_millis(),
                    );
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            log::info!(
                "[hide:{pid}] thread exited after {}ms ({} iters, {} hide calls, {} hides-while-visible, direct={}, via-parent={})",
                started.elapsed().as_millis(),
                iter,
                total_hides,
                total_visible_hides,
                logged_direct_class,
                logged_via_parent_class,
            );
        });
    }
}
