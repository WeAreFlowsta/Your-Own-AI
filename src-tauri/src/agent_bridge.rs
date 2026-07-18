// Bridge to the Your Own AI Build coding agent over ACP (JSON-RPC on stdio).
//
// The agent runs as a child process (`your-own-ai-build agent stdio`). This
// module owns the process, drives the handshake (initialize -> session/new),
// and forwards traffic to the frontend as events:
//   agent-update      every session/update + agent notification (raw JSON)
//   agent-ready       { sessionId } once the session is open
//   agent-permission  a session/request_permission request needing a UI answer
//   agent-turn        the response that ends a prompt turn (stopReason, usage)
//   agent-log         stderr lines
//   agent-exit        { code } when the process terminates
//
// Permission requests are ALWAYS surfaced to the user; nothing is auto-approved.

use serde_json::{json, Value};
use std::sync::atomic::{AtomicU64, Ordering};
use tauri::{AppHandle, Emitter, Manager, State};
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri_plugin_shell::ShellExt;
use tokio::sync::Mutex;

pub struct AgentBridgeState {
    child: Mutex<Option<CommandChild>>,
    session_id: Mutex<Option<String>>,
    next_id: AtomicU64,
}

impl AgentBridgeState {
    pub fn new() -> Self {
        Self {
            child: Mutex::new(None),
            session_id: Mutex::new(None),
            next_id: AtomicU64::new(1),
        }
    }
}

const INIT_ID: u64 = 1;
const SESSION_NEW_ID: u64 = 2;
/// `session/set_model` sent right after the session opens (when a model was
/// requested). The agent ignores metas on session/new; this request is the
/// real selection mechanism - ids are the `:11435` catalog's name slugs
/// (e.g. `kimiveebo:agent`).
const SET_MODEL_ID: u64 = 3;

/// Stable identifier the agent uses to key persisted permission grants
/// (per folder, per client). Changing it would silently reset every
/// "don't ask again" the user has granted.
const CLIENT_IDENTIFIER: &str = "your-own-ai";

/// The agent's selectable model catalog is CONFIG-DEFINED: `session/set_model`
/// only resolves ids that exist in `~/.your-own-ai-build/config.toml` (by
/// `[model.*]` section key, or by an entry's `model` string). Nothing is
/// discovered from the local server's model list. So before starting the
/// agent, make sure the conversation's AI has an entry - appending a new
/// `[model.<slug>]` table is valid TOML and leaves the user's file intact.
fn ensure_agent_model_entry(home: &std::path::Path, slug: &str) -> Result<(), String> {
    let config_path = home.join(".your-own-ai-build").join("config.toml");
    let existing = std::fs::read_to_string(&config_path).unwrap_or_default();
    let header = format!("[model.{}]", slug);
    if existing.contains(&header) {
        return Ok(());
    }
    let entry = format!(
        "\n{header}\nmodel = \"{slug}:agent\"\nbase_url = \"http://localhost:11435/v1\"\nname = \"{slug}\"\napi_key = \"local\"\napi_backend = \"chat_completions\"\ncontext_window = 32768\n",
    );
    if let Some(parent) = config_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create agent config dir: {}", e))?;
    }
    let mut content = existing;
    content.push_str(&entry);
    std::fs::write(&config_path, content)
        .map_err(|e| format!("cannot write agent config: {}", e))
}

async fn write_line(state: &AgentBridgeState, value: &Value) -> Result<(), String> {
    let mut guard = state.child.lock().await;
    let child = guard.as_mut().ok_or("agent is not running")?;
    let mut line = value.to_string();
    line.push('\n');
    child
        .write(line.as_bytes())
        .map_err(|e| format!("failed to write to agent: {}", e))
}

