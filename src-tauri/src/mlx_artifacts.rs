//! MLX model artifacts: directory-shaped models (safetensors + config +
//! tokenizer) from mlx-community, downloaded file by file into
//! `models_dir/<artifact-dir>/`.
//!
//! Every file rides `llm::download_model`, so the whole guard set applies
//! per file for free: .part resume, If-Range revision guard, disk gate,
//! progress events, reattach. The catalog pins a REVISION; the manifest is
//! fetched from that revision, so the file list and every URL are immutable.
//!
//! Completeness is manifest-based (the directory analog of the .part
//! rename): `manifest.json` is written only after every file is present
//! and its size verified. A dir without a complete manifest is an
//! in-flight or damaged artifact - never offered to the loader.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Emitter};

/// Directory prefix for MLX artifacts under the models folder. Keeps them
/// unmistakable in scans and greppable in support logs.
pub const MLX_DIR_PREFIX: &str = "mlx-";

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MlxManifestFile {
    pub name: String,
    pub size: u64,
    /// LFS sha256 when Hugging Face reports one (big files); small config
    /// files come without and are size-checked only.
    pub sha256: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct MlxManifest {
    pub repo: String,
    pub revision: String,
    pub files: Vec<MlxManifestFile>,
    /// Present-and-true only when every file landed and verified.
    pub complete: bool,
}

/// `models_dir/mlx-<repo-basename>` - e.g. repo
/// "mlx-community/Qwen3.6-35B-A3B-4bit" -> "mlx-Qwen3.6-35B-A3B-4bit".
pub fn artifact_dir_name(repo: &str) -> String {
    let base = repo.rsplit('/').next().unwrap_or(repo);
    format!("{}{}", MLX_DIR_PREFIX, base)
}

fn artifact_dir(app: &AppHandle, repo: &str) -> Result<PathBuf, String> {
    Ok(crate::llm::get_models_dir(app)?.join(artifact_dir_name(repo)))
}

fn manifest_path(dir: &std::path::Path) -> PathBuf {
    dir.join("manifest.json")
}

/// Read a dir's manifest; None = no manifest (in-flight, foreign, or
/// pre-manifest dir).
pub fn read_manifest(dir: &std::path::Path) -> Option<MlxManifest> {
    let bytes = std::fs::read(manifest_path(dir)).ok()?;
    serde_json::from_slice(&bytes).ok()
}

/// Why a present artifact directory does not count as complete.
fn incomplete_reason(dir: &std::path::Path) -> String {
    let path = manifest_path(dir);
    let bytes = match std::fs::read(&path) {
        Ok(b) => b,
        Err(e) => return format!("no manifest ({e})"),
    };
    let m: MlxManifest = match serde_json::from_slice(&bytes) {
        Ok(m) => m,
        Err(e) => return format!("manifest unreadable ({e})"),
    };
    if !m.complete {
        return "manifest not marked complete (download did not finish)".to_string();
    }
    for f in &m.files {
        match std::fs::metadata(dir.join(&f.name)) {
            Ok(md) if md.len() == f.size => {}
            Ok(md) => return format!("{} is {} bytes, manifest says {}", f.name, md.len(), f.size),
            Err(e) => return format!("{} missing ({e})", f.name),
        }
    }
    "unknown".to_string()
}

/// Is this artifact completely downloaded and size-sound right now?
/// (Cheap: stat per file. Hash verification happens once, at download.)
pub fn artifact_complete(dir: &std::path::Path) -> bool {
    let Some(m) = read_manifest(dir) else {
        return false;
    };
    m.complete
        && m.files.iter().all(|f| {
            std::fs::metadata(dir.join(&f.name))
                .map(|md| md.len() == f.size)
                .unwrap_or(false)
        })
}

#[derive(Serialize)]
pub struct MlxArtifactStatus {
    pub dir: String,
    pub present: bool,
    pub complete: bool,
    /// A download command for this artifact is running right now - the UI
    /// reattaches to it instead of offering the button again.
    pub downloading: bool,
    pub bytes_done: u64,
    pub bytes_total: u64,
}

/// Repos with a download command in flight (whole-artifact granularity -
/// the per-file guard lives in the downloader).
fn artifact_inflight_set() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static S: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    S.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

/// Fetch the pinned revision's file list from the Hugging Face API.
/// Flat dirs only (mlx-community's shape); nested paths are refused
/// loudly rather than silently flattened into collisions.
async fn fetch_file_list(repo: &str, revision: &str) -> Result<Vec<MlxManifestFile>, String> {
    let url = format!("https://huggingface.co/api/models/{repo}/tree/{revision}");
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Couldn't reach the model repo: {e}"))?;
    if !resp.status().is_success() {
        return Err(format!("Model repo answered {} for {repo}@{revision}", resp.status()));
    }
    #[derive(Deserialize)]
    struct TreeEntry {
        r#type: String,
        path: String,
        size: Option<u64>,
        lfs: Option<LfsInfo>,
    }
    #[derive(Deserialize)]
    struct LfsInfo {
        oid: Option<String>,
    }
    let entries: Vec<TreeEntry> = resp.json().await.map_err(|e| format!("bad repo listing: {e}"))?;
    let mut files = Vec::new();
    for e in entries {
        if e.r#type != "file" {
            continue;
        }
        // Repo housekeeping files are not part of the model.
        if e.path.starts_with('.') {
            continue;
        }
        if e.path.contains('/') {
            return Err(format!(
                "Model repo {repo} has nested files ({}) - not a flat MLX artifact; refusing rather than guessing",
                e.path
            ));
        }
        files.push(MlxManifestFile {
            name: e.path,
            size: e.size.unwrap_or(0),
            sha256: e.lfs.and_then(|l| l.oid),
        });
    }
    if !files.iter().any(|f| f.name == "config.json") {
        return Err(format!("Model repo {repo}@{revision} has no config.json - not an MLX model"));
    }
    if !files.iter().any(|f| f.name.ends_with(".safetensors")) {
        return Err(format!("Model repo {repo}@{revision} has no safetensors weights"));
    }
    Ok(files)
}

