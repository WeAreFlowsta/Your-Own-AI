//! Sign in with Flowsta — via the local Flowsta Vault (no browser, no
//! Flowsta API).
//!
//! Flow: probe Vault on localhost:27777-9 → proxy issues a challenge →
//! Vault signs it after user approval (its own dialog) → proxy verifies
//! and mints JWTs → tokens live in a Rust-side store (flowsta-auth.json)
//! and never enter the webview. Online-model requests (llm.rs) attach
//! the access token; billing identity joins via "Link my plan" in the
//! browser (the only browser step, and it rides Stripe checkout anyway).

use serde::{Deserialize, Serialize};
use tauri_plugin_store::StoreExt;

/// Dev-only endpoint overrides. Release builds ALWAYS use production, so
/// non-production URLs never live in the committed (open-source) code.
/// A dev build reads overrides from a gitignored `dev-urls.json` at the
/// project root, e.g. `{ "proxy_url": "...", "account_url": "..." }`
/// (env vars YOAI_PROXY_URL / YOAI_ACCOUNT_URL take precedence if set).
/// The file persists across restarts — no per-launch setup to forget.
#[cfg(debug_assertions)]
fn dev_override(env_key: &str, json_key: &str) -> Option<String> {
    if let Ok(v) = std::env::var(env_key) {
        if !v.is_empty() {
            return Some(v);
        }
    }
    use std::sync::OnceLock;
    static CFG: OnceLock<serde_json::Value> = OnceLock::new();
    let cfg = CFG.get_or_init(|| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../dev-urls.json");
        std::fs::read_to_string(path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(serde_json::Value::Null)
    });
    cfg.get(json_key).and_then(|v| v.as_str()).map(String::from)
}

pub fn proxy_url() -> String {
    const PROD: &str = "https://yoai-model-proxy-386500392150.us-central1.run.app";
    #[cfg(debug_assertions)]
    {
        return dev_override("YOAI_PROXY_URL", "proxy_url").unwrap_or_else(|| PROD.to_string());
    }
    #[cfg(not(debug_assertions))]
    PROD.to_string()
}

pub fn account_url() -> String {
    const PROD: &str = "https://yourownai.net/dashboard/";
    #[cfg(debug_assertions)]
    {
        return dev_override("YOAI_ACCOUNT_URL", "account_url").unwrap_or_else(|| PROD.to_string());
    }
    #[cfg(not(debug_assertions))]
    PROD.to_string()
}

/// Dev-portal client_id of the "Your Own AI" Holochain-type app
/// (registered 2026-06-13) — shown (API-verified) in Vault's approval
/// dialog + MAU analytics. Client ids are public identifiers by design.
pub(crate) const YOAI_HOLOCHAIN_CLIENT_ID: &str =
    "flowsta_app_583c054abe01f0179d2c396aaa8adcb754b8c5be8187471476bc3a060d44c4c4";

pub(crate) const AUTH_STORE: &str = "flowsta-auth.json";

/// Stable Origin for Vault IPC. Vault keys scope grants and linked-app
/// records by the caller's Origin header (browsers send it; Rust must).
pub(crate) const VAULT_ORIGIN: &str = "yoai://app";

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct VaultStatus {
    pub installed: bool,
    pub unlocked: bool,
    pub port: Option<u16>,
    pub agent_pub_key: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct FlowstaSession {
    pub signed_in: bool,
    pub agent_pub_key: Option<String>,
    pub did: Option<String>,
    pub tier: Option<String>,
    pub linked: Option<bool>,
    pub display_name: Option<String>,
    pub web_username: Option<String>,
    pub profile_picture: Option<String>,
}

/// One shared client so repeated Vault/proxy calls reuse connections
/// instead of opening (and leaking into TIME_WAIT) a fresh socket each
/// time — the churn that helped wedge Vault's IPC accept loop.
pub(crate) fn http() -> reqwest::Client {
    use std::sync::OnceLock;
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT
        .get_or_init(|| {
            reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(20))
                .pool_max_idle_per_host(2)
                .build()
                .expect("reqwest client")
        })
        .clone()
}

/// Last Vault port that answered, so the common path is a single probe
/// rather than scanning all three ports on every status poll.
fn cached_port() -> &'static std::sync::Mutex<Option<u16>> {
    use std::sync::OnceLock;
    static PORT: OnceLock<std::sync::Mutex<Option<u16>>> = OnceLock::new();
    PORT.get_or_init(|| std::sync::Mutex::new(None))
}

/// The Vault's three ports (it moves up when 27777 is taken).
const VAULT_PORTS: [u16; 3] = [27777, 27778, 27779];

/// Every Vault answering on this computer: (port, status JSON). All three
/// ports are asked at once. A Mac was seen with THREE Vault copies running
/// (27777, 27778, 27779, 2026-09-25): the first answer is not the right one.
async fn probe_vaults(timeout: std::time::Duration) -> Vec<(u16, serde_json::Value)> {
    let client = http();
    let one = |port: u16| {
        let client = client.clone();
        async move {
            let resp = client
                .get(format!("http://127.0.0.1:{}/status", port))
                .timeout(timeout)
                .send()
                .await
                .ok()?;
            let v = resp.json::<serde_json::Value>().await.ok()?;
            Some((port, v))
        }
    };
    let (a, b, c) = tokio::join!(one(VAULT_PORTS[0]), one(VAULT_PORTS[1]), one(VAULT_PORTS[2]));
    let mut mine = Vec::new();
    for (port, v) in [a, b, c].into_iter().flatten() {
        if vault_is_mine(port).await {
            mine.push((port, v));
        }
    }
    mine
}

/// Is the Vault answering on `port` running as THIS OS user? Loopback ports
/// are shared by every account on a computer: on 2026-09-25 a Mac had the
/// production Vault of ANOTHER user account on 27777 and a staging build on
/// 27778, and this app (and the login page) talked to them - a sign-in whose
/// dialog could never appear, and, with the unlocked-first pick, a false
/// "your Vault changed identity" card. Another user's Vault is ignored.
/// When the owner cannot be read (a tool missing, a localized netstat), the
/// Vault is kept: never lose the person's own Vault to a failed check.
/// The verdict is cached per port for a minute.
async fn vault_is_mine(port: u16) -> bool {
    use std::time::{Duration, Instant};
    static CACHE: std::sync::Mutex<Vec<(u16, bool, Instant)>> = std::sync::Mutex::new(Vec::new());
    if let Ok(c) = CACHE.lock() {
        if let Some((_, v, _)) = c.iter().find(|(p, _, at)| *p == port && at.elapsed() < Duration::from_secs(60)) {
            return *v;
        }
    }
    let read = tokio::task::spawn_blocking(move || listener_is_mine(port)).await.ok().flatten();
    let mine = read.unwrap_or(true);
    if let Ok(mut c) = CACHE.lock() {
        let said_before = c.iter().any(|(p, v, _)| *p == port && !*v);
        c.retain(|(p, _, _)| *p != port);
        c.push((port, mine, Instant::now()));
        if !mine && !said_before {
            log::info!("[vault] the Vault on port {port} runs as another user of this computer - ignored");
        }
    }
    mine
}

