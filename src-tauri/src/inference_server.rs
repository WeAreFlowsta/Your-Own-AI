//! Your Own AI — local inference server (OpenAI-compatible).
//!
//! A small HTTP server on 127.0.0.1 that lets external apps (and agent
//! frameworks) use the user's **custom AIs** for inference. External callers
//! select an AI by name via the OpenAI `model` field — e.g.
//! `model: "veebo"` — and the request is routed through that AI: its persona,
//! its chosen (offline) model, and (in later phases) its memory + signed
//! transcript.
//!
//! Phase 1: the **persona path**. We resolve `model` → AI from the same
//! `ai-data.json` store the UI writes, ensure the AI's offline model is loaded,
//! prepend the persona system prompt, and **reverse-proxy** to the bundled
//! llama-server on :8080 (which already emits the exact OpenAI wire contract —
//! streaming `tool_calls`, `finish_reason`, `usage`, `[DONE]`). The response
//! body is streamed straight back to the caller.
//!
//! Not yet (later phases): memory injection, transcript recording, agent mode
//! (`model: "veebo:agent"`), online models, real auth. Keyless-localhost only.

use axum::{
    body::Body,
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use futures::StreamExt;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use tauri::{AppHandle, Manager};
use tauri_plugin_store::StoreExt;

/// Maps a caller-supplied conversation key → (transcript conversation hash,
/// next sequence number), so multi-turn API sessions append to one signed
/// conversation. Keyless calls each get their own one-off conversation.
static CONV_MAP: OnceLock<Mutex<HashMap<String, (String, u32)>>> = OnceLock::new();
fn conv_map() -> &'static Mutex<HashMap<String, (String, u32)>> {
    CONV_MAP.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Dedicated Your Own AI port (deliberately NOT Ollama's 11434). Doubles as
/// the single-instance lock: instance_guard binds it before anything spawns.
pub(crate) const PORT: u16 = 11435;

/// The local chat server's OpenAI endpoint (port owned by llm.rs).
fn llama_upstream() -> String {
    format!(
        "http://127.0.0.1:{}/v1/chat/completions",
        crate::llm::CHAT_PORT
    )
}
const AI_STORE: &str = "ai-data.json";
const CUSTOM_AIS_KEY: &str = "custom-ais";

/// A custom AI resolved from the store, reduced to what inference needs.
struct ResolvedAi {
    /// AI id — used by Phase 2 (memory + transcript routing), not yet read here.
    #[allow(dead_code)]
    id: String,
    name: String,
    /// Offline GGUF filename, or an `online:` id (not yet served).
    model: String,
    system_prompt: String,
    use_emojis: bool,
    /// Holochain agent pub key (hex), set on first provisioning. Empty if the
    /// AI hasn't been provisioned yet → transcript recording is skipped.
    agent_pub_key: String,
}

/// LAN access config: OFF = loopback-only, keyless (the default). ON =
/// the server also accepts connections from the local network, which MUST
/// present the access key as a standard `Authorization: Bearer` token
/// (the "API key" field every OpenAI-compatible client already has).
/// Loopback callers stay keyless either way - being on the machine is the
/// credential there.
#[derive(Clone, serde::Serialize, serde::Deserialize, Default)]
pub struct LanAccess {
    pub enabled: bool,
    pub key: String,
}

fn lan_config_path(app: &AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    app.path().app_data_dir().ok().map(|d| d.join("lan-access.json"))
}

pub fn lan_config(app: &AppHandle) -> LanAccess {
    lan_config_path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn save_lan_config(app: &AppHandle, cfg: &LanAccess) -> Result<(), String> {
    let p = lan_config_path(app).ok_or("no app data dir")?;
    std::fs::write(&p, serde_json::to_string(cfg).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}

fn generate_key() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut bytes);
    format!("yoai-{}", hex::encode(bytes))
}

/// Non-loopback machine addresses, for showing the user their reachable
/// URLs. Best-effort: derived by asking the OS which local address routes
/// outward (no packets are actually sent to the probe address).
fn lan_ips() -> Vec<String> {
    let mut out = Vec::new();
    if let Ok(socket) = std::net::UdpSocket::bind("0.0.0.0:0") {
        if socket.connect("192.0.2.1:9").is_ok() {
            if let Ok(addr) = socket.local_addr() {
                if !addr.ip().is_loopback() {
                    out.push(addr.ip().to_string());
                }
            }
        }
    }
    out
}

#[tauri::command]
pub fn lan_access_status(app: AppHandle) -> serde_json::Value {
    let cfg = lan_config(&app);
    json!({
        "enabled": cfg.enabled,
        "key": cfg.key,
        "ips": lan_ips(),
        "port": PORT,
    })
}

#[tauri::command]
pub fn lan_access_set(app: AppHandle, enabled: bool) -> Result<serde_json::Value, String> {
    let mut cfg = lan_config(&app);
    cfg.enabled = enabled;
    if enabled && cfg.key.is_empty() {
        cfg.key = generate_key();
    }
    save_lan_config(&app, &cfg)?;
    restart_server(app.clone());
    Ok(json!({ "enabled": cfg.enabled, "key": cfg.key, "ips": lan_ips(), "port": PORT }))
}

#[tauri::command]
pub fn lan_access_regenerate_key(app: AppHandle) -> Result<String, String> {
    let mut cfg = lan_config(&app);
    cfg.key = generate_key();
    save_lan_config(&app, &cfg)?;
    Ok(cfg.key)
}

/// Shutdown signal for the running server so a LAN-access toggle can
/// rebind on the right interface.
static SHUTDOWN: std::sync::Mutex<Option<tokio::sync::watch::Sender<bool>>> =
    std::sync::Mutex::new(None);

fn restart_server(app: AppHandle) {
    if let Some(tx) = SHUTDOWN.lock().unwrap().take() {
        let _ = tx.send(true);
    }
    spawn(app, None);
}

/// Reject non-loopback callers without the access key. Loopback is always
/// allowed keyless.
async fn lan_guard(
    axum::extract::State(app): axum::extract::State<AppHandle>,
    axum::extract::ConnectInfo(peer): axum::extract::ConnectInfo<std::net::SocketAddr>,
    req: axum::extract::Request,
    next: axum::middleware::Next,
) -> Response {
    if peer.ip().is_loopback() {
        return next.run(req).await;
    }
    let cfg = lan_config(&app);
    if !cfg.enabled || cfg.key.is_empty() {
        return err(StatusCode::FORBIDDEN, "Network access is not enabled.", "lan_disabled");
    }
    let presented = req
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::trim)
        .unwrap_or("");
    if presented != cfg.key {
        return err(
            StatusCode::UNAUTHORIZED,
            "A valid access key is required from other devices. Copy it from Settings, External access.",
            "invalid_access_key",
        );
    }
    next.run(req).await
}

/// Spawn the inference server in a background task. At app startup the
/// listener comes pre-bound from instance_guard (it IS the single-instance
/// lock); the LAN-toggle restart path passes None and binds fresh. A bind
/// failure on the restart path is logged, never fatal.
pub fn spawn(app: AppHandle, pre_bound: Option<std::net::TcpListener>) {
    let (tx, mut rx) = tokio::sync::watch::channel(false);
    *SHUTDOWN.lock().unwrap() = Some(tx);
    tauri::async_runtime::spawn(async move {
        let lan = lan_config(&app).enabled;
        let router = Router::new()
            // The `app` field is the single-instance health probe's identity
            // check - a foreign server on this port must not read as us.
            .route("/health", get(|| async { Json(json!({ "status": "ok", "app": "your-own-ai" })) }))
            .route("/v1/models", get(list_models))
            .route("/v1/chat/completions", post(chat_completions))
            .route("/v1/responses", post(responses_web_search))
            .route("/mcp", post(mcp_post).get(mcp_get))
            .route("/internal/route-preview", get(route_preview))
            .layer(axum::middleware::from_fn_with_state(app.clone(), lan_guard))
            .with_state(app);

        let addr = if lan {
            format!("0.0.0.0:{PORT}")
        } else {
            format!("127.0.0.1:{PORT}")
        };
        let listener = match pre_bound {
            Some(l) => {
                // Bound synchronously (and blocking) by instance_guard;
                // convert for tokio.
                l.set_nonblocking(true)
                    .map_err(|e| e.to_string())
                    .and_then(|_| tokio::net::TcpListener::from_std(l).map_err(|e| e.to_string()))
            }
            None => tokio::net::TcpListener::bind(&addr)
                .await
                .map_err(|e| e.to_string()),
        };
        match listener {
            Ok(listener) => {
                log::info!("[inference] Your Own AI inference server on http://{addr}");
                let serve = axum::serve(
                    listener,
                    router.into_make_service_with_connect_info::<std::net::SocketAddr>(),
                )
                .with_graceful_shutdown(async move {
                    let _ = rx.wait_for(|v| *v).await;
                });
                if let Err(e) = serve.await {
                    log::error!("[inference] server exited: {e}");
                }
            }
            Err(e) => log::error!("[inference] could not bind {addr}: {e}"),
        }
    });
}

// ── AI resolution ───────────────────────────────────────────────────────────

/// Lower-case, non-alphanumerics → hyphens (matches the UI's slug shape).
fn slug(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev_hyphen = false;
    for c in s.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_hyphen = false;
        } else if !prev_hyphen {
            out.push('-');
            prev_hyphen = true;
        }
    }
    out.trim_matches('-').to_string()
}

