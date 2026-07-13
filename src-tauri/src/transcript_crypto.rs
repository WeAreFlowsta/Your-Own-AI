//! Transcript encryption + recovery material (Phase A).
//!
//! Design:
//!
//! - **User-level data key**, not per-agent: a random 256-bit key
//!   generated at first launch, used to encrypt ALL transcript content
//!   (XSalsa20-Poly1305 secretbox). Per-agent encrypt-to-self would
//!   strand data the moment a second device or archive node exists.
//! - **Per-user network seed**, generated alongside the key and applied
//!   at install time, so each user's transcript DHT is their own
//!   network (the baked-in dna.yaml seed is a template, always
//!   overridden).
//! - **Standalone-first**: no account involved. Vault integration later
//!   adds key *escrow* (a wrapped copy of this key in the user's Vault
//!   backup) — never becomes the key source.
//! - Both values live in `transcript-recovery.json` in the app data dir
//!   (0600). Losing this file before exporting/escrowing it means the
//!   ciphertext — including on the user's own future nodes — is
//!   unrecoverable. The export command exists for exactly that reason.

use crypto_secretbox::aead::{Aead, KeyInit};
use crypto_secretbox::XSalsa20Poly1305;
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::path::Path;

const RECOVERY_FILE: &str = "transcript-recovery.json";

/// The user's transcript recovery material. Treat like a key: export it,
/// escrow it (Vault), never log it.
#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
pub struct RecoveryMaterial {
    /// Per-user Holochain network seed for the transcript DNA.
    pub network_seed: String,
    /// Hex-encoded 32-byte XSalsa20-Poly1305 key for transcript content.
    pub data_key_hex: String,
    /// Format version for future migrations of this file itself.
    pub version: u32,
}

impl RecoveryMaterial {
    pub fn data_key(&self) -> Result<[u8; 32], String> {
        let bytes = hex::decode(&self.data_key_hex)
            .map_err(|e| format!("Invalid data key hex: {}", e))?;
        bytes
            .try_into()
            .map_err(|_| "Data key must be 32 bytes".to_string())
    }
}

/// Read the recovery material WITHOUT generating it - `None` when the file
/// doesn't exist yet. For callers (escrow, restore) that must never mint a
/// fresh key as a side effect of a read.
pub fn load_recovery_material(app_data_dir: &Path) -> Result<Option<RecoveryMaterial>, String> {
    let path = app_data_dir.join(RECOVERY_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let raw = std::fs::read_to_string(&path)
        .map_err(|e| format!("Failed to read recovery material: {}", e))?;
    serde_json::from_str(&raw)
        .map(Some)
        .map_err(|e| format!("Failed to parse recovery material: {}", e))
}

/// Replace the recovery material on disk, preserving the old file as a
/// timestamped sibling (a key must never be silently destroyed - old
/// ciphertext is unrecoverable without it).
pub fn replace_recovery_material(
    app_data_dir: &Path,
    new_material: &RecoveryMaterial,
) -> Result<(), String> {
    let path = app_data_dir.join(RECOVERY_FILE);
    if path.exists() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let backup = app_data_dir.join(format!("transcript-recovery.replaced-{}.json", ts));
        std::fs::rename(&path, &backup)
            .map_err(|e| format!("Failed to preserve the old recovery material: {}", e))?;
    }
    let json = serde_json::to_string_pretty(new_material)
        .map_err(|e| format!("Failed to serialize recovery material: {}", e))?;
    std::fs::write(&path, &json)
        .map_err(|e| format!("Failed to write recovery material: {}", e))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// Load the recovery material, generating it on first launch.
pub fn ensure_recovery_material(app_data_dir: &Path) -> Result<RecoveryMaterial, String> {
    let path = app_data_dir.join(RECOVERY_FILE);

    if path.exists() {
        let raw = std::fs::read_to_string(&path)
            .map_err(|e| format!("Failed to read recovery material: {}", e))?;
        return serde_json::from_str(&raw)
            .map_err(|e| format!("Failed to parse recovery material: {}", e));
    }

    // First launch: generate key + seed.
    let mut key = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut key);
    let mut seed_bytes = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut seed_bytes);

    let material = RecoveryMaterial {
        network_seed: format!("yoai-user-{}", hex::encode(seed_bytes)),
        data_key_hex: hex::encode(key),
        version: 1,
    };

    std::fs::create_dir_all(app_data_dir)
        .map_err(|e| format!("Failed to create app data dir: {}", e))?;
    let json = serde_json::to_string_pretty(&material)
        .map_err(|e| format!("Failed to serialize recovery material: {}", e))?;
    std::fs::write(&path, &json)
        .map_err(|e| format!("Failed to write recovery material: {}", e))?;

    // Owner-only permissions on Unix.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }

    log::info!("Generated new transcript recovery material (key + per-user network seed)");
    Ok(material)
}

/// Encrypt plaintext with the user data key. Returns (nonce, ciphertext).
pub fn encrypt(key: &[u8; 32], plaintext: &[u8]) -> Result<([u8; 24], Vec<u8>), String> {
    let cipher = XSalsa20Poly1305::new(key.into());
    let mut nonce = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(&nonce.into(), plaintext)
        .map_err(|e| format!("Encryption failed: {}", e))?;
    Ok((nonce, ct))
}

/// Decrypt ciphertext with the user data key.
pub fn decrypt(key: &[u8; 32], nonce: &[u8], ciphertext: &[u8]) -> Result<Vec<u8>, String> {
    if nonce.len() != 24 {
        return Err("Nonce must be 24 bytes".to_string());
    }
    let cipher = XSalsa20Poly1305::new(key.into());
    let nonce_arr: [u8; 24] = nonce.try_into().map_err(|_| "Bad nonce".to_string())?;
    cipher
        .decrypt(&nonce_arr.into(), ciphertext)
        .map_err(|_| "Decryption failed (wrong key or corrupted data)".to_string())
}