fn sha256_of(path: &std::path::Path) -> Result<String, String> {
    use sha2::{Digest, Sha256};
    let mut file = std::fs::File::open(path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    std::io::copy(&mut file, &mut hasher).map_err(|e| e.to_string())?;
    Ok(format!("{:x}", hasher.finalize()))
}

/// Download a complete MLX artifact at a pinned revision. Resumable at
/// file granularity: files already present at their manifest size (and
/// hash, where known) are skipped, so a retry finishes instead of
/// restarting. Emits "model-download-progress" under the ARTIFACT DIR
/// name with aggregate percent, alongside the per-file events every file
/// emits under its own path.
fn map_path(app: &AppHandle) -> Option<PathBuf> {
    use tauri::Manager;
    app.path().app_data_dir().ok().map(|d| d.join("mlx-map.json"))
}

fn read_map(app: &AppHandle) -> std::collections::HashMap<String, String> {
    map_path(app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

/// Remember which GGUF model an artifact upgrades - the loader's lookup key.
fn record_mapping(app: &AppHandle, gguf_filename: &str, repo: &str) {
    let mut map = read_map(app);
    map.insert(gguf_filename.to_string(), repo.to_string());
    if let Some(p) = map_path(app) {
        let _ = std::fs::write(p, serde_json::to_vec_pretty(&map).unwrap_or_default());
    }
}

/// Models whose MLX serving failed this session - retries land on
/// llama.cpp instead of looping the same failure. Cleared on restart.
fn mlx_failed_set() -> &'static std::sync::Mutex<std::collections::HashSet<String>> {
    static S: std::sync::OnceLock<std::sync::Mutex<std::collections::HashSet<String>>> =
        std::sync::OnceLock::new();
    S.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()))
}

pub fn note_mlx_serving_failed(gguf_filename: &str) {
    if let Ok(mut s) = mlx_failed_set().lock() {
        s.insert(gguf_filename.to_string());
    }
}