/// Read the user's custom AIs from the store the frontend persists.
fn load_ais(app: &AppHandle) -> Vec<Value> {
    let Ok(store) = app.store(AI_STORE) else {
        return vec![];
    };
    match store.get(CUSTOM_AIS_KEY) {
        Some(Value::Array(arr)) => arr,
        _ => vec![],
    }
}

/// Resolve an OpenAI `model` string to a custom AI. Matches by id, then exact
/// name (case-insensitive), then slug. A trailing `:agent` suffix is stripped
/// (agent mode lands in a later phase; treated as persona for now).
fn resolve_ai(app: &AppHandle, model: &str) -> Option<ResolvedAi> {
    let base = model
        .strip_suffix(":agent-device")
        .or_else(|| model.strip_suffix(":agent"))
        .or_else(|| model.strip_suffix(":plan"))
        .or_else(|| model.strip_suffix(":summary"))
        .unwrap_or(model)
        .trim();
    let want = slug(base);
    let ais = load_ais(app);

    let pick = ais.iter().find(|a| {
        let id = a.get("id").and_then(Value::as_str).unwrap_or("");
        let name = a.get("name").and_then(Value::as_str).unwrap_or("");
        id == base
            || name.eq_ignore_ascii_case(base)
            || slug(id) == want
            || slug(name) == want
    })?;

    Some(ResolvedAi {
        id: pick.get("id").and_then(Value::as_str).unwrap_or("").to_string(),
        name: pick.get("name").and_then(Value::as_str).unwrap_or("AI").to_string(),
        model: pick.get("model").and_then(Value::as_str).unwrap_or("").to_string(),
        system_prompt: pick
            .get("systemPrompt")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
        use_emojis: pick.get("useEmojis").and_then(Value::as_bool).unwrap_or(false),
        agent_pub_key: pick
            .get("agentPubKey")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    })
}

/// Build the persona system prompt. `user_memory` fills the `{{userNameInfo}}`
/// slot (the assembled memory block, or the name-unknown line). Mirrors the
/// essentials of the frontend `buildSystemPrompt`.
fn build_persona_prompt(ai: &ResolvedAi, user_memory: &str) -> String {
    let mut sp = ai.system_prompt.clone();
    sp = sp.replace("{{userNameInfo}}", user_memory);
    // Response-length text is not ported yet (Phase 1b); drop the slot cleanly.
    sp = sp.replace("{{aiResponseLength}}", "");
    sp = sp.replace("{{aiName}}", &ai.name);
    sp.push_str("\n\nWhen including code in your response, always use markdown code blocks with the language identifier (e.g. ```python, ```javascript, ```html). Always close code blocks with ```.");
    if ai.use_emojis {
        sp.push_str(" It is a requirement that you generously use relevant emojis in this response to give it a friendly and expressive tone.");
    } else {
        sp.push_str(" You must not use any emojis in your response.");
    }
    sp
}

// ── handlers ────────────────────────────────────────────────────────────────

/// OpenAI error envelope.
fn err(status: StatusCode, msg: &str, kind: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "message": msg, "type": kind } })),
    )
        .into_response()
}

/// `GET /v1/models` — list the user's AIs (persona + `:agent` variant each).
async fn list_models(State(app): State<AppHandle>) -> Response {
    let mut data = vec![];
    for a in load_ais(&app) {
        // Use the friendly name-slug as the model id (what a caller types),
        // not the cryptic Holochain-derived store id.
        let name = a.get("name").and_then(Value::as_str).unwrap_or("");
        let id = slug(name);
        if id.is_empty() {
            continue;
        }
        for variant in [id.clone(), format!("{id}:agent")] {
            data.push(json!({
                "id": variant,
                "object": "model",
                "owned_by": "your-own-ai",
            }));
        }
    }
    Json(json!({ "object": "list", "data": data })).into_response()
}

/// `GET /internal/route-preview` - the routing DECISION for a parameter
/// combination, without loading or generating anything. Dev builds only:
/// this exists for the routing test matrix (tools/route-matrix.mjs),
/// which sweeps every mode x task x difficulty x eagerness x lean x role
/// combination and asserts the invariants (offline never online, privacy
/// never leaves, agents never get a tools-blind model...).
async fn route_preview(
    State(app): State<AppHandle>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    if !cfg!(debug_assertions) {
        return err(StatusCode::NOT_FOUND, "not found", "not_found");
    }
    let g = |k: &str, d: &str| q.get(k).cloned().unwrap_or_else(|| d.to_string());
    let picks = crate::router::OnlinePicks::from_store(&app);
    match crate::router::route_with(
        &app,
        &g("mode", "online-offline"),
        &g("q", "hello"),
        &g("eagerness", "balanced"),
        &g("task", "general"),
        &g("difficulty", "easy"),
        &g("lean", "balanced"),
        &picks,
        None,
        g("agent", "0") == "1",
        g("plan", "0") == "1",
        q.get("turn_tokens").and_then(|t| t.parse::<u32>().ok()),
    )
    .await
    {
        Ok(r) => Json(json!({ "model": r.model, "reason": r.reason })).into_response(),
        Err(e) => Json(json!({ "error": e })).into_response(),
    }
}

