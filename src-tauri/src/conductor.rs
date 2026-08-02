//! Holochain conductor lifecycle management for Your Own AI.
//!
//! Starts lair-keystore and holochain as child processes, waits for readiness.
//! Adapted from ProofPoll's conductor.rs — simplified for local-only use
//! (no DHT networking, no migration clients).

use crate::lair;
use crate::process_ext::CommandExt;
use lair_keystore_api::prelude::LairClient;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use tauri::Emitter;

/// Admin WebSocket port for the local Holochain conductor.
/// Different from Flowsta Vault (4455) and ProofPoll (4466).
pub const ADMIN_WS_PORT: u16 = 4477;

/// Handle to a running conductor + lair-keystore pair.
pub struct ConductorHandle {
    pub lair_child: Child,
    pub conductor_child: Child,
    pub admin_port: u16,
    pub app_port: u16,
}

impl ConductorHandle {
    pub fn shutdown(mut self) {
        log::info!("Shutting down conductor...");
        if let Err(e) = self.conductor_child.kill() {
            log::warn!("Failed to kill conductor process: {}", e);
        }
        let _ = self.conductor_child.wait();

        log::info!("Shutting down lair-keystore...");
        if let Err(e) = self.lair_child.kill() {
            log::warn!("Failed to kill lair-keystore process: {}", e);
        }
        let _ = self.lair_child.wait();

        log::info!("Conductor and lair-keystore stopped");
    }
}

/// Conductor status reported to the frontend.
#[derive(Clone, serde::Serialize)]
#[serde(tag = "status")]
pub enum ConductorStatus {
    #[serde(rename = "stopped")]
    Stopped,
    #[serde(rename = "starting")]
    Starting { message: String },
    #[serde(rename = "ready")]
    Ready { admin_port: u16, app_port: u16 },
    #[serde(rename = "error")]
    Error { message: String },
}

/// Result of the startup sequence.
pub struct StartupResult {
    pub handle: ConductorHandle,
    pub lair_client: LairClient,
}

/// Generate conductor-config.yaml for local-only use.
///
/// INVARIANT — this conductor must NEVER be networked. Transcripts are
/// plaintext entries on a DNA whose network seed is shared by every
/// YOAI install; privacy comes entirely from the black-hole localhost
/// network endpoints below. If cross-device sync is ever needed, it
/// goes through Flowsta Vault's encrypted per-app namespace (see
/// a later release), NOT by giving this conductor real
/// bootstrap/signal/relay URLs.
fn generate_conductor_config(
    conductor_dir: &Path,
    lair_connection_url: &str,
    admin_port: u16,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(conductor_dir)
        .map_err(|e| format!("Failed to create conductor directory: {}", e))?;

    // Path values use SINGLE-quoted YAML strings — double-quoted YAML
    // interprets backslash escapes (e.g. "C:\Users\..." reads "\U" as a
    // Unicode escape and fails on the first non-hex character). Single
    // quotes pass backslashes through verbatim; the only escape needed
    // is doubling embedded single quotes. (Same fix as ProofPoll/Vault —
    // this bit ProofPoll's first Windows install in beta11.)
    let data_root = conductor_dir.display().to_string().replace('\'', "''");
    let lair_url = lair_connection_url.replace('\'', "''");

    // relay_url is new in Holochain 0.6.1 (Iroh transport); localhost
    // black-hole like bootstrap/signal — connection failures are
    // tolerated, gossip simply never happens.
    let config = format!(
        r#"data_root_path: '{data_root}'
keystore:
  type: lair_server
  connection_url: '{lair_url}'
admin_interfaces:
- driver:
    type: websocket
    port: {admin_port}
    allowed_origins: '*'
network:
  bootstrap_url: https://localhost/
  signal_url: wss://localhost/
  relay_url: https://localhost/
db_sync_strategy: Resilient
"#,
        data_root = data_root,
        admin_port = admin_port,
        lair_url = lair_url,
    );

    let config_path = conductor_dir.join("conductor-config.yaml");
    std::fs::write(&config_path, &config)
        .map_err(|e| format!("Failed to write conductor config: {}", e))?;

    log::info!("Conductor config written to {:?}", config_path);
    Ok(config_path)
}

