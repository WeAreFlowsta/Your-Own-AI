//! One-click diagnostic report: everything a support conversation needs,
//! gathered and redacted on this side of the IPC boundary and written to a
//! file the user controls. Nothing is uploaded anywhere.
//!
//! Every collector is wrapped so a failure prints INSIDE the report
//! ("collector failed: ...") instead of silently omitting a section - a
//! support file must explain its own gaps.

use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tauri::{AppHandle, Manager};

use crate::commands_holochain::HolochainState;

const LOG_TAIL_LINES: usize = 500;
const CRASH_LOOKBACK_DAYS: u64 = 7;
/// Strings that identify our processes in OS crash records.
const PROCESS_MARKERS: &[&str] = &[
    "Your Own AI",
    "Your.Own.AI",
    "your-own-ai",
    "yourowai",
    "llama-server",
];

#[tauri::command]
pub async fn export_diagnostics(app: AppHandle, path: String) -> Result<String, String> {
    let mut r = String::new();

    let _ = writeln!(r, "Your Own AI diagnostic report");
    let _ = writeln!(r, "Generated: {} (UTC)", utc_now_string());
    let _ = writeln!(
        r,
        "App version: {} ({} {})",
        app.package_info().version,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = writeln!(
        r,
        "Webview: {}",
        tauri::webview_version().unwrap_or_else(|e| format!("unknown ({e})"))
    );

    section(&mut r, "System", &system_section());
    section(&mut r, "GPU safety", &gpu_section(&app));
    section(&mut r, "Downloaded models", &models_section(&app).await);
    section(&mut r, "Conductor", &conductor_section(&app));
    section(&mut r, "Recent routing decisions", &routing_section());
    section(
        &mut r,
        "System crash records (last 7 days)",
        &crash_records(),
    );
    section(&mut r, "App log files", &log_list_section(&app));
    section(
        &mut r,
        &format!("App log tail (last {LOG_TAIL_LINES} lines)"),
        &log_tail_section(&app),
    );

    let report = redact(&r);
    std::fs::write(&path, report).map_err(|e| format!("Could not write the report: {e}"))?;
    Ok(path)
}

fn section(out: &mut String, title: &str, body: &str) {
    let _ = writeln!(out, "\n== {title} ==");
    let _ = writeln!(out, "{}", body.trim_end());
}

fn system_section() -> String {
    match crate::llm::get_system_info() {
        Ok(si) => {
            let gpu = match &si.gpu_name {
                Some(name) if si.gpu_integrated => {
                    format!("{name} (integrated, shares system memory)")
                }
                Some(name) => match si.total_vram_gb {
                    Some(v) => format!("{name} ({v:.1}GB VRAM)"),
                    None => name.clone(),
                },
                None => "none detected".to_string(),
            };
            format!(
                "OS: {} {}\nCPU: {} ({} cores)\nMemory: {:.1}GB total, {:.1}GB used\nGraphics: {}",
                si.os_name,
                si.os_version,
                si.cpu_brand,
                si.cpu_count,
                si.total_memory_gb,
                si.used_memory_gb,
                gpu
            )
        }
        Err(e) => format!("collector failed: {e}"),
    }
}

fn gpu_section(app: &AppHandle) -> String {
    let st = crate::gpu_safety::gpu_safe_mode_status(app.clone());
    format!(
        "Safe mode (forced CPU): {}\nCUDA laddered out: {}",
        if st.active { "ACTIVE" } else { "no" },
        if st.cuda_disabled { "yes" } else { "no" }
    )
}

async fn models_section(app: &AppHandle) -> String {
    match crate::llm::list_local_models(app.clone()).await {
        Ok(models) if models.is_empty() => "none".to_string(),
        Ok(models) => models
            .iter()
            .map(|m| format!("{} ({})", m.name, m.size))
            .collect::<Vec<_>>()
            .join("\n"),
        Err(e) => format!("collector failed: {e}"),
    }
}

fn conductor_section(app: &AppHandle) -> String {
    let ready = app
        .try_state::<Arc<HolochainState>>()
        .map(|s| s.manager.get().is_some());
    match ready {
        Some(true) => "ready (conductor running, app websocket connected)".to_string(),
        Some(false) => "NOT ready (conductor not fully started)".to_string(),
        None => "state unavailable".to_string(),
    }
}

fn routing_section() -> String {
    let decisions = crate::router::recent_routing_decisions();
    if decisions.is_empty() {
        return "none this session".to_string();
    }
    decisions
        .iter()
        .map(|d| format!("[{}] {} - {}", d.at_ms, d.model, d.reason))
        .collect::<Vec<_>>()
        .join("\n")
}

fn log_dir(app: &AppHandle) -> Result<PathBuf, String> {
    app.path().app_log_dir().map_err(|e| e.to_string())
}

/// All log files with sizes and modified times, newest first.
fn log_list_section(app: &AppHandle) -> String {
    let dir = match log_dir(app) {
        Ok(d) => d,
        Err(e) => return format!("collector failed: {e}"),
    };
    match sorted_log_files(&dir) {
        Ok(files) if files.is_empty() => "no log files found".to_string(),
        Ok(files) => files
            .iter()
            .map(|(p, len, age_secs)| {
                format!(
                    "{} ({:.1}KB, modified {}m ago)",
                    p.file_name().unwrap_or_default().to_string_lossy(),
                    *len as f64 / 1024.0,
                    age_secs / 60
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Err(e) => format!("collector failed: {e}"),
    }
}

/// Tail of the newest log file. Rotation keeps every file (KeepAll), so the
/// newest by mtime is the live one.
fn log_tail_section(app: &AppHandle) -> String {
    let dir = match log_dir(app) {
        Ok(d) => d,
        Err(e) => return format!("collector failed: {e}"),
    };
    let files = match sorted_log_files(&dir) {
        Ok(f) => f,
        Err(e) => return format!("collector failed: {e}"),
    };
    let Some((newest, _, _)) = files.first() else {
        return "no log files found".to_string();
    };
    match std::fs::read_to_string(newest) {
        Ok(content) => {
            let lines: Vec<&str> = content.lines().collect();
            let start = lines.len().saturating_sub(LOG_TAIL_LINES);
            lines[start..].join("\n")
        }
        Err(e) => format!("collector failed: {e}"),
    }
}

/// (path, size, seconds-since-modified) for *.log files, newest first.
fn sorted_log_files(dir: &PathBuf) -> Result<Vec<(PathBuf, u64, u64)>, String> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path.extension().is_none_or(|e| e != "log") {
            continue;
        }
        let meta = entry.metadata().map_err(|e| e.to_string())?;
        let age = meta
            .modified()
            .ok()
            .and_then(|m| m.elapsed().ok())
            .map(|d| d.as_secs())
            .unwrap_or(u64::MAX);
        files.push((path, meta.len(), age));
    }
    files.sort_by_key(|(_, _, age)| *age);
    Ok(files)
}

// --- OS crash records -----------------------------------------------------

fn matches_marker(text: &str) -> bool {
    let lower = text.to_lowercase();
    PROCESS_MARKERS.iter().any(|m| lower.contains(&m.to_lowercase()))
}

#[cfg(target_os = "windows")]
fn crash_records() -> String {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let lookback_ms = CRASH_LOOKBACK_DAYS * 24 * 60 * 60 * 1000;
    let query = format!(
        "*[System[(EventID=1000 or EventID=1001) and TimeCreated[timediff(@SystemTime) <= {lookback_ms}]]]"
    );
    let out = std::process::Command::new("wevtutil")
        .args(["qe", "Application", &format!("/q:{query}"), "/f:text", "/c:100"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let out = match out {
        Ok(o) => o,
        Err(e) => return format!("collector failed: could not run wevtutil: {e}"),
    };
    if !out.status.success() {
        return format!(
            "collector failed: wevtutil exited with {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr)
        );
    }
    let text = String::from_utf8_lossy(&out.stdout);
    // Events are emitted as "Event[n]:" blocks; keep the ones about us.
    let mut kept = Vec::new();
    let mut current = String::new();
    for line in text.lines() {
        if line.starts_with("Event[") {
            if matches_marker(&current) {
                kept.push(current.clone());
            }
            current.clear();
        }
        current.push_str(line);
        current.push('\n');
    }
    if matches_marker(&current) {
        kept.push(current);
    }
    if kept.is_empty() {
        "no crash records for this app in the last 7 days".to_string()
    } else {
        kept.join("\n")
    }
}

#[cfg(target_os = "macos")]
fn crash_records() -> String {
    let Some(home) = dirs_home() else {
        return "collector failed: no home directory".to_string();
    };
    let dir = home.join("Library/Logs/DiagnosticReports");
    let entries = match std::fs::read_dir(&dir) {
        Ok(e) => e,
        Err(e) => return format!("collector failed: could not read DiagnosticReports: {e}"),
    };
    let week = std::time::Duration::from_secs(CRASH_LOOKBACK_DAYS * 24 * 60 * 60);
    let mut ours: Vec<(PathBuf, u64)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path.file_name().unwrap_or_default().to_string_lossy().to_string();
        if !matches_marker(&name) {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        let Ok(age) = meta.modified().and_then(|m| Ok(m.elapsed().unwrap_or_default())) else {
            continue;
        };
        if age <= week {
            ours.push((path, age.as_secs()));
        }
    }
    if ours.is_empty() {
        return "no crash reports for this app in the last 7 days".to_string();
    }
    ours.sort_by_key(|(_, age)| *age);
    let mut out = format!("{} report(s) found:\n", ours.len());
    for (path, _) in &ours {
        let _ = writeln!(out, "- {}", path.file_name().unwrap_or_default().to_string_lossy());
    }
    // Headers of the newest three carry the termination reason.
    for (path, _) in ours.iter().take(3) {
        if let Ok(content) = std::fs::read_to_string(path) {
            let head: Vec<&str> = content.lines().take(30).collect();
            let _ = writeln!(
                out,
                "\n--- {} (first 30 lines) ---\n{}",
                path.file_name().unwrap_or_default().to_string_lossy(),
                head.join("\n")
            );
        }
    }
    out
}

#[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
fn crash_records() -> String {
    let pattern = "yourowai|your-own-ai|Your.Own.AI|Your Own AI|llama-server";
    let out = std::process::Command::new("journalctl")
        .args(["--since", "7 days ago", "--no-pager", "-q", "-g", pattern])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if text.is_empty() {
                "no journal entries for this app in the last 7 days".to_string()
            } else {
                text
            }
        }
        Ok(o) => format!(
            "collector failed: journalctl exited with {}: {}",
            o.status,
            String::from_utf8_lossy(&o.stderr)
        ),
        Err(e) => format!("collector failed: could not run journalctl: {e}"),
    }
}

#[cfg(target_os = "macos")]
fn dirs_home() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

// --- Redaction ------------------------------------------------------------

/// Strip the user's identity from the report: home directory (in every path
/// flavor that appears in logs) and lair pipe keys from pre-0.2.0 log lines.
fn redact(report: &str) -> String {
    let mut out = report.to_string();
    if let Some(home) = home_dir_string() {
        // As-is, forward-slash, and JSON/log-escaped backslash variants.
        let variants = [
            home.clone(),
            home.replace('\\', "/"),
            home.replace('\\', "\\\\"),
        ];
        for v in variants {
            if !v.is_empty() {
                out = out.replace(&v, "~");
            }
        }
    }
    redact_pipe_keys(&out)
}

fn home_dir_string() -> Option<String> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("USERPROFILE").ok()
    }
    #[cfg(not(target_os = "windows"))]
    {
        std::env::var("HOME").ok()
    }
}

