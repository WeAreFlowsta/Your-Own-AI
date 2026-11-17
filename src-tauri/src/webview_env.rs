//! The webview's platform fix stays with the webview. On Linux with the
//! NVIDIA driver loaded, `flowsta_hc_lines::webview::apply()` sets one or two
//! environment variables at startup so WebKitGTK does not draw through
//! NVIDIA's EGL (which segfaults WebKitGTK 2.52 on a page change). Every
//! program the app starts would inherit them: the engine, the Build agent
//! and what it runs, a tool call, the person's terminal. These helpers take
//! them out of a child's environment again.

/// The variables the fix set in this process (none on most machines).
fn set_here() -> &'static [&'static str] {
    flowsta_hc_lines::webview::set_here()
}

/// A child started with `std::process::Command`.
pub(crate) fn strip(cmd: &mut std::process::Command) -> &mut std::process::Command {
    for k in set_here() {
        cmd.env_remove(k);
    }
    cmd
}

/// A child started with `tokio::process::Command`.
pub(crate) fn strip_tokio(cmd: &mut tokio::process::Command) -> &mut tokio::process::Command {
    for k in set_here() {
        cmd.env_remove(k);
    }
    cmd
}

/// What a child's environment is once `removed` are taken out of `all`.
fn without<'a>(all: impl Iterator<Item = (std::ffi::OsString, std::ffi::OsString)> + 'a, removed: &'a [&str]) -> impl Iterator<Item = (std::ffi::OsString, std::ffi::OsString)> + 'a {
    all.filter(move |(k, _)| !removed.iter().any(|r| k == std::ffi::OsStr::new(r)))
}

/// A child started through the shell plugin, which can clear and set but not
/// remove: when the fix set something, the child gets this process's
/// environment without it. Call it where the command is made, before any
/// `.env(..)` of the caller's own.
pub(crate) fn strip_shell(cmd: tauri_plugin_shell::process::Command) -> tauri_plugin_shell::process::Command {
    let removed = set_here();
    if removed.is_empty() {
        return cmd;
    }
    cmd.env_clear().envs(without(std::env::vars_os(), removed))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_child_gets_the_environment_without_the_fix() {
        let all = vec![
            ("PATH".into(), "/usr/bin".into()),
            ("__EGL_VENDOR_LIBRARY_FILENAMES".into(), "/usr/share/glvnd/egl_vendor.d/50_mesa.json".into()),
            ("WEBKIT_DMABUF_RENDERER_DISABLE_GBM".into(), "1".into()),
            ("HOME".into(), "/home/a".into()),
        ];
        let kept: Vec<String> = without(all.clone().into_iter(), &["__EGL_VENDOR_LIBRARY_FILENAMES", "WEBKIT_DMABUF_RENDERER_DISABLE_GBM"]).map(|(k, _)| k.to_string_lossy().into_owned()).collect();
        assert_eq!(kept, ["PATH", "HOME"]);
        assert_eq!(without(all.into_iter(), &[]).count(), 4, "nothing set here: nothing removed");
    }

    /// Every program start goes through one of the helpers: `quiet(` (which
    /// strips), `strip(`, `strip_tokio(` or `strip_shell(`.
    #[test]
    fn every_long_lived_child_start_strips_the_fix() {
        let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        for (file, needle, count) in [("agent_bridge.rs", "strip_shell(", 1), ("llm.rs", "strip_shell(", 3), ("terminal.rs", "webview_env::strip(", 1), ("mcp.rs", "webview_env::strip(", 2)] {
            let text = std::fs::read_to_string(src.join(file)).unwrap();
            assert!(text.matches(needle).count() >= count, "{file}: expected at least {count} `{needle}`");
        }
    }
}

/// The parent's process id in a `/proc/<pid>/stat` line. The program's name
/// sits in brackets and may itself hold spaces and brackets, so the fields
/// are counted from the last closing bracket.
#[cfg(any(target_os = "linux", test))]
fn parent_in_stat(stat: &str) -> Option<u32> {
    let after = &stat[stat.rfind(')')? + 1..];
    after.split_whitespace().nth(1)?.parse().ok()
}

/// Linux, at quit: end the page's own processes before this one exits.
///
/// Left to itself, WebKitGTK's page process notices the app has gone and
/// calls `exit`, whose clean-up frees its graphics device while its drawing
/// thread is still letting go of it. On WebKitGTK 2.52 with Mesa that has
/// ended in glibc's "corrupted double-linked list" abort, a second after a
/// clean quit, and the desktop then shows a crash dialog for a quit where
/// nothing went wrong. The page has nothing left to do by now (what it
/// stores is written by the network process, which is left alone), so it is
/// ended outright and never reaches that clean-up. Returns how many.
#[cfg(target_os = "linux")]
pub(crate) fn end_page_processes() -> usize {
    let me = std::process::id();
    let Ok(dir) = std::fs::read_dir("/proc") else { return 0 };
    let mut ended = 0;
    for entry in dir.flatten() {
        let Some(pid) = entry.file_name().to_str().and_then(|s| s.parse::<i32>().ok()) else { continue };
        let base = entry.path();
        let ours = std::fs::read_to_string(base.join("stat")).ok().and_then(|s| parent_in_stat(&s)) == Some(me);
        if !ours {
            continue;
        }
        let is_page = std::fs::read_link(base.join("exe"))
            .ok()
            .and_then(|p| p.file_name().map(|n| n == "WebKitWebProcess"))
            .unwrap_or(false);
        // SAFETY: a signal to a process id read a moment ago, checked to be
        // this process's own child.
        if is_page && unsafe { libc::kill(pid, libc::SIGKILL) } == 0 {
            ended += 1;
        }
    }
    ended
}

#[cfg(not(target_os = "linux"))]
pub(crate) fn end_page_processes() -> usize {
    0
}

#[cfg(test)]
mod stat_tests {
    use super::parent_in_stat;

    #[test]
    fn the_parent_is_read_past_a_name_with_spaces_and_brackets() {
        assert_eq!(parent_in_stat("777844 (WebKitWebProcess) S 777651 777651 4099 0 -1"), Some(777651));
        assert_eq!(parent_in_stat("12 (a (odd) name) R 7 12 12 0 -1"), Some(7));
        assert_eq!(parent_in_stat("12 (short) S"), None);
        assert_eq!(parent_in_stat("no brackets here"), None);
    }
}
