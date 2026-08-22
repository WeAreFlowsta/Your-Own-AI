//! Cross-platform helpers for spawning the sidecar processes (lair-keystore,
//! the holochain conductor) so that they are tied to the lifetime of the app
//! process and never show a console window.
//!
//! Without the lifetime tie, lair-keystore and the conductor outlive the
//! app when it is killed (SIGKILL, OOM, dev-mode reload, package upgrade,
//! crash) and hold the conductor admin-WS port, blocking the next launch.
//!
//! Linux: `prctl(PR_SET_PDEATHSIG, SIGTERM)`.
//! Windows: a kill-on-close Job Object (the app owns the only handle), and
//! the console window is CREATED hidden - see below.
//! macOS: no-op for now. Clean exits work via `RunEvent::Exit`; abnormal
//! terminations can still leak children. kqueue remains TODO.
//!
//! ## Windows: why a raw `CreateProcessW` with `SW_HIDE`
//!
//! The sidecars are console programs; the app is a GUI process, so each
//! spawn allocates a fresh console. Three ways to keep it off the screen:
//!
//! - `CREATE_NO_WINDOW` (0.5.0): the child gets no usable console handles at
//!   all. `holochain.exe` then died with an access violation in MSVCP140
//!   during WASM compilation - its stdio path dereferences the null handle.
//! - Post-spawn `ShowWindow(SW_HIDE)` polling (0.5.1): works, but the
//!   console exists visibly for a moment, and on machines where Windows
//!   Terminal is the default terminal the new console is handed off to WT -
//!   which, if the app dies mid-spawn, leaves the user a WT error dialog
//!   ("error 0x800700e8 when launching ...lair-keystore.exe") that reads
//!   like a lair failure and is not one. Seen on two installs.
//! - `STARTF_USESHOWWINDOW` + `SW_HIDE` in the `STARTUPINFO` (this module):
//!   the console is allocated normally - every handle valid - and its window
//!   is created hidden; the console host skips the terminal handoff for a
//!   window that starts hidden. No flash, no dialog, no null handles.
//!   `std::process::Command` cannot set `wShowWindow` on stable Rust, so the
//!   Windows spawn is a small `CreateProcessW` wrapper with its own pipes.
//!
//! The post-spawn hide threads are kept as a second line: they cost nothing
//! and log what they find, which is how the field cases were read.

use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::path::PathBuf;
use std::process::Command;

pub trait CommandExt {
    /// Configure the child to be managed as a sidecar:
    /// - Linux: kernel sends `SIGTERM` when the parent dies.
    /// - Windows / macOS: no-op (Windows uses the job object at spawn).
    fn tie_to_parent(&mut self) -> &mut Self;
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
}

/// A sidecar spawn: program + args + working dir, stdin always piped (the
/// passphrase goes down it), stdout/stderr to the given files or discarded.
/// `spawn` yields a [`SidecarChild`] with the same small surface on every
/// platform: `stdin`, `id`, `try_wait`, `wait`, `kill`.
pub struct SidecarCommand {
    program: PathBuf,
    args: Vec<OsString>,
    cwd: Option<PathBuf>,
    stdout: Option<File>,
    stderr: Option<File>,
}

impl SidecarCommand {
    pub fn new(program: impl Into<PathBuf>) -> Self {
        Self {
            program: program.into(),
            args: Vec::new(),
            cwd: None,
            stdout: None,
            stderr: None,
        }
    }

    pub fn arg(mut self, a: impl Into<OsString>) -> Self {
        self.args.push(a.into());
        self
    }

    pub fn current_dir(mut self, d: impl Into<PathBuf>) -> Self {
        self.cwd = Some(d.into());
        self
    }

    /// Where the child's stdout goes (a log file). Default: discarded.
    pub fn stdout(mut self, f: File) -> Self {
        self.stdout = Some(f);
        self
    }

    /// Where the child's stderr goes (a log file). Default: discarded.
    pub fn stderr(mut self, f: File) -> Self {
        self.stderr = Some(f);
        self
    }

    #[cfg(not(target_os = "windows"))]
    pub fn spawn(self) -> io::Result<SidecarChild> {
        use std::process::Stdio;
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args)
            .stdin(Stdio::piped())
            .stdout(self.stdout.map(Stdio::from).unwrap_or_else(Stdio::null))
            .stderr(self.stderr.map(Stdio::from).unwrap_or_else(Stdio::null));
        if let Some(d) = &self.cwd {
            cmd.current_dir(d);
        }
        cmd.tie_to_parent().spawn()
    }

    #[cfg(target_os = "windows")]
    pub fn spawn(self) -> io::Result<SidecarChild> {
        let child = win_spawn::spawn(self)?;
        let pid = child.id();
        log::info!("[hide] spawned child pid {pid} with a hidden console; dispatching hide thread");
        windows_hide::hide_console_for_pid_async(pid);
        windows_hide::start_window_manager_once();
        Ok(child)
    }
}

