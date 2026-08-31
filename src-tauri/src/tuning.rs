//! Per-model fine-tune settings (FINE_TUNE_PANEL.md, layer 2).
//!
//! `model-tuning.json` in app data: machine-specific overrides per model
//! file - deliberately NOT a sidecar in the models folder, because "right
//! for this VRAM" must not travel to another computer with the file.
//! Every field unset = the automatics decide, byte-for-byte as before.

use std::collections::HashMap;
use std::path::PathBuf;
use tauri::{AppHandle, Manager, State};

#[derive(serde::Serialize, serde::Deserialize, Clone, Copy, Default)]
pub struct ModelTuning {
    /// Pinned context size. Wins over the sizing AND over growth requests.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<u64>,
    /// MoE expert layers on the CPU. 0 = everything on the card.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub moe_cpu_layers: Option<u32>,
    /// Leave the registered speed-up draft out of the next loads.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub draft_off: Option<bool>,
}

impl ModelTuning {
    pub fn is_empty(&self) -> bool {
        self.context.is_none() && self.moe_cpu_layers.is_none() && self.draft_off.is_none()
    }
}

fn path(app: &AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("model-tuning.json"))
}

fn load_all(app: &AppHandle) -> HashMap<String, ModelTuning> {
    let Some(p) = path(app) else { return HashMap::new() };
    std::fs::read_to_string(p)
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default()
}

/// The overrides for one model file; all-None when nothing was set.
pub fn get(app: &AppHandle, model: &str) -> ModelTuning {
    load_all(app).get(model).copied().unwrap_or_default()
}

/// Machine-level worker-thread choice (settings.json `engineThreads`).
pub fn engine_threads(app: &AppHandle) -> Option<u32> {
    use tauri_plugin_store::StoreExt;
    let store = app.store("settings.json").ok()?;
    store
        .get("engineThreads")
        .and_then(|v| v.as_u64())
        .filter(|&t| t >= 1 && t <= 256)
        .map(|t| t as u32)
}

#[tauri::command]
pub async fn tuning_get(app: AppHandle, model: String) -> Result<ModelTuning, String> {
    Ok(get(&app, &model))
}

#[tauri::command]
pub async fn tuning_set(app: AppHandle, model: String, tuning: ModelTuning) -> Result<(), String> {
    let mut all = load_all(&app);
    if tuning.is_empty() {
        all.remove(&model);
    } else {
        all.insert(model, tuning);
    }
    let p = path(&app).ok_or("cannot resolve app data dir")?;
    std::fs::write(&p, serde_json::to_string_pretty(&all).map_err(|e| e.to_string())?)
        .map_err(|e| format!("cannot write {}: {e}", p.display()))
}

#[tauri::command]
pub async fn tuning_set_engine_threads(app: AppHandle, threads: Option<u32>) -> Result<(), String> {
    use tauri_plugin_store::StoreExt;
    let store = app.store("settings.json").map_err(|e| e.to_string())?;
    match threads.filter(|&t| t >= 1 && t <= 256) {
        Some(t) => store.set("engineThreads", serde_json::json!(t)),
        None => {
            store.delete("engineThreads");
        }
    }
    store.save().map_err(|e| e.to_string())
}

/// Apply a changed tuning immediately when THIS model is the loaded one:
/// the load_model short-circuit would otherwise no-op the reload. Returns
/// whether a reload actually ran (false = it applies at the next load).
#[tauri::command]
pub async fn tuning_apply_now(
    app: AppHandle,
    state: State<'_, crate::llm::LLMState>,
    model: String,
) -> Result<bool, String> {
    let current = state.current_model.lock().await.clone();
    if current.as_deref() != Some(model.as_str()) {
        return Ok(false);
    }
    crate::llm::FORCE_RELOAD_NEXT.store(true, std::sync::atomic::Ordering::SeqCst);
    let with_vision = state.current_mmproj.lock().await.is_some();
    crate::llm::load_model(app, state, model, with_vision, "fine-tune".into()).await?;
    Ok(true)
}
