//! Apple Silicon MLX engine (preview) - SwiftLM.
//!
//! The optional MLX engine follows the CUDA pattern (engine.rs): a
//! release-pinned prebuilt server, downloaded on demand into a versioned
//! dir, removable, never bundled. It serves mlx-community safetensors
//! artifacts over the same OpenAI-style chat API our client already
//! speaks; llama.cpp Metal remains the universal default and the fallback
//! for every model without an MLX artifact.
//!
//! v1 scope (MLX_IMPLEMENTATION_PLAN.md): CHAT turns only - agent and
//! project sessions, embeddings, memory extraction and vision stay on
//! llama.cpp. Everything here is inert off macOS/aarch64.

use serde::Serialize;
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

/// The SwiftLM release this app is pinned to. Bump deliberately, with the
/// sha256 below, after a spike pass on real hardware - never float.
pub const SWIFTLM_TAG: &str = "b699";

/// sha256 of the release tarball, measured 2026-08-24 at pin time. The
/// download is refused if it does not match - an engine binary is the one
/// artifact we never run unverified.
const SWIFTLM_SHA256: &str = "73c845aca312093cb6bfd3e4854006929520e0a94daf61c0df0ad6c3416fe09e";

const SWIFTLM_REPO: &str = "SharpAI/SwiftLM";

/// The server binary's port. Distinct from the llama.cpp chat port - only
/// one of the two serves at a time (same single-slot model), but a fixed,
/// different port keeps logs and health probes unambiguous.
pub const MLX_CHAT_PORT: u16 = 8090;

pub fn supported() -> bool {
    cfg!(all(target_os = "macos", target_arch = "aarch64"))
}

fn download_url() -> String {
    format!(
        "https://github.com/{}/releases/download/{}/SwiftLM-{}-macos-arm64.tar.gz",
        SWIFTLM_REPO, SWIFTLM_TAG, SWIFTLM_TAG
    )
}

fn tarball_name() -> String {
    format!("SwiftLM-{}-macos-arm64.tar.gz", SWIFTLM_TAG)
}

/// Versioned engine dir: `<app-data>/engines/mlx-<tag>/`.
fn engine_dir(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_data_dir()
        .map_err(|e| format!("No app data dir: {}", e))?
        .join("engines")
        .join(format!("mlx-{}", SWIFTLM_TAG)))
}

/// The installed SwiftLM binary at the pinned tag, if present.
pub fn swiftlm_binary(app: &AppHandle) -> Option<PathBuf> {
    let bin = engine_dir(app).ok()?.join("SwiftLM");
    bin.exists().then_some(bin)
}

/// An older mlx-<other-tag> install lingering on disk (update available).
fn other_mlx_version_installed(app: &AppHandle) -> bool {
    let Ok(dir) = engine_dir(app) else {
        return false;
    };
    let Some(parent) = dir.parent() else {
        return false;
    };
    let current = format!("mlx-{}", SWIFTLM_TAG);
    std::fs::read_dir(parent)
        .ok()
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .any(|n| n.starts_with("mlx-") && n != current && !n.ends_with(".partial"))
        })
        .unwrap_or(false)
}

#[derive(Serialize)]
pub struct MlxEngineStatus {
    /// This platform can run the MLX engine (Apple Silicon macOS).
    pub supported: bool,
    /// SwiftLM is installed at the app's pinned tag.
    pub installed: bool,
    /// An older tag is on disk (update available).
    pub stale_version_installed: bool,
    pub tag: String,
    pub download_url: String,
    /// Tarball size, for the install button's honest label.
    pub download_mb: u32,
}

#[tauri::command]
pub async fn mlx_engine_status(app: AppHandle) -> Result<MlxEngineStatus, String> {
    Ok(MlxEngineStatus {
        supported: supported(),
        installed: swiftlm_binary(&app).is_some(),
        stale_version_installed: other_mlx_version_installed(&app),
        tag: SWIFTLM_TAG.to_string(),
        download_url: download_url(),
        download_mb: 50,
    })
}

