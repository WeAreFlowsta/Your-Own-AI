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
use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// A setting a tool asks for. The listing describes it; the person fills it
/// in on Add; the app keeps it on this device (secrets encrypted with the
/// same key that protects transcripts). `where_` says how the value reaches
/// the server: "env" (default), "arg" (replaces `${KEY}` in args/command/url),
/// or "header:<Name>" for the http transport.
#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ConfigField {
    pub key: String,
    #[serde(default)]
    pub label: String,
    /// "url" | "secret" | "text" | "path"
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub hint: String,
    #[serde(default, rename = "where")]
    pub where_: String,
    /// Put in front of the value on the way out ("Bearer " for a token header).
    #[serde(default)]
    pub prefix: String,
}

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
    /// Settings the tool asks for (from its listing).
    #[serde(default)]
    pub config: Vec<ConfigField>,
    /// Filled-in values for non-secret settings; secrets live encrypted apart.
    #[serde(default)]
    pub values: HashMap<String, String>,
    /// Where it came from: "manual" | "directory:<id>" | a preset id.
    #[serde(default)]
    pub source: String,
    /// How to use it well, from the listing - handed to the agent at the
    /// start of every project session that carries this tool.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub guidance: String,
    /// The clone this tool runs from (`~/<dest>`), when the app fetched it -
    /// what "Check for updates" looks at. Never checked without a click.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub fetch_dir: Option<String>,
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