/// Some(true/false) when the owner of the process listening on `port` could
/// be read, None when it could not.
#[cfg(target_os = "linux")]
fn listener_is_mine(port: u16) -> Option<bool> {
    use std::os::unix::fs::MetadataExt;
    let me = std::fs::metadata("/proc/self").ok()?.uid();
    let mut seen: Option<bool> = None;
    for f in ["/proc/net/tcp", "/proc/net/tcp6"] {
        if let Ok(text) = std::fs::read_to_string(f) {
            for uid in proc_net_listen_uids(&text, port) {
                if uid == me {
                    return Some(true);
                }
                seen = Some(false);
            }
        }
    }
    seen
}

#[cfg(target_os = "macos")]
fn listener_is_mine(port: u16) -> Option<bool> {
    use std::os::unix::fs::MetadataExt;
    // This user's uid: the owner of their home folder.
    let me = std::fs::metadata(std::env::var_os("HOME")?).ok()?.uid();
    let out = std::process::Command::new("/usr/sbin/lsof")
        .args(["-nP", &format!("-iTCP:{port}"), "-sTCP:LISTEN", "-Fu"])
        .output()
        .ok()?;
    let uids = lsof_uids(&String::from_utf8_lossy(&out.stdout));
    if uids.contains(&me) {
        return Some(true);
    }
    if !uids.is_empty() {
        return Some(false);
    }
    // Unprivileged lsof does not list other users' processes: a Vault that
    // answers on the port but is not listed runs as someone else. lsof says
    // "nothing found" with exit 1 and no error text.
    if out.status.code() == Some(1) && out.stderr.is_empty() {
        return Some(false);
    }
    None
}

