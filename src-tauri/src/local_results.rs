//! What each local model file did on our own questions and on the built-in
//! memory check, measured by us and shipped with the release, as new
//! local models are: nothing is fetched. A result belongs to a model FILE
//! (its name and, where it matters, its hash), not to a computer: the same
//! file gives the same answers everywhere, and speed is measured on each
//! computer separately.
//!
//! The data is `local_model_results.json` beside this file. Results come
//! from the matrix's grading part: ours first, then anyone's who sends the
//! report's block. A model with no result is treated as it always was.

use serde::Deserialize;
use std::sync::OnceLock;

/// The harder questions (`matrix::QUALITY_HARD`): how many were right.
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct Questions {
    /// The version of the question set the result is for.
    pub set: u32,
    pub right: u16,
    pub of: u16,
}

/// The built-in memory check's result (`memory_check`).
#[derive(Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct Memory {
    /// The version of the check's example set.
    pub set: u32,
    pub may_keep: bool,
    pub may_close: bool,
    pub may_tidy: bool,
    /// Right answers on the judging part, in its worst run.
    #[serde(default)]
    pub judge_right: u32,
}

#[derive(Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct LocalResult {
    /// The model file's name.
    pub file: String,
    /// The file's hash when it was measured.
    #[serde(default)]
    pub sha256: Option<String>,
    #[serde(default)]
    pub questions: Option<Questions>,
    #[serde(default)]
    pub memory: Option<Memory>,
}

const DATA: &str = include_str!("local_model_results.json");

pub(crate) fn all() -> &'static [LocalResult] {
    static ALL: OnceLock<Vec<LocalResult>> = OnceLock::new();
    ALL.get_or_init(|| parse(DATA))
}

fn parse(text: &str) -> Vec<LocalResult> {
    #[derive(Deserialize)]
    struct File {
        results: Vec<LocalResult>,
    }
    serde_json::from_str::<File>(text).map(|f| f.results).unwrap_or_default()
}

/// The result shipped for a model file, by its name.
pub(crate) fn for_file(name: &str) -> Option<&'static LocalResult> {
    all().iter().find(|r| r.file.eq_ignore_ascii_case(name))
}

/// A share of the harder questions answered right, on the scale the
/// capability table uses (0 to 10). The marks are set against what the
/// table already says of models measured both ways: nearly all right is
/// an 8, and each step down is about a seventh of the set.
pub(crate) fn score_from_questions(right: u16, of: u16) -> u8 {
    if of == 0 {
        return 0;
    }
    match u32::from(right) * 36 / u32::from(of) {
        34.. => 8,
        31..=33 => 7,
        26..=30 => 6,
        21..=25 => 5,
        _ => 4,
    }
}

/// The measured score for a model on the kind of question our set tests
/// (general questions, reasoning, sums), for this version of the set.
/// None for coding, vision and medical work, which the set does not test,
/// and for a model with no result: the capability table decides those.
pub(crate) fn measured_score(name: &str, task: &str, set: u32) -> Option<u8> {
    if matches!(task, "code" | "vision" | "medical") {
        return None;
    }
    let q = for_file(name)?.questions.as_ref().filter(|q| q.set == set)?;
    Some(score_from_questions(q.right, q.of))
}

/// The memory check's result shipped for this exact file (its hash must
/// match) and this version of the check's examples.
pub(crate) fn memory_for(name: &str, sha256: Option<&str>, set: u32) -> Option<&'static Memory> {
    let r = for_file(name)?;
    let same_file = matches!((r.sha256.as_deref(), sha256), (Some(a), Some(b)) if a.eq_ignore_ascii_case(b));
    r.memory.as_ref().filter(|m| same_file && m.set == set)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_shipped_results_parse_and_name_each_file_once() {
        let all = all();
        assert!(serde_json::from_str::<serde_json::Value>(DATA).is_ok(), "the results file is JSON");
        let mut names: Vec<String> = all.iter().map(|r| r.file.to_lowercase()).collect();
        names.sort();
        let before = names.len();
        names.dedup();
        assert_eq!(before, names.len(), "a file is listed once");
        for r in all {
            assert!(r.file.to_lowercase().ends_with(".gguf"), "{}", r.file);
            if let Some(q) = &r.questions {
                assert!(q.of > 0 && q.right <= q.of, "{}", r.file);
            }
            if r.memory.is_some() {
                assert!(r.sha256.as_deref().is_some_and(|h| h.len() == 64), "a memory result names its file by hash: {}", r.file);
            }
        }
    }

    #[test]
    fn a_share_of_right_answers_becomes_a_score() {
        assert_eq!(score_from_questions(36, 36), 8);
        assert_eq!(score_from_questions(34, 36), 8);
        assert_eq!(score_from_questions(33, 36), 7);
        assert_eq!(score_from_questions(28, 36), 6);
        assert_eq!(score_from_questions(25, 36), 5);
        assert_eq!(score_from_questions(21, 36), 5);
        assert_eq!(score_from_questions(12, 36), 4);
        assert_eq!(score_from_questions(0, 0), 0);
        // Another length of set is read by its share.
        assert_eq!(score_from_questions(17, 18), 8);
    }

    #[test]
    fn a_result_is_used_only_where_it_applies() {
        let text = r#"{"results":[{"file":"Made-Up-4B-Q4_K_M.gguf","sha256":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa","questions":{"set":1,"right":33,"of":36},"memory":{"set":3,"may_keep":true,"may_close":true,"may_tidy":false,"judge_right":21}}]}"#;
        let all = parse(text);
        assert_eq!(all.len(), 1);
        let r = &all[0];
        assert_eq!(r.questions.as_ref().map(|q| score_from_questions(q.right, q.of)), Some(7));
        assert!(r.memory.as_ref().is_some_and(|m| m.may_keep && !m.may_tidy));
        // A file that is not listed has no measured score, whatever the task.
        assert_eq!(measured_score("not-a-listed-file.gguf", "general", 1), None);
        // Coding, vision and medical are the table's.
        for task in ["code", "vision", "medical"] {
            assert_eq!(measured_score("anything.gguf", task, 1), None, "{task}");
        }
        assert_eq!(memory_for("not-a-listed-file.gguf", Some("aa"), 3), None);
    }
}