// --- secrets: `<app data>/mcp-secrets.json` = secretbox of a JSON map
// "<server>:<KEY>" -> value, under the user's transcript data key.
fn secrets_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_data_dir().map_err(|e| format!("cannot resolve app data dir: {e}"))?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create app data dir: {e}"))?;
    Ok(dir.join("mcp-secrets.json"))
}
fn secrets_key(app: &AppHandle) -> Result<[u8; 32], String> {
    let dir = app.path().app_data_dir().map_err(|e| format!("cannot resolve app data dir: {e}"))?;
    crate::transcript_crypto::ensure_recovery_material(&dir)?.data_key()
}
fn secrets_load(app: &AppHandle) -> Result<HashMap<String, String>, String> {
    let p = secrets_path(app)?;
    if !p.is_file() {
        return Ok(HashMap::new());
    }
    #[derive(Deserialize)]
    struct Sealed { nonce: String, cipher: String }
    let sealed: Sealed = serde_json::from_str(&std::fs::read_to_string(&p).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD;
    let nonce = b64.decode(sealed.nonce).map_err(|e| e.to_string())?;
    let cipher = b64.decode(sealed.cipher).map_err(|e| e.to_string())?;
    let plain = crate::transcript_crypto::decrypt(&secrets_key(app)?, &nonce, &cipher)?;
    serde_json::from_slice(&plain).map_err(|e| e.to_string())
}
fn secrets_save(app: &AppHandle, map: &HashMap<String, String>) -> Result<(), String> {
    use base64::Engine as _;
    let b64 = base64::engine::general_purpose::STANDARD;
    let plain = serde_json::to_vec(map).map_err(|e| e.to_string())?;
    let (nonce, cipher) = crate::transcript_crypto::encrypt(&secrets_key(app)?, &plain)?;
    let text = serde_json::to_string(&json!({ "nonce": b64.encode(nonce), "cipher": b64.encode(cipher) })).map_err(|e| e.to_string())?;
    std::fs::write(secrets_path(app)?, text).map_err(|e| e.to_string())
}

/// `${KEY}` -> value, for every known value.
fn fill(template: &str, vals: &HashMap<String, String>) -> String {
    let mut out = template.to_string();
    for (k, v) in vals {
        out = out.replace(&format!("${{{k}}}"), v);
    }
    out
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

/// A bare program name -> its full path when we can find it (see
/// `mcp_which`); otherwise unchanged and the agent's own PATH decides.
fn resolve_program(cmd: &str) -> String {
    if cmd.contains('/') || cmd.contains('\\') {
        return cmd.to_string();
    }
    which_sync(cmd).unwrap_or_else(|| cmd.to_string())
}

/// ACP `session/new` entries for the named servers (unknown names are
/// skipped, never an error - an AI may reference a server that was removed).
pub(crate) fn acp_entries(app: &AppHandle, names: &[String]) -> Vec<Value> {
    let Ok(list) = load(app) else { return vec![] };
    let secrets = secrets_load(app).unwrap_or_default();
    names
        .iter()
        .filter_map(|n| list.iter().find(|s| &s.name == n))
        .filter_map(|s| {
            // Every value the tool asked for, secrets included.
            let mut vals = s.values.clone();
            for f in &s.config {
                if f.kind == "secret" {
                    if let Some(v) = secrets.get(&format!("{}:{}", s.name, f.key)) {
                        vals.insert(f.key.clone(), v.clone());
                    }
                }
            }
            if let Some(missing) = s.config.iter().find(|f| f.required && vals.get(&f.key).map(|v| v.trim().is_empty()).unwrap_or(true)) {
                log::warn!("[mcp] {} skipped: setting {} not filled in", s.name, missing.key);
                return None;
            }
            let mut env: Vec<Value> = s.env.iter().map(|(k, v)| json!({ "name": k, "value": v })).collect();
            let mut headers: Vec<Value> = vec![];
            for f in &s.config {
                let Some(v) = vals.get(&f.key) else { continue };
                let v = format!("{}{}", f.prefix, v);
                if let Some(h) = f.where_.strip_prefix("header:") {
                    headers.push(json!({ "name": h, "value": v }));
                } else if f.where_ != "arg" {
                    env.push(json!({ "name": f.key, "value": v }));
                }
            }
            match s.transport.as_str() {
                "stdio" => Some(json!({
                    "name": s.name,
                    "command": resolve_program(&expand_home(app, &fill(&s.command.clone().unwrap_or_default(), &vals))),
                    "args": s.args.iter().map(|a| expand_home(app, &fill(a, &vals))).collect::<Vec<_>>(),
                    "env": env,
                })),
                "http" => Some(json!({
                    "type": "http",
                    "name": s.name,
                    "url": fill(&s.url.clone().unwrap_or_default(), &vals),
                    "headers": headers,
                })),
                _ => None,
            }
        })
        .collect()
}

/// Local, LAN, or a template the person's settings fill in - never the
/// open internet (a tool server is something on your machine or your network).
fn is_local_or_lan(url: &str) -> bool {
    if url.contains("${") {
        return true;
    }
    let Some(rest) = url.strip_prefix("http://").or_else(|| url.strip_prefix("https://")) else { return false };
    let host = rest.split(['/', ':']).next().unwrap_or("").to_lowercase();
    host == "localhost"
        || host.ends_with(".local")
        || host.ends_with(".lan")
        || host.ends_with(".home")
        || host.starts_with("127.")
        || host.starts_with("10.")
        || host.starts_with("192.168.")
        || (host.starts_with("172.") && host.split('.').nth(1).and_then(|s| s.parse::<u8>().ok()).map(|n| (16..=31).contains(&n)).unwrap_or(false))
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
            if !is_local_or_lan(url) {
                return Err("only servers on this computer or your own network can be added here".into());
            }
        }
        _ => return Err("transport must be stdio or http".into()),
    }
    let mut list = load(&app)?;
    let previous_values = list.iter().find(|s| s.name == name).map(|s| s.values.clone()).unwrap_or_default();
    list.retain(|s| s.name != name);
    let mut server = server;
    for (k, v) in previous_values {
        server.values.entry(k).or_insert(v);
    }
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
    Ok(which_sync(&program))
}