/// Download + verify + install the MLX engine. Reuses the model
/// downloader's transport (resume, If-Range, disk gate, progress events
/// keyed by the tarball name), verifies the pinned sha256, then extracts
/// atomically (a `.partial` dir renamed into place only when complete).
#[tauri::command]
pub async fn download_mlx_engine(app: AppHandle, url: Option<String>) -> Result<(), String> {
    if !supported() {
        return Err("unsupported_platform".to_string());
    }
    let url = url.unwrap_or_else(download_url);
    let name = tarball_name();
    crate::llm::download_model(app.clone(), url, name.clone()).await?;
    let result = verify_and_install(&app, &name);
    // The tarball is an installer artifact, not a model - clean it up
    // either way.
    if let Ok(models) = crate::llm::get_models_dir(&app) {
        let _ = std::fs::remove_file(models.join(&name));
    }
    result?;
    remove_stale_mlx_versions(&app);
    log::info!("[Engine] MLX engine installed (SwiftLM {})", SWIFTLM_TAG);
    Ok(())
}

fn verify_and_install(app: &AppHandle, name: &str) -> Result<(), String> {
    use sha2::{Digest, Sha256};
    let tar_path = crate::llm::get_models_dir(app)?.join(name);

    // The one artifact we never run unverified.
    let mut file = std::fs::File::open(&tar_path)
        .map_err(|e| format!("engine tarball missing after download: {}", e))?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| format!("hash read failed: {}", e))?;
    let got = format!("{:x}", hasher.finalize());
    if got != SWIFTLM_SHA256 {
        return Err(format!(
            "Engine download doesn't match the release this app is pinned to (checksum {}… vs expected {}…). Try again; if it repeats, wait for an app update.",
            &got[..12],
            &SWIFTLM_SHA256[..12]
        ));
    }

    let final_dir = engine_dir(app)?;
    let parent = final_dir.parent().ok_or("engine dir has no parent")?;
    std::fs::create_dir_all(parent).map_err(|e| format!("cannot create engines dir: {}", e))?;
    let partial = parent.join(format!("mlx-{}.partial", SWIFTLM_TAG));
    let _ = std::fs::remove_dir_all(&partial);
    std::fs::create_dir_all(&partial).map_err(|e| format!("cannot create engine dir: {}", e))?;

    // tar ships on every macOS this engine supports (same pattern as the
    // Build agent install). --strip-components=0 with ./ entries is fine.
    let status = std::process::Command::new("tar")
        .arg("xzf")
        .arg(&tar_path)
        .arg("-C")
        .arg(&partial)
        .status()
        .map_err(|e| format!("tar failed to start: {}", e))?;
    if !status.success() {
        return Err(format!("tar exited with {}", status));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let bin = partial.join("SwiftLM");
        std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o755))
            .map_err(|e| format!("cannot mark engine executable: {}", e))?;
    }

    if !partial.join("SwiftLM").exists() {
        return Err("engine tarball did not contain SwiftLM".into());
    }

    let _ = std::fs::remove_dir_all(&final_dir);
    std::fs::rename(&partial, &final_dir)
        .map_err(|e| format!("could not finalize engine install: {}", e))?;
    Ok(())
}

fn remove_stale_mlx_versions(app: &AppHandle) {
    let Ok(dir) = engine_dir(app) else { return };
    let Some(parent) = dir.parent() else { return };
    let current = format!("mlx-{}", SWIFTLM_TAG);
    if let Ok(rd) = std::fs::read_dir(parent) {
        for e in rd.flatten() {
            let name = e.file_name();
            let Some(n) = name.to_str() else { continue };
            if (n.starts_with("mlx-") && n != current) || n == format!("{current}.partial") {
                let _ = std::fs::remove_dir_all(e.path());
            }
        }
    }
}

/// Remove the installed MLX engine (and any stale versions). Models keep
/// their artifacts; they simply run on llama.cpp Metal again.
#[tauri::command]
pub async fn remove_mlx_engine(app: AppHandle) -> Result<(), String> {
    let dir = engine_dir(&app)?;
    let _ = std::fs::remove_dir_all(&dir);
    remove_stale_mlx_versions(&app);
    log::info!("[Engine] MLX engine removed");
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn pinned_url_is_versioned_and_not_floating() {
        let url = super::download_url();
        assert!(url.contains(super::SWIFTLM_TAG));
        assert!(!url.contains("latest"), "engine downloads must be release-pinned");
        assert_eq!(super::SWIFTLM_SHA256.len(), 64);
    }
}
