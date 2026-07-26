//! Open a command in the user's terminal - the "run this yourself" path
//! for commands the AI suggests. The command runs in the user's own
//! terminal app (visible, interruptible, theirs), never silently.
//!
//! Two modes: the default puts the command PRE-FILLED on the shell prompt
//! (editable, Enter to run - the final look stays with the user); the
//! "immediate" setting executes right away for people who have decided
//! the click is the confirmation.

use std::io::Write;
use std::process::Command;

/// Single-quote a string for safe embedding in a bash script.
fn sh_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r#"'\''"#))
}

/// A temp script that pre-fills the command on an editable prompt via
/// readline (`read -e -i`), runs it on Enter, then hands over to the
/// user's shell. Multi-line blocks join with " && " (readline is one line).
fn write_prefill_script(command: &str) -> Result<std::path::PathBuf, String> {
    let one_line = command
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .collect::<Vec<_>>()
        .join(" && ");
    let script = format!(
        "#!/usr/bin/env bash\nread -e -p \"$ \" -i {} YOAI_LINE && eval \"$YOAI_LINE\"\nexec \"${{SHELL:-bash}}\"\n",
        sh_quote(&one_line)
    );
    let path = std::env::temp_dir().join(format!(
        "yoai-run-{}.sh",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0)
    ));
    let mut f = std::fs::File::create(&path).map_err(|e| format!("temp script: {e}"))?;
    f.write_all(script.as_bytes())
        .map_err(|e| format!("temp script: {e}"))?;
    Ok(path)
}

#[cfg(target_os = "macos")]
fn applescript_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[tauri::command]
pub fn open_in_terminal(
    command: String,
    cwd: Option<String>,
    immediate: Option<bool>,
) -> Result<(), String> {
    let dir = cwd
        .filter(|c| !c.is_empty())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| ".".to_string());
    let immediate = immediate.unwrap_or(false);

    #[cfg(target_os = "linux")]
    {
        // Keep the terminal open after the command so the user can read the
        // output and keep working.
        let script = if immediate {
            format!("{command}\nexec \"${{SHELL:-bash}}\"")
        } else {
            let path = write_prefill_script(&command)?;
            format!("bash {}", sh_quote(&path.to_string_lossy()))
        };
        // (terminal binary, args before the shell invocation)
        let candidates: [(&str, &[&str]); 7] = [
            ("gnome-terminal", &["--"]),
            ("konsole", &["-e"]),
            ("xfce4-terminal", &["-x"]),
            ("x-terminal-emulator", &["-e"]),
            ("alacritty", &["-e"]),
            ("kitty", &[]),
            ("xterm", &["-e"]),
        ];
        for (bin, pre) in candidates {
            let mut cmd = Command::new(bin);
            cmd.args(pre)
                .args(["bash", "-c", &script])
                .current_dir(&dir);
            if cmd.spawn().is_ok() {
                return Ok(());
            }
        }
        Err("No terminal application found".to_string())
    }

    #[cfg(target_os = "macos")]
    {
        let line = if immediate {
            format!("cd {:?} && {}", dir, command)
        } else {
            let path = write_prefill_script(&command)?;
            format!("cd {:?} && bash {}", dir, sh_quote(&path.to_string_lossy()))
        };
        let script = format!(
            "tell application \"Terminal\"\nactivate\ndo script \"{}\"\nend tell",
            applescript_escape(&line)
        );
        Command::new("osascript")
            .args(["-e", &script])
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not open Terminal: {e}"))
    }

    #[cfg(target_os = "windows")]
    {
        // cmd has no readline pre-fill: confirm mode shows the suggestion
        // (it is also on the clipboard from the same click path).
        let args: Vec<String> = if immediate {
            vec!["/c".into(), "start".into(), "".into(), "cmd".into(), "/k".into(), command]
        } else {
            vec![
                "/c".into(),
                "start".into(),
                "".into(),
                "cmd".into(),
                "/k".into(),
                format!("echo Suggested: {command}"),
            ]
        };
        Command::new("cmd")
            .args(&args)
            .current_dir(&dir)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not open a terminal: {e}"))
    }
}
