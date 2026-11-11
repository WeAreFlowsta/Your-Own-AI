//! A guard against console windows on Windows. The app is a GUI process, so
//! every console program it starts gets a window of its own unless the call
//! says otherwise: `crate::mcp::quiet(..)` (CREATE_NO_WINDOW) for a tool
//! call, `flowsta_hc_lines::process::SidecarCommand` for a sidecar. A battery
//! check added without the flag put a PowerShell window on the screen once a
//! minute (0.9.1 betas); this test is what stops the next one.

/// Every `Command::new(` in the crate that has neither `quiet(` nor
/// `creation_flags` within the 12 lines from it, by file. Each one is here
/// for a reason: it never runs on Windows, it is test code, or the window is
/// the point.
const WITHOUT_THE_FLAG: &[(&str, usize, &str)] = &[
    ("build_install.rs", 1, "tar: macOS and Linux archives only (Windows unpacks the zip in process)"),
    ("diagnostics.rs", 2, "journalctl, coredumpctl: Linux"),
    ("flowsta.rs", 1, "lsof: macOS"),
    ("llm.rs", 9, "lsof, vm_stat, sysctl: Unix; the other six are tests"),
    ("machine.rs", 3, "tests"),
    ("matrix.rs", 2, "caffeinate, systemd-inhibit: macOS and Linux"),
    ("mcp.rs", 1, "sh: Unix"),
    ("mlx_engine.rs", 1, "tar: macOS"),
    ("move_07.rs", 2, "tests"),
    ("multi_device_live.rs", 1, "a live test"),
    ("terminal.rs", 3, "opens a terminal for the person: the window is the point"),
    ("tuning.rs", 1, "a test"),
];

#[test]
fn no_new_program_start_without_the_no_window_flag() {
    let src = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found: Vec<(String, usize)> = Vec::new();
    for entry in std::fs::read_dir(&src).unwrap().flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        if !name.ends_with(".rs") || name == "spawn_guard.rs" {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let n = (0..lines.len())
            .filter(|&i| lines[i].contains("Command::new(") && !lines[i].trim_start().starts_with("//"))
            .filter(|&i| {
                let window = lines[i..(i + 12).min(lines.len())].join("\n");
                !window.contains("quiet(") && !window.contains("creation_flags")
            })
            .count();
        if n > 0 {
            found.push((name, n));
        }
    }
    found.sort();
    let known: Vec<(String, usize)> = WITHOUT_THE_FLAG.iter().map(|(f, n, _)| (f.to_string(), *n)).collect();
    assert_eq!(
        found, known,
        "a program is started without the no-window flag (or one was removed). On Windows that is a console \
         window on the screen. Wrap the call in crate::mcp::quiet(..); only if it can never run on Windows, \
         change the count in spawn_guard.rs and say why."
    );
}