/// `POST /v1/chat/completions` — resolve the AI, ensure its model is loaded,
/// prepend the persona, and reverse-proxy to llama-server, streaming the
/// response straight through.
async fn chat_completions(
    State(app): State<AppHandle>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> Response {
    let model = body
        .get("model")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let Some(mut ai) = resolve_ai(&app, &model) else {
        return err(
            StatusCode::NOT_FOUND,
            &format!("No AI matches model '{model}'. Use one from GET /v1/models."),
            "model_not_found",
        );
    };

    if ai.model.is_empty() {
        return err(
            StatusCode::BAD_REQUEST,
            &format!("AI '{}' has no model configured.", ai.name),
            "unsupported_model",
        );
    }

    // This turn's user text (last user message) — drives the router and memory.
    let incoming = body
        .get("messages")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let query = incoming
        .iter()
        .rev()
        .find(|m| m.get("role").and_then(Value::as_str) == Some("user"))
        .and_then(|m| m.get("content").and_then(Value::as_str))
        .unwrap_or("")
        .to_string();

    // Auto modes: resolve "auto:*" to a concrete model for this question, so the
    // online/offline logic below runs on the resolved value.
    let auto_mode = ai.model.strip_prefix("auto:").map(str::to_string);
    if let Some(m) = auto_mode {
        // "my-hardware" maps to offline HERE: this server has no
        // external-chat path yet, so a resolved external model couldn't be
        // served. The in-app chat handles the full my-hardware mode.
        let mode = if m == "online-offline" { "online-offline" } else { "offline" };
        // Per-request routing hints (a code editor sends "X-Your-Own-AI-Task: code").
        // Validated to known values; absent/unknown → neutral defaults (= prior
        // behavior), so existing callers are unaffected.
        let task = match header_str(&headers, "x-your-own-ai-task")
            .map(|s| s.trim().to_lowercase())
            .as_deref()
        {
            Some("code") => "code",
            Some("math") => "math",
            Some("reasoning") => "reasoning",
            _ => "general",
        };
        let difficulty = match header_str(&headers, "x-your-own-ai-difficulty")
            .map(|s| s.trim().to_lowercase())
            .as_deref()
        {
            Some("hard") => "hard",
            _ => "easy",
        };
        let eagerness = match header_str(&headers, "x-your-own-ai-eagerness")
            .map(|s| s.trim().to_lowercase())
            .as_deref()
        {
            Some("privacy") => "privacy",
            Some("freshness") => "freshness",
            _ => "balanced",
        };
        // Offline-pick lean, mirroring the eagerness header. The in-app
        // Settings knobs live in the webview's storage and don't reach this
        // server; API callers opt in per request. Difficulty escalation is
        // inherent to online-offline mode (same as in-app); online model
        // picks use the recommended defaults - there are no per-request
        // override headers yet.
        let lean = match header_str(&headers, "x-your-own-ai-lean")
            .map(|s| s.trim().to_lowercase())
            .as_deref()
        {
            Some("speed") => "speed",
            Some("quality") => "quality",
            _ => "balanced",
        };
        let picks = crate::router::OnlinePicks::from_store(&app);
        // Agent sessions route deterministically to a tool-capable model
        // (the flag is computed again below for prompt handling - cheap).
        let plan_routing = model.trim().ends_with(":plan");
        // ":agent-device" = the subagent-offload variant: simple side-work
        // (explore fan-outs) forced onto a capable DEVICE model - free and
        // private - while the leader stays wherever it routes.
        let device_only = model.trim().ends_with(":agent-device");
        // ":summary" = the agent compacting its own context (the rail already
        // shows this as "Tidying the conversation memory" from the agent's
        // own auto_compact events). Routes like the session's agent turns -
        // a summary of agent work should not drop to a tiny model - and is
        // named in the log so a long compaction is identifiable in a report.
        let summary_routing = model.trim().ends_with(":summary");
        if summary_routing {
            log::info!("[inference] agent compaction call for {} (session_summary role)", ai.name);
        }
        let agent_routing = plan_routing
            || device_only
            || summary_routing
            || model.trim().ends_with(":agent")
            || header_str(&headers, "x-your-own-ai-mode").as_deref() == Some("agent");
        // Attached documents are a privacy fact: in online-offline mode an
        // agent turn carrying attachment context routes OFFLINE unless the
        // user gave standing consent for attachments to go online (Settings
        // > Routing, mirrored into the store as allowAttachmentsOnline). The
        // app writes attachment context with a fixed marker, so the turn is
        // recognizable without any new wire field. Direct chats apply the
        // same rule client-side; this is the agent path's half.
        // Images count too: a content array with an image part is an
        // attachment as much as a marked document (the Build agent's
        // image-description calls ride this path).
        let carries_attachment = incoming.iter().any(|m| {
            let content = m.get("content");
            let marked_doc = content
                .and_then(Value::as_str)
                .map(|c| c.contains("[Attached files for context]"))
                .unwrap_or(false);
            let has_image = content
                .and_then(Value::as_array)
                .map(|parts| {
                    parts.iter().any(|p| {
                        matches!(p.get("type").and_then(Value::as_str), Some("image_url") | Some("input_image"))
                    })
                })
                .unwrap_or(false);
            marked_doc || has_image
        });
        let attachments_may_go_online = crate::router::store_pref_bool(&app, "allowAttachmentsOnline");
        let route_mode = if device_only {
            "offline"
        } else if mode == "online-offline" && carries_attachment && !attachments_may_go_online {
            log::info!("[inference] attachment without online consent - routing {} offline", ai.name);
            "offline"
        } else {
            mode
        };
        match crate::router::route(&app, route_mode, &query, eagerness, task, difficulty, lean, &picks, None, agent_routing, plan_routing).await {
            Ok(r) => {
                log::info!("[inference] router ({}): {mode} task={task} diff={difficulty} eag={eagerness} -> {} ({})", ai.name, r.model, r.reason);
                // The rail's pearl needs to know WHAT it is waiting on: a
                // silent minute is "thinking" on a local model and "no reply
                // yet from gpt-5.6-sol" on an online one - the honest words
                // differ, and only this side knows the pick.
                if agent_routing {
                    use tauri::Emitter as _;
                    let _ = app.emit(
                        "agent-route",
                        json!({ "ai": ai.name, "model": r.model, "online": r.model.starts_with("online:"), "reason": r.reason }),
                    );
                }
                ai.model = r.model;
            }
            Err(e) => {
                return err(
                    StatusCode::SERVICE_UNAVAILABLE,
                    &format!("Routing failed for '{}': {e}", ai.name),
                    "routing_failed",
                )
            }
        }
    }

    // Online models ("online:<id>") route to the Flowsta proxy with the
    // Vault-grant token; offline models run on the local llama-server.
    let online_id: Option<String> = ai.model.strip_prefix("online:").map(str::to_string);

    // Offline: ensure the AI's model is the one loaded. Online: nothing local.
    if online_id.is_none() {
        let state = app.state::<crate::llm::LLMState>();
        let current = state.current_model.lock().await.clone();
        // v1 MLX rule: agent/project turns are served by llama.cpp even
        // when the model has an MLX artifact (the KV-rewind scar). A same-
        // model agent turn arriving while SwiftLM serves must flip engines.
        let mlx_serving = state
            .spawned_backend
            .try_lock()
            .map(|b| b.as_deref() == Some("mlx"))
            .unwrap_or(false);
        // Recomputed here (the earlier binding lives in the auto-routing
        // scope): every agent-shaped call carries the mode header or an
        // :agent/:plan/:summary suffix.
        let agent_turn = header_str(&headers, "x-your-own-ai-mode").as_deref() == Some("agent")
            || model.trim().ends_with(":agent")
            || model.trim().ends_with(":plan")
            || model.trim().ends_with(":summary");
        let engine_flip = agent_turn && mlx_serving;
        if current.as_deref() != Some(ai.model.as_str()) || engine_flip {
            if agent_turn {
                crate::llm::FORCE_GGUF_NEXT_LOAD.store(true, std::sync::atomic::Ordering::SeqCst);
            }
            if engine_flip && current.as_deref() == Some(ai.model.as_str()) {
                // Same model, wrong engine: stop the MLX server so the
                // load below actually restarts instead of "already loaded".
                if let Some(child) = state.server_process.lock().await.take() {
                    let _ = child.kill();
                }
                *state.is_server_running.lock().await = false;
                *state.current_model.lock().await = None;
            }
            log::info!(
                "[inference] switching model for AI '{}': {:?} -> {} (engine_flip: {engine_flip})",
                ai.name,
                current,
                ai.model
            );
            // with_vision=false: pair the projector only if it fits the GPU for
            // free (no per-turn reload flow on the API surface yet — capable
            // hardware still gets vision; small-GPU API vision is a follow-up).
            if let Err(e) = crate::llm::load_model(app.clone(), state, ai.model.clone(), false, "inference-api".to_string()).await {
                return err(
                    StatusCode::SERVICE_UNAVAILABLE,
                    &format!("Couldn't load model '{}' for '{}': {e}", ai.model, ai.name),
                    "model_load_failed",
                );
            }
            // load_model can return before the restarted server is serving.
            // Wait for /health ready, or the first request races the reload.
            for _ in 0..120 {
                if matches!(crate::llm::is_llama_server_ready().await, Ok(true)) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }

    // Agent mode (model "<ai>:agent", or the X-Your-Own-AI-Mode header): the
    // CALLER owns the system prompt; the AI lends only its substrate (model +
    // memory + transcript + identity), not its persona. Persona mode (default):
    // we drive the system prompt.
    let agent_mode = model.trim().ends_with(":agent")
        || model.trim().ends_with(":plan")
        || model.trim().ends_with(":agent-device")
        || model.trim().ends_with(":summary")
        || header_str(&headers, "x-your-own-ai-mode").as_deref() == Some("agent");


    // Agent mode injects memory by DEFAULT; a caller can opt out — e.g. to keep
    // tool-calling reliable on smaller models, where a memory block can tip the
    // model toward refusing tools — with `X-Your-Own-AI-Memory: off`.
    let agent_memory = !matches!(
        header_str(&headers, "x-your-own-ai-memory")
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Some("off" | "false" | "0" | "no")
    );
    // `X-Your-Own-AI-Record: off` skips transcript recording for this
    // exchange - for internal calls whose output is stored elsewhere (the
    // workspace-memory updater writes its own revision entry; recording the
    // exchange too would double-store it as a provider conversation).
    let record_exchange = !matches!(
        header_str(&headers, "x-your-own-ai-record")
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Some("off" | "false" | "0" | "no")
    );

    let messages = if agent_mode {
        // Caller owns the system prompt (no persona). Memory is injected as a
        // bounded supplementary system message AFTER the caller's own system
        // prompt (so theirs stays authoritative), unless opted out.
        let mut msgs = incoming.clone();
        if agent_memory {
            // Agent sessions call per STEP; the user-memory block barely
            // changes within one, but rebuilding it costs a CPU embedding
            // query every time (measured ~0.7s/step on modest hardware).
            // Cache per AI briefly - identical injection, one rebuild per
            // couple of minutes instead of per step.
            let user_memory = {
                use std::sync::Mutex;
                use std::time::{Duration, Instant};
                static AGENT_MEM_CACHE: Mutex<Option<(String, String, Instant)>> = Mutex::new(None);
                let cached = AGENT_MEM_CACHE
                    .lock()
                    .unwrap()
                    .as_ref()
                    .filter(|(id, _, at)| *id == ai.id && at.elapsed() < Duration::from_secs(120))
                    .map(|(_, block, _)| block.clone());
                match cached {
                    Some(block) => block,
                    None => {
                        let block =
                            crate::inference_memory::build_memory_block(&app, &query, &ai.id, None)
                                .await;
                        *AGENT_MEM_CACHE.lock().unwrap() =
                            Some((ai.id.clone(), block.clone(), Instant::now()));
                        block
                    }
                }
            };
            if !user_memory.trim().is_empty() {
                let ctx = json!({
                    "role": "system",
                    "content": format!(
                        "Context about the person you're assisting (use naturally where relevant; do not recite this):\n{user_memory}"
                    ),
                });
                let pos = usize::from(
                    msgs.first().and_then(|m| m.get("role")).and_then(Value::as_str) == Some("system"),
                );
                msgs.insert(pos, ctx);
            }
        }
        msgs
    } else {
        // Persona mode: we own the system prompt (persona + memory in the slot).
        let user_memory =
            crate::inference_memory::build_memory_block(&app, &query, &ai.id, None).await;
        let persona = build_persona_prompt(&ai, &user_memory);
        let mut msgs = vec![json!({ "role": "system", "content": persona })];
        msgs.extend(incoming.clone());
        msgs
    };
    // Some chat templates (Mistral/Ministral) accept only ONE system message —
    // coalesce any system messages into a single leading one (order-preserving).
    body["messages"] = Value::Array(coalesce_system(messages));
    // Offline: llama-server keys off the loaded model. Online: the proxy needs
    // the bare provider id (no "online:" prefix).
    body["model"] = json!(online_id.clone().unwrap_or_else(|| ai.model.clone()));

    // Recording context: the exchange is recorded to the signed transcript once
    // we have the assistant's reply (grouped by an optional caller-supplied
    // conversation key; tagged with the calling app).
    let rec = RecordCtx {
        app: app.clone(),
        agent_key: if record_exchange { ai.agent_pub_key.clone() } else { String::new() },
        ai_id: ai.id.clone(),
        ai_name: ai.name.clone(),
        model: ai.model.clone(),
        conv_key: header_str(&headers, "x-your-own-ai-conversation")
            .or_else(|| body.get("user").and_then(Value::as_str).map(str::to_string)),
        app_name: header_str(&headers, "x-title")
            .or_else(|| header_str(&headers, "user-agent"))
            .unwrap_or_else(|| "API".to_string()),
        user_text: query,
        agent: agent_mode,
    };

    // Send upstream: the Flowsta online proxy (Vault-grant token) for online
    // models, else the local llama-server. No client timeout — generations
    // are long. The response shape is identical OpenAI SSE either way.
    let client = reqwest::Client::new();
    let send = if online_id.is_some() {
        let token = match crate::flowsta::get_access_token(&app).await {
            Ok(t) => t,
            Err(_) => {
                return err(
                    StatusCode::UNAUTHORIZED,
                    &format!(
                        "AI '{}' uses an online model — sign in with Flowsta (in the app) to enable it.",
                        ai.name
                    ),
                    "auth_required",
                )
            }
        };
        client
            .post(format!("{}/v1/chat/completions", crate::flowsta::proxy_url()))
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
    } else {
        client.post(llama_upstream()).json(&body).send().await
    };
    let upstream = match send {
        Ok(r) => r,
        Err(e) => {
            return err(
                StatusCode::BAD_GATEWAY,
                &format!("Upstream error: {e}"),
                "upstream_error",
            )
        }
    };

    let status = StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::OK);
    let content_type = upstream
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("application/json")
        .to_string();

    // Branch on the RESPONSE shape, not the request flag — the online proxy
    // always streams SSE even when the caller asked for stream:false.
    if content_type.contains("event-stream") {
        // Tee: forward each SSE chunk to the caller while accumulating the body,
        // then record the exchange once the stream completes.
        let body_stream = async_stream::stream! {
            let mut s = upstream.bytes_stream();
            let mut raw: Vec<u8> = Vec::new();      // full body (incl. <think>) → recording
            let mut pending: Vec<u8> = Vec::new();  // bytes not yet split into events
            let mut stripper = ThinkStripper::default();
            while let Some(item) = s.next().await {
                match item {
                    Ok(bytes) => {
                        raw.extend_from_slice(&bytes);
                        pending.extend_from_slice(&bytes);
                        // Emit each complete SSE event (terminated by a blank line),
                        // stripping <think> from the streamed content as we go.
                        while let Some(pos) = find_sub(&pending, b"\n\n") {
                            let event: Vec<u8> = pending.drain(..pos + 2).collect();
                            let s = String::from_utf8_lossy(&event[..event.len() - 2]);
                            if let Some(out) = process_event(&s, &mut stripper) {
                                yield Ok::<_, std::io::Error>(out.into_bytes());
                            }
                        }
                    }
                    Err(e) => {
                        yield Err(std::io::Error::new(std::io::ErrorKind::Other, e));
                        break;
                    }
                }
            }
            if !pending.is_empty() {
                let s = String::from_utf8_lossy(&pending);
                if let Some(out) = process_event(&s, &mut stripper) {
                    yield Ok(out.into_bytes());
                }
            }
            // Record from the RAW body (with <think>) — spawn_record splits the
            // reasoning into the transcript's `thinking` field for audit.
            let reply = parse_sse_content(&raw);
            spawn_record(rec, reply);
        };
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from_stream(body_stream))
            .unwrap_or_else(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &format!("{e}"), "internal_error"))
    } else {
        // Buffer the JSON reply, record it, and return it unchanged.
        let bytes = match upstream.bytes().await {
            Ok(b) => b,
            Err(e) => return err(StatusCode::BAD_GATEWAY, &format!("upstream read: {e}"), "upstream_error"),
        };
        spawn_record(rec, parse_json_content(&bytes));
        Response::builder()
            .status(status)
            .header(header::CONTENT_TYPE, content_type)
            .body(Body::from(bytes))
            .unwrap_or_else(|e| err(StatusCode::INTERNAL_SERVER_ERROR, &format!("{e}"), "internal_error"))
    }
}

