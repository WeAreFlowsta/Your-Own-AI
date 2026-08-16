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
    const PROD: &str = "https://yourownai.net/account/";
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

/// Probe localhost for a running Vault.
pub async fn find_vault() -> VaultStatus {
    let client = http();
    let cached = *cached_port().lock().unwrap();
    let order: Vec<u16> = match cached {
        Some(p) => std::iter::once(p)
            .chain([27777u16, 27778, 27779].into_iter().filter(|x| *x != p))
            .collect(),
        None => vec![27777, 27778, 27779],
    };
    for port in order {
        let url = format!("http://127.0.0.1:{}/status", port);
        // Short per-probe timeout so a dead port fails fast.
        if let Ok(resp) = client
            .get(&url)
            .timeout(std::time::Duration::from_secs(4))
            .send()
            .await
        {
            if let Ok(v) = resp.json::<serde_json::Value>().await {
                *cached_port().lock().unwrap() = Some(port);
                return VaultStatus {
                    installed: true,
                    unlocked: v["unlocked"].as_bool().unwrap_or(false),
                    port: Some(port),
                    agent_pub_key: v["agent_pub_key"].as_str().map(String::from),
                };
            }
        }
    }
    *cached_port().lock().unwrap() = None;
    VaultStatus::default()
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
    let store = app.store(AUTH_STORE).map_err(|e| e.to_string())?;
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

    let resp: serde_json::Value = client
        .post(format!("http://127.0.0.1:{}/link-identity", port))
        .header("Origin", VAULT_ORIGIN)
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
            if let Ok(store) = app.store(AUTH_STORE) {
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
        .timeout(std::time::Duration::from_secs(75))
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
    let store = app.store(AUTH_STORE).map_err(|e| e.to_string())?;
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

    // Profile (display name / username / avatar) via the scope grant
    fetch_vault_profile(&app, &client, port).await;

    session_from_store(&app).await
}

#[tauri::command]
pub async fn flowsta_sign_out(app: tauri::AppHandle) -> Result<(), String> {
    let store = app.store(AUTH_STORE).map_err(|e| e.to_string())?;
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
    pub overage_usd: f64,
    pub overage_opt_in: bool,
    pub requests: u64,
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
        overage_usd: v["overage_usd"].as_f64().unwrap_or(0.0),
        overage_opt_in: v["overage_opt_in"].as_bool().unwrap_or(false),
        requests: v["requests"].as_u64().unwrap_or(0),
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
    let store = app.store(AUTH_STORE).map_err(|e| e.to_string())?;
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
                    pricing: m["pricing"].as_object().map(|p| OnlinePricing {
                        input_per_mtok: p.get("input_per_mtok").and_then(|x| x.as_f64()).unwrap_or(0.0),
                        output_per_mtok: p.get("output_per_mtok").and_then(|x| x.as_f64()).unwrap_or(0.0),
                        request_fee_usd: p.get("request_fee_usd").and_then(|x| x.as_f64()).unwrap_or(0.0),
                        search_per_call_usd: p.get("search_per_call_usd").and_then(|x| x.as_f64()),
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
    // 60s positive cache: this guard runs before EVERY online request and
    // its Vault /status probe can stall up to 1.5s. Identity switches are
    // rare and still invalidate within a minute - a mis-billed request
    // window of seconds, against a probe on the hot path of every turn.
    static LAST_OK: std::sync::Mutex<Option<std::time::Instant>> = std::sync::Mutex::new(None);
    if let Ok(guard) = LAST_OK.lock() {
        if let Some(t) = *guard {
            if t.elapsed() < std::time::Duration::from_secs(60) {
                return true;
            }
        }
    }
    let ok = session_identity_ok_uncached(app).await;
    if ok {
        if let Ok(mut guard) = LAST_OK.lock() {
            *guard = Some(std::time::Instant::now());
        }
    }
    ok
}

async fn session_identity_ok_uncached(app: &tauri::AppHandle) -> bool {
    let store = match app.store(AUTH_STORE) {
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
        Some((true, Some(ref vault_key))) if vault_key != &stored => {
            for key in SESSION_KEYS_FULL {
                store.delete(key);
            }
            let _ = store.save();
            false
        }
        _ => true,
    }
}

/// Fast single-probe read of Vault's `(unlocked, agent_pub_key)` for the
/// identity guard. Unlike `find_vault`, it never scans all three ports: it hits
/// the last-known port (or the default 27777 on a cold start) with a short
/// timeout. The guard only needs an answer when Vault is genuinely up; a
/// down/slow Vault yields `None` ("can't tell"), so the caller keeps the cached
/// session instead of stalling online use behind a multi-port scan.
async fn vault_identity_quick() -> Option<(bool, Option<String>)> {
    let port = (*cached_port().lock().unwrap()).unwrap_or(27777);
    let resp = http()
        .get(format!("http://127.0.0.1:{}/status", port))
        .timeout(std::time::Duration::from_millis(1500))
        .send()
        .await
        .ok()?;
    let v = resp.json::<serde_json::Value>().await.ok()?;
    Some((
        v["unlocked"].as_bool().unwrap_or(false),
        v["agent_pub_key"].as_str().map(String::from),
    ))
}

/// One launch-time reconcile pass. Returns `true` when there's nothing more to
/// do (no session, already linked, just re-linked, identity mismatch, or a link
/// error) and `false` only when Vault wasn't reachable/unlocked yet, so a retry
/// later may succeed. Keeps Vault's connected-apps list accurate after a reset:
/// if YOAI has a session but its link was wiped, it re-links (which pops Vault's
/// approval), matching ProofPoll.
async fn reconcile_vault_link(app: &tauri::AppHandle) -> bool {
    let store = match app.store(AUTH_STORE) {
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
    let store = app.store(AUTH_STORE).map_err(|e| e.to_string())?;
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
    let store = app.store(AUTH_STORE).map_err(|e| e.to_string())?;
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
    if !resp.status().is_success() {
        let body: serde_json::Value = resp.json().await.unwrap_or_default();
        let err = body["error"].as_str().unwrap_or("sign_failed");
        // A Vault from before linked-app publishing refuses commit for
        // non-Flowsta origins - surface that as "update your Vault".
        if err == "tier_forbidden" {
            return Err("vault_outdated".into());
        }
        return Err(err.to_string());
    }
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