#[tauri::command]
pub async fn start_build_agent(
    app_handle: AppHandle,
    state: State<'_, AgentBridgeState>,
    binary: String,
    cwd: String,
    model: Option<String>,
) -> Result<(), String> {
    // One agent at a time: replace any previous instance.
    if let Some(old) = state.child.lock().await.take() {
        let _ = old.kill();
    }
    *state.session_id.lock().await = None;

    std::fs::create_dir_all(&cwd).map_err(|e| format!("cannot create workspace dir: {}", e))?;

    // The model must exist in the agent's config catalog before the process
    // starts, or session/set_model has nothing to resolve.
    if let Some(slug) = &model {
        let home = app_handle
            .path()
            .home_dir()
            .map_err(|e| format!("cannot resolve home dir: {}", e))?;
        ensure_agent_model_entry(&home, slug)?;
    }

    let (mut rx, child) = app_handle
        .shell()
        .command(&binary)
        .args(["agent", "stdio"])
        .current_dir(std::path::PathBuf::from(&cwd))
        .spawn()
        .map_err(|e| format!("failed to start agent: {}", e))?;

    *state.child.lock().await = Some(child);
    state.next_id.store(4, Ordering::SeqCst); // 1..=3 = handshake requests

    let workspace = cwd.clone();
    let session_model = model.clone();
    let reader_app = app_handle.clone();
    tauri::async_runtime::spawn(async move {
        let mut exit_code: Option<i32> = None;
        while let Some(event) = rx.recv().await {
            match event {
                CommandEvent::Stdout(line) => {
                    let text = String::from_utf8_lossy(&line);
                    let Ok(msg) = serde_json::from_str::<Value>(&text) else {
                        continue;
                    };
                    handle_agent_message(&reader_app, &workspace, &session_model, msg).await;
                }
                CommandEvent::Stderr(line) => {
                    let _ = reader_app
                        .emit("agent-log", String::from_utf8_lossy(&line).trim_end());
                }
                CommandEvent::Terminated(payload) => {
                    exit_code = payload.code;
                    break;
                }
                _ => {}
            }
        }
        let bridge = reader_app.state::<AgentBridgeState>();
        *bridge.child.lock().await = None;
        *bridge.session_id.lock().await = None;
        let _ = reader_app.emit("agent-exit", json!({ "code": exit_code }));
    });

    write_line(
        &state,
        &json!({
            "jsonrpc": "2.0",
            "id": INIT_ID,
            "method": "initialize",
            "params": {
                "protocolVersion": 1,
                "clientInfo": {
                    "name": CLIENT_IDENTIFIER,
                    "title": "Your Own AI",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "clientCapabilities": {
                    "fs": { "readTextFile": false, "writeTextFile": false },
                    "terminal": false
                },
                "_meta": { "clientIdentifier": CLIENT_IDENTIFIER }
            }
        }),
    )
    .await
}

async fn handle_agent_message(
    app: &AppHandle,
    workspace: &str,
    session_model: &Option<String>,
    msg: Value,
) {
    let state = app.state::<AgentBridgeState>();
    let method = msg.get("method").and_then(Value::as_str);
    let id = msg.get("id").and_then(Value::as_u64);

    match (method, id) {
        // A request FROM the agent that needs a user decision.
        (Some("session/request_permission"), Some(_)) => {
            let _ = app.emit("agent-permission", &msg);
        }
        // Notifications: session updates and agent housekeeping.
        (Some(_), None) => {
            let _ = app.emit("agent-update", &msg);
        }
        // Responses to our own requests.
        (None, Some(INIT_ID)) => {
            let request = json!({
                "jsonrpc": "2.0",
                "id": SESSION_NEW_ID,
                "method": "session/new",
                "params": {
                    "cwd": workspace,
                    "mcpServers": [],
                    "_meta": { "clientIdentifier": CLIENT_IDENTIFIER }
                }
            });
            let _ = write_line(&state, &request).await;
        }
        (None, Some(SESSION_NEW_ID)) => {
            let session_id = msg
                .pointer("/result/sessionId")
                .and_then(Value::as_str)
                .map(str::to_string);
            match session_id {
                Some(sid) => {
                    *state.session_id.lock().await = Some(sid.clone());
                    if let Some(model) = session_model {
                        // Select the conversation's AI; agent-ready follows
                        // the set_model response so the first prompt can't
                        // race onto the config-default model.
                        let request = json!({
                            "jsonrpc": "2.0",
                            "id": SET_MODEL_ID,
                            "method": "session/set_model",
                            "params": { "sessionId": sid, "modelId": model }
                        });
                        let _ = write_line(&state, &request).await;
                    } else {
                        let _ = app.emit("agent-ready", json!({ "sessionId": sid }));
                    }
                }
                None => {
                    let _ = app.emit("agent-log", format!("session/new failed: {}", msg));
                }
            }
        }
        (None, Some(SET_MODEL_ID)) => {
            // Soft-fail: an unknown model id keeps the agent's default model
            // rather than killing the session - but say so loudly.
            if let Some(err) = msg.get("error") {
                let _ = app.emit(
                    "agent-log",
                    format!(
                        "couldn't set model {:?}: {} - running on the agent's default model",
                        session_model, err
                    ),
                );
            }
            let sid = state.session_id.lock().await.clone();
            if let Some(sid) = sid {
                let _ = app.emit("agent-ready", json!({ "sessionId": sid }));
            }
        }
        // Prompt-turn completions (and any other response to our requests).
        (None, Some(_)) => {
            let _ = app.emit("agent-turn", &msg);
        }
        // Other agent-side requests (fs/terminal are disabled, so none are
        // expected) - refuse politely so the agent never hangs on us.
        (Some(_), Some(req_id)) => {
            let refusal = json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "error": { "code": -32601, "message": "not supported by this client" }
            });
            let _ = write_line(&state, &refusal).await;
        }
        (None, None) => {}
    }
}