/// Start the holochain conductor process.
fn start_conductor_process(
    config_path: &Path,
    conductor_dir: &Path,
    passphrase: &str,
) -> Result<Child, String> {
    log::info!("Starting holochain conductor...");

    let stdout_path = conductor_dir.join("holochain-stdout.log");
    let stderr_path = conductor_dir.join("holochain-stderr.log");

    let stdout_file = std::fs::File::create(&stdout_path)
        .map_err(|e| format!("Failed to create conductor stdout log: {}", e))?;
    let stderr_file = std::fs::File::create(&stderr_path)
        .map_err(|e| format!("Failed to create conductor stderr log: {}", e))?;

    let holochain_bin = crate::resolve_sidecar_bin("yourowai-holochain");
    log::info!("Using holochain binary: {:?}", holochain_bin);

    let mut child = std::process::Command::new(&holochain_bin)
        .arg("-c")
        .arg(config_path)
        .arg("--piped")
        .stdin(Stdio::piped())
        .stdout(stdout_file)
        .stderr(stderr_file)
        .tie_to_parent()
        .spawn_hidden()
        .map_err(|e| format!("Failed to spawn holochain conductor: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        use std::io::Write;
        stdin
            .write_all(format!("{}\n", passphrase).as_bytes())
            .map_err(|e| format!("Failed to write passphrase to conductor: {}", e))?;
    }

    log::info!("Holochain conductor started (pid {})", child.id());

    // Brief check for immediate failure.
    std::thread::sleep(std::time::Duration::from_millis(500));
    match child.try_wait() {
        Ok(Some(status)) => {
            let output = read_conductor_logs(conductor_dir);
            Err(format!(
                "Holochain conductor exited immediately (status {}): {}",
                status, output.trim()
            ))
        }
        Ok(None) => Ok(child),
        Err(e) => Err(format!("Failed to check conductor process status: {}", e)),
    }
}

fn read_conductor_logs(conductor_dir: &Path) -> String {
    let stderr_path = conductor_dir.join("holochain-stderr.log");
    let stdout_path = conductor_dir.join("holochain-stdout.log");

    let stderr = std::fs::read_to_string(&stderr_path).unwrap_or_default();
    let stdout = std::fs::read_to_string(&stdout_path).unwrap_or_default();

    let output = if !stderr.is_empty() { stderr } else { stdout };
    if output.len() > 500 {
        format!("{}...", &output[..500])
    } else {
        output
    }
}

/// Wait for the conductor admin WebSocket to be ready.
async fn wait_for_admin_ws(
    port: u16,
    timeout_secs: u64,
    conductor_child: &mut Child,
    conductor_dir: &Path,
) -> Result<(), String> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);
    let mut attempt = 0;

    while std::time::Instant::now() < deadline {
        attempt += 1;

        match conductor_child.try_wait() {
            Ok(Some(status)) => {
                let output = read_conductor_logs(conductor_dir);
                return Err(format!(
                    "Conductor exited during startup (status {}): {}",
                    status,
                    output.trim()
                ));
            }
            Ok(None) => {}
            Err(e) => return Err(format!("Failed to check conductor process: {}", e)),
        }

        match tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port)).await {
            Ok(_) => {
                log::info!(
                    "Conductor admin WS ready on port {} (attempt {})",
                    port,
                    attempt
                );
                return Ok(());
            }
            Err(_) => {
                if attempt <= 3 {
                    log::info!("Waiting for conductor admin WS (attempt {})...", attempt);
                }
                tokio::time::sleep(std::time::Duration::from_millis(500)).await;
            }
        }
    }

    let output = read_conductor_logs(conductor_dir);
    if !output.trim().is_empty() {
        Err(format!(
            "Conductor not ready after {}s. Logs: {}",
            timeout_secs, output.trim()
        ))
    } else {
        Err(format!(
            "Conductor admin WS not ready after {}s on port {}",
            timeout_secs, port
        ))
    }
}