/// The MLX artifact dir that should SERVE this GGUF-named model's chat
/// turns right now: platform supported, engine installed, an artifact
/// mapped + complete, and no failure this session. None = llama.cpp.
pub fn serving_dir_for(app: &AppHandle, gguf_filename: &str) -> Option<PathBuf> {
    if !crate::mlx_engine::supported() {
        return None;
    }
    crate::mlx_engine::swiftlm_binary(app)?;
    if mlx_failed_set().lock().ok()?.contains(gguf_filename) {
        return None;
    }
    let repo = read_map(app).get(gguf_filename)?.to_string();
    let dir = artifact_dir(app, &repo).ok()?;
    artifact_complete(&dir).then_some(dir)
}

#[tauri::command]
pub async fn download_mlx_artifact(
    app: AppHandle,
    repo: String,
    revision: String,
    // for_model: the GGUF filename this artifact upgrades (the row's identity).
    for_model: Option<String>,
) -> Result<(), String> {
    let dir = artifact_dir(&app, &repo)?;
    let dir_name = artifact_dir_name(&repo);
    // Whole-artifact single flight: a second click reattaches in the UI,
    // it must not start a competing pass over the file list.
    if let Ok(mut set) = artifact_inflight_set().lock() {
        if !set.insert(repo.clone()) {
            return Err("This MLX version is already downloading".to_string());
        }
    }
    struct Inflight(String);
    impl Drop for Inflight {
        fn drop(&mut self) {
            if let Ok(mut s) = artifact_inflight_set().lock() {
                s.remove(&self.0);
            }
        }
    }
    let _inflight = Inflight(repo.clone());
    std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create artifact dir: {e}"))?;

    let files = fetch_file_list(&repo, &revision).await?;
    let bytes_total: u64 = files.iter().map(|f| f.size).sum();

    // Order: small metadata first (fail fast on repo problems), weights
    // after, so an interrupted download has the cheap files done.
    let mut ordered = files.clone();
    ordered.sort_by_key(|f| f.size);

    let mut bytes_done: u64 = 0;
    for f in &ordered {
        let target = dir.join(&f.name);
        let already = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
        if already == f.size && f.size > 0 {
            bytes_done += f.size;
            continue;
        }
        let url = format!(
            "https://huggingface.co/{}/resolve/{}/{}",
            repo, revision, f.name
        );
        // The path inside models_dir doubles as the progress/in-flight key.
        let rel = format!("{}/{}", dir_name, f.name);
        // Live aggregate progress DURING the file (a 2.7 GB weights file
        // otherwise leaves the bar at its pre-file value the whole way -
        // field 08-24: "stuck at 0%").
        let ticker = {
            let app = app.clone();
            let rel = rel.clone();
            let dir_name = dir_name.clone();
            let base = bytes_done;
            tauri::async_runtime::spawn(async move {
                loop {
                    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                    let Some((done, _)) = crate::llm::download_progress_of(&rel) else {
                        continue;
                    };
                    let agg = base.saturating_add(done);
                    let percent = if bytes_total > 0 {
                        ((agg as f64 / bytes_total as f64) * 100.0) as u32
                    } else {
                        0
                    };
                    let _ = app.emit(
                        "model-download-progress",
                        serde_json::json!({
                            "filename": dir_name,
                            "downloaded": agg,
                            "total": bytes_total,
                            "percent": percent,
                        }),
                    );
                }
            })
        };
        let dl = crate::llm::download_model(app.clone(), url, rel).await;
        ticker.abort();
        dl?;
        let got = std::fs::metadata(&target).map(|m| m.len()).unwrap_or(0);
        if f.size > 0 && got != f.size {
            // Delete before erroring - a wrong-size file left in place
            // would make the retry's downloader refuse as "already
            // downloaded".
            let _ = std::fs::remove_file(&target);
            return Err(format!(
                "{} finished at {} bytes but the pinned revision says {} - try again",
                f.name, got, f.size
            ));
        }
        if let Some(ref expected) = f.sha256 {
            let actual = sha256_of(&target)?;
            if &actual != expected {
                let _ = std::fs::remove_file(&target);
                return Err(format!(
                    "{} doesn't match the pinned revision (checksum {}… vs {}…) - deleted; try again",
                    f.name,
                    &actual[..12],
                    &expected[..12]
                ));
            }
        }
        bytes_done += f.size;
        let percent = if bytes_total > 0 {
            ((bytes_done as f64 / bytes_total as f64) * 100.0) as u32
        } else {
            100
        };
        let _ = app.emit(
            "model-download-progress",
            serde_json::json!({
                "filename": dir_name,
                "downloaded": bytes_done,
                "total": bytes_total,
                "percent": percent,
            }),
        );
    }

    let manifest = MlxManifest {
        repo: repo.clone(),
        revision,
        files,
        complete: true,
    };
    std::fs::write(
        manifest_path(&dir),
        serde_json::to_vec_pretty(&manifest).map_err(|e| e.to_string())?,
    )
    .map_err(|e| format!("cannot write manifest: {e}"))?;

    if let Some(ref gguf) = for_model {
        record_mapping(&app, gguf, &repo);
    }
    let _ = app.emit(
        "model-download-complete",
        serde_json::json!({ "filename": dir_name }),
    );
    log::info!("[MLX] artifact complete: {} ({} files, {} bytes)", dir_name, manifest.files.len(), bytes_total);
    Ok(())
}