// ── transcript recording ──────────────────────────────────────────────────

/// What recording an exchange needs, captured before the upstream call.
struct RecordCtx {
    app: AppHandle,
    agent_key: String,
    ai_id: String,
    ai_name: String,
    model: String,
    conv_key: Option<String>,
    app_name: String,
    user_text: String,
    /// Agent mode: skip episodic indexing (agent scaffolding / tool turns would
    /// pollute the AI's recall). The signed transcript is still recorded.
    agent: bool,
}

fn header_str(h: &HeaderMap, name: &str) -> Option<String> {
    h.get(name).and_then(|v| v.to_str().ok()).map(str::to_string)
}

/// Merge all `system` messages into one leading system message (joined in
/// order), keeping the rest as-is. Mistral-family templates reject more than
/// one system message; this keeps us compatible without losing any instruction.
fn coalesce_system(messages: Vec<Value>) -> Vec<Value> {
    let mut sys: Vec<String> = Vec::new();
    let mut rest: Vec<Value> = Vec::new();
    for m in messages {
        if m.get("role").and_then(Value::as_str) == Some("system") {
            if let Some(c) = m.get("content").and_then(Value::as_str) {
                if !c.trim().is_empty() {
                    sys.push(c.to_string());
                }
            }
        } else {
            rest.push(m);
        }
    }
    let mut out = Vec::with_capacity(rest.len() + 1);
    if !sys.is_empty() {
        out.push(json!({ "role": "system", "content": sys.join("\n\n") }));
    }
    out.extend(rest);
    out
}

