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

    // The config pins absolute paths. After the directory has moved they
    // must point here, or the key store binds (and we wait on) a socket in
    // a directory that no longer exists.
    if let Err(e) = repoint_config(lair_dir) {
        log::warn!("[lair] config not repointed: {e}");
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
/// Make `lair-keystore-config.yaml` point at the directory it now lives in.
///
/// Lair writes three absolute paths at init: `connectionUrl`
/// (`unix://<dir>/socket?k=...`), `pidFile` and `storeFile`. The URL's path
/// is PERCENT-ENCODED (`Application%20Support` on every Mac), so replacing
/// the old directory as plain text fixes the two file paths and silently
/// leaves the socket address behind. This parses the URL instead, compares
/// decoded paths, and lets the URL type do the encoding. Idempotent - safe
/// on every start, and it repairs a config left stale by an earlier move.
pub(crate) fn repoint_config(lair_dir: &Path) -> Result<bool, String> {
    let config_path = lair_dir.join("lair-keystore-config.yaml");
    if !config_path.exists() {
        return Ok(false);
    }
    let content = std::fs::read_to_string(&config_path)
        .map_err(|e| format!("lair config unreadable: {e}"))?;
    let mut changed = false;
    let mut out: Vec<String> = Vec::new();
    for line in content.lines() {
        let trimmed = line.trim_start();
        let indent = &line[..line.len() - trimmed.len()];
        if let Some(rest) = trimmed.strip_prefix("connectionUrl:") {
            let raw = rest.trim();
            if let Ok(mut url) = lair_keystore_api::dependencies::url::Url::parse(raw) {
                if url.scheme() == "unix" {
                    let decoded = percent_encoding::percent_decode_str(url.path())
                        .decode_utf8_lossy()
                        .to_string();
                    let expected = lair_dir.join("socket");
                    if Path::new(&decoded) != expected {
                        url.set_path(&expected.to_string_lossy());
                        out.push(format!("{indent}connectionUrl: {url}"));
                        changed = true;
                        continue;
                    }
                }
            }
        } else {
            let mut handled = false;
            for (key, file) in [("pidFile:", "pid_file"), ("storeFile:", "store_file")] {
                if let Some(rest) = trimmed.strip_prefix(key) {
                    let expected = lair_dir.join(file);
                    if Path::new(rest.trim()) != expected {
                        out.push(format!("{indent}{key} {}", expected.display()));
                        changed = true;
                        handled = true;
                    }
                    break;
                }
            }
            if handled {
                continue;
            }
        }
        out.push(line.to_string());
    }
    if !changed {
        return Ok(false);
    }
    let tmp = config_path.with_extension("yaml.tmp");
    std::fs::write(&tmp, out.join("\n") + "\n").map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, &config_path).map_err(|e| e.to_string())?;
    log::info!("[lair] key store config repointed to {:?}", lair_dir);
    Ok(true)
}

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

#[cfg(test)]
mod tests {
    use super::*;

    /// The config lair writes on a Mac: the socket address is a URL, so the
    /// space in "Application Support" is `%20` there and a plain space in
    /// the two file paths.
    fn mac_style_config(dir: &Path) -> String {
        let url_path = dir.to_string_lossy().replace(' ', "%20");
        format!(
            "---\nconnectionUrl: unix://{url_path}/socket?k=AbC-123_x\npidFile: {0}/pid_file\nstoreFile: {0}/store_file\nsignatureFallback: none\n",
            dir.display()
        )
    }

    #[test]
    fn a_moved_key_store_is_repointed_when_its_path_has_a_space() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("Application Support").join("app").join("lair");
        let new = tmp.path().join("Application Support").join("app").join("profiles").join("local").join("lair");
        std::fs::create_dir_all(&new).unwrap();
        // Moved intact: the config still names the old directory everywhere.
        std::fs::write(new.join("lair-keystore-config.yaml"), mac_style_config(&old)).unwrap();