#[cfg(target_os = "windows")]
fn listener_is_mine(port: u16) -> Option<bool> {
    use std::os::windows::process::CommandExt;
    const NO_WINDOW: u32 = 0x0800_0000;
    let out = std::process::Command::new("netstat")
        .args(["-ano", "-p", "TCP"])
        .creation_flags(NO_WINDOW)
        .output()
        .ok()?;
    let pid = crate::llm::netstat_listening_pids(&String::from_utf8_lossy(&out.stdout), &port.to_string())
        .into_iter()
        .next()?;
    let list = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/V", "/FO", "CSV", "/NH"])
        .creation_flags(NO_WINDOW)
        .output()
        .ok()?;
    let user = tasklist_user(&String::from_utf8_lossy(&list.stdout))?;
    let name = std::env::var("USERNAME").ok()?;
    let domain = std::env::var("USERDOMAIN").unwrap_or_default();
    // Another user's process reads "N/A" (in the system's language) to an
    // unelevated caller - so anything but our own name is someone else.
    let me_full = format!("{domain}\\{name}");
    Some(user.eq_ignore_ascii_case(&me_full) || user.eq_ignore_ascii_case(&name))
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn listener_is_mine(_port: u16) -> Option<bool> {
    None
}

/// uids of the processes LISTENING on `port` in a /proc/net/tcp(6) table.
#[allow(dead_code)]
pub(crate) fn proc_net_listen_uids(text: &str, port: u16) -> Vec<u32> {
    text.lines()
        .skip(1)
        .filter_map(|line| {
            let c: Vec<&str> = line.split_whitespace().collect();
            // sl local_address rem_address st tx:rx tr:when retrnsmt uid ...
            if c.len() < 8 || c[3] != "0A" {
                return None; // 0A = LISTEN
            }
            let p = u16::from_str_radix(c[1].rsplit(':').next()?, 16).ok()?;
            if p != port {
                return None;
            }
            c[7].parse().ok()
        })
        .collect()
}

/// uids in `lsof -Fu` output (lines "u<uid>").
#[allow(dead_code)]
pub(crate) fn lsof_uids(text: &str) -> Vec<u32> {
    text.lines().filter_map(|l| l.strip_prefix('u')?.trim().parse().ok()).collect()
}

/// The "User Name" column of one `tasklist /V /FO CSV /NH` line.
#[allow(dead_code)]
pub(crate) fn tasklist_user(text: &str) -> Option<String> {
    let line = text.lines().find(|l| l.starts_with('"'))?;
    let cols: Vec<&str> = line.trim().trim_matches('"').split("\",\"").collect();
    // Image, PID, Session name, Session#, Mem usage, Status, User name, ...
    cols.get(6).map(|s| s.to_string())
}

#[cfg(test)]
mod vault_owner_tests {
    use super::*;

    #[test]
    fn proc_net_names_the_listener_uid_for_the_port() {
        // 27777 = 0x6C81, 127.0.0.1 = 0100007F; state 0A = LISTEN, 01 = ESTABLISHED.
        let text = "  sl  local_address rem_address   st tx_queue rx_queue tr tm->when retrnsmt   uid  timeout inode
   0: 0100007F:6C81 00000000:0000 0A 00000000:00000000 00:00000000 00000000   501        0 11111 1
   1: 0100007F:6C82 00000000:0000 0A 00000000:00000000 00:00000000 00000000  1000        0 22222 1
   2: 0100007F:D1F4 0100007F:6C81 01 00000000:00000000 00:00000000 00000000  1000        0 33333 1
";
        assert_eq!(proc_net_listen_uids(text, 27777), vec![501]);
        assert_eq!(proc_net_listen_uids(text, 27778), vec![1000]);
        assert!(proc_net_listen_uids(text, 27779).is_empty());
    }

    #[test]
    fn lsof_fu_lists_uids() {
        assert_eq!(lsof_uids("p4120\nu501\nf12\n"), vec![501]);
        assert!(lsof_uids("").is_empty());
    }

    #[test]
    fn tasklist_user_is_the_seventh_column() {
        let mine = "\"Flowsta Vault.exe\",\"4120\",\"Console\",\"1\",\"120,332 K\",\"Running\",\"DESKTOP-1\\eric\",\"0:00:05\",\"Flowsta Vault\"\r\n";
        assert_eq!(tasklist_user(mine).as_deref(), Some("DESKTOP-1\\eric"));
        let other = "\"Flowsta Vault.exe\",\"5000\",\"Console\",\"2\",\"98,000 K\",\"Unknown\",\"N/A\",\"0:00:00\",\"N/A\"\r\n";
        assert_eq!(tasklist_user(other).as_deref(), Some("N/A"));
        assert_eq!(tasklist_user("INFO: No tasks are running."), None);
    }
}

/// The Vault to talk to: an unlocked one first (it is the one the person is
/// using), then one that is set up, then the lowest port that answered.
fn pick_vault(answers: &[(u16, serde_json::Value)]) -> Option<&(u16, serde_json::Value)> {
    answers
        .iter()
        .find(|(_, v)| v["unlocked"].as_bool().unwrap_or(false))
        .or_else(|| answers.iter().find(|(_, v)| v["initialized"].as_bool().unwrap_or(true)))
        .or_else(|| answers.first())
}

/// Probe localhost for a running Vault.
pub async fn find_vault() -> VaultStatus {
    let answers = probe_vaults(std::time::Duration::from_secs(4)).await;
    match pick_vault(&answers) {
        Some((port, v)) => {
            *cached_port().lock().unwrap() = Some(*port);
            VaultStatus {
                installed: true,
                unlocked: v["unlocked"].as_bool().unwrap_or(false),
                port: Some(*port),
                agent_pub_key: v["agent_pub_key"].as_str().map(String::from),
            }
        }
        None => {
            *cached_port().lock().unwrap() = None;
            VaultStatus::default()
        }
    }
}

#[tauri::command]
pub async fn flowsta_vault_status() -> VaultStatus {
    find_vault().await
}

/// One-time /link-identity ceremony: Vault shows the app's API-verified
/// name AND the registered scopes for approval, then stores the scope
/// grant (keyed by client_id, resolved via our Origin). This is what
/// unlocks profile fields (display name / username / avatar) on /status.
/// The app_agent_pub_key is a random install identifier in agent-key
/// format — YOAI's real per-AI agents stay private by design.
async fn ensure_linked(
    app: &tauri::AppHandle,
    client: &reqwest::Client,
    port: u16,
) -> Result<(), String> {
    let store = app.store(crate::profile::store_path(&app, AUTH_STORE)).map_err(|e| e.to_string())?;
    // Don't trust a stale local link flag. Vault drops every app link on reset,
    // so confirm the link still exists there before skipping re-link. A wiped
    // Vault would otherwise leave YOAI absent from its connected apps and make
    // /authenticate fail (it requires a current link). If Vault is unreachable
    // we can't tell — assume linked and let /authenticate surface any problem.
    if store.get("link_done").and_then(|v| v.as_bool()).unwrap_or(false) {
        match store.get("app_link_key").and_then(|v| v.as_str().map(String::from)) {
            Some(key) => match vault_link_active(client, port, &key).await {
                Some(true) | None => return Ok(()),
                Some(false) => {
                    // Link was wiped on the Vault side — fall through to re-link
                    // (reusing the same app_link_key already in the store).
                    store.delete("link_done");
                    let _ = store.save();
                }
            },
            None => {
                store.delete("link_done");
                let _ = store.save();
            }
        }
    }

    let link_key = match store.get("app_link_key").and_then(|v| v.as_str().map(String::from)) {
        Some(k) => k,
        None => {
            use base64::Engine;
            use rand::RngCore;
            let mut raw = [0u8; 39];
            raw[0] = 0x84;
            raw[1] = 0x20;
            raw[2] = 0x24;
            rand::thread_rng().fill_bytes(&mut raw[3..]);
            let key = format!("u{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw));
            store.set("app_link_key", serde_json::json!(key.clone()));
            let _ = store.save();
            key
        }
    };

    // The Vault's link dialog waits 60 s for the person; the shared client's
    // 20 s default cut the ceremony off while they were still reading it.
    let resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{}/link-identity", port))
        .header("Origin", VAULT_ORIGIN)
        .timeout(std::time::Duration::from_secs(75))
        .json(&serde_json::json!({
            "app_name": "Your Own AI",
            "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
            "app_agent_pub_key": link_key,
        }))
        .send()
        .await
        .map_err(|e| format!("vault unreachable: {}", e))?
        .json()
        .await
        .map_err(|e| format!("bad link response: {}", e))?;
    if !resp["success"].as_bool().unwrap_or(false) {
        return Err(resp["error"].as_str().unwrap_or("link_denied").to_string());
    }
    store.set("link_done", serde_json::json!(true));
    store.save().map_err(|e| e.to_string())?;
    Ok(())
}

/// Scope-gated profile from Vault /status (requires the link grant).
async fn fetch_vault_profile(
    app: &tauri::AppHandle,
    client: &reqwest::Client,
    port: u16,
) {
    if let Ok(resp) = client
        .get(format!("http://127.0.0.1:{}/status", port))
        .header("Origin", VAULT_ORIGIN)
        .send()
        .await
    {
        if let Ok(v) = resp.json::<serde_json::Value>().await {
            if let Ok(store) = app.store(crate::profile::store_path(&app, AUTH_STORE)) {
                store.set("display_name", v["display_name"].clone());
                store.set("web_username", v["web_username"].clone());
                store.set("profile_picture", v["profile_picture"].clone());
                let _ = store.save();
            }
        }
    }
}

#[tauri::command]
pub async fn flowsta_sign_in(app: tauri::AppHandle) -> Result<FlowstaSession, String> {
    // After a switch this profile still belongs to the previous identity;
    // signing in here would put the new identity's session in the old
    // identity's folder. The restart opens the right one.
    if crate::identity_watch::switched() {
        use tauri::Emitter as _;
        let _ = app.emit("vault-identity-switched", serde_json::json!({}));
        return Err("identity_switched".into());
    }
    let vault = find_vault().await;
    let port = vault.port.ok_or("vault_not_found")?;
    if !vault.unlocked {
        return Err("vault_locked".into());
    }
    let client = http();

    // 0. One-time scope-granting link (Vault shows app + scopes dialog)
    ensure_linked(&app, &client, port).await?;

    // 1. Challenge from the proxy
    let resp = client
        .post(format!("{}/auth/vault/challenge", proxy_url()))
        // GFE returns 411 for POSTs without Content-Length; an empty JSON
        // body sets it.
        .json(&serde_json::json!({}))
        .send()
        .await
        .map_err(|e| format!("proxy unreachable: {}", e))?;
    let status = resp.status();
    let body = resp.text().await.unwrap_or_default();
    if !status.is_success() {
        return Err(format!("challenge failed ({}): {}", status, body.chars().take(200).collect::<String>()));
    }
    let challenge: serde_json::Value = serde_json::from_str(&body)
        .map_err(|e| format!("bad challenge response ({}; body: {:?}): {}", status, body.chars().take(200).collect::<String>(), e))?;
    let challenge = challenge["challenge"]
        .as_str()
        .ok_or("no challenge issued")?
        .to_string();

    // 2. Vault signs it (user approves in Vault's own dialog). The call
    // blocks until the user responds, so a long timeout; a dropped
    // connection here means Vault stopped or locked mid-approval.
    let auth_resp = client
        .post(format!("http://127.0.0.1:{}/authenticate", port))
        .header("Origin", VAULT_ORIGIN)
        // A locked Vault holds the request ~55 s for the unlock, then runs
        // its 60 s dialog - budget past both.
        .timeout(std::time::Duration::from_secs(125))
        .json(&serde_json::json!({
            "app_name": "Your Own AI",
            "challenge": challenge,
            "reason": "Sign in to use online models",
            "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
        }))
        .send()
        .await
        .map_err(|_| "vault_interrupted".to_string())?;
    if auth_resp.status() == reqwest::StatusCode::FORBIDDEN {
        // vault_locked / user_denied
        let body: serde_json::Value = auth_resp.json().await.unwrap_or_default();
        return Err(body["error"].as_str().unwrap_or("vault_denied").to_string());
    }
    let auth: serde_json::Value = auth_resp
        .json()
        .await
        .map_err(|e| format!("bad vault response: {}", e))?;
    if !auth["success"].as_bool().unwrap_or(false) {
        return Err(auth["error"]
            .as_str()
            .unwrap_or("vault_denied")
            .to_string());
    }
    let (agent_pub_key, did, signature) = (
        auth["agent_pub_key"].as_str().unwrap_or_default().to_string(),
        auth["did"].as_str().unwrap_or_default().to_string(),
        auth["signature"].as_str().unwrap_or_default().to_string(),
    );

    // 3. Trade the signature for proxy JWTs
    let tokens: serde_json::Value = client
        .post(format!("{}/auth/vault/token", proxy_url()))
        .json(&serde_json::json!({
            "challenge": challenge,
            "agent_pub_key": agent_pub_key,
            "did": did,
            "signature": signature,
        }))
        .send()
        .await
        .map_err(|e| format!("proxy unreachable: {}", e))?
        .json()
        .await
        .map_err(|e| format!("bad token response: {}", e))?;
    let access = tokens["access_token"].as_str().ok_or_else(|| {
        tokens["error"]["message"]
            .as_str()
            .unwrap_or("token_mint_failed")
            .to_string()
    })?;

    // 4. Persist Rust-side; tokens never enter the webview
    let store = app.store(crate::profile::store_path(&app, AUTH_STORE)).map_err(|e| e.to_string())?;
    store.set("access_token", serde_json::json!(access));
    store.set("refresh_token", tokens["refresh_token"].clone());
    store.set("agent_pub_key", serde_json::json!(agent_pub_key));
    store.set("did", serde_json::json!(did));
    store.set(
        "expires_at",
        serde_json::json!(now_secs() + tokens["expires_in"].as_u64().unwrap_or(86400) - 300),
    );
    // First sign-in on this install: record who this device's escrow and
    // data backups belong to. A LATER sign-in under a different identity
    // deliberately does NOT re-point it - the escrow gate fails closed and
    // the account panel surfaces the mismatch instead (the local data still
    // belongs to whoever created it).
    if store
        .get(crate::vault_escrow::ESCROW_OWNER_KEY)
        .and_then(|v| v.as_str().map(String::from))
        .is_none()
    {
        store.set(
            crate::vault_escrow::ESCROW_OWNER_KEY,
            serde_json::json!(agent_pub_key),
        );
    }
    store.save().map_err(|e| e.to_string())?;
    crate::profile::bind(&app, &agent_pub_key);

    // Profile (display name / username / avatar) via the scope grant
    fetch_vault_profile(&app, &client, port).await;

    session_from_store(&app).await
}

#[tauri::command]
pub async fn flowsta_sign_out(app: tauri::AppHandle) -> Result<(), String> {
    let store = app.store(crate::profile::store_path(&app, AUTH_STORE)).map_err(|e| e.to_string())?;
    // Clear the session but keep the link ceremony state (link_done,
    // app_link_key) — the Vault-side link persists, so re-linking on
    // every sign-in would just re-prompt the user pointlessly.
    for key in ["access_token", "refresh_token", "agent_pub_key", "did", "expires_at",
                "display_name", "web_username", "profile_picture"] {
        store.delete(key);
    }
    store.save().map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn flowsta_session(app: tauri::AppHandle) -> Result<FlowstaSession, String> {
    session_from_store(&app).await
}

/// This month's online usage against the plan allowance, for the Settings
/// account card and the optional header ticker. None when signed out or the
/// proxy is unreachable - display surfaces simply hide.
#[derive(Debug, serde::Serialize)]
pub struct UsageSummary {
    pub month: String,
    pub tier: String,
    pub cost_usd: f64,
    pub allowance_usd: f64,
    /// The plan's monthly price - the allowance's origin. 0 on older proxies.
    #[serde(default)]
    pub plan_usd: f64,
    pub overage_usd: f64,
    pub overage_opt_in: bool,
    pub requests: u64,
    /// An overage invoice the card could not pay: online models pause past
    /// the allowance until it is paid. Absent on older proxies.
    #[serde(default)]
    pub overage_hold: Option<OverageHold>,
}

#[derive(serde::Serialize, serde::Deserialize, Clone, Debug)]
pub struct OverageHold {
    pub invoice: String,
    pub amount_usd: f64,
    pub month: String,
    #[serde(default)]
    pub since: Option<String>,
    #[serde(default)]
    pub hosted_invoice_url: Option<String>,
}

#[tauri::command]
pub async fn flowsta_usage(app: tauri::AppHandle) -> Result<Option<UsageSummary>, String> {
    let Ok(token) = get_access_token(&app).await else {
        return Ok(None);
    };
    let resp = match http()
        .get(format!("{}/billing/usage", proxy_url()))
        .bearer_auth(&token)
        .send()
        .await
    {
        Ok(r) if r.status().is_success() => r,
        _ => return Ok(None),
    };
    let Ok(v) = resp.json::<serde_json::Value>().await else {
        return Ok(None);
    };
    Ok(Some(UsageSummary {
        month: v["month"].as_str().unwrap_or_default().to_string(),
        tier: v["tier"].as_str().unwrap_or("free").to_string(),
        cost_usd: v["cost_usd"].as_f64().unwrap_or(0.0),
        allowance_usd: v["allowance_usd"].as_f64().unwrap_or(0.0),
        plan_usd: v["plan_usd"].as_f64().unwrap_or(0.0),
        overage_usd: v["overage_usd"].as_f64().unwrap_or(0.0),
        overage_opt_in: v["overage_opt_in"].as_bool().unwrap_or(false),
        requests: v["requests"].as_u64().unwrap_or(0),
        overage_hold: serde_json::from_value::<Option<OverageHold>>(v["overage_hold"].clone()).unwrap_or(None),
    }))
}

/// Keep the online proxy warm while the app is open. It scales to zero and a
/// cold start adds ~3.5s to the first online request after idle (measured
/// 2026-07-17); a 10-minute /health ping is effectively free and avoids
/// paying for an always-on instance. SIGNED-IN users only: an offline-only
/// install must never emit periodic network beacons.
pub fn spawn_proxy_keepalive(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let client = reqwest::Client::new();
        loop {
            tokio::time::sleep(std::time::Duration::from_secs(600)).await;
            let signed_in = session_from_store(&app)
                .await
                .map(|s| s.signed_in)
                .unwrap_or(false);
            if !signed_in {
                continue;
            }
            let url = format!("{}/health", proxy_url());
            let _ = client
                .get(&url)
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await;
        }
    });
}

/// URL the frontend opens (system browser) to link this device's plan.
/// Async so it can refuse a stale key: the URL carries the stored identity
/// into a plan-linking flow, so a Vault that is unlocked under a DIFFERENT
/// key must block it (the positive-mismatch wipe may not have run yet).
/// Locked/unreachable keeps the stored key - same contract as the session.
#[tauri::command]
pub async fn flowsta_link_url(app: tauri::AppHandle) -> Result<String, String> {
    if !session_identity_ok(&app).await {
        return Err("identity_mismatch".into());
    }
    let store = app.store(crate::profile::store_path(&app, AUTH_STORE)).map_err(|e| e.to_string())?;
    let key = store
        .get("agent_pub_key")
        .and_then(|v| v.as_str().map(String::from))
        .ok_or("not signed in")?;
    Ok(format!("{}?link={}", account_url(), key))
}

/// Plain account URL (no `?link=`) — for "Manage plan" when the device is
/// already set up. `flowsta_link_url` always appends the device key, which
/// re-triggers the account page's link-confirmation prompt; this doesn't.
#[tauri::command]
pub fn flowsta_account_url() -> String {
    account_url()
}

/// Per-model rates from the proxy (USD, YOAI margin already applied — i.e. what
/// the user actually pays). Provider list prices + margin stay server-side.
#[derive(Serialize, Clone)]
pub struct OnlinePricing {
    pub input_per_mtok: f64,
    pub output_per_mtok: f64,
    pub request_fee_usd: f64,
    /// Extra per-web-search-call fee (web-search models only); None otherwise.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub search_per_call_usd: Option<f64>,
    /// Rate for input tokens the provider serves from its prompt cache (the
    /// earlier turns of a conversation) - a fraction of input_per_mtok. None
    /// when the provider has no cache pricing.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cached_input_per_mtok: Option<f64>,
}

/// The catalog's routing block for a model (optional; absent on older
/// catalogs). Lets the catalog promote a model into a routing slot or
/// declare its capability scores without an app release.
#[derive(Serialize, Clone, Debug, Default)]
pub struct OnlineRouting {
    /// Slots this model is the recommended default for: everyday | hard |
    /// hard_code | hard_general | fresh | agent | plan.
    pub slots: Vec<String>,
    /// Capability scores (0-10): overall, coding, reasoning, math, vision, medical.
    pub caps: Option<[u8; 6]>,
    /// Tool-driving capability through the proxy (0-9).
    pub tools: Option<u8>,
    /// everyday | flagship | search
    pub tier: Option<String>,
}

#[derive(Serialize, Clone)]
pub struct OnlineModel {
    pub id: String,
    pub display_name: String,
    pub description: String,
    pub context_window: u64,
    /// Whether this model accepts image inputs (from the proxy catalog). The app
    /// won't send an attached image to a model where this is false.
    pub vision: bool,
    pub category: String,
    /// Every shelf this model belongs to (proxy defaults it to [category]).
    pub categories: Vec<String>,
    /// ISO ship date from the catalog - the online page's "Newest" sort key.
    pub released: Option<String>,
    pub pricing: Option<OnlinePricing>,
    /// Catalog routing block (see OnlineRouting); None on older catalogs.
    pub routing: Option<OnlineRouting>,
}
// ⚠️ This struct is the bridge for /v1/models: a field the frontend reads
// but isn't parsed here silently vanishes - the frontend types are optional,
// so neither tsc nor cargo notices. "Newest" sorted alphabetically and
// multi-shelf membership never rendered for WEEKS because released and
// categories were missing from this parse. New catalog field = add it here
// in the same change.

/// Short session cache for the online catalog. The router asks for this
/// list on EVERY agent step (~220-300 ms of network each time - measured
/// 38 times in one real session, i.e. ~10 s of pure waiting) and the
/// catalog changes on the order of days. 60 s is far longer than any gap
/// between agent steps and far shorter than a catalog change matters;
/// failures are not cached so a flaky moment retries next call.
static ONLINE_MODELS_CACHE: std::sync::OnceLock<
    tokio::sync::Mutex<Option<(std::time::Instant, Vec<OnlineModel>)>>,
> = std::sync::OnceLock::new();
const ONLINE_MODELS_TTL: std::time::Duration = std::time::Duration::from_secs(60);

#[tauri::command]
pub async fn list_online_models() -> Result<Vec<OnlineModel>, String> {
    let cache = ONLINE_MODELS_CACHE.get_or_init(|| tokio::sync::Mutex::new(None));
    if let Some((at, list)) = cache.lock().await.as_ref() {
        if at.elapsed() < ONLINE_MODELS_TTL {
            return Ok(list.clone());
        }
    }
    let fresh = fetch_online_models().await?;
    *cache.lock().await = Some((std::time::Instant::now(), fresh.clone()));
    Ok(fresh)
}

async fn fetch_online_models() -> Result<Vec<OnlineModel>, String> {
    let v: serde_json::Value = http()
        .get(format!("{}/v1/models", proxy_url()))
        .send()
        .await
        .map_err(|e| format!("proxy unreachable: {}", e))?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let list = v["data"]
        .as_array()
        .map(|arr| {
            arr.iter()
                .map(|m| OnlineModel {
                    id: format!("online:{}", m["id"].as_str().unwrap_or_default()),
                    display_name: m["display_name"].as_str().unwrap_or_default().to_string(),
                    description: m["description"].as_str().unwrap_or_default().to_string(),
                    context_window: m["context_window"].as_u64().unwrap_or(0),
                    vision: m["vision"].as_bool().unwrap_or(false),
                    category: m["category"].as_str().unwrap_or("chat").to_string(),
                    categories: m["categories"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|x| x.as_str().map(str::to_string))
                                .collect::<Vec<_>>()
                        })
                        .filter(|v| !v.is_empty())
                        .unwrap_or_else(|| {
                            vec![m["category"].as_str().unwrap_or("chat").to_string()]
                        }),
                    released: m["released"].as_str().map(str::to_string),
                    routing: m["routing"].as_object().map(|r| {
                        let u8_of = |k: &str| r.get("caps").and_then(|c| c.get(k)).and_then(|x| x.as_u64()).map(|x| x.min(10) as u8);
                        let caps = match (u8_of("overall"), u8_of("coding"), u8_of("reasoning"), u8_of("math")) {
                            (Some(o), Some(c), Some(re), Some(ma)) => Some([o, c, re, ma, u8_of("vision").unwrap_or(0), u8_of("medical").unwrap_or(3)]),
                            _ => None,
                        };
                        OnlineRouting {
                            slots: r.get("slots").and_then(|v| v.as_array()).map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
                            caps,
                            tools: r.get("tools").and_then(|x| x.as_u64()).map(|x| x.min(9) as u8),
                            tier: r.get("tier").and_then(|x| x.as_str()).map(str::to_string),
                        }
                    }),
                    pricing: m["pricing"].as_object().map(|p| OnlinePricing {
                        input_per_mtok: p.get("input_per_mtok").and_then(|x| x.as_f64()).unwrap_or(0.0),
                        output_per_mtok: p.get("output_per_mtok").and_then(|x| x.as_f64()).unwrap_or(0.0),
                        request_fee_usd: p.get("request_fee_usd").and_then(|x| x.as_f64()).unwrap_or(0.0),
                        search_per_call_usd: p.get("search_per_call_usd").and_then(|x| x.as_f64()),
                        cached_input_per_mtok: p.get("cached_input_per_mtok").and_then(|x| x.as_f64()),
                    }),
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(list)
}

/// Every key written during sign-in, link ceremony included. Used to wipe a
/// session that belongs to a *different* identity — unlike `flowsta_sign_out`,
/// the Vault-side link is also gone in that case (Vault was reset, or it's
/// another user), so `link_done`/`app_link_key` must clear too for a clean
/// re-link on the next sign-in.
#[allow(dead_code)]
const SESSION_KEYS_FULL: [&str; 10] = [
    "access_token",
    "refresh_token",
    "agent_pub_key",
    "did",
    "expires_at",
    "display_name",
    "web_username",
    "profile_picture",
    "link_done",
    "app_link_key",
];

/// Guard against spending or displaying a cached session under the wrong
/// identity.
///
/// Returns `true` when the cached tokens are safe to use: either the stored
/// identity matches the currently-unlocked Vault, or we can't tell because
/// Vault is locked/absent (the session still works via the proxy JWT, so we
/// leave it intact). Returns `false` — and wipes the cached session — only on a
/// POSITIVE mismatch: Vault is unlocked under a different `agent_pub_key`. That
/// happens when Vault is reset and reconnected with a different recovery
/// phrase, or a second person uses this machine. Without this, YOAI would show
/// the prior identity's plan and bill their subscription for new usage. The
/// same-identity reconnect keeps working untouched: the key is deterministic,
/// so it matches.
async fn session_identity_ok(app: &tauri::AppHandle) -> bool {
    // After a switch nothing is authorised for this profile until a restart
    // opens the new identity's profile. Nothing is wiped.
    if crate::identity_watch::switched() {
        return false;
    }
    // 60s positive cache: this guard runs before EVERY online request and
    // its Vault /status probe can stall up to 1.5s. A switch clears it
    // (`clear_identity_cache`); the watcher sees one within ten seconds.
    if let Ok(guard) = IDENTITY_OK_AT.lock() {
        if let Some(t) = *guard {
            if t.elapsed() < std::time::Duration::from_secs(60) {
                return true;
            }
        }
    }
    let ok = session_identity_ok_uncached(app).await;
    if ok {
        if let Ok(mut guard) = IDENTITY_OK_AT.lock() {
            *guard = Some(std::time::Instant::now());
        }
    }
    ok
}

async fn session_identity_ok_uncached(app: &tauri::AppHandle) -> bool {
    let store = match app.store(crate::profile::store_path(&app, AUTH_STORE)) {
        Ok(s) => s,
        Err(_) => return true,
    };
    let stored = match store
        .get("agent_pub_key")
        .and_then(|v| v.as_str().map(String::from))
    {
        Some(k) => k,
        None => return true, // no cached session to protect
    };
    // Only a reachable, unlocked Vault under a DIFFERENT key invalidates the
    // session. If Vault is down/slow we get None and keep the cached session —
    // online models are designed to keep working off the proxy JWT without a
    // live Vault, so we must NOT block or log out on an unreachable Vault.
    match vault_identity_quick().await {
        Some((true, Some(ref vault_key))) if !crate::identity_watch::same_identity(vault_key, &stored) => {
            // The Vault holds another identity. This session (and this
            // profile) belong to `stored`: refuse, and hand over to the
            // switch path - which keeps the session for when `stored` comes
            // back. It used to be wiped, which signed the old identity out
            // for good.
            crate::identity_watch::note_switch(app, Some(&stored), vault_key);
            false
        }
        _ => true,
    }
}

/// Fast read of the Vault's `(unlocked, agent_pub_key)` for the identity
/// guard and the launch-time profile pick: all three ports at once with a
/// short timeout, the unlocked Vault first (one probe used to hit 27777
/// only, and at launch the cache is empty - a locked stray copy there hid
/// the unlocked Vault on 27778). A down/slow Vault yields `None` ("can't
/// tell"), so the caller keeps the cached session instead of stalling.
pub(crate) async fn vault_identity_quick() -> Option<(bool, Option<String>)> {
    let answers = probe_vaults(std::time::Duration::from_millis(1500)).await;
    let (port, v) = pick_vault(&answers)?;
    *cached_port().lock().unwrap() = Some(*port);
    Some((
        v["unlocked"].as_bool().unwrap_or(false),
        v["agent_pub_key"].as_str().map(String::from),
    ))
}

/// The identity of the Flowsta session stored in this profile, if any.
pub(crate) fn stored_session_key(app: &tauri::AppHandle) -> Option<String> {
    let store = app.store(crate::profile::store_path(app, AUTH_STORE)).ok()?;
    store.get("agent_pub_key").and_then(|v| v.as_str().map(String::from))
}

/// The identity guard's positive cache (see `session_identity_ok`).
static IDENTITY_OK_AT: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);

/// Forget the positive cache - a switch must be felt on the next request,
/// not up to a minute later.
pub(crate) fn clear_identity_cache() {
    if let Ok(mut g) = IDENTITY_OK_AT.lock() {
        *g = None;
    }
}

/// One launch-time reconcile pass. Returns `true` when there's nothing more to
/// do (no session, already linked, just re-linked, identity mismatch, or a link
/// error) and `false` only when Vault wasn't reachable/unlocked yet, so a retry
/// later may succeed. Keeps Vault's connected-apps list accurate after a reset:
/// if YOAI has a session but its link was wiped, it re-links (which pops Vault's
/// approval), matching ProofPoll.
async fn reconcile_vault_link(app: &tauri::AppHandle) -> bool {
    let store = match app.store(crate::profile::store_path(&app, AUTH_STORE)) {
        Ok(s) => s,
        Err(_) => return true,
    };
    let stored_key = match store
        .get("agent_pub_key")
        .and_then(|v| v.as_str().map(String::from))
    {
        Some(k) => k,
        None => return true, // no session to reconnect
    };
    let vault = find_vault().await;
    if !vault.unlocked {
        return false; // Vault not ready (locked/absent) — worth retrying
    }
    let port = match vault.port {
        Some(p) => p,
        None => return false,
    };
    // Never re-link a stale session onto a different identity; the session guard
    // clears it on the next read.
    if let Some(ref vault_key) = vault.agent_pub_key {
        if vault_key != &stored_key {
            return true;
        }
    }
    let client = http();
    if let Err(e) = ensure_linked(app, &client, port).await {
        log::warn!("[flowsta] launch re-link failed: {}", e);
    }
    true
}

/// Launch-time Vault re-link with a short retry window, so a Vault that's
/// unlocked shortly after YOAI starts still gets reconnected. Stops as soon as
/// there's nothing left to do (or the window elapses).
pub async fn reconcile_vault_link_on_launch(app: tauri::AppHandle) {
    for _ in 0..6 {
        if reconcile_vault_link(&app).await {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
}

/// Ask Vault whether our app link still exists. `Some(true)`/`Some(false)` is
/// Vault's answer; `None` means Vault was unreachable, so the caller should not
/// conclude the link is gone. Keyed on our `app_agent_pub_key` (the link key).
async fn vault_link_active(
    client: &reqwest::Client,
    port: u16,
    app_agent_pub_key: &str,
) -> Option<bool> {
    let resp = client
        .get(format!("http://127.0.0.1:{}/link-status", port))
        .header("Origin", VAULT_ORIGIN)
        .query(&[("app_agent_pub_key", app_agent_pub_key)])
        .timeout(std::time::Duration::from_secs(4))
        .send()
        .await
        .ok()?;
    let v = resp.json::<serde_json::Value>().await.ok()?;
    Some(v["linked"].as_bool().unwrap_or(false))
}

/// Valid access token for llm.rs's online branch (refreshes if stale).
pub async fn get_access_token(app: &tauri::AppHandle) -> Result<String, String> {
    // Refuse to hand out a token minted for a different identity than the one
    // Vault currently holds — otherwise new usage would bill the prior user.
    if !session_identity_ok(app).await {
        return Err("auth_required".to_string());
    }
    let store = app.store(crate::profile::store_path(&app, AUTH_STORE)).map_err(|e| e.to_string())?;
    let access = store
        .get("access_token")
        .and_then(|v| v.as_str().map(String::from))
        .ok_or("auth_required")?;
    let expires_at = store
        .get("expires_at")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    if now_secs() < expires_at {
        return Ok(access);
    }

    // Refresh
    let refresh = store
        .get("refresh_token")
        .and_then(|v| v.as_str().map(String::from))
        .ok_or("auth_required")?;
    let tokens: serde_json::Value = http()
        .post(format!("{}/auth/vault/refresh", proxy_url()))
        .json(&serde_json::json!({ "refresh_token": refresh }))
        .send()
        .await
        .map_err(|e| format!("proxy unreachable: {}", e))?
        .json()
        .await
        .map_err(|e| e.to_string())?;
    let access = tokens["access_token"]
        .as_str()
        .ok_or("auth_required")?
        .to_string();
    store.set("access_token", serde_json::json!(access.clone()));
    // Refresh rotation: the proxy returns a NEW refresh token on every
    // refresh (0.4.1+). Store it so this device rotates; a used token is
    // honored for a grace window server-side, so nothing breaks if a
    // write fails between here and the next refresh.
    if let Some(rotated) = tokens["refresh_token"].as_str().filter(|r| !r.is_empty()) {
        store.set("refresh_token", serde_json::json!(rotated));
    }
    store.set(
        "expires_at",
        serde_json::json!(now_secs() + tokens["expires_in"].as_u64().unwrap_or(86400) - 300),
    );
    store.save().map_err(|e| e.to_string())?;
    Ok(access)
}

async fn session_from_store(app: &tauri::AppHandle) -> Result<FlowstaSession, String> {
    // If Vault is now unlocked under a different identity, wipe the stale
    // session first so the reads below report signed-out (not the prior
    // identity's plan/profile).
    let _ = session_identity_ok(app).await;
    let store = app.store(crate::profile::store_path(&app, AUTH_STORE)).map_err(|e| e.to_string())?;
    let agent_pub_key = store.get("agent_pub_key").and_then(|v| v.as_str().map(String::from));
    let did = store.get("did").and_then(|v| v.as_str().map(String::from));
    if agent_pub_key.is_none() {
        return Ok(FlowstaSession {
            signed_in: false,
            agent_pub_key: None,
            did: None,
            tier: None,
            linked: None,
            display_name: None,
            web_username: None,
            profile_picture: None,
        });
    }

    // Entitlement (tier + linked) for the UI; soft-fails to unknown.
    let mut tier = None;
    let mut linked = None;
    if let Ok(token) = get_access_token(app).await {
        if let Ok(resp) = http()
            .get(format!("{}/billing/entitlement", proxy_url()))
            .bearer_auth(&token)
            .send()
            .await
        {
            if let Ok(v) = resp.json::<serde_json::Value>().await {
                tier = v["tier"].as_str().map(String::from);
                linked = v["linked"].as_bool();
            }
        }
    }

    let get_str = |k: &str| store.get(k).and_then(|v| v.as_str().map(String::from));
    Ok(FlowstaSession {
        signed_in: true,
        agent_pub_key,
        did,
        tier,
        linked,
        display_name: get_str("display_name"),
        web_username: get_str("web_username"),
        profile_picture: get_str("profile_picture"),
    })
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

// ── Pack signing (knowledge/lore packs) ────────────────────────────────────
// Sign a pack manifest hash with the user's Flowsta identity via Vault's
// /sign endpoint (reusing the existing linking), and verify a signature
// locally with ed25519. No DNA changes — the signature lives in the pack file.

/// Sign `bytes_b64` (base64 of the manifest hash) with the user's Vault key.
/// Returns the base64 signature + the signer's agent pub key. Errors:
/// "vault_not_found" | "vault_locked" | "vault_interrupted" | link/sign errors.
#[tauri::command]
pub async fn vault_sign(
    app: tauri::AppHandle,
    bytes_b64: String,
    reason: String,
) -> Result<serde_json::Value, String> {
    let vault = find_vault().await;
    let port = vault.port.ok_or("vault_not_found")?;
    if !vault.unlocked {
        return Err("vault_locked".into());
    }
    let client = http();
    ensure_linked(&app, &client, port).await?;

    let resp = client
        .post(format!("http://127.0.0.1:{}/sign", port))
        .header("Origin", VAULT_ORIGIN)
        .timeout(std::time::Duration::from_secs(75)) // per-action dialog waits 60 s
        .json(&serde_json::json!({
            "type": "bytes",
            "bytes": bytes_b64,
            "reason": reason,
        }))
        .send()
        .await
        .map_err(|_| "vault_interrupted".to_string())?;

    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        return Err(body["error"].as_str().unwrap_or("sign_failed").to_string());
    }
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("bad sign response: {}", e))?;
    Ok(serde_json::json!({
        "signature": v["signature"].as_str().unwrap_or_default(),
        "agent_pub_key": v["agent_pub_key"].as_str().unwrap_or_default(),
    }))
}

/// The add-ons share service (opens the directory pull request for the
/// person; see build-docs SKILLS.md "Directory + sharing").
pub fn share_url() -> String {
    const PROD: &str = "https://yoai-share-386500392150.us-central1.run.app";
    #[cfg(debug_assertions)]
    {
        return dev_override("YOAI_SHARE_URL", "share_url").unwrap_or_else(|| PROD.to_string());
    }
    #[cfg(not(debug_assertions))]
    PROD.to_string()
}

/// Submit a signed listing (a character pack or a skill zip) to the
/// directory. The service verifies the signature again, commits the files
/// and opens the pull request under the maker's Flowsta handle. Returns
/// what the service says: the PR link and the listing's future page.
#[tauri::command]
pub async fn share_submit(app: tauri::AppHandle, submission: serde_json::Value) -> Result<serde_json::Value, String> {
    let _ = &app;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .post(format!("{}/v1/share", share_url()))
        .json(&submission)
        .send()
        .await
        .map_err(|e| format!("couldn't reach the share service: {e}"))?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().await.unwrap_or(serde_json::Value::Null);
    if !status.is_success() {
        let msg = body.get("error").and_then(|v| v.as_str()).unwrap_or("the share service refused the submission");
        return Err(format!("{msg} ({})", status.as_u16()));
    }
    Ok(body)
}

/// Verify an ed25519 signature over `hash_b64` by the Holochain agent key
/// `agent_pub_key` ("uhCAk…"). Pure crypto — no Vault needed. Returns whether
/// the signature is valid (false on any decode/length problem, never panics).
#[tauri::command]
pub fn verify_pack_signature(
    agent_pub_key: String,
    hash_b64: String,
    signature_b64: String,
) -> Result<bool, String> {
    use base64::Engine;
    use ed25519_dalek::Verifier;

    // Holochain AgentPubKey string: 'u' + base64url(no-pad) of 39 bytes
    // = [0x84,0x20,0x24] prefix + 32-byte ed25519 key + 4-byte DHT location.
    let s = agent_pub_key.strip_prefix('u').unwrap_or(&agent_pub_key);
    let raw = match base64::engine::general_purpose::URL_SAFE_NO_PAD.decode(s) {
        Ok(r) => r,
        Err(_) => return Ok(false),
    };
    if raw.len() != 39 {
        return Ok(false);
    }
    let mut pk = [0u8; 32];
    pk.copy_from_slice(&raw[3..35]);
    let vk = match ed25519_dalek::VerifyingKey::from_bytes(&pk) {
        Ok(k) => k,
        Err(_) => return Ok(false),
    };

    let sig_bytes = match base64::engine::general_purpose::STANDARD.decode(&signature_b64) {
        Ok(b) => b,
        Err(_) => return Ok(false),
    };
    if sig_bytes.len() != 64 {
        return Ok(false);
    }
    let mut sb = [0u8; 64];
    sb.copy_from_slice(&sig_bytes);
    let sig = ed25519_dalek::Signature::from_bytes(&sb);

    let msg = match base64::engine::general_purpose::STANDARD.decode(&hash_b64) {
        Ok(m) => m,
        Err(_) => return Ok(false),
    };

    Ok(vk.verify(&msg, &sig).is_ok())
}

// ── Sign It document signing (transcript export receipts) ──────────────────
// Publish a signature for an exported file to the Sign It network via the
// Vault's /sign-document commit path (linked-app publish; needs a Vault
// released after 1.1.0 - older Vaults answer tier_forbidden). The record
// lands on the user's signing cell, shows in the Vault signatures list and
// the Sign It dashboard, and draws one signature from the user's quota.
// Anyone can then verify the exported file on flowsta.com by its hash.

/// Sign + publish a document hash via Sign It. `file_hash_hex` = SHA-256 of
/// the exact exported bytes (64 hex chars). Runs as a Vault job and polls
/// /op-status so a slow approval or cold conductor never times the call out;
/// stage changes are emitted as `sign-document-stage` for the export modal.
/// Returns `{signature, agent_pub_key, signed_at, action_hash}`. Errors:
/// "vault_not_found" | "vault_locked" | "vault_outdated" | "quota_exceeded"
/// | "user_denied" | "sign_timeout" | link/sign errors (the Vault's
/// human-readable description, when present, rides after a `|`).
#[tauri::command]
pub async fn vault_sign_document(
    app: tauri::AppHandle,
    file_hash_hex: String,
    label: String,
    comment: Option<String>,
) -> Result<serde_json::Value, String> {
    use tauri::Emitter;

    let vault = find_vault().await;
    let port = vault.port.ok_or("vault_not_found")?;
    if !vault.unlocked {
        return Err("vault_locked".into());
    }
    let client = http();
    ensure_linked(&app, &client, port).await?;

    // Publishing needs the Vault's trust gate to recognize this app by
    // ORIGIN + client_id. /link-status answers by app key, so a link can
    // read as active and still fail that gate (a legacy or origin-less
    // link entry). If the Vault refuses with its "linked apps only"
    // answer, force the link ceremony again - the Vault shows its own
    // approval dialog - and retry once; only then is it a real refusal.
    let mut relinked = false;
    let resp = loop {
        let resp = client
            .post(format!("http://127.0.0.1:{}/sign-document", port))
            .header("Origin", VAULT_ORIGIN)
            .json(&serde_json::json!({
                "file_hash": file_hash_hex,
                "label": label,
                "comment": comment,
                "app_name": "Your Own AI",
                "client_id": YOAI_HOLOCHAIN_CLIENT_ID,
                "intent": "receipt",
                "ai_generation": "assisted",
                "commit": true,
                "job": true,
            }))
            .send()
            .await
            .map_err(|_| "vault_interrupted".to_string())?;
        if resp.status().is_success() {
            break resp;
        }
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let err = body["error"].as_str().unwrap_or("sign_failed");
        if err == "tier_forbidden" {
            let desc = body["description"].as_str().unwrap_or("");
            if desc.contains("linked apps") {
                if !relinked {
                    relinked = true;
                    if let Ok(store) = app.store(crate::profile::store_path(&app, AUTH_STORE)) {
                        store.delete("link_done");
                        let _ = store.save();
                    }
                    ensure_linked(&app, &client, port).await?;
                    continue;
                }
                return Err("vault_not_linked".into());
            }
            // A Vault from before linked-app publishing refuses every
            // non-Flowsta origin - that one needs an update.
            return Err("vault_outdated".into());
        }
        return Err(err.to_string());
    };
    let v: serde_json::Value = resp
        .json()
        .await
        .map_err(|e| format!("bad sign response: {}", e))?;
    let Some(job_id) = v["job_id"].as_str().map(String::from) else {
        return Ok(v); // Vault answered synchronously
    };

    // Poll until the job settles. Budget covers the approval window (up to
    // 120s) plus a cold-conductor publish, which can take minutes.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(600);
    let mut last_stage = String::new();
    loop {
        tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
        if std::time::Instant::now() > deadline {
            return Err("sign_timeout".into());
        }
        let snap: serde_json::Value = match client
            .get(format!("http://127.0.0.1:{}/op-status/{}", port, job_id))
            .header("Origin", VAULT_ORIGIN)
            .send()
            .await
        {
            Ok(r) if r.status().is_success() => match r.json().await {
                Ok(s) => s,
                Err(_) => continue,
            },
            Ok(_) => return Err("sign_job_lost".into()),
            Err(_) => continue, // transient - keep polling within budget
        };
        let stage = snap["stage"].as_str().unwrap_or("").to_string();
        if stage != last_stage {
            let _ = app.emit("sign-document-stage", serde_json::json!({ "stage": stage }));
            last_stage = stage.clone();
        }
        match stage.as_str() {
            "done" => return Ok(snap["result"].clone()),
            "failed" => {
                let err = snap["error"].as_str().unwrap_or("sign_failed");
                return Err(match snap["description"].as_str() {
                    Some(d) if !d.is_empty() => format!("{}|{}", err, d),
                    _ => err.to_string(),
                });
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod pack_sign_tests {
    use super::*;
    use base64::Engine;
    use ed25519_dalek::{Signer, SigningKey};

    // Build a Holochain "uhCAk…" agent string from a 32-byte ed25519 key the
    // same way Vault does (prefix + key + 4 location bytes; verify ignores loc).
    fn agent_key_string(pk: &[u8; 32]) -> String {
        let mut raw = [0u8; 39];
        raw[0] = 0x84;
        raw[1] = 0x20;
        raw[2] = 0x24;
        raw[3..35].copy_from_slice(pk);
        format!("u{}", base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(raw))
    }

    #[test]
    fn verify_roundtrip_and_failures() {
        let sk = SigningKey::from_bytes(&[7u8; 32]);
        let agent = agent_key_string(&sk.verifying_key().to_bytes());
        let msg = b"manifest-hash-bytes-32xxxxxxxxxxx";
        let sig = sk.sign(msg);
        let hash_b64 = base64::engine::general_purpose::STANDARD.encode(msg);
        let sig_b64 = base64::engine::general_purpose::STANDARD.encode(sig.to_bytes());

        // valid signature → true
        assert!(verify_pack_signature(agent.clone(), hash_b64.clone(), sig_b64.clone()).unwrap());
        // tampered message → false
        let bad = base64::engine::general_purpose::STANDARD.encode(b"DIFFERENT-hash-bytes-32xxxxxxxxxx");
        assert!(!verify_pack_signature(agent.clone(), bad, sig_b64.clone()).unwrap());
        // wrong signer key → false
        let agent2 = agent_key_string(&SigningKey::from_bytes(&[9u8; 32]).verifying_key().to_bytes());
        assert!(!verify_pack_signature(agent2, hash_b64, sig_b64).unwrap());
        // garbage agent key → false (no panic)
        assert!(!verify_pack_signature("not-a-key".into(), "AA==".into(), "AA==".into()).unwrap());
    }
}