#[cfg(not(target_os = "windows"))]
pub type SidecarChild = std::process::Child;

#[cfg(target_os = "windows")]
pub use win_spawn::SidecarChild;

/// Quote one argument for a Windows command line so that the child's
/// `CommandLineToArgvW` / CRT parsing yields exactly the original string
/// (the rules: quote when there is whitespace or a quote; backslashes are
/// literal except immediately before a quote, where they double).
/// Platform-independent so the rule is unit-tested everywhere.
#[cfg_attr(not(target_os = "windows"), allow(dead_code))]
pub fn quote_windows_arg(arg: &str) -> String {
    let needs_quotes = arg.is_empty()
        || arg
            .chars()
            .any(|c| matches!(c, ' ' | '\t' | '\n' | '\u{0b}' | '"'));
    if !needs_quotes {
        return arg.to_string();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    let chars: Vec<char> = arg.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let mut backslashes = 0;
        while i < chars.len() && chars[i] == '\\' {
            backslashes += 1;
            i += 1;
        }
        if i == chars.len() {
            // Trailing backslashes: double them so the closing quote stays a quote.
            out.extend(std::iter::repeat('\\').take(backslashes * 2));
            break;
        }
        if chars[i] == '"' {
            out.extend(std::iter::repeat('\\').take(backslashes * 2 + 1));
            out.push('"');
        } else {
            out.extend(std::iter::repeat('\\').take(backslashes));
            out.push(chars[i]);
        }
        i += 1;
    }
    out.push('"');
    out
}

