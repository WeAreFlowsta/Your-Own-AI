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

/// Dedicated Your Own AI port (deliberately NOT Ollama's 11434).
const PORT: u16 = 11435;

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

/// Spawn the inference server in a background task. Best-effort: a bind failure
/// is logged and the app continues (the server is an add-on, not core).
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let router = Router::new()
            .route("/health", get(|| async { Json(json!({ "status": "ok" })) }))
            .route("/v1/models", get(list_models))
            .route("/v1/chat/completions", post(chat_completions))
            .with_state(app);

        let addr = format!("127.0.0.1:{PORT}");
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => {
                log::info!("[inference] Your Own AI inference server on http://{addr}");
                if let Err(e) = axum::serve(listener, router).await {
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
    let base = model.strip_suffix(":agent").unwrap_or(model).trim();
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
        // server; API callers opt in per request. The API's difficulty
        // escalation stays coupled to its freshness header (that was its
        // shipped contract; there is no per-request toggle yet).
        let lean = match header_str(&headers, "x-your-own-ai-lean")
            .map(|s| s.trim().to_lowercase())
            .as_deref()
        {
            Some("speed") => "speed",
            Some("quality") => "quality",
            _ => "balanced",
        };
        let escalate_hard = eagerness == "freshness";
        match crate::router::route(&app, mode, &query, eagerness, task, difficulty, lean, escalate_hard).await {
            Ok(r) => {
                log::info!("[inference] router ({}): {mode} task={task} diff={difficulty} eag={eagerness} -> {} ({})", ai.name, r.model, r.reason);
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
        if current.as_deref() != Some(ai.model.as_str()) {
            log::info!(
                "[inference] switching model for AI '{}': {:?} -> {}",
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

    let messages = if agent_mode {
        // Caller owns the system prompt (no persona). Memory is injected as a
        // bounded supplementary system message AFTER the caller's own system
        // prompt (so theirs stays authoritative), unless opted out.
        let mut msgs = incoming.clone();
        if agent_memory {
            let user_memory =
                crate::inference_memory::build_memory_block(&app, &query, &ai.id, None).await;
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
        agent_key: ai.agent_pub_key.clone(),
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

/// Stateful `<think>…</think>` remover for the streamed content (the tags can
/// span SSE chunks). Strips reasoning from what the CALLER sees; the recorded
/// transcript keeps it (recorded from the raw body, separated into `thinking`).
#[derive(Default)]
struct ThinkStripper {
    in_think: bool,
    pending: String, // held tail that might be a partial tag
}

impl ThinkStripper {
    fn feed(&mut self, s: &str) -> String {
        let mut work = std::mem::take(&mut self.pending);
        work.push_str(s);
        let mut out = String::new();
        loop {
            if self.in_think {
                match work.find("</think>") {
                    Some(i) => {
                        work = work.split_off(i + "</think>".len());
                        self.in_think = false;
                    }
                    None => {
                        self.pending = keep_partial_tail(&work, "</think>");
                        return out;
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
                        return out;
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

/// Transform one SSE event for the caller: strip `<think>` from `delta.content`,
/// pass everything else through. Returns the event to emit (with framing), or
/// None to drop it.
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
        let visible = stripper.feed(content);
        v["choices"][0]["delta"]["content"] = Value::String(visible);
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
                    app, agent, conv, role, content, sequence, model, thinking, None, None, hc,
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

#[cfg(test)]
mod think_tests {
    use super::ThinkStripper;

    /// Feed `pieces` one at a time; return the concatenated visible output.
    fn run(pieces: &[&str]) -> String {
        let mut s = ThinkStripper::default();
        pieces.iter().map(|p| s.feed(p)).collect()
    }

    #[test]
    fn strips_think_blocks_across_chunks() {
        assert_eq!(run(&["hello"]), "hello");
        assert_eq!(run(&["<think>reasoning</think>answer"]), "answer");
        // tag split across chunks
        assert_eq!(run(&["a<thi", "nk>secret</thi", "nk>visible"]), "avisible");
        // think in the middle
        assert_eq!(run(&["pre", "<think>", "hidden", "</think>", "post"]), "prepost");
        // a lone '<' that isn't a think tag is held then released
        assert_eq!(run(&["hi <", "br> there"]), "hi <br> there");
        // unclosed think → everything after is dropped
        assert_eq!(run(&["ok <think> never closes"]), "ok ");
    }
}