pub(crate) fn which_sync(program: &str) -> Option<String> {
    let p = program.trim();
    if p.is_empty() {
        return None;
    }
    if std::path::Path::new(p).is_file() {
        return Some(p.to_string());
    }
    let mut dirs: Vec<PathBuf> = std::env::var_os("PATH").map(|v| std::env::split_paths(&v).collect()).unwrap_or_default();
    // Installers put things where a freshly started app may not look yet.
    if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(home);
        for extra in [".local/bin", ".cargo/bin", ".bun/bin"] {
            dirs.push(home.join(extra));
        }
    }
    for extra in ["/opt/homebrew/bin", "/usr/local/bin", "/usr/bin"] {
        dirs.push(PathBuf::from(extra));
    }
    if cfg!(windows) {
        if let Ok(lad) = std::env::var("LOCALAPPDATA") {
            dirs.push(PathBuf::from(&lad).join("Programs").join("Git").join("cmd"));
            dirs.push(PathBuf::from(&lad).join("Microsoft").join("WinGet").join("Links"));
            dirs.push(PathBuf::from(&lad).join("Programs").join("uv"));
            dirs.push(PathBuf::from(&lad).join("uv"));
            dirs.push(PathBuf::from(&lad).join("Programs").join("Python").join("Launcher"));
        }
        if let Some(up) = std::env::var_os("USERPROFILE") {
            dirs.push(PathBuf::from(&up).join(".local").join("bin"));
            dirs.push(PathBuf::from(&up).join(".cargo").join("bin"));
        }
        if let Ok(pf) = std::env::var("ProgramFiles") {
            dirs.push(PathBuf::from(&pf).join("Git").join("cmd"));
            dirs.push(PathBuf::from(&pf).join("nodejs"));
        }
    }
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".EXE;.CMD;.BAT".into())
            .split(';')
            .map(|s| s.to_lowercase())
            .collect()
    } else {
        vec![String::new()]
    };
    for dir in dirs {
        for ext in &exts {
            let cand = dir.join(format!("{p}{ext}"));
            if cand.is_file() {
                return Some(cand.to_string_lossy().to_string());
            }
        }
    }
    None
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

/// How this machine can get a program a tool needs. `run` = the app can do
/// it here (an official installer script, or the system's package manager);
/// `terminal` = we open the person's terminal with the exact command (it
/// needs their password or an interactive step); `link` = a download page.
#[derive(Serialize, Clone, Debug)]
pub struct RequirementPlan {
    pub mode: String,
    pub command: String,
    pub note: String,
}

fn has(program: &str) -> bool {
    which_sync(program).is_some()
}