/// Terminate a process by PID, platform-correct. `kill` does not exist on
/// Windows - calling it there is a silent no-op that left conductor + lair
/// running (and their databases locked) straight through resets and key
/// restores. Windows uses taskkill with /T so the child tree goes too.
pub fn stop_pid(pid: u32) {
    #[cfg(target_os = "windows")]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let _ = std::process::Command::new("taskkill")
            .args(["/PID", &pid.to_string(), "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = std::process::Command::new("kill").arg(pid.to_string()).output();
    }
}

#[cfg(target_os = "windows")]
mod win_spawn {
    //! `CreateProcessW` with the console window created hidden
    //! (`STARTF_USESHOWWINDOW` + `SW_HIDE`), stdin piped to us, stdout and
    //! stderr to inheritable file handles (or NUL). The child is assigned to
    //! the kill-on-close job right after creation.
    use super::SidecarCommand;
    use std::fs::File;
    use std::io;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::{AsRawHandle, FromRawHandle, RawHandle};
    use std::os::windows::process::ExitStatusExt;
    use std::process::ExitStatus;
    use windows_sys::Win32::Foundation::{
        CloseHandle, DuplicateHandle, SetHandleInformation, DUPLICATE_SAME_ACCESS, HANDLE,
        HANDLE_FLAG_INHERIT, WAIT_OBJECT_0,
    };
    use windows_sys::Win32::Security::SECURITY_ATTRIBUTES;
    use windows_sys::Win32::System::Pipes::CreatePipe;
    use windows_sys::Win32::System::Threading::{
        CreateProcessW, GetCurrentProcess, GetExitCodeProcess, TerminateProcess,
        WaitForSingleObject, INFINITE, PROCESS_INFORMATION, STARTF_USESHOWWINDOW,
        STARTF_USESTDHANDLES, STARTUPINFOW,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::SW_HIDE;

    /// A spawned sidecar on Windows. Mirrors the bits of `std::process::Child`
    /// the conductor code uses.
    pub struct SidecarChild {
        pid: u32,
        /// Process handle (owned; closed on drop). Stored as usize so the
        /// struct is Send without a wrapper.
        process: usize,
        /// Our end of the child's stdin pipe.
        pub stdin: Option<File>,
        status: Option<ExitStatus>,
    }

    impl SidecarChild {
        pub fn id(&self) -> u32 {
            self.pid
        }

        fn handle(&self) -> HANDLE {
            self.process as HANDLE
        }

        fn exit_status(&self) -> io::Result<ExitStatus> {
            let mut code: u32 = 0;
            // SAFETY: valid process handle, out-pointer to a u32.
            if unsafe { GetExitCodeProcess(self.handle(), &mut code) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(ExitStatus::from_raw(code))
        }

        /// Has the child exited? Non-blocking.
        pub fn try_wait(&mut self) -> io::Result<Option<ExitStatus>> {
            if let Some(s) = self.status {
                return Ok(Some(s));
            }
            // SAFETY: valid process handle; zero timeout = poll.
            let r = unsafe { WaitForSingleObject(self.handle(), 0) };
            if r == WAIT_OBJECT_0 {
                let s = self.exit_status()?;
                self.status = Some(s);
                Ok(Some(s))
            } else {
                Ok(None)
            }
        }

        /// Block until the child exits.
        pub fn wait(&mut self) -> io::Result<ExitStatus> {
            if let Some(s) = self.status {
                return Ok(s);
            }
            // Closing our stdin end first so a child reading stdin to EOF
            // can finish (std::process::Child does the same).
            drop(self.stdin.take());
            // SAFETY: valid process handle.
            unsafe { WaitForSingleObject(self.handle(), INFINITE) };
            let s = self.exit_status()?;
            self.status = Some(s);
            Ok(s)
        }

        /// Terminate the child (no-op if it already exited).
        pub fn kill(&mut self) -> io::Result<()> {
            if self.try_wait()?.is_some() {
                return Ok(());
            }
            // SAFETY: valid process handle.
            if unsafe { TerminateProcess(self.handle(), 1) } == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        }
    }

    impl Drop for SidecarChild {
        fn drop(&mut self) {
            // SAFETY: we own this handle; close exactly once.
            unsafe {
                CloseHandle(self.handle());
            }
        }
    }

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    /// An inheritable duplicate of `h` (the child gets the copy; the
    /// original stays ours and non-inheritable).
    fn inheritable_dup(h: HANDLE) -> io::Result<HANDLE> {
        let mut out: HANDLE = std::ptr::null_mut();
        // SAFETY: handles are valid for this process; out-pointer valid.
        let ok = unsafe {
            DuplicateHandle(
                GetCurrentProcess(),
                h,
                GetCurrentProcess(),
                &mut out,
                0,
                1, // bInheritHandle
                DUPLICATE_SAME_ACCESS,
            )
        };
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(out)
    }

    /// The child's stdout/stderr: the given file, or NUL.
    fn out_handle(f: Option<File>) -> io::Result<(HANDLE, File)> {
        let file = match f {
            Some(f) => f,
            None => File::create("NUL")?,
        };
        let dup = inheritable_dup(file.as_raw_handle() as HANDLE)?;
        Ok((dup, file))
    }

    pub fn spawn(cmd: SidecarCommand) -> io::Result<SidecarChild> {
        // stdin pipe: the child reads the inheritable end, we keep the
        // writer and make sure it is NOT inherited (else the child never
        // sees EOF on it).
        let mut sa: SECURITY_ATTRIBUTES = unsafe { std::mem::zeroed() };
        sa.nLength = std::mem::size_of::<SECURITY_ATTRIBUTES>() as u32;
        sa.bInheritHandle = 1;
        let mut stdin_read: HANDLE = std::ptr::null_mut();
        let mut stdin_write: HANDLE = std::ptr::null_mut();
        // SAFETY: out-pointers valid; sa fully initialized.
        if unsafe { CreatePipe(&mut stdin_read, &mut stdin_write, &sa, 0) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: valid handle.
        unsafe { SetHandleInformation(stdin_write, HANDLE_FLAG_INHERIT, 0) };
        // Own the writer as a File from here on (closed on every error path).
        let stdin_file = unsafe { File::from_raw_handle(stdin_write as RawHandle) };

        let (stdout_h, _stdout_keep) = match out_handle(cmd.stdout) {
            Ok(v) => v,
            Err(e) => {
                unsafe { CloseHandle(stdin_read) };
                return Err(e);
            }
        };
        let (stderr_h, _stderr_keep) = match out_handle(cmd.stderr) {
            Ok(v) => v,
            Err(e) => {
                unsafe {
                    CloseHandle(stdin_read);
                    CloseHandle(stdout_h);
                }
                return Err(e);
            }
        };

        // Command line: quoted program + quoted args.
        let mut line = super::quote_windows_arg(&cmd.program.to_string_lossy());
        for a in &cmd.args {
            line.push(' ');
            line.push_str(&super::quote_windows_arg(&a.to_string_lossy()));
        }
        let mut line_w = wide(std::ffi::OsStr::new(&line));
        let program_w = wide(cmd.program.as_os_str());
        let cwd_w = cmd.cwd.as_ref().map(|d| wide(d.as_os_str()));

        let mut si: STARTUPINFOW = unsafe { std::mem::zeroed() };
        si.cb = std::mem::size_of::<STARTUPINFOW>() as u32;
        si.dwFlags = STARTF_USESHOWWINDOW | STARTF_USESTDHANDLES;
        si.wShowWindow = SW_HIDE as u16;
        si.hStdInput = stdin_read;
        si.hStdOutput = stdout_h;
        si.hStdError = stderr_h;
        let mut pi: PROCESS_INFORMATION = unsafe { std::mem::zeroed() };

        // SAFETY: all pointers reference live, NUL-terminated buffers /
        // initialized structs for the duration of the call; handles valid.
        let ok = unsafe {
            CreateProcessW(
                program_w.as_ptr(),
                line_w.as_mut_ptr(),
                std::ptr::null(),
                std::ptr::null(),
                1, // bInheritHandles: the three std handles above
                0, // no creation flags: a normal console, created hidden via si
                std::ptr::null(),
                cwd_w.as_ref().map(|w| w.as_ptr()).unwrap_or(std::ptr::null()),
                &si,
                &mut pi,
            )
        };
        let spawn_err = if ok == 0 { Some(io::Error::last_os_error()) } else { None };

        // The child holds its own copies now; ours go regardless of outcome.
        // SAFETY: each handle closed exactly once.
        unsafe {
            CloseHandle(stdin_read);
            CloseHandle(stdout_h);
            CloseHandle(stderr_h);
        }
        if let Some(e) = spawn_err {
            return Err(e);
        }
        // SAFETY: pi is populated on success; the thread handle is not needed.
        unsafe { CloseHandle(pi.hThread) };

        super::win_job::assign_to_kill_on_close_job(pi.hProcess, pi.dwProcessId);

        Ok(SidecarChild {
            pid: pi.dwProcessId,
            process: pi.hProcess as usize,
            stdin: Some(stdin_file),
            status: None,
        })
    }
}

#[cfg(target_os = "windows")]
mod win_job {
    //! Kill-on-close Job Object: the Windows stand-in for Linux's
    //! `PR_SET_PDEATHSIG`. Every sidecar (`yourowai-holochain`,
    //! `yourowai-lair-keystore`) is assigned to one process-wide job that has
    //! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`. The app process owns the only
    //! handle to that job, so when it exits - gracefully, by crash, or by
    //! Task Manager - Windows closes the handle and terminates every process
    //! still in the job. No more orphaned conductors holding ports/sockets and
    //! leaving console windows open.
    use std::sync::OnceLock;
    use windows_sys::Win32::Foundation::HANDLE;
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JobObjectExtendedLimitInformation,
        SetInformationJobObject, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };

    // Stored as usize so the HANDLE is Send + Sync inside the static. Created
    // once, lazily; reused for every sidecar.
    static JOB: OnceLock<usize> = OnceLock::new();

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
            job as usize
        });
        h as HANDLE
    }

    pub(super) fn assign_to_kill_on_close_job(process: HANDLE, pid: u32) {
        let job = job_handle();
        if job.is_null() {
            return;
        }
        unsafe {
            if AssignProcessToJobObject(job, process) == 0 {
                log::warn!(
                    "[job] failed to assign pid {} to kill-on-close job (orphan possible)",
                    pid,
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

#[cfg(test)]
mod tests {
    use super::quote_windows_arg;

    /// The rules a Windows child's argv parser applies, so a config path
    /// under "C:\Users\Chris\Your Own AI\..." arrives intact.
    #[test]
    fn windows_quoting_rules() {
        assert_eq!(quote_windows_arg("--piped"), "--piped");
        assert_eq!(quote_windows_arg(""), "\"\"");
        assert_eq!(
            quote_windows_arg(r"C:\Users\Chris\Your Own AI\conductor-config.yaml"),
            "\"C:\\Users\\Chris\\Your Own AI\\conductor-config.yaml\""
        );
        // A trailing backslash inside quotes doubles.
        assert_eq!(quote_windows_arg(r"C:\Program Files\"), "\"C:\\Program Files\\\\\"");
        // An embedded quote is escaped, along with the backslashes before it.
        assert_eq!(quote_windows_arg(r#"a\"b"#), "\"a\\\\\\\"b\"");
        assert_eq!(quote_windows_arg(r#"say "hi""#), "\"say \\\"hi\\\"\"");
    }
}
