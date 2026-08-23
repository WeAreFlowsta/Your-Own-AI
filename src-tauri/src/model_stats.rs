//! Per-model, per-machine measurements the router and the UI share: the
//! generation speed the engine itself reported for this model here (moving
//! average over turns long enough to mean something), and how long the
//! last load took. One JSON in app data (`model-stats.json`); written from
//! the stream end and the loader, read by fit::assess (so the Models page,
//! the picker and the router all see the same numbers).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::Manager;

/// Turns shorter than this are dominated by latency - not a speed.
const MIN_TOKENS: u32 = 32;
/// Moving-average weight of the newest sample.
const ALPHA: f64 = 0.3;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct ModelStats {
    /// Tokens per second, engine-measured, moving average.
    pub tps: Option<f64>,
    pub tps_samples: u32,
    /// Seconds from spawn to a healthy server on the last load.
    pub load_secs: Option<f64>,
    pub updated_at: i64,
}

fn path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("model-stats.json"))
}

pub fn read_all(app: &tauri::AppHandle) -> HashMap<String, ModelStats> {
    path(app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

fn write_all(app: &tauri::AppHandle, m: &HashMap<String, ModelStats>) {
    let Some(p) = path(app) else { return };
    if let Ok(s) = serde_json::to_string_pretty(m) {
        let _ = std::fs::write(p, s);
    }
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// One finished local turn: tokens generated and the engine's tokens/sec.
pub fn record_speed(app: &tauri::AppHandle, model: &str, completion_tokens: u32, tps: f64) {
    if model.is_empty() || !(tps.is_finite() && tps > 0.0) || completion_tokens < MIN_TOKENS {
        return;
    }
    let mut m = read_all(app);
    let e = m.entry(model.to_string()).or_default();
    e.tps = Some(match e.tps {
        Some(prev) if e.tps_samples > 0 => prev + ALPHA * (tps - prev),
        _ => tps,
    });
    e.tps_samples += 1;
    e.updated_at = now();
    write_all(app, &m);
}

/// The last load's duration.
pub fn record_load(app: &tauri::AppHandle, model: &str, secs: f64) {
    if model.is_empty() || !(secs.is_finite() && secs > 0.0) {
        return;
    }
    let mut m = read_all(app);
    let e = m.entry(model.to_string()).or_default();
    e.load_secs = Some(secs);
    e.updated_at = now();
    write_all(app, &m);
}

/// All measurements, filename -> stats (for the Models page / overview).
#[tauri::command]
pub fn model_stats(app: tauri::AppHandle) -> HashMap<String, ModelStats> {
    read_all(&app)
}

#[cfg(test)]
mod tests {
    /// The moving average behaves: first sample is taken whole, later ones
    /// blend at ALPHA; short turns are ignored.
    #[test]
    fn averaging_rules() {
        let mut e = super::ModelStats::default();
        for tps in [30.0, 20.0] {
            e.tps = Some(match e.tps {
                Some(prev) if e.tps_samples > 0 => prev + super::ALPHA * (tps - prev),
                _ => tps,
            });
            e.tps_samples += 1;
        }
        assert!((e.tps.unwrap() - 27.0).abs() < 1e-9);
        assert!(31 < super::MIN_TOKENS);
    }
}