#[tauri::command]
pub async fn mcp_requirement_plan(program: String) -> Result<RequirementPlan, String> {
    let p = program.trim().to_lowercase();
    let plan = |mode: &str, command: &str, note: &str| RequirementPlan { mode: mode.into(), command: command.into(), note: note.into() };
    let winget = cfg!(windows) && has("winget");
    let brew = cfg!(target_os = "macos") && has("brew");
    let apt = cfg!(target_os = "linux") && has("apt-get");
    let dnf = cfg!(target_os = "linux") && has("dnf");
    let pacman = cfg!(target_os = "linux") && has("pacman");
    Ok(match p.as_str() {
        "uv" | "uvx" => {
            if cfg!(windows) {
                plan("run", "powershell -NoProfile -ExecutionPolicy ByPass -c \"irm https://astral.sh/uv/install.ps1 | iex\"", "Runs uv's official installer from astral.sh into your user folder. No admin rights needed.")
            } else {
                plan("run", "curl -LsSf https://astral.sh/uv/install.sh | sh", "Runs uv's official installer from astral.sh into ~/.local/bin. No admin rights needed.")
            }
        }
        "git" => {
            if winget { plan("run", "winget install --id Git.Git -e --source winget --accept-package-agreements --accept-source-agreements", "Installs Git with winget, Windows' own package manager.") }
            else if cfg!(target_os = "macos") { plan("terminal", "xcode-select --install", "macOS installs Git with its command line tools - a system dialog asks you to confirm.") }
            else if apt { plan("terminal", "sudo apt-get install -y git", "Needs your password, so it runs in your terminal.") }
            else if dnf { plan("terminal", "sudo dnf install -y git", "Needs your password, so it runs in your terminal.") }
            else if pacman { plan("terminal", "sudo pacman -S --noconfirm git", "Needs your password, so it runs in your terminal.") }
            else { plan("link", "https://git-scm.com/downloads", "Download Git from git-scm.com.") }
        }
        "node" | "npx" | "npm" => {
            if winget { plan("run", "winget install --id OpenJS.NodeJS.LTS -e --source winget --accept-package-agreements --accept-source-agreements", "Installs Node.js LTS with winget, Windows' own package manager.") }
            else if brew { plan("run", "brew install node", "Installs Node.js with Homebrew.") }
            else if apt { plan("terminal", "sudo apt-get install -y nodejs npm", "Needs your password, so it runs in your terminal.") }
            else if dnf { plan("terminal", "sudo dnf install -y nodejs npm", "Needs your password, so it runs in your terminal.") }
            else if pacman { plan("terminal", "sudo pacman -S --noconfirm nodejs npm", "Needs your password, so it runs in your terminal.") }
            else { plan("link", "https://nodejs.org/en/download", "Download Node.js from nodejs.org.") }
        }
        "python" | "python3" => {
            if winget { plan("run", "winget install --id Python.Python.3.12 -e --source winget --accept-package-agreements --accept-source-agreements", "Installs Python 3.12 with winget, Windows' own package manager.") }
            else if brew { plan("run", "brew install python", "Installs Python with Homebrew.") }
            else if apt { plan("terminal", "sudo apt-get install -y python3 python3-venv python3-pip", "Needs your password, so it runs in your terminal.") }
            else if dnf { plan("terminal", "sudo dnf install -y python3 python3-pip", "Needs your password, so it runs in your terminal.") }
            else if pacman { plan("terminal", "sudo pacman -S --noconfirm python python-pip", "Needs your password, so it runs in your terminal.") }
            else { plan("link", "https://www.python.org/downloads/", "Download Python from python.org.") }
        }
        "docker" => {
            if winget { plan("run", "winget install --id Docker.DockerDesktop -e --source winget --accept-package-agreements --accept-source-agreements", "Installs Docker Desktop with winget; it asks you to sign out and in once.") }
            else if brew { plan("run", "brew install --cask docker", "Installs Docker Desktop with Homebrew; open it once to finish setup.") }
            else if apt { plan("terminal", "sudo apt-get install -y docker.io && sudo usermod -aG docker $USER", "Needs your password, so it runs in your terminal; sign out and in afterwards.") }
            else if dnf { plan("terminal", "sudo dnf install -y docker && sudo systemctl enable --now docker && sudo usermod -aG docker $USER", "Needs your password, so it runs in your terminal.") }
            else if pacman { plan("terminal", "sudo pacman -S --noconfirm docker && sudo systemctl enable --now docker && sudo usermod -aG docker $USER", "Needs your password, so it runs in your terminal.") }
            else { plan("link", "https://docs.docker.com/get-docker/", "Get Docker from docker.com.") }
        }
        _ => plan("link", "", "No installer known for this program."),
    })
}