        assert_eq!(repoint_config(&new), Ok(true));
        let url = read_connection_url(&new.join("lair-keystore-config.yaml")).unwrap();
        let parsed = lair_keystore_api::dependencies::url::Url::parse(&url).unwrap();
        let decoded = percent_encoding::percent_decode_str(parsed.path()).decode_utf8_lossy().to_string();
        assert_eq!(Path::new(&decoded), new.join("socket"), "the socket address follows the move");
        assert!(url.contains("Application%20Support"), "and stays a valid encoded URL: {url}");
        assert!(url.ends_with("?k=AbC-123_x"), "the key in the address is kept: {url}");
        let after = std::fs::read_to_string(new.join("lair-keystore-config.yaml")).unwrap();
        assert!(after.contains(&format!("pidFile: {}", new.join("pid_file").display())));
        assert!(after.contains(&format!("storeFile: {}", new.join("store_file").display())));
        assert!(after.contains("signatureFallback: none"), "other settings untouched");
        // Idempotent: a second start changes nothing.
        assert_eq!(repoint_config(&new), Ok(false));
    }

    #[test]
    fn a_config_left_half_fixed_by_a_text_replace_is_healed() {
        // What 0.7.3-beta.6 left behind on a Mac: the two file paths moved,
        // the encoded socket address did not.
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("Application Support").join("app").join("lair");
        let new = tmp.path().join("Application Support").join("app").join("profiles").join("local").join("lair");
        std::fs::create_dir_all(&new).unwrap();
        let half = mac_style_config(&old).replace(
            &old.to_string_lossy().to_string(),
            &new.to_string_lossy().to_string(),
        );
        assert!(half.contains(&format!("pidFile: {}", new.join("pid_file").display())));
        assert!(half.contains("app/lair/socket"), "precondition: the socket address is still the old one");
        std::fs::write(new.join("lair-keystore-config.yaml"), half).unwrap();

        assert_eq!(repoint_config(&new), Ok(true));
        let url = read_connection_url(&new.join("lair-keystore-config.yaml")).unwrap();
        assert!(url.contains("profiles/local/lair/socket"), "{url}");
    }

    #[test]
    fn a_config_already_in_place_is_left_alone() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("Application Support").join("lair");
        std::fs::create_dir_all(&dir).unwrap();
        let original = mac_style_config(&dir);
        std::fs::write(dir.join("lair-keystore-config.yaml"), &original).unwrap();
        assert_eq!(repoint_config(&dir), Ok(false));
        assert_eq!(std::fs::read_to_string(dir.join("lair-keystore-config.yaml")).unwrap(), original);
    }

    /// The real key store binary, the real failure: init under a directory
    /// with a space, move it intact, start it. Without the repoint its
    /// socket never appears where we wait (field 2026-09-18, every Mac);
    /// with it, it does. `cargo test --lib -- --ignored live_lair`
    #[test]
    #[ignore]
    fn live_lair_survives_a_move_under_a_path_with_a_space() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let bin = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("binaries")
            .join("yourowai-lair-keystore-x86_64-unknown-linux-gnu");
        assert!(bin.exists(), "no key store binary at {bin:?}");
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("Application Support").join("app").join("lair");
        let new = tmp.path().join("Application Support").join("app").join("profiles").join("local").join("lair");
        std::fs::create_dir_all(&old).unwrap();
        let run = |dir: &Path, arg: &str| {
            let mut c = Command::new(&bin).arg(arg).arg("--piped").current_dir(dir)
                .stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::null()).spawn().unwrap();
            c.stdin.take().unwrap().write_all(b"test-passphrase\n").unwrap();
            c
        };
        assert!(run(&old, "init").wait().unwrap().success(), "init");
        let before = std::fs::read_to_string(old.join("lair-keystore-config.yaml")).unwrap();
        assert!(before.contains("Application%20Support"), "lair encodes the space in its address: {before}");
        std::fs::create_dir_all(new.parent().unwrap()).unwrap();
        std::fs::rename(&old, &new).unwrap();
        let socket_appears = |dir: &Path| {
            let mut server = run(dir, "server");
            let mut seen = false;
            for _ in 0..40 {
                if dir.join("socket").exists() { seen = true; break; }
                std::thread::sleep(std::time::Duration::from_millis(250));
            }
            let _ = server.kill();
            let _ = server.wait();
            seen
        };
        // What beta.6 did: plain text replace of the old directory.
        let half = before.replace(&old.to_string_lossy().to_string(), &new.to_string_lossy().to_string());
        std::fs::write(new.join("lair-keystore-config.yaml"), half).unwrap();
        assert!(!socket_appears(&new), "reproduction: the text replace leaves the socket address behind");
        // The fix.
        assert_eq!(repoint_config(&new), Ok(true));
        assert!(socket_appears(&new), "after the repoint the key store comes up in its new directory");
    }
}
