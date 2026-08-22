//! Lair keystore management.
//!
//! Lair is Holochain's key management daemon. It stores agent Ed25519
//! signing keys and handles cryptographic operations. This module starts
//! lair-keystore as a child process and connects to it via Unix socket.
//!
//! Adapted from ProofPoll's lair.rs with sidecar binary resolution
//! and URL-encoded path handling from Flowsta Vault.

use lair_keystore_api::prelude::*;
use percent_encoding::percent_decode_str;
use crate::process_ext::{SidecarChild, SidecarCommand};
use std::io::Write;
use std::path::Path;
use std::sync::Arc;

/// Start a lair-keystore process.
///
/// On first run (no config file), initializes the keystore.
/// Then starts the server process.
/// Returns the child process handle and the connection URL.
pub fn start_lair_process(
    lair_dir: &Path,
    passphrase: &str,
) -> Result<(SidecarChild, String), String> {
    std::fs::create_dir_all(lair_dir)
        .map_err(|e| format!("Failed to create lair directory: {}", e))?;

    let config_path = lair_dir.join("lair-keystore-config.yaml");
    let is_first_run = !config_path.exists();

    let lair_bin = crate::resolve_sidecar_bin("yourowai-lair-keystore");
    log::info!("Using lair-keystore binary: {:?}", lair_bin);

    if is_first_run {
        log::info!("First run: initializing lair-keystore...");
        let mut child = SidecarCommand::new(&lair_bin)
            .arg("init")
            .arg("--piped")
            .current_dir(lair_dir)
            .spawn()
            .map_err(|e| format!("Failed to spawn lair-keystore init: {}", e))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(format!("{}\n", passphrase).as_bytes())
                .map_err(|e| format!("Failed to write passphrase to lair init: {}", e))?;
        }

        let status = child
            .wait()
            .map_err(|e| format!("Failed to wait for lair init: {}", e))?;
        if !status.success() {
            return Err(format!("lair-keystore init failed with status: {}", status));
        }
        log::info!("Lair-keystore initialized successfully");
    }

    // Read connection URL from config file.
    let connection_url = read_connection_url(&config_path)?;

    // Clean up stale socket file from a previous run.
    let socket_path = lair_dir.join("socket");
    if socket_path.exists() {
        log::info!("Removing stale lair socket: {:?}", socket_path);
        let _ = std::fs::remove_file(&socket_path);
    }

    // Start the lair server.
    log::info!("Starting lair-keystore server...");
    // Lair's own output goes to log files beside its store (it was a pipe
    // nobody read before - a chatty lair could have stalled on a full pipe).
    let mut cmd = SidecarCommand::new(&lair_bin)
        .arg("server")
        .arg("--piped")
        .current_dir(lair_dir);
    if let Ok(f) = std::fs::File::create(lair_dir.join("lair-stdout.log")) {
        cmd = cmd.stdout(f);
    }
    if let Ok(f) = std::fs::File::create(lair_dir.join("lair-stderr.log")) {
        cmd = cmd.stderr(f);
    }
    let mut child = cmd
        .spawn()
        .map_err(|e| format!("Failed to spawn lair-keystore server: {}", e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(format!("{}\n", passphrase).as_bytes())
            .map_err(|e| format!("Failed to write passphrase to lair server: {}", e))?;
    }

    log::info!("Lair-keystore server started (pid {})", child.id());
    Ok((child, connection_url))
}

/// Read the connection URL from lair's config file.
fn read_connection_url(config_path: &Path) -> Result<String, String> {
    let content = std::fs::read_to_string(config_path)
        .map_err(|e| format!("Failed to read lair config at {:?}: {}", config_path, e))?;

    for line in content.lines() {
        let line = line.trim();
        if line.starts_with("connectionUrl:") {
            let url = line
                .strip_prefix("connectionUrl:")
                .unwrap()
                .trim()
                .to_string();
            return Ok(url);
        }
    }

    Err(format!(
        "No connectionUrl found in lair config: {:?}",
        config_path
    ))
}

/// Connect to a running lair-keystore via its connection URL.
pub async fn connect_to_lair(
    connection_url: &str,
    passphrase: &str,
) -> Result<LairClient, String> {
    let url = lair_keystore_api::dependencies::url::Url::parse(connection_url)
        .map_err(|e| format!("Invalid lair connection URL: {}", e))?;
    let passphrase_array: SharedLockedArray = Arc::new(std::sync::Mutex::new(
        lair_keystore_api::dependencies::sodoken::LockedArray::from(
            passphrase.as_bytes().to_vec(),
        ),
    ));
    lair_keystore_api::ipc_keystore_connect(url, passphrase_array)
        .await
        .map_err(|e| format!("Failed to connect to lair: {}", e))
}

/// Wait for the lair unix socket to be ready.
/// Handles URL-encoded paths (e.g. %20 for spaces in "Application Support").
pub async fn wait_for_lair_socket(connection_url: &str, timeout_secs: u64) -> Result<(), String> {
    let url = lair_keystore_api::dependencies::url::Url::parse(connection_url)
        .map_err(|e| format!("Invalid connection URL: {}", e))?;
    // Decode percent-encoded paths (e.g. %20 for spaces on macOS "Application Support")
    let decoded_path = percent_decode_str(url.path()).decode_utf8_lossy();
    let socket_path = std::path::PathBuf::from(decoded_path.as_ref());

    let deadline =
        std::time::Instant::now() + std::time::Duration::from_secs(timeout_secs);

    while std::time::Instant::now() < deadline {
        if socket_path.exists() {
            log::info!("Lair socket ready at {:?}", socket_path);
            return Ok(());
        }
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }

    Err(format!(
        "Lair socket not ready after {}s: {:?}",
        timeout_secs, socket_path
    ))
}