/// Run a `run`-mode plan here and report the tail of its output. The
/// button that calls this shows the exact command first.
#[tauri::command]
pub async fn mcp_requirement_install(program: String) -> Result<String, String> {
    let plan = mcp_requirement_plan(program.clone()).await?;
    if plan.mode != "run" {
        return Err("this one runs in your terminal".into());
    }
    let out = tauri::async_runtime::spawn_blocking(move || {
        if cfg!(windows) {
            // PowerShell directly - a quoted pipe inside `cmd /C` is not the
            // same command the person was shown.
            if let Some(rest) = plan.command.strip_prefix("powershell ") {
                let args: Vec<String> = shell_words(rest);
                std::process::Command::new("powershell").args(args).output()
            } else {
                std::process::Command::new("cmd").args(["/C", &plan.command]).output()
            }
        } else {
            std::process::Command::new("sh").args(["-c", &plan.command]).output()
        }
    })
    .await
    .map_err(|e| e.to_string())?
    .map_err(|e| format!("could not run the installer: {e}"))?;
    let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    let tail: String = text.lines().rev().take(6).collect::<Vec<_>>().into_iter().rev().collect::<Vec<_>>().join("\n");
    if !out.status.success() {
        return Err(format!("installer failed: {}", tail.trim()));
    }
    if !has(&program) {
        return Err(format!(
            "the installer finished but {program} was not found in the usual places - its last lines: {} - open a new terminal or restart the app, then Check again",
            tail.trim()
        ));
    }
    Ok(tail.trim().to_string())
}

/// Split a command line the way a shell would for our own plans: words,
/// double-quoted strings kept whole (quotes removed).
fn shell_words(s: &str) -> Vec<String> {
    let mut out = vec![];
    let mut cur = String::new();
    let mut quoted = false;
    for c in s.chars() {
        match c {
            '"' => quoted = !quoted,
            ' ' if !quoted => {
                if !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                }
            }
            _ => cur.push(c),
        }
    }
    if !cur.is_empty() {
        out.push(cur);
    }
    out
}

/// Save the settings a tool asked for. Secret-kind fields go to the
/// encrypted store; the rest sit with the server entry. An empty value
/// clears the setting.
#[tauri::command]
pub async fn mcp_set_config(app: AppHandle, name: String, values: HashMap<String, String>) -> Result<Vec<McpServer>, String> {
    let mut list = load(&app)?;
    let Some(server) = list.iter_mut().find(|s| s.name == name) else { return Err("no such tool".into()) };
    let mut secrets = secrets_load(&app).unwrap_or_default();
    for f in &server.config {
        let Some(v) = values.get(&f.key) else { continue };
        let v = v.trim().to_string();
        if f.kind == "secret" {
            let k = format!("{}:{}", server.name, f.key);
            if v.is_empty() { secrets.remove(&k); } else { secrets.insert(k, v); }
        } else if v.is_empty() {
            server.values.remove(&f.key);
        } else {
            server.values.insert(f.key.clone(), v);
        }
    }
    secrets_save(&app, &secrets)?;
    save(&app, &list)?;
    Ok(list)
}

/// Which of a tool's settings are filled in (secrets reported as present,
/// never returned). The page uses it for the "needs its settings" line.
#[tauri::command]
pub async fn mcp_config_status(app: AppHandle, name: String) -> Result<HashMap<String, bool>, String> {
    let list = load(&app)?;
    let Some(server) = list.iter().find(|s| s.name == name) else { return Err("no such tool".into()) };
    let secrets = secrets_load(&app).unwrap_or_default();
    Ok(server
        .config
        .iter()
        .map(|f| {
            let present = if f.kind == "secret" {
                secrets.get(&format!("{}:{}", server.name, f.key)).map(|v| !v.is_empty()).unwrap_or(false)
            } else {
                server.values.get(&f.key).map(|v| !v.trim().is_empty()).unwrap_or(false)
            };
            (f.key.clone(), present)
        })
        .collect())
}

#[derive(Serialize, Clone, Debug)]
pub struct SourceStatus {
    pub behind: bool,
    pub local: String,
    pub remote: String,
}

fn fetch_dir_of(app: &AppHandle, name: &str) -> Result<PathBuf, String> {
    let list = load(app)?;
    let s = list.iter().find(|s| s.name == name).ok_or("no such tool")?;
    let d = s.fetch_dir.clone().ok_or("this tool was not fetched by the app")?;
    let dir = PathBuf::from(expand_home(app, &d));
    if !dir.join(".git").is_dir() {
        return Err("the tool's source folder is missing - fetch it again".into());
    }
    Ok(dir)
}

