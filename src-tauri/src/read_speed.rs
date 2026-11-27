//! How fast each local model reads on this computer, and what that allows
//! memory to put ahead of a question.
//!
//! A model reads everything it is given before it writes its first word.
//! What memory found is the largest part of that, so on a slow computer a
//! full set of results is minutes of silence. The engine reports how fast
//! it read each turn; the latest figure per model file is kept here, and
//! what memory gives a turn is sized so that reading it adds no more than
//! `WAIT_SECS` before the first word. The best result is always given.
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};
use tauri::Manager;

/// The most that reading what memory found may add before the first word.
pub(crate) const WAIT_SECS: f64 = 10.0;
/// About three characters to a token, as the block's own room is counted.
const CHARS_PER_TOKEN: f64 = 3.0;
/// A reading figure from a turn that read less than this is too noisy to
/// keep (a turn whose beginning the engine already held reads a few tokens).
const READ_AT_LEAST: u64 = 200;
/// Results given to a model whose reading speed is not known yet.
pub(crate) const UNKNOWN_SPEED_AT_MOST: usize = 3;

static SPEEDS: OnceLock<Mutex<HashMap<String, f64>>> = OnceLock::new();

fn file(app: &tauri::AppHandle) -> Option<PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join("read-speeds.json"))
}

fn speeds(app: &tauri::AppHandle) -> &'static Mutex<HashMap<String, f64>> {
    SPEEDS.get_or_init(|| {
        let kept = file(app).and_then(|p| std::fs::read_to_string(p).ok()).and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default();
        Mutex::new(kept)
    })
}

/// The engine's figure for a turn: tokens a second over `read` tokens.
pub(crate) fn note(app: &tauri::AppHandle, model: &str, per_second: Option<f64>, read: Option<u64>) {
    let Some(tps) = per_second.filter(|t| t.is_finite() && *t > 0.0) else { return };
    if read.unwrap_or(0) < READ_AT_LEAST || !is_local_file(model) {
        return;
    }
    let Ok(mut all) = speeds(app).lock() else { return };
    let tps = (tps * 10.0).round() / 10.0;
    if all.get(model) == Some(&tps) {
        return;
    }
    all.insert(model.to_string(), tps);
    if let (Some(p), Ok(s)) = (file(app), serde_json::to_string_pretty(&*all)) {
        let _ = std::fs::write(p, s);
    }
}

/// A model file on this computer, not an online model or a way of choosing one.
pub(crate) fn is_local_file(model: &str) -> bool {
    !model.is_empty() && model != "remote" && model != "auto" && !model.contains(':')
}

/// The reading speed last seen for a model file on this computer.
pub(crate) fn of(app: &tauri::AppHandle, model: &str) -> Option<f64> {
    speeds(app).lock().ok()?.get(model).copied()
}

/// The characters memory may put ahead of a question for a model that
/// reads at `per_second`.
pub(crate) fn wait_chars(per_second: f64) -> usize {
    (per_second * WAIT_SECS * CHARS_PER_TOKEN).max(0.0) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_wait_sets_the_characters() {
        // A processor-only computer reading 22 tokens a second, and a
        // graphics card reading 130.
        assert_eq!(wait_chars(22.0), 660);
        assert_eq!(wait_chars(130.0), 3_900);
        assert_eq!(wait_chars(0.0), 0);
    }

    #[test]
    fn only_a_model_file_has_a_speed() {
        assert!(is_local_file("Qwen3.8-4B-Q4_K_M.gguf"));
        assert!(!is_local_file("remote"));
        assert!(!is_local_file("online:some-model"));
        assert!(!is_local_file("auto"));
        assert!(!is_local_file("auto:offline"));
        assert!(!is_local_file(""));
    }
}