fn find_sub(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Stateful `<think>…</think>` splitter for the streamed content (the tags can
/// span SSE chunks). The caller's `content` stays clean, and the reasoning is
/// re-emitted as `delta.reasoning_content` - the OpenAI-compatible field agent
/// clients already turn into visible thoughts. Models whose reasoning arrives
/// inline as <think> (Sol through the proxy's Responses adapter) used to lose
/// it here entirely: agent sessions sat frozen through every reasoning
/// stretch. The recorded transcript still keeps the raw body.
#[derive(Default)]
struct ThinkStripper {
    in_think: bool,
    pending: String, // held tail that might be a partial tag
}

impl ThinkStripper {
    /// Returns (visible content, reasoning) extracted from this chunk.
    fn feed(&mut self, s: &str) -> (String, String) {
        let mut work = std::mem::take(&mut self.pending);
        work.push_str(s);
        let mut out = String::new();
        let mut thought = String::new();
        loop {
            if self.in_think {
                match work.find("</think>") {
                    Some(i) => {
                        thought.push_str(&work[..i]);
                        work = work.split_off(i + "</think>".len());
                        self.in_think = false;
                    }
                    None => {
                        let tail = keep_partial_tail(&work, "</think>");
                        let keep = work.len() - tail.len();
                        thought.push_str(&work[..keep]);
                        self.pending = tail;
                        return (out, thought);
                    }
                }
            } else {
                match work.find("<think>") {
                    Some(i) => {
                        out.push_str(&work[..i]);
                        work = work.split_off(i + "<think>".len());
                        self.in_think = true;
                    }
                    None => {
                        let tail = keep_partial_tail(&work, "<think>");
                        let keep = work.len() - tail.len();
                        out.push_str(&work[..keep]);
                        self.pending = tail;
                        return (out, thought);
                    }
                }
            }
        }
    }
}

/// Longest proper prefix of `tag` that `s` ends with (held for the next chunk).
fn keep_partial_tail(s: &str, tag: &str) -> String {
    for len in (1..tag.len()).rev() {
        if s.ends_with(&tag[..len]) {
            return tag[..len].to_string();
        }
    }
    String::new()
}

/// Transform one SSE event for the caller: split `<think>` out of
/// `delta.content` into `delta.reasoning_content`, pass everything else
/// through. Returns the event to emit (with framing), or None to drop it.
fn process_event(event: &str, stripper: &mut ThinkStripper) -> Option<String> {
    let line = event.lines().find(|l| l.starts_with("data:"))?;
    let payload = line["data:".len()..].trim();
    if payload.is_empty() {
        return None;
    }
    if payload == "[DONE]" {
        return Some("data: [DONE]\n\n".to_string());
    }
    let mut v: Value = match serde_json::from_str(payload) {
        Ok(v) => v,
        Err(_) => return Some(format!("data: {payload}\n\n")),
    };
    if let Some(content) = v
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("delta"))
        .and_then(|d| d.get("content"))
        .and_then(Value::as_str)
    {
        let (visible, thought) = stripper.feed(content);
        v["choices"][0]["delta"]["content"] = Value::String(visible);
        if !thought.is_empty() {
            v["choices"][0]["delta"]["reasoning_content"] = Value::String(thought);
        }
    }
    Some(format!("data: {v}\n\n"))
}

/// Split a reply into (thinking, content), preserving BOTH — the transcript
/// records reasoning in its own `thinking` field for audit while `content`
/// stays clean. Online thinking models stream `<think>…</think>` as content.
fn split_think(s: &str) -> (String, String) {
    let mut think = String::new();
    let mut content = String::new();
    let mut rest = s;
    while let Some(start) = rest.find("<think>") {
        content.push_str(&rest[..start]);
        let after = &rest[start + "<think>".len()..];
        match after.find("</think>") {
            Some(end) => {
                think.push_str(&after[..end]);
                rest = &after[end + "</think>".len()..];
            }
            None => {
                think.push_str(after); // unclosed → the remainder is all thinking
                rest = "";
                break;
            }
        }
    }
    content.push_str(rest);
    (think.trim().to_string(), content.trim().to_string())
}

/// Assistant text from a buffered non-streaming JSON response.
fn parse_json_content(bytes: &[u8]) -> String {
    serde_json::from_slice::<Value>(bytes)
        .ok()
        .and_then(|v| {
            v.get("choices")?
                .get(0)?
                .get("message")?
                .get("content")?
                .as_str()
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// Assistant text from an accumulated SSE stream (concatenate delta.content).
fn parse_sse_content(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let mut out = String::new();
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:") else { continue };
        let data = data.trim();
        if data == "[DONE]" || data.is_empty() {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<Value>(data) {
            if let Some(c) = v
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("delta"))
                .and_then(|d| d.get("content"))
                .and_then(Value::as_str)
            {
                out.push_str(c);
            }
        }
    }
    out
}

/// Record the user + assistant turn to the signed transcript (fire-and-forget).
/// Skips silently if the AI isn't provisioned (no agent key) or the reply is
/// empty (e.g. a pure tool-call turn).
fn spawn_record(rec: RecordCtx, assistant: String) {
    if rec.agent_key.is_empty() || (assistant.trim().is_empty() && rec.user_text.trim().is_empty()) {
        return;
    }
    tauri::async_runtime::spawn(async move {
        let hc_state = rec.app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>();

        // Resolve (or create) the conversation + its next sequence number.
        let (conv_hash, mut seq) = match conv_key_lookup(&rec.conv_key) {
            Some(existing) => existing,
            None => {
                let title = Some(rec.user_text.chars().take(80).collect::<String>());
                match crate::commands_holochain::start_conversation(
                    rec.app.clone(),
                    rec.agent_key.clone(),
                    rec.ai_name.clone(),
                    rec.model.clone(),
                    title,
                    Some(rec.app_name.clone()), // source → the calling app (badge)
                    hc_state.clone(),
                )
                .await
                {
                    Ok(h) => (h, 0u32),
                    Err(e) => {
                        log::warn!("[inference] start_conversation failed: {e}");
                        return;
                    }
                }
            }
        };

        let record = |role: &str, content: String, thinking: Option<String>, sequence: u32| {
            let app = rec.app.clone();
            let agent = rec.agent_key.clone();
            let conv = conv_hash.clone();
            let model = rec.model.clone();
            let role = role.to_string();
            let hc = hc_state.clone();
            async move {
                crate::commands_holochain::record_transcript_entry(
                    app, agent, conv, role, content, sequence, model, thinking, None, None, None,
                    None, hc,
                )
                .await
            }
        };

        if !rec.user_text.trim().is_empty() {
            let _ = record("user", rec.user_text.clone(), None, seq).await;
            seq += 1;
        }
        // Separate (don't drop) the model's reasoning: content stays clean while
        // <think> is preserved in the transcript's own `thinking` field for audit.
        let (thinking, content) = split_think(&assistant);
        if !content.trim().is_empty() || !thinking.trim().is_empty() {
            let think = if thinking.trim().is_empty() { None } else { Some(thinking) };
            let _ = record("assistant", content, think, seq).await;
            seq += 1;
        }

        // Index the user turn into this AI's episodic recall (so it remembers
        // this exchange next time). Skipped in agent mode — the "user" turn is
        // often tool results / agent scaffolding, which would pollute recall.
        if !rec.agent {
            crate::inference_memory::index_turn(&rec.app, &rec.ai_id, &conv_hash, &rec.user_text)
                .await;
        }

        // Remember the conversation + next sequence for follow-up turns.
        if let Some(key) = &rec.conv_key {
            conv_map().lock().unwrap().insert(key.clone(), (conv_hash, seq));
        }
    });
}

/// Look up an existing conversation for a caller key (None ⇒ new each time).
fn conv_key_lookup(key: &Option<String>) -> Option<(String, u32)> {
    let key = key.as_ref()?;
    conv_map().lock().unwrap().get(key).cloned()
}

// ── project-memory MCP endpoint ─────────────────────────────────────────────
//
// The Build agent's deliberate-memory tools, served over MCP Streamable
// HTTP. The bridge declares this server in `session/new` with headers
// naming the project and the writing AI; the agent then saves durable
// notes THE MOMENT it learns them (`remember_for_project`) and can check
// what is already known (`read_project_memory`). Responses mirror the
// minimal shape the agent's own client tests accept: plain JSON bodies,
// an mcp-session-id on initialize, bare 202 for notifications, and a
// hang-open SSE stream on GET.

fn mcp_result(id: &Value, result: Value) -> Response {
    Json(json!({ "jsonrpc": "2.0", "id": id, "result": result })).into_response()
}

fn mcp_tool_text(id: &Value, text: String, is_error: bool) -> Response {
    mcp_result(
        id,
        json!({ "content": [{ "type": "text", "text": text }], "isError": is_error }),
    )
}

async fn mcp_post(
    State(app): State<AppHandle>,
    headers: HeaderMap,
    Json(req): Json<Value>,
) -> Response {
    let id = req["id"].clone();
    let folder = header_str(&headers, "x-project").unwrap_or_default();
    let agent_key = header_str(&headers, "x-agent-key").unwrap_or_default();
    let agent_label = header_str(&headers, "x-agent-label").unwrap_or_else(|| "your AI".into());
    match req["method"].as_str() {
        Some("initialize") => {
            let mut resp = mcp_result(
                &id,
                json!({
                    "protocolVersion": req["params"]["protocolVersion"].clone(),
                    "capabilities": { "tools": {} },
                    "serverInfo": { "name": "project-memory", "version": env!("CARGO_PKG_VERSION") },
                }),
            );
            resp.headers_mut()
                .insert("mcp-session-id", header::HeaderValue::from_static("project-memory"));
            resp
        }
        Some("tools/list") => mcp_result(
            &id,
            json!({ "tools": [
                {
                    "name": "remember_for_project",
                    "description": "Save one durable note to this project's shared memory - a command that works, a key file location, a convention, a decision. Use it the moment you learn something future sessions will need. Keep each note to one line.",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "note": { "type": "string", "description": "The durable note, one line." }
                        },
                        "required": ["note"]
                    }
                },
                {
                    "name": "read_project_memory",
                    "description": "Read this project's current shared memory notes.",
                    "inputSchema": { "type": "object", "properties": {} }
                }
            ] }),
        ),
        Some("tools/call") => {
            if folder.is_empty() {
                return mcp_tool_text(&id, "No project is open.".to_string(), true);
            }
            match req["params"]["name"].as_str() {
                Some("remember_for_project") => {
                    let note = req["params"]["arguments"]["note"].as_str().unwrap_or("").to_string();
                    if agent_key.is_empty() {
                        return mcp_tool_text(&id, "No writer identity for this session.".to_string(), true);
                    }
                    if note.trim().is_empty() {
                        return mcp_tool_text(&id, "Nothing to remember - the note was empty.".to_string(), true);
                    }
                    // The save walks every AI's records and writes to the
                    // chain - minutes on a busy machine - and the agent's
                    // whole turn used to block on it. Answer now, write in
                    // the background, one write at a time so two notes can
                    // never race for the same memory conversation. A
                    // failure is logged app-side; the note is not lost
                    // silently, it is lost loudly.
                    let (app2, key, label, folder2) =
                        (app.clone(), agent_key.clone(), agent_label.clone(), folder.clone());
                    tauri::async_runtime::spawn(async move {
                        let _one_at_a_time = PROJECT_MEMORY_WRITE_LOCK.lock().await;
                        let started = std::time::Instant::now();
                        match crate::commands_holochain::project_memory_append_note(
                            &app2, &key, &label, &folder2, &note,
                        )
                        .await
                        {
                            Ok(()) => log::info!(
                                "[mcp] {} remembered a note for {} ({} ms, background)",
                                label, folder2, started.elapsed().as_millis()
                            ),
                            Err(e) => log::warn!(
                                "[mcp] remember_for_project failed after {} ms: {e}",
                                started.elapsed().as_millis()
                            ),
                        }
                    });
                    log::info!("[mcp] {} queued a note for {}", agent_label, folder);
                    mcp_tool_text(
                        &id,
                        "Remembered for this project (saving in the background).".to_string(),
                        false,
                    )
                }
                Some("read_project_memory") => {
                    match crate::commands_holochain::project_memory_read(&app, &folder).await {
                        Ok(content) if !content.trim().is_empty() => {
                            mcp_tool_text(&id, content, false)
                        }
                        Ok(_) => mcp_tool_text(&id, "(no project memory yet)".to_string(), false),
                        Err(e) => mcp_tool_text(&id, format!("Could not read memory: {e}"), true),
                    }
                }
                other => mcp_tool_text(&id, format!("Unknown tool: {:?}", other), true),
            }
        }
        // notifications/initialized and anything else: acknowledge quietly.
        _ => StatusCode::ACCEPTED.into_response(),
    }
}

/// `POST /v1/responses` - the Build agent's web_search tool speaks the
/// OpenAI Responses shape (`model`, `input`, `tools: [{type: web_search}]`)
/// at this server. Until now nothing answered and the agent saw a bare 404
/// and quietly fell back to fetching pages by hand. Now: an AI that may go
/// online (a pinned online model, or Auto - Online and Offline) gets the
/// search routed to the online search model through the Flowsta proxy, billed
/// per call like any other online turn; an offline-only AI gets a plain
/// refusal it can act on (fetch a known URL, or ask the user to switch the
/// AI's model) instead of a mystery.
async fn responses_web_search(
    State(app): State<AppHandle>,
    Json(body): Json<Value>,
) -> Response {
    let model = body.get("model").and_then(Value::as_str).unwrap_or("").trim().to_string();
    let Some(ai) = resolve_ai(&app, &model) else {
        return err(
            StatusCode::NOT_FOUND,
            &format!("No AI matches model '{model}'. Use one from GET /v1/models."),
            "model_not_found",
        );
    };
    // The query: `input` is a string, or an array of messages / content parts.
    let query: String = match body.get("input") {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|it| {
                if let Some(s) = it.as_str() { return Some(s.to_string()); }
                match it.get("content") {
                    Some(Value::String(s)) => Some(s.clone()),
                    Some(Value::Array(parts)) => Some(
                        parts.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join(" "),
                    ),
                    _ => it.get("text").and_then(Value::as_str).map(str::to_string),
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    };
    if query.trim().is_empty() {
        return err(StatusCode::BAD_REQUEST, "No search query in `input`.", "invalid_request");
    }
    // May THIS AI go online? Its own model setting is the consent: a pinned
    // online model, or the automatic mode that is allowed to reach out. Any
    // offline setting means web search is off for it - a web search is by
    // definition an online act, so no header or heuristic overrides this.
    let may_go_online = ai.model.starts_with("online:") || ai.model == "auto:online-offline";
    if !may_go_online {
        log::info!("[inference] web search refused for {} - AI is offline-only ({})", ai.name, ai.model);
        // Tell the rail, so the person sees WHY and how to turn it on -
        // the agent's own wording of the refusal is not guaranteed.
        use tauri::Emitter as _;
        let _ = app.emit(
            "agent-hint",
            json!({
                "kind": "web_search_offline",
                "ai": ai.name,
                "text": format!(
                    "{} stays offline, so it can't search the web. To allow that, set its model to Auto - Online and Offline or an online model (the model chip beside Ask, or the AI's card).",
                    ai.name
                )
            }),
        );
        return err(
            StatusCode::FORBIDDEN,
            &format!(
                "Web search is off for '{}': this AI is set to stay offline. Fetch a specific URL you already know instead, or ask the user to set the AI to Auto - Online and Offline (or an online model) if live web results are needed.",
                ai.name
            ),
            "web_search_offline",
        );
    }
    let token = match crate::flowsta::get_access_token(&app).await {
        Ok(t) => t,
        Err(_) => {
            return err(
                StatusCode::UNAUTHORIZED,
                "Web search needs the online models: sign in with Flowsta in the app to enable it.",
                "auth_required",
            )
        }
    };
    let search_model = crate::router::DEFAULT_FRESH.trim_start_matches("online:");
    log::info!("[inference] web search for {} via {} ({} chars)", ai.name, search_model, query.len());
    let upstream = reqwest::Client::new()
        .post(format!("{}/v1/chat/completions", crate::flowsta::proxy_url()))
        .bearer_auth(token)
        .json(&json!({
            "model": search_model,
            "messages": [
                { "role": "system", "content": "You are a web search assistant. Search the web for the user's query and answer concisely with the facts found, citing sources." },
                { "role": "user", "content": query }
            ],
            "stream": true,
            "max_tokens": 4096
        }))
        .send()
        .await;
    let upstream = match upstream {
        Ok(r) => r,
        Err(e) => return err(StatusCode::BAD_GATEWAY, &format!("Search upstream error: {e}"), "upstream_error"),
    };
    let status = upstream.status();
    let text = upstream.text().await.unwrap_or_default();
    if !status.is_success() {
        // Pass the proxy's own reason through (entitlement_required, allowance..)
        // so the agent's message names the real block.
        let reason = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v.pointer("/error/message").and_then(Value::as_str).map(str::to_string))
            .unwrap_or_else(|| text.chars().take(300).collect());
        return err(
            StatusCode::from_u16(status.as_u16()).unwrap_or(StatusCode::BAD_GATEWAY),
            &format!("Web search unavailable: {reason}"),
            "upstream_error",
        );
    }
    // The proxy always streams SSE: gather the answer text and the search
    // results (title + url) it sends with the final chunk.
    let mut answer = String::new();
    let mut sources: Vec<(String, String)> = Vec::new();
    for line in text.lines() {
        let Some(data) = line.strip_prefix("data:") else { continue };
        let data = data.trim();
        if data.is_empty() || data == "[DONE]" { continue; }
        let Ok(chunk) = serde_json::from_str::<Value>(data) else { continue };
        if let Some(c) = chunk.pointer("/choices/0/delta/content").and_then(Value::as_str) {
            answer.push_str(c);
        }
        if let Some(results) = chunk.get("search_results").and_then(Value::as_array) {
            let found: Vec<(String, String)> = results
                .iter()
                .filter_map(|r| {
                    let url = r["url"].as_str()?.to_string();
                    let title = r["title"].as_str().unwrap_or(&url).to_string();
                    Some((title, url))
                })
                .collect();
            if !found.is_empty() { sources = found; }
        }
    }
    // The search model thinks out loud in <think> tags - that is not part of
    // the answer the agent should read back to the user.
    let mut answer = answer;
    while let (Some(a), Some(b)) = (answer.find("<think>"), answer.find("</think>")) {
        if b > a { answer.replace_range(a..b + "</think>".len(), ""); } else { break; }
    }
    let mut out = answer.trim().to_string();
    if !sources.is_empty() {
        out.push_str("\n\nSources:\n");
        for (t, u) in &sources {
            out.push_str(&format!("- {t}: {u}\n"));
        }
    }
    // Citations in the OpenAI Responses shape the agent parses strictly:
    // start/end are the URL's character span in the text (required fields).
    let out_chars: Vec<char> = out.chars().collect();
    let char_index_of = |needle: &str| -> Option<usize> {
        out.find(needle).map(|b| out[..b].chars().count())
    };
    let annotations: Vec<Value> = sources
        .iter()
        .map(|(t, u)| {
            let start = char_index_of(u).unwrap_or(0);
            let end = (start + u.chars().count()).min(out_chars.len());
            json!({ "type": "url_citation", "title": t, "url": u, "start_index": start as u32, "end_index": end as u32 })
        })
        .collect();
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default();
    let stamp = now.as_micros();
    let id = format!("resp_{stamp}");
    // Responses-shaped reply: one output message with the text and citations.
    Json(json!({
        "id": id,
        "object": "response",
        "created_at": now.as_secs(),
        "model": search_model,
        "status": "completed",
        "output": [{
            "type": "message",
            "id": format!("msg_{stamp}"),
            "role": "assistant",
            "status": "completed",
            "content": [{ "type": "output_text", "text": out, "annotations": annotations }]
        }],
        "output_text": out
    }))
    .into_response()
}

/// Project-memory writes from the MCP tool run in the background; this keeps
/// them strictly sequential so two notes cannot both read the same base and
/// drop each other's row.
static PROJECT_MEMORY_WRITE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn mcp_get() -> Response {
    // A hang-open SSE stream (the client keeps it for server-initiated
    // messages; we never send any).
    let stream = async_stream::stream! {
        let () = std::future::pending().await;
        yield Ok::<_, std::io::Error>(String::new());
    };
    (
        [(header::CONTENT_TYPE, "text/event-stream")],
        Body::from_stream(stream),
    )
        .into_response()
}

#[cfg(test)]
mod think_tests {
    use super::ThinkStripper;

    /// Feed `pieces` one at a time; return (visible, reasoning) concatenated.
    fn run(pieces: &[&str]) -> (String, String) {
        let mut s = ThinkStripper::default();
        let mut visible = String::new();
        let mut thought = String::new();
        for p in pieces {
            let (v, t) = s.feed(p);
            visible.push_str(&v);
            thought.push_str(&t);
        }
        (visible, thought)
    }

    #[test]
    fn strips_think_blocks_across_chunks() {
        assert_eq!(run(&["hello"]).0, "hello");
        assert_eq!(run(&["<think>reasoning</think>answer"]).0, "answer");
        // tag split across chunks
        assert_eq!(run(&["a<thi", "nk>secret</thi", "nk>visible"]).0, "avisible");
        // think in the middle
        assert_eq!(run(&["pre", "<think>", "hidden", "</think>", "post"]).0, "prepost");
        // a lone '<' that isn't a think tag is held then released
        assert_eq!(run(&["hi <", "br> there"]).0, "hi <br> there");
        // unclosed think → everything after leaves content (it is reasoning)
        assert_eq!(run(&["ok <think> never closes"]).0, "ok ");
    }

    #[test]
    fn reasoning_is_kept_not_dropped() {
        // The stripped text streams on as reasoning_content - Sol's thinking
        // must stay visible to agent clients, not vanish.
        assert_eq!(run(&["<think>reasoning</think>answer"]).1, "reasoning");
        assert_eq!(run(&["a<thi", "nk>sec", "ret</thi", "nk>b"]).1, "secret");
        // reasoning streams live even before the close tag arrives
        assert_eq!(run(&["<think>partial thought"]).1, "partial thought");
        assert_eq!(run(&["plain text only"]).1, "");
    }
}