/// One explicit network call: fetch the tool's source and say whether the
/// local copy is behind. Only ever on a click.
#[tauri::command]
pub async fn mcp_source_check(app: AppHandle, name: String) -> Result<SourceStatus, String> {
    let dir = fetch_dir_of(&app, &name)?;
    let d = dir.to_string_lossy().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let run = |args: &[&str]| -> Result<String, String> {
            let out = std::process::Command::new("git").arg("-C").arg(&d).args(args).output().map_err(|e| format!("git could not run: {e}"))?;
            if !out.status.success() {
                return Err(format!("git failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
            }
            Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
        };
        run(&["fetch", "--quiet"])?;
        let local = run(&["rev-parse", "HEAD"])?;
        let remote = run(&["rev-parse", "@{u}"])?;
        Ok(SourceStatus { behind: local != remote, local: local[..7.min(local.len())].to_string(), remote: remote[..7.min(remote.len())].to_string() })
    })
    .await
    .map_err(|e| e.to_string())?
}

/// Bring the fetched source up to date (fast-forward only).
#[tauri::command]
pub async fn mcp_source_update(app: AppHandle, name: String) -> Result<String, String> {
    let dir = fetch_dir_of(&app, &name)?;
    let d = dir.to_string_lossy().to_string();
    tauri::async_runtime::spawn_blocking(move || {
        let out = std::process::Command::new("git").args(["-C", &d, "pull", "--ff-only", "--quiet"]).output().map_err(|e| format!("git could not run: {e}"))?;
        if !out.status.success() {
            return Err(format!("update failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
        }
        let head = std::process::Command::new("git").args(["-C", &d, "rev-parse", "--short", "HEAD"]).output().map_err(|e| e.to_string())?;
        Ok(String::from_utf8_lossy(&head.stdout).trim().to_string())
    })
    .await
    .map_err(|e| e.to_string())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_replaces_every_known_key() {
        let mut v = HashMap::new();
        v.insert("HASS_URL".to_string(), "http://ha.local:8123".to_string());
        assert_eq!(fill("${HASS_URL}/api/mcp", &v), "http://ha.local:8123/api/mcp");
        assert_eq!(fill("${OTHER}/x", &v), "${OTHER}/x");
    }

    #[test]
    fn local_and_lan_only() {
        for ok in ["http://127.0.0.1:9191/mcp", "http://localhost/mcp", "http://homeassistant.local:8123/api/mcp", "http://192.168.1.5/", "http://10.0.0.2:3000/mcp", "http://172.16.0.9/", "${HASS_URL}/api/mcp", "https://printer.lan/"] {
            assert!(is_local_or_lan(ok), "{ok}");
        }
        for bad in ["https://example.com/mcp", "http://172.32.0.1/", "ftp://127.0.0.1/", "http://evil.example/x"] {
            assert!(!is_local_or_lan(bad), "{bad}");
        }
    }

    #[test]
    fn requirement_plan_knows_the_common_programs() {
        for p in ["uv", "git", "npx", "python3", "docker"] {
            let plan = tauri::async_runtime::block_on(mcp_requirement_plan(p.to_string())).unwrap();
            assert!(["run", "terminal", "link"].contains(&plan.mode.as_str()), "{p}: {}", plan.mode);
            assert!(!plan.command.is_empty() || plan.mode == "link", "{p} has no command");
        }
        let unknown = tauri::async_runtime::block_on(mcp_requirement_plan("nonesuch".to_string())).unwrap();
        assert_eq!(unknown.mode, "link");
    }

    #[test]
    fn which_finds_a_shell_and_not_nonsense() {
        assert!(which_sync("sh").is_some() || cfg!(windows));
        assert!(which_sync("definitely-not-a-program-xyz").is_none());
        assert!(which_sync("").is_none());
    }

    #[test]
    fn add_validates_transport_and_urls() {
        // pure checks mirrored from mcp_add: the LAN rule and the launcher shape
        assert!(!is_local_or_lan("http://example.com/"));
        assert!(is_local_or_lan("http://127.0.0.1:3000/mcp"));
        assert_eq!(crate::skills::normalize_name("My Tool!"), "my-tool");
    }
}

// --- Blender's own add-on (the Lab server talks to it inside Blender).
// Blender ships an extension CLI: build the package from the clone we
// fetched, then install + enable it. Only behind a button that says so.

fn blender_binary() -> Option<String> {
    if let Some(p) = which_sync("blender") {
        return Some(p);
    }
    let mut cands: Vec<PathBuf> = vec![PathBuf::from("/snap/bin/blender"), PathBuf::from("/Applications/Blender.app/Contents/MacOS/Blender")];
    if let Ok(pf) = std::env::var("ProgramFiles") {
        if let Ok(rd) = std::fs::read_dir(PathBuf::from(&pf).join("Blender Foundation")) {
            let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
            dirs.sort();
            for d in dirs.into_iter().rev() {
                cands.push(d.join("blender.exe"));
            }
        }
    }
    cands.into_iter().find(|c| c.is_file()).map(|c| c.to_string_lossy().to_string())
}

/// Blender's per-version config roots that may hold user extensions.
fn blender_extension_dirs() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = vec![];
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        roots.push(home.join(".config").join("blender"));
        roots.push(home.join("snap").join("blender").join("current").join(".config").join("blender"));
        roots.push(home.join("Library").join("Application Support").join("Blender"));
    }
    if let Ok(ad) = std::env::var("APPDATA") {
        roots.push(PathBuf::from(ad).join("Blender Foundation").join("Blender"));
    }
    let mut out = vec![];
    for r in roots {
        if let Ok(rd) = std::fs::read_dir(&r) {
            for e in rd.flatten() {
                let p = e.path().join("extensions").join("user_default");
                if p.is_dir() {
                    out.push(p);
                }
            }
        }
    }
    out
}