/// Replace `?k=<base64url>` (lair connection keys in old logs) with a marker.
fn redact_pipe_keys(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(idx) = rest.find("?k=") {
        let (before, after) = rest.split_at(idx + 3);
        out.push_str(before);
        out.push_str("[redacted]");
        let end = after
            .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == '_'))
            .unwrap_or(after.len());
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}

// --- Time (no chrono dependency) ------------------------------------------

/// "YYYY-MM-DD HH:MM:SS" for now, UTC. Civil-from-days per Howard Hinnant.
fn utc_now_string() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86_400) as i64;
    let (h, m, s) = ((secs / 3600) % 24, (secs / 60) % 60, secs % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mth = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if mth <= 2 { y + 1 } else { y };
    format!("{y:04}-{mth:02}-{d:02} {h:02}:{m:02}:{s:02}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pipe_keys_are_redacted() {
        let input = "Connecting at named-pipe:\\\\.\\pipe\\0x-abc?k=fvRuE-1uVi4YSk5zz2yEu84sptrHRaXlAXkEj3rtaGY then more";
        let out = redact_pipe_keys(input);
        assert!(out.contains("?k=[redacted] then more"));
        assert!(!out.contains("fvRuE"));
    }

    #[test]
    fn pipe_key_at_end_of_text() {
        let out = redact_pipe_keys("x?k=abc123");
        assert_eq!(out, "x?k=[redacted]");
    }

    #[test]
    fn marker_matching_is_case_insensitive() {
        assert!(matches_marker("Faulting application name: YOUROWAI-holochain.exe"));
        assert!(!matches_marker("Faulting application name: notepad.exe"));
    }
}