/// Full startup sequence: lair → conductor → wait for ready.
///
/// Does NOT install DNAs or set up app interfaces — that's handled
/// by the multi-agent manager (holochain.rs) which provisions agents lazily.
pub async fn start_holochain(
    app_handle: tauri::AppHandle,
    data_dir: PathBuf,
    passphrase: String,
) -> Result<StartupResult, String> {
    let _ = app_handle.emit(
        "conductor-status",
        ConductorStatus::Starting {
            message: "Starting lair-keystore...".into(),
        },
    );

    // 1. Start lair-keystore.
    let lair_dir = data_dir.join("lair");
    let (mut lair_child, connection_url) = lair::start_lair_process(&lair_dir, &passphrase)?;

    macro_rules! fail_with_lair_cleanup {
        ($err:expr) => {{
            let _ = lair_child.kill();
            let _ = lair_child.wait();
            return Err($err);
        }};
    }

    // 2. Wait for lair socket.
    if let Err(e) = lair::wait_for_lair_socket(&connection_url, 15).await {
        fail_with_lair_cleanup!(e);
    }

    // 3. Connect to lair. Timeout is load-bearing: this connect has hung
    // indefinitely in the field (lair up, socket present, no answer) - the
    // UI then spins forever with nothing in the log between "socket ready"
    // and "config written". Bound it and say so, loudly.
    let _ = app_handle.emit(
        "conductor-status",
        ConductorStatus::Starting {
            message: "Connecting to lair-keystore...".into(),
        },
    );
    log::info!("Connecting to lair-keystore at {}", connection_url);
    let lair_client = match tokio::time::timeout(
        std::time::Duration::from_secs(30),
        lair::connect_to_lair(&connection_url, &passphrase),
    )
    .await
    {
        Ok(Ok(c)) => c,
        Ok(Err(e)) => {
            log::error!("Lair connect failed: {}", e);
            fail_with_lair_cleanup!(e);
        }
        Err(_) => {
            log::error!(
                "Lair connect timed out after 30s (server running, socket present, no response)"
            );
            fail_with_lair_cleanup!(
                "Timed out connecting to the key store after 30 seconds. Please send us your log file - it now shows exactly where startup stopped.".to_string()
            );
        }
    };
    log::info!("Connected to lair-keystore");

    // 4. Generate conductor config.
    let _ = app_handle.emit(
        "conductor-status",
        ConductorStatus::Starting {
            message: "Starting Holochain conductor...".into(),
        },
    );
    let conductor_dir = data_dir.join("conductor");
    let config_path = match generate_conductor_config(&conductor_dir, &connection_url, ADMIN_WS_PORT) {
        Ok(p) => p,
        Err(e) => fail_with_lair_cleanup!(e),
    };

    // 5. Start conductor process.
    let mut conductor_child = match start_conductor_process(&config_path, &conductor_dir, &passphrase) {
        Ok(c) => c,
        Err(e) => fail_with_lair_cleanup!(e),
    };

    // 6. Wait for admin WebSocket.
    let _ = app_handle.emit(
        "conductor-status",
        ConductorStatus::Starting {
            message: "Waiting for conductor...".into(),
        },
    );
    if let Err(e) = wait_for_admin_ws(ADMIN_WS_PORT, 30, &mut conductor_child, &conductor_dir).await {
        let _ = conductor_child.kill();
        let _ = conductor_child.wait();
        fail_with_lair_cleanup!(e);
    }

    // 7. Attach app interface (port 0 = auto-assign).
    let _ = app_handle.emit(
        "conductor-status",
        ConductorStatus::Starting {
            message: "Setting up app interface...".into(),
        },
    );
    let admin_ws = holochain_client::AdminWebsocket::connect(
        format!("localhost:{}", ADMIN_WS_PORT),
        Some("your-own-ai".to_string()),
    )
    .await
    .map_err(|e| {
        let _ = conductor_child.kill();
        let _ = conductor_child.wait();
        format!("Failed to connect to admin WebSocket: {}", e)
    })?;

    let app_port = admin_ws
        .attach_app_interface(0, None, holochain_client::AllowedOrigins::Any, None)
        .await
        .map_err(|e| format!("Failed to attach app interface: {}", e))?;

    log::info!("App interface attached on port {}", app_port);

    // 8. Emit ready status.
    let _ = app_handle.emit(
        "conductor-status",
        ConductorStatus::Ready {
            admin_port: ADMIN_WS_PORT,
            app_port,
        },
    );
    log::info!(
        "Holochain conductor ready (admin: {}, app: {})",
        ADMIN_WS_PORT,
        app_port
    );

    Ok(StartupResult {
        handle: ConductorHandle {
            lair_child,
            conductor_child,
            admin_port: ADMIN_WS_PORT,
            app_port,
        },
        lair_client,
    })
}