#[derive(Serialize, Clone, Debug)]
pub struct BlenderAddonStatus {
    pub blender: Option<String>,
    pub installed: bool,
    pub source_present: bool,
}

#[tauri::command]
pub async fn mcp_blender_addon_status(app: AppHandle) -> Result<BlenderAddonStatus, String> {
    let installed = blender_extension_dirs().iter().any(|d| d.join("mcp").is_dir());
    let src = PathBuf::from(expand_home(&app, "~/blender_mcp/addon/blender_mcp_addon"));
    Ok(BlenderAddonStatus { blender: blender_binary(), installed, source_present: src.join("blender_manifest.toml").is_file() })
}

/// Build the add-on package from the fetched clone and install + enable it
/// with Blender's extension CLI. Blender must be closed or it will not see
/// the new add-on until restarted - the page says so.
#[tauri::command]
pub async fn mcp_blender_addon_install(app: AppHandle) -> Result<String, String> {
    let blender = blender_binary().ok_or("Blender was not found on this computer")?;
    let src = PathBuf::from(expand_home(&app, "~/blender_mcp/addon/blender_mcp_addon"));
    if !src.join("blender_manifest.toml").is_file() {
        return Err("fetch the Blender tool first - the add-on's source comes with it".into());
    }
    let out_dir = std::env::temp_dir().join("your-own-ai-blender-addon");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let s = src.to_string_lossy().to_string();
    let o = out_dir.to_string_lossy().to_string();
    let result = tauri::async_runtime::spawn_blocking(move || -> Result<String, String> {
        let run = |args: &[&str]| -> Result<String, String> {
            let out = std::process::Command::new(&blender).arg("-b").arg("--command").arg("extension").args(args).output().map_err(|e| format!("Blender could not run: {e}"))?;
            let text = format!("{}\n{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
            if !out.status.success() {
                return Err(format!("Blender's installer failed: {}", text.lines().filter(|l| !l.trim().is_empty()).last().unwrap_or("").trim()));
            }
            Ok(text)
        };
        run(&["build", "--source-dir", &s, "--output-dir", &o])?;
        let zip = std::fs::read_dir(&o).map_err(|e| e.to_string())?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().map(|x| x == "zip").unwrap_or(false))
            .max_by_key(|p| std::fs::metadata(p).and_then(|m| m.modified()).ok())
            .ok_or("the add-on package was not built")?;
        run(&["install-file", &zip.to_string_lossy(), "--repo=user_default", "--enable"])?;
        // `--enable` in a headless run installs the files but the GUI does not
        // see the enabled flag until preferences are saved: enable + save.
        let out = std::process::Command::new(&blender)
            // Blender gates the add-on's LOCAL socket behind its own "Allow
            // Online Access" preference (off on a fresh install) - the add-on
            // refuses to start without it. The card says so before this runs.
            .args(["-b", "--python-expr", "import bpy; bpy.context.preferences.system.use_online_access = True; bpy.ops.preferences.addon_enable(module='bl_ext.user_default.mcp'); bpy.ops.wm.save_userpref()"])
            .output()
            .map_err(|e| format!("Blender could not run: {e}"))?;
        if !out.status.success() {
            return Err("the add-on installed but could not be enabled - enable MCP under Edit > Preferences > Extensions".into());
        }
        Ok("installed".into())
    })
    .await
    .map_err(|e| e.to_string())??;
    Ok(result)
}

