//! Holochain conductor lifecycle management for Your Own AI.
//!
//! Starts lair-keystore and holochain as child processes, waits for readiness.
//! Adapted from ProofPoll's conductor.rs — simplified for local-only use
//! (no DHT networking, no migration clients).

use crate::lair;
use crate::process_ext::{SidecarChild, SidecarCommand};
use lair_keystore_api::prelude::LairClient;
use std::path::{Path, PathBuf};
use tauri::Emitter;

/// Admin WebSocket port for the local Holochain conductor.
/// Different from Flowsta Vault (4455) and ProofPoll (4466).
pub const ADMIN_WS_PORT: u16 = 4477;

/// Handle to a running conductor + lair-keystore pair.
pub struct ConductorHandle {
    pub lair_child: SidecarChild,
    pub conductor_child: SidecarChild,
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

/// Where this conductor meets the person's OTHER devices (build-docs
/// MULTI_DEVICE.md). The network is private by construction - the seed is
/// per identity (or per install) and every record is ciphertext under the
/// person's data key before it reaches the DNA - so a reachable peer is
/// only ever one of the person's own devices, and the rendezvous learns
/// agent keys, a space hash and an address, never content or an identity.
///
/// Bootstrap + signal URL and the per-app auth material are baked at build
/// time (`FLOWSTA_BOOTSTRAP_URL`, `FLOWSTA_SIGNAL_URL`, `FLOWSTA_AUTH_MATERIAL`
/// - the Vault's names) or set at run time for a dev box. Without auth
/// material the conductor keeps the localhost black hole it always had:
/// one device, no peers, nothing to reach - the bootstrap refuses
/// unauthenticated clients anyway.
struct Rendezvous {
    bootstrap_url: String,
    signal_url: String,
    auth_material: Option<String>,
}

impl Rendezvous {
    fn env(k: &str) -> Option<String> {
        std::env::var(k).ok().filter(|v| !v.is_empty())
    }

    /// Flowsta's own rendezvous, usable only with this app's auth material.
    fn primary() -> Option<Self> {
        let auth_material = Self::env("FLOWSTA_AUTH_MATERIAL")
            .or_else(|| option_env!("FLOWSTA_AUTH_MATERIAL").filter(|m| !m.is_empty()).map(String::from))?;
        let bootstrap_url = Self::env("FLOWSTA_BOOTSTRAP_URL")
            .or_else(|| option_env!("FLOWSTA_BOOTSTRAP_URL").map(String::from))
            .unwrap_or_else(|| "https://bootstrap.flowsta.com".to_string());
        let signal_url = Self::env("FLOWSTA_SIGNAL_URL")
            .or_else(|| option_env!("FLOWSTA_SIGNAL_URL").map(String::from))
            .unwrap_or_else(|| bootstrap_url.replacen("https://", "wss://", 1));
        Some(Self { bootstrap_url, signal_url, auth_material: Some(auth_material) })
    }

    /// Open rendezvous for when Flowsta's is dark (community nodes): a
    /// comma-separated list, https only (the conductor refuses a plaintext
    /// relay), no auth material by design - the Vault's rule.
    fn fallbacks() -> Vec<Self> {
        let list = Self::env("FLOWSTA_BOOTSTRAP_FALLBACKS")
            .or_else(|| option_env!("FLOWSTA_BOOTSTRAP_FALLBACKS").map(String::from))
            .unwrap_or_default();
        parse_fallbacks(&list)
    }

    /// The rendezvous for this conductor session: the primary when it
    /// answers (or when there is nothing else to try); else the first live
    /// fallback; with everything dark, the primary anyway - the conductor
    /// works alone and reconnects when anything returns. `None` = no
    /// material and no fallbacks: the localhost black hole, one device.
    async fn resolve() -> Option<Self> {
        let primary = Self::primary();
        let fallbacks = Self::fallbacks();
        if fallbacks.is_empty() {
            return primary;
        }
        if let Some(p) = &primary {
            if bootstrap_alive(&p.bootstrap_url).await {
                return primary;
            }
            log::warn!("[conductor] rendezvous {} unreachable - trying {} fallback(s)", p.bootstrap_url, fallbacks.len());
        }
        for f in &fallbacks {
            if bootstrap_alive(&f.bootstrap_url).await {
                log::info!("[conductor] using open rendezvous {}", f.bootstrap_url);
                return Some(f.clone());
            }
        }
        log::warn!("[conductor] every rendezvous is dark - starting alone, reconnecting when one returns");
        primary.or_else(|| fallbacks.into_iter().next())
    }

