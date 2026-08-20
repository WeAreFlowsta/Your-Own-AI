//! Model artifact hashes - the incorruptible half of model provenance.
//!
//! A transcript entry records which model FILE answered (by filename);
//! the hash proves which exact artifact that was - nobody can swap
//! weights and inherit the name. Hashes live in a plain manifest
//! (model-hashes.json - hashes are not secrets) keyed by filename, with
//! size+mtime so a changed file is never vouched for by a stale hash.
//!
//! Hashing reads the whole file (GGUFs run to tens of GB), so it only
//! ever happens in a background task, one file at a time: after a
//! download completes, and as a startup backfill for files downloaded
//! before this existed.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::io::Read;
use std::path::PathBuf;
use tauri::Manager;

#[derive(Serialize, Deserialize, Clone)]
struct HashEntry {
    sha256: String,
    size: u64,
    mtime: i64,
}

fn manifest_path(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path()
        .app_data_dir()
        .ok()
        .map(|d| d.join("model-hashes.json"))
}

fn read_manifest(app: &tauri::AppHandle) -> HashMap<String, HashEntry> {
    let Some(p) = manifest_path(app) else {
        return HashMap::new();
    };
    std::fs::read_to_string(p)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_manifest(app: &tauri::AppHandle, m: &HashMap<String, HashEntry>) {
    let Some(p) = manifest_path(app) else { return };
    if let Ok(s) = serde_json::to_string_pretty(m) {
        let _ = std::fs::write(p, s);
    }
}

fn mtime_secs(meta: &std::fs::Metadata) -> i64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// The recorded hash for a model filename, if we have hashed that exact
/// file (size and mtime must still match). None for online model ids
/// and files not yet hashed - a record simply omits the field then.
pub(crate) fn get(app: &tauri::AppHandle, model: &str) -> Option<String> {
    if !model.to_lowercase().ends_with(".gguf") {
        return None;
    }
    let dir = crate::llm::get_models_dir(app).ok()?;
    let meta = std::fs::metadata(dir.join(model)).ok()?;
    let m = read_manifest(app);
    let e = m.get(model)?;
    if e.size == meta.len() && e.mtime == mtime_secs(&meta) {
        Some(e.sha256.clone())
    } else {
        None
    }
}

/// Hash one file and store it. Blocking - call from a blocking task.
fn note_file_blocking(app: &tauri::AppHandle, filename: &str) -> Result<(), String> {
    let dir = crate::llm::get_models_dir(app)?;
    let path = dir.join(filename);
    let meta = std::fs::metadata(&path).map_err(|e| e.to_string())?;
    let mut f = std::fs::File::open(&path).map_err(|e| e.to_string())?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 8 * 1024 * 1024];
    loop {
        let n = f.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let sha256 = hex::encode(hasher.finalize());
    let mut m = read_manifest(app);
    m.insert(
        filename.to_string(),
        HashEntry {
            sha256: sha256.clone(),
            size: meta.len(),
            mtime: mtime_secs(&meta),
        },
    );
    write_manifest(app, &m);
    log::info!("[model-hash] {} = sha256:{}...", filename, &sha256[..16]);
    Ok(())
}

/// Hash every model file that lacks a current manifest entry, one at a
/// time. Cheap when everything is already hashed (metadata reads only).
pub(crate) fn backfill_async(app: tauri::AppHandle) {
    tauri::async_runtime::spawn(async move {
        let _ = tauri::async_runtime::spawn_blocking(move || {
            let Ok(dir) = crate::llm::get_models_dir(&app) else {
                return;
            };
            let Ok(entries) = std::fs::read_dir(&dir) else {
                return;
            };
            let manifest = read_manifest(&app);
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                if !name.to_lowercase().ends_with(".gguf") {
                    continue;
                }
                let Ok(meta) = entry.metadata() else { continue };
                let current = manifest
                    .get(&name)
                    .map(|e| e.size == meta.len() && e.mtime == mtime_secs(&meta))
                    .unwrap_or(false);
                if current {
                    continue;
                }
                if let Err(e) = note_file_blocking(&app, &name) {
                    log::warn!("[model-hash] {} failed: {}", name, e);
                }
            }
        })
        .await;
    });
}