/// The scratch workspace a chat "tools session" runs in - the agent needs a
/// folder, the conversation is not about one. Per AI, under app data, kept
/// between turns (a tool may leave files there; the person can find them).
#[tauri::command]
pub async fn tool_session_dir(app: AppHandle, ai_id: String) -> Result<String, String> {
    let safe: String = ai_id.chars().filter(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_').collect();
    if safe.is_empty() {
        return Err("no AI id".into());
    }
    let dir = app
        .path()
        .app_data_dir()
        .map_err(|e| format!("cannot resolve app data dir: {e}"))?
        .join("tool-sessions")
        .join(safe);
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create the tools workspace: {e}"))?;
    Ok(dir.to_string_lossy().to_string())
}

#[derive(Serialize, Clone, Debug)]
pub struct StartFailure {
    pub name: String,
    pub tail: String,
}

/// Which of the named servers failed to start in the last minute, judged
/// from the agent's per-server stderr logs (`~/.your-own-ai-build/logs/mcp/
/// <name>.stderr.log`). The only place a dead server shows up otherwise.
#[tauri::command]
pub async fn mcp_start_failures(names: Vec<String>) -> Result<Vec<StartFailure>, String> {
    let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from) else { return Ok(vec![]) };
    let dir = home.join(".your-own-ai-build").join("logs").join("mcp");
    let mut out = vec![];
    for name in names {
        let p = dir.join(format!("{name}.stderr.log"));
        let Ok(meta) = std::fs::metadata(&p) else { continue };
        let fresh = meta.modified().ok().and_then(|m| m.elapsed().ok()).map(|d| d.as_secs() < 90).unwrap_or(false);
        if !fresh {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&p) else { continue };
        let recent: Vec<&str> = text.lines().rev().take(40).collect::<Vec<_>>().into_iter().rev().collect();
        let failed = recent.iter().any(|l| l.contains("Traceback") || l.contains("Error:") || l.contains("error:") || l.contains("ModuleNotFoundError") || l.contains("command not found") || l.contains("No such file"));
        if failed {
            let tail = recent.iter().rev().filter(|l| !l.trim().is_empty()).take(3).collect::<Vec<_>>().into_iter().rev().map(|l| l.trim()).collect::<Vec<_>>().join(" | ");
            out.push(StartFailure { name, tail: tail.chars().take(300).collect() });
        }
    }
    Ok(out)
}
