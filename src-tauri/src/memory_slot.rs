//! The memory slot in the router (build-docs MEMORY_REDESIGN.md §7): which
//! LOCAL model does the memory work. Two task kinds - `memory-precision`
//! (fact extraction, note typing, the remember-this verdict) and
//! `memory-prose` (portrait, consolidation, distillation). The pick: a
//! model that is already warm and whose grade clears the floor does the
//! work on the chat server; otherwise the helper (the floor) on its own
//! port. Nothing is ever loaded for memory work, and nothing goes online.
//! Grades come from the measured leg (`memory-grades.json`); without a
//! measurement the capability registry stands in.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tauri::Manager;

const GRADES_FILE: &str = "memory-grades.json";
/// A model must grade at least this to take memory work from the helper.
pub const FLOOR: u8 = 6;

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct MemoryGrade {
    /// Extraction precision/recall on the fixed set, 0-10.
    #[serde(default)]
    pub precision: Option<u8>,
    /// Consolidation / portrait quality on the fixed set, 0-10.
    #[serde(default)]
    pub prose: Option<u8>,
    pub measured_at: i64,
}

fn grades_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    app.path().app_data_dir().ok().map(|d| d.join(GRADES_FILE))
}

pub fn load_grades(app: &tauri::AppHandle) -> HashMap<String, MemoryGrade> {
    grades_path(app)
        .and_then(|p| std::fs::read(p).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn save_grades(app: &tauri::AppHandle, grades: &HashMap<String, MemoryGrade>) {
    if let Some(p) = grades_path(app) {
        if let Ok(json) = serde_json::to_vec_pretty(grades) {
            let _ = std::fs::write(p, json);
        }
    }
}

/// A model's grade for a memory task: the measurement when there is one,
/// else the registry's nearest capability (reasoning for precision work,
/// overall for prose). Unknown families rank conservatively.
pub fn grade_for(grades: &HashMap<String, MemoryGrade>, model: &str, task: &str) -> u8 {
    let measured = grades.get(model).and_then(|g| if task == "memory-prose" { g.prose } else { g.precision });
    if let Some(m) = measured {
        return m;
    }
    match crate::model_caps::known_caps(model) {
        Some(c) => if task == "memory-prose" { c.overall } else { c.reasoning },
        None => 0,
    }
}

#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct MemoryPick {
    /// GGUF filename.
    pub model: String,
    /// "loaded" = the chat server's warm model does it; "helper" = the floor.
    pub via: String,
    pub grade: u8,
    pub reason: String,
}

/// The pure rule: the warm model when it clears the floor and the helper's
/// own grade, else the helper. While the helper is installed, only a
/// MEASURED grade lets the warm model take the work: the registry's guess
/// (a weak extractor can sit at 6 there) is for machines without a helper.
pub fn choose(task: &str, loaded: Option<&str>, loaded_grade: u8, loaded_measured: bool, helper: &str, helper_grade: u8, helper_installed: bool) -> MemoryPick {
    match loaded {
        Some(m) if m != helper && helper_installed && !loaded_measured => MemoryPick {
            model: helper.to_string(),
            via: "helper".into(),
            grade: helper_grade,
            reason: format!("{task}: the loaded model ({}) is not measured yet - the helper until the matrix grades it", &m[..m.len().min(24)]),
        },
        Some(m) if m != helper && loaded_grade >= FLOOR && loaded_grade >= helper_grade => MemoryPick {
            model: m.to_string(),
            via: "loaded".into(),
            grade: loaded_grade,
            reason: format!("{task}: the loaded model grades {loaded_grade}, at or above the helper's {helper_grade} - no load, no switch"),
        },
        Some(m) => MemoryPick {
            model: helper.to_string(),
            via: "helper".into(),
            grade: helper_grade,
            reason: format!("{task}: the loaded model ({}) grades {loaded_grade}, below the floor {FLOOR} or the helper's {helper_grade}", &m[..m.len().min(24)]),
        },
        None => MemoryPick {
            model: helper.to_string(),
            via: "helper".into(),
            grade: helper_grade,
            reason: format!("{task}: no model loaded - the helper, never a load for memory work"),
        },
    }
}

/// The pick for this machine right now.
pub fn pick(app: &tauri::AppHandle, task: &str) -> MemoryPick {
    let grades = load_grades(app);
    let helper = crate::llm::UTILITY_MODEL_FILE;
    let helper_grade = grades.get(helper).and_then(|g| if task == "memory-prose" { g.prose } else { g.precision }).unwrap_or(FLOOR);
    let loaded: Option<String> = app
        .try_state::<crate::llm::LLMState>()
        .and_then(|st| st.current_model.try_lock().ok().and_then(|m| m.clone()));
    let loaded_grade = loaded.as_deref().map(|m| grade_for(&grades, m, task)).unwrap_or(0);
    let loaded_measured = loaded.as_deref().and_then(|m| grades.get(m)).map(|g| if task == "memory-prose" { g.prose.is_some() } else { g.precision.is_some() }).unwrap_or(false);
    let helper_installed = crate::llm::get_models_dir(app).map(|d| d.join(helper).exists()).unwrap_or(false);
    choose(task, loaded.as_deref(), loaded_grade, loaded_measured, helper, helper_grade, helper_installed)
}

/// Pure: is the 2 GB helper needed on this machine? Not when it is
/// installed, not when nothing is installed yet (nothing to judge), and
/// not when any installed chat model grades at or above the floor for
/// precision work - then memory works without it and nobody is asked.
pub fn helper_needed(grades: &HashMap<String, MemoryGrade>, installed: &[String], helper: &str) -> bool {
    if installed.is_empty() || installed.iter().any(|m| m == helper) {
        return false;
    }
    !installed.iter().any(|m| grade_for(grades, m, "memory-precision") >= FLOOR)
}

/// The helper offer's gate (MEMORY_REDESIGN.md §7 / NEXT.md 10-08: the
/// helper is a floor, offered only when the installed models grade below
/// it - and only ever as an offer).
#[tauri::command]
pub async fn memory_helper_needed(app: tauri::AppHandle) -> bool {
    let Ok(dir) = crate::llm::get_models_dir(&app) else { return false };
    let installed: Vec<String> = std::fs::read_dir(&dir)
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_str().map(String::from))
                .filter(|n| n.ends_with(".gguf") && !n.to_lowercase().contains("mmproj"))
                .filter(|n| crate::gguf::read_meta(&dir.join(n)).map(|m| !m.is_embedding()).unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    helper_needed(&load_grades(&app), &installed, crate::llm::UTILITY_MODEL_FILE)
}

#[tauri::command]
pub fn memory_slot_pick(app: tauri::AppHandle, task: String) -> MemoryPick {
    pick(&app, &task)
}

/// The measured leg's write path (no command boundary).
pub fn set_grade(app: &tauri::AppHandle, model: &str, precision: Option<u8>, prose: Option<u8>) {
    let mut grades = load_grades(app);
    let g = grades.entry(model.to_string()).or_default();
    if precision.is_some() {
        g.precision = precision.map(|v| v.min(10));
    }
    if prose.is_some() {
        g.prose = prose.map(|v| v.min(10));
    }
    g.measured_at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    save_grades(app, &grades);
}

/// The measured leg writes a model's grades here.
#[tauri::command]
pub fn memory_grades_set(app: tauri::AppHandle, model: String, precision: Option<u8>, prose: Option<u8>) -> Result<(), String> {
    let mut grades = load_grades(&app);
    let g = grades.entry(model).or_default();
    if precision.is_some() {
        g.precision = precision.map(|v| v.min(10));
    }
    if prose.is_some() {
        g.prose = prose.map(|v| v.min(10));
    }
    g.measured_at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    save_grades(&app, &grades);
    Ok(())
}

#[tauri::command]
pub fn memory_grades(app: tauri::AppHandle) -> HashMap<String, MemoryGrade> {
    load_grades(&app)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_warm_model_takes_memory_work_only_when_it_clears_the_floor_and_the_helper() {
        let helper = "Ministral-3-3B-Instruct-2512-Q4_K_M.gguf";
        let p = choose("memory-precision", Some("Big-14B.gguf"), 8, true, helper, 6, true);
        assert_eq!((p.via.as_str(), p.model.as_str()), ("loaded", "Big-14B.gguf"));
        let p = choose("memory-precision", Some("Big-14B.gguf"), 8, false, helper, 6, true);
        assert_eq!(p.via, "helper", "unmeasured while the helper is installed: the helper");
        let p = choose("memory-precision", Some("Big-14B.gguf"), 8, false, helper, 6, false);
        assert_eq!(p.via, "loaded", "no helper on disk: the registry decides");
        let p = choose("memory-precision", Some("Small-1B.gguf"), 3, true, helper, 6, true);
        assert_eq!((p.via.as_str(), p.model.as_str()), ("helper", helper), "below the floor");
        let p = choose("memory-prose", Some("Mid-7B.gguf"), 6, true, helper, 7, true);
        assert_eq!(p.via, "helper", "the helper measured better for prose");
        let p = choose("memory-prose", None, 0, false, helper, 6, true);
        assert_eq!(p.via, "helper", "nothing loaded: never a load for memory work");
        let p = choose("memory-precision", Some(helper), 9, true, helper, 6, true);
        assert_eq!(p.via, "helper", "the helper itself loaded counts as the helper");
    }

    #[test]
    fn the_helper_is_offered_only_when_every_installed_model_grades_below_the_floor() {
        let helper = "Ministral-3-3B-Instruct-2512-Q4_K_M.gguf";
        let mut grades = HashMap::new();
        grades.insert("good.gguf".to_string(), MemoryGrade { precision: Some(9), prose: None, measured_at: 1 });
        grades.insert("weak.gguf".to_string(), MemoryGrade { precision: Some(4), prose: None, measured_at: 1 });
        assert!(!helper_needed(&grades, &[], helper), "nothing installed: nothing to ask yet");
        assert!(!helper_needed(&grades, &["good.gguf".into()], helper));
        assert!(helper_needed(&grades, &["weak.gguf".into()], helper));
        assert!(!helper_needed(&grades, &["weak.gguf".into(), "good.gguf".into()], helper));
        assert!(!helper_needed(&grades, &["weak.gguf".into(), helper.into()], helper), "already installed");
        assert!(helper_needed(&grades, &["unmeasured-unknown-family.gguf".into()], helper), "unknown and unmeasured ranks at zero");
    }

    #[test]
    fn a_measurement_outranks_the_registry_and_an_unknown_family_ranks_at_zero() {
        let mut grades = HashMap::new();
        grades.insert("x.gguf".to_string(), MemoryGrade { precision: Some(9), prose: None, measured_at: 1 });
        assert_eq!(grade_for(&grades, "x.gguf", "memory-precision"), 9);
        assert_eq!(grade_for(&grades, "totally-unknown-family.gguf", "memory-prose"), 0);
    }
}