    /// The `network:` block. The relay URL is the bootstrap host with a
    /// trailing dot (an absolute DNS name, the form the relay wants).
    fn network_block(this: Option<&Self>) -> String {
        match this {
            Some(r) => {
                let auth = match &r.auth_material {
                    Some(m) => format!("  base64_auth_material_bootstrap: \"{m}\"\n  base64_auth_material_relay: \"{m}\"\n"),
                    None => String::new(),
                };
                format!(
                    "network:\n  bootstrap_url: {b}\n  signal_url: {s}\n  relay_url: {r}\n{auth}",
                    b = r.bootstrap_url,
                    s = r.signal_url,
                    r = relay_url_for(&r.bootstrap_url),
                )
            }
            None => "network:\n  bootstrap_url: https://localhost/\n  signal_url: wss://localhost/\n  relay_url: https://localhost/\n".to_string(),
        }
    }
}

impl Clone for Rendezvous {
    fn clone(&self) -> Self {
        Self { bootstrap_url: self.bootstrap_url.clone(), signal_url: self.signal_url.clone(), auth_material: self.auth_material.clone() }
    }
}

fn parse_fallbacks(list: &str) -> Vec<Rendezvous> {
    list.split(',')
        .map(str::trim)
        .filter(|u| !u.is_empty())
        .filter(|u| {
            let ok = u.starts_with("https://");
            if !ok {
                log::error!("[conductor] ignoring non-https fallback {} - the conductor forbids plaintext relays", u);
            }
            ok
        })
        .map(|u| {
            let bootstrap_url = u.trim_end_matches('/').to_string();
            let host = bootstrap_url.trim_start_matches("https://").to_string();
            Rendezvous { bootstrap_url, signal_url: format!("wss://{}", host), auth_material: None }
        })
        .collect()
}

/// Is a bootstrap server answering? kitsune2-bootstrap-srv serves
/// GET /health (200, no auth); community nodes run the same binary.
async fn bootstrap_alive(url: &str) -> bool {
    let health = format!("{}/health", url.trim_end_matches('/'));
    let Ok(client) = reqwest::Client::builder().timeout(std::time::Duration::from_millis(2500)).build() else {
        return false;
    };
    matches!(client.get(&health).send().await, Ok(r) if r.status().is_success())
}

/// `https://host[:port]/...` → `https://host.[:port]/` (the Vault's rule).
fn relay_url_for(bootstrap_url: &str) -> String {
    let trimmed = bootstrap_url.trim_end_matches('/');
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return format!("{}./", trimmed.trim_end_matches('.'));
    };
    let (host, port) = match rest.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => (h, Some(p)),
        _ => (rest, None),
    };
    let host = host.trim_end_matches('.');
    match port {
        Some(p) => format!("{}://{}.:{}/", scheme, host, p),
        None => format!("{}://{}./", scheme, host),
    }
}

fn generate_conductor_config(
    conductor_dir: &Path,
    lair_connection_url: &str,
    admin_port: u16,
    rendezvous: Option<&Rendezvous>,
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
    match rendezvous {
        Some(r) => log::info!("[conductor] rendezvous {} (the person's own devices can meet)", r.bootstrap_url),
        None => log::info!("[conductor] no rendezvous - this conductor stays alone"),
    }
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
{network}db_sync_strategy: Resilient
"#,
        data_root = data_root,
        admin_port = admin_port,
        lair_url = lair_url,
        network = Rendezvous::network_block(rendezvous),
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
) -> Result<SidecarChild, String> {
    log::info!("Starting holochain conductor...");

    let stdout_path = conductor_dir.join("holochain-stdout.log");
    let stderr_path = conductor_dir.join("holochain-stderr.log");

    let stdout_file = std::fs::File::create(&stdout_path)
        .map_err(|e| format!("Failed to create conductor stdout log: {}", e))?;
    let stderr_file = std::fs::File::create(&stderr_path)
        .map_err(|e| format!("Failed to create conductor stderr log: {}", e))?;

    let holochain_bin = crate::resolve_sidecar_bin("yourowai-holochain");
    log::info!("Using holochain binary: {:?}", holochain_bin);

    let mut child = SidecarCommand::new(&holochain_bin)
        .arg("-c")
        .arg(config_path)
        .arg("--piped")
        .stdout(stdout_file)
        .stderr(stderr_file)
        .spawn()
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
    conductor_child: &mut SidecarChild,
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
    // Log the pipe address only - the URL's query string carries the
    // connection token, and users share these logs with support.
    log::info!(
        "Connecting to lair-keystore at {}",
        connection_url.split('?').next().unwrap_or("<unparseable>")
    );
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
    let rendezvous = Rendezvous::resolve().await;
    let config_path = match generate_conductor_config(&conductor_dir, &connection_url, ADMIN_WS_PORT, rendezvous.as_ref()) {
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

#[cfg(test)]
mod rendezvous_tests {
    #[test]
    fn fallbacks_are_https_only_open_and_in_order() {
        let f = super::parse_fallbacks(" https://node1.example.com/, http://plain.example.com, https://node2.example.com:8443 ,");
        assert_eq!(f.len(), 2, "the plaintext one is dropped");
        assert_eq!(f[0].bootstrap_url, "https://node1.example.com");
        assert_eq!(f[0].signal_url, "wss://node1.example.com");
        assert!(f[0].auth_material.is_none(), "fallbacks are open by design");
        assert_eq!(f[1].bootstrap_url, "https://node2.example.com:8443");
        assert_eq!(super::relay_url_for(&f[1].bootstrap_url), "https://node2.example.com.:8443/");
        assert!(super::parse_fallbacks("").is_empty());
    }

    #[test]
    fn relay_url_is_the_bootstrap_host_as_an_absolute_name() {
        assert_eq!(super::relay_url_for("https://bootstrap.flowsta.com"), "https://bootstrap.flowsta.com./");
        assert_eq!(super::relay_url_for("https://bootstrap-staging.flowsta.com/"), "https://bootstrap-staging.flowsta.com./");
        assert_eq!(super::relay_url_for("http://localhost:8787"), "http://localhost.:8787/");
    }
}
