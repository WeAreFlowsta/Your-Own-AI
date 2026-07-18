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

/// Stable identifier the agent uses to key persisted permission grants
/// (per folder, per client). Changing it would silently reset every
/// "don't ask again" the user has granted.
const CLIENT_IDENTIFIER: &str = "your-own-ai";

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

    let (mut rx, child) = app_handle
        .shell()
        .command(&binary)
        .args(["agent", "stdio"])
        .current_dir(std::path::PathBuf::from(&cwd))
        .spawn()
        .map_err(|e| format!("failed to start agent: {}", e))?;

    *state.child.lock().await = Some(child);
    state.next_id.store(3, Ordering::SeqCst); // 1 = initialize, 2 = session/new

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
            let mut meta = json!({ "clientIdentifier": CLIENT_IDENTIFIER });
            if let Some(model) = session_model {
                meta["modelId"] = json!(model);
            }
            let request = json!({
                "jsonrpc": "2.0",
                "id": SESSION_NEW_ID,
                "method": "session/new",
                "params": { "cwd": workspace, "mcpServers": [], "_meta": meta }
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
                    let _ = app.emit("agent-ready", json!({ "sessionId": sid }));
                }
                None => {
                    let _ = app.emit("agent-log", format!("session/new failed: {}", msg));
                }
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