#[tauri::command]
pub async fn mlx_artifact_status(app: AppHandle, repo: String) -> Result<MlxArtifactStatus, String> {
    let dir = artifact_dir(&app, &repo)?;
    let present = dir.exists();
    let complete = present && artifact_complete(&dir);
    if present && !complete {
        // Say WHY in the log: a row offering "Get the MLX version" for a
        // directory that looks downloaded needs a reason in the diagnostics.
        log::info!("[MLX] {} present but not complete: {}", artifact_dir_name(&repo), incomplete_reason(&dir));
    }
    let dir_name = artifact_dir_name(&repo);
    let downloading = artifact_inflight_set()
        .lock()
        .map(|s| s.contains(&repo))
        .unwrap_or(false)
        || crate::llm::any_download_in_flight_under(&format!("{}/", dir_name));
    let (mut done, mut total) = (0u64, 0u64);
    if let Some(m) = read_manifest(&dir) {
        for f in &m.files {
            total += f.size;
            done += std::fs::metadata(dir.join(&f.name))
                .map(|md| md.len().min(f.size))
                .unwrap_or(0);
        }
    }
    Ok(MlxArtifactStatus {
        dir: dir_name,
        present,
        complete,
        downloading,
        bytes_done: done,
        bytes_total: total,
    })
}

/// Delete an MLX artifact directory entirely (its GGUF sibling, if any,
/// is untouched - deleting one artifact never deletes the model).
#[tauri::command]
pub async fn delete_mlx_artifact(app: AppHandle, repo: String) -> Result<(), String> {
    let dir = artifact_dir(&app, &repo)?;
    // Belt: never remove anything outside the models dir or without our prefix.
    let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if !name.starts_with(MLX_DIR_PREFIX) {
        return Err("refusing to delete a non-MLX directory".to_string());
    }
    std::fs::remove_dir_all(&dir).map_err(|e| format!("delete failed: {e}"))?;
    log::info!("[MLX] artifact deleted: {}", name);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dir_names_are_prefixed_and_flat() {
        assert_eq!(
            artifact_dir_name("mlx-community/Qwen3.6-35B-A3B-4bit"),
            "mlx-Qwen3.6-35B-A3B-4bit"
        );
        assert_eq!(artifact_dir_name("solo-repo"), "mlx-solo-repo");
    }

    #[test]
    fn completeness_is_manifest_and_size_based() {
        let base = std::env::temp_dir().join(format!("yoai-mlx-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        std::fs::create_dir_all(&base).unwrap();
        // No manifest: incomplete.
        assert!(!artifact_complete(&base));
        std::fs::write(base.join("config.json"), b"{}").unwrap();
        let m = MlxManifest {
            repo: "r".into(),
            revision: "rev".into(),
            files: vec![MlxManifestFile { name: "config.json".into(), size: 2, sha256: None }],
            complete: true,
        };
        std::fs::write(manifest_path(&base), serde_json::to_vec(&m).unwrap()).unwrap();
        assert!(artifact_complete(&base));
        // A shortened file breaks completeness.
        std::fs::write(base.join("config.json"), b"{").unwrap();
        assert!(!artifact_complete(&base));
        let _ = std::fs::remove_dir_all(&base);
    }
}