#[tauri::command]
pub async fn send_agent_prompt(
    state: State<'_, AgentBridgeState>,
    text: String,
) -> Result<u64, String> {
    let session_id = state
        .session_id
        .lock()
        .await
        .clone()
        .ok_or("agent session is not ready")?;
    let id = state.next_id.fetch_add(1, Ordering::SeqCst);
    write_line(
        &state,
        &json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": "session/prompt",
            "params": {
                "sessionId": session_id,
                "prompt": [{ "type": "text", "text": text }]
            }
        }),
    )
    .await?;
    Ok(id)
}

/// Cancel the in-flight prompt turn (ACP `session/cancel` notification).
/// The agent stops its work and still ends the turn with a response whose
/// stopReason is `cancelled` - the frontend keeps listening for it.
#[tauri::command]
pub async fn cancel_agent_turn(state: State<'_, AgentBridgeState>) -> Result<(), String> {
    let session_id = state
        .session_id
        .lock()
        .await
        .clone()
        .ok_or("agent session is not ready")?;
    write_line(
        &state,
        &json!({
            "jsonrpc": "2.0",
            "method": "session/cancel",
            "params": { "sessionId": session_id }
        }),
    )
    .await
}

#[tauri::command]
pub async fn respond_agent_permission(
    state: State<'_, AgentBridgeState>,
    request_id: u64,
    option_id: Option<String>,
) -> Result<(), String> {
    let outcome = match option_id {
        Some(opt) => json!({ "outcome": "selected", "optionId": opt }),
        None => json!({ "outcome": "cancelled" }),
    };
    write_line(
        &state,
        &json!({
            "jsonrpc": "2.0",
            "id": request_id,
            "result": { "outcome": outcome }
        }),
    )
    .await
}

#[tauri::command]
pub async fn stop_build_agent(state: State<'_, AgentBridgeState>) -> Result<(), String> {
    *state.session_id.lock().await = None;
    if let Some(child) = state.child.lock().await.take() {
        child.kill().map_err(|e| format!("failed to stop agent: {}", e))?;
    }
    Ok(())
}

/// Dropped-path classifier for the chat's drag-and-drop: a folder drop opens
/// the folder, a file drop stays an attachment.
#[tauri::command]
pub fn path_is_dir(path: String) -> bool {
    std::path::Path::new(&path).is_dir()
}

#[tauri::command]
pub async fn build_agent_status(state: State<'_, AgentBridgeState>) -> Result<Value, String> {
    let running = state.child.lock().await.is_some();
    let session = state.session_id.lock().await.clone();
    Ok(json!({ "running": running, "sessionId": session }))
}

/// Kill the agent on app exit (called from the RunEvent::ExitRequested handler).
pub fn kill_on_exit(app_handle: &AppHandle) {
    let state = app_handle.state::<AgentBridgeState>();
    let mut child = state.child.blocking_lock();
    if let Some(c) = child.take() {
        let _ = c.kill();
    }
}
