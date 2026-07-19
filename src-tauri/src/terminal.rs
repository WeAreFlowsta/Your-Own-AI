//! Open a command in the user's terminal - the "run this yourself" path
//! for commands the AI suggests. The command runs in the user's own
//! terminal app (visible, interruptible, theirs), never silently.

use std::process::Command;

/// Escape a string for embedding inside AppleScript double quotes.
#[cfg(target_os = "macos")]
fn applescript_escape(s: &str) -> String {
    s.replace('\\', "\\\\").replace('"', "\\\"")
}

#[tauri::command]
pub fn open_in_terminal(command: String, cwd: Option<String>) -> Result<(), String> {
    let dir = cwd
        .filter(|c| !c.is_empty())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| ".".to_string());

    #[cfg(target_os = "linux")]
    {
        // Keep the terminal open after the command so the user can read the
        // output and keep working.
        let script = format!("{command}\nexec $SHELL");
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
        let line = format!("cd {:?} && {}", dir, command);
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
        Command::new("cmd")
            .args(["/c", "start", "", "cmd", "/k", &command])
            .current_dir(&dir)
            .spawn()
            .map(|_| ())
            .map_err(|e| format!("Could not open a terminal: {e}"))
    }
}
