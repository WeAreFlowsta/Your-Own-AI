//! Add-ons > Tools: MCP servers an AI may use in a project session.
//!
//! An MCP server is a program (or a local HTTP endpoint) that offers tools -
//! Blender, a browser, a 3D printer, a smart home. The list lives in
//! `~/.your-own-ai-build/mcp-servers.json`; which servers an AI carries is
//! chosen on the AI (like skills), and the bridge hands exactly those to the
//! agent on `session/new` next to our own project-memory server. Nothing
//! here starts a server: the agent launches it for the session and every
//! tool call goes through the same approval step as a file edit.
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct McpServer {
    /// Key the AI refers to; lowercase `a-z0-9-`.
    pub name: String,
    #[serde(default)]
    pub description: String,
    /// "stdio" (a program we launch) | "http" (a local streamable-HTTP endpoint)
    pub transport: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub env: Vec<(String, String)>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Where it came from: "manual" | "directory:<id>" | a preset id.
    #[serde(default)]
    pub source: String,
    #[serde(default)]
    pub added_at: u64,
}

fn store_path(app: &AppHandle) -> Result<PathBuf, String> {
    let home = app
        .path()
        .home_dir()
        .map_err(|e| format!("cannot resolve home dir: {e}"))?;
    let dir = home.join(".your-own-ai-build");
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create agent folder: {e}"))?;
    Ok(dir.join("mcp-servers.json"))
}

pub(crate) fn load(app: &AppHandle) -> Result<Vec<McpServer>, String> {
    let p = store_path(app)?;
    if !p.is_file() {
        return Ok(vec![]);
    }
    let text = std::fs::read_to_string(&p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
    serde_json::from_str::<Vec<McpServer>>(&text).map_err(|e| format!("mcp-servers.json is not valid: {e}"))
}

fn save(app: &AppHandle, list: &[McpServer]) -> Result<(), String> {
    let p = store_path(app)?;
    let text = serde_json::to_string_pretty(list).map_err(|e| e.to_string())?;
    std::fs::write(&p, text).map_err(|e| format!("cannot write {}: {e}", p.display()))
}

/// `~/x` -> the user's home; presets store paths that way so one entry
/// works on every machine.
fn expand_home(app: &AppHandle, v: &str) -> String {
    if let Some(rest) = v.strip_prefix("~/") {
        if let Ok(home) = app.path().home_dir() {
            return home.join(rest).to_string_lossy().to_string();
        }
    }
    v.to_string()
}

/// ACP `session/new` entries for the named servers (unknown names are
/// skipped, never an error - an AI may reference a server that was removed).
pub(crate) fn acp_entries(app: &AppHandle, names: &[String]) -> Vec<Value> {
    let Ok(list) = load(app) else { return vec![] };
    names
        .iter()
        .filter_map(|n| list.iter().find(|s| &s.name == n))
        .filter_map(|s| match s.transport.as_str() {
            "stdio" => Some(json!({
                "name": s.name,
                "command": expand_home(app, &s.command.clone().unwrap_or_default()),
                "args": s.args.iter().map(|a| expand_home(app, a)).collect::<Vec<_>>(),
                "env": s.env.iter().map(|(k, v)| json!({ "name": k, "value": v })).collect::<Vec<_>>(),
            })),
            "http" => Some(json!({
                "type": "http",
                "name": s.name,
                "url": s.url.clone().unwrap_or_default(),
                "headers": [],
            })),
            _ => None,
        })
        .collect()
}

#[tauri::command]
pub async fn mcp_list(app: AppHandle) -> Result<Vec<McpServer>, String> {
    load(&app)
}

#[tauri::command]
pub async fn mcp_add(app: AppHandle, server: McpServer) -> Result<Vec<McpServer>, String> {
    let name = crate::skills::normalize_name(&server.name);
    if name.is_empty() {
        return Err("give the server a name".into());
    }
    match server.transport.as_str() {
        "stdio" => {
            if server.command.as_deref().unwrap_or("").trim().is_empty() {
                return Err("a program to run is needed".into());
            }
        }
        "http" => {
            let url = server.url.as_deref().unwrap_or("").trim();
            if !(url.starts_with("http://127.0.0.1") || url.starts_with("http://localhost")) {
                return Err("only local servers (127.0.0.1 or localhost) can be added here".into());
            }
        }
        _ => return Err("transport must be stdio or http".into()),
    }
    let mut list = load(&app)?;
    list.retain(|s| s.name != name);
    let added_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    list.push(McpServer { name, added_at, ..server });
    list.sort_by(|a, b| a.name.cmp(&b.name));
    save(&app, &list)?;
    Ok(list)
}

#[tauri::command]
pub async fn mcp_remove(app: AppHandle, name: String) -> Result<Vec<McpServer>, String> {
    let mut list = load(&app)?;
    list.retain(|s| s.name != name);
    save(&app, &list)?;
    Ok(list)
}

/// Is `program` on this machine's PATH (or an existing path)? The page uses
/// it to say "needs uv - install it" before anyone hits a dead session.
#[tauri::command]
pub async fn mcp_which(program: String) -> Result<Option<String>, String> {
    let p = program.trim();
    if p.is_empty() {
        return Ok(None);
    }
    if std::path::Path::new(p).is_file() {
        return Ok(Some(p.to_string()));
    }
    let Some(path_var) = std::env::var_os("PATH") else { return Ok(None) };
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|s| s.to_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in std::env::split_paths(&path_var) {
        for ext in &exts {
            let cand = dir.join(format!("{p}{ext}"));
            if cand.is_file() {
                return Ok(Some(cand.to_string_lossy().to_string()));
            }
        }
    }
    Ok(None)
}

/// Fetch a server's source with git into `~/<dest>` (a preset's one download,
/// always behind a button that says what and from where). Returns the path.
#[tauri::command]
pub async fn mcp_fetch_git(app: AppHandle, url: String, dest: String) -> Result<String, String> {
    if !(url.starts_with("https://") && !url.contains("..")) {
        return Err("only https git URLs".into());
    }
    let dest = dest.trim().trim_start_matches("~/").to_string();
    if dest.is_empty() || dest.contains("..") || dest.starts_with('/') {
        return Err("destination must be a folder name under your home".into());
    }
    let home = app.path().home_dir().map_err(|e| format!("cannot resolve home dir: {e}"))?;
    let target = home.join(&dest);
    let target_s = target.to_string_lossy().to_string();
    let out = if target.join(".git").is_dir() {
        std::process::Command::new("git").args(["-C", &target_s, "pull", "--ff-only"]).output()
    } else {
        std::process::Command::new("git").args(["clone", "--depth", "1", &url, &target_s]).output()
    }
    .map_err(|e| format!("git could not run: {e}"))?;
    if !out.status.success() {
        return Err(format!("git failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(target_s)
}
