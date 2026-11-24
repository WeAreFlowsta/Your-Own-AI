//! The built-in check: can the model on this computer do memory's
//! judging? Before a model tidies or keeps anything by itself, it is
//! given a set of made-up examples whose answers are known, and its
//! answers are marked here. The marks are strict where a mistake does
//! harm: wrongly saying "this replaces that" loses something true.
//!
//! No model is named anywhere: any model is marked the same way, on its
//! answers alone.

use serde::{Deserialize, Serialize};

/// The three parts of the check.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum Part {
    /// Does a later passage replace an earlier one?
    Judge,
    /// Was something settled in this exchange?
    Gate,
    /// Was this open thing done by later work?
    Done,
}

/// One example's outcome: what was expected and what the model said
/// (`None`: no usable answer, which counts as wrong but not as harm).
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Answered {
    pub part: Part,
    /// The answer that changes something: REPLACES, settled, DONE.
    pub expected_yes: bool,
    pub said_yes: Option<bool>,
}

/// A part's marks.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PartMarks {
    pub part: Part,
    pub examples: usize,
    /// Expected yes, said yes.
    pub caught: usize,
    pub of_yes: usize,
    /// Expected no, said yes: the harmful mistake.
    pub wrong_yes: usize,
    pub of_no: usize,
    /// Right answers of either kind.
    pub right: usize,
    pub passed: bool,
}

/// Mark one part. The pass marks:
/// - judging: at most 1 wrong "replaces" among the pairs that both stand,
///   and at least 10 of 12 replacements caught (five in six, for a set of
///   another size);
/// - the gate: at least 16 of 20 right (four in five);
/// - done by later work: at most 1 wrong "done", and at least 7 of 10
///   right.
pub(crate) fn mark(part: Part, answers: &[Answered]) -> PartMarks {
    let mine: Vec<&Answered> = answers.iter().filter(|a| a.part == part).collect();
    let of_yes = mine.iter().filter(|a| a.expected_yes).count();
    let of_no = mine.len() - of_yes;
    let caught = mine.iter().filter(|a| a.expected_yes && a.said_yes == Some(true)).count();
    let wrong_yes = mine.iter().filter(|a| !a.expected_yes && a.said_yes == Some(true)).count();
    let right = mine.iter().filter(|a| a.said_yes == Some(a.expected_yes)).count();
    // "At least a of b", for a set of any size: a share, never rounded in the model's favor.
    let at_least = |got: usize, a: usize, b: usize, of: usize| got * b >= a * of;
    let passed = !mine.is_empty()
        && match part {
            Part::Judge => wrong_yes <= 1 && at_least(caught, 10, 12, of_yes),
            Part::Gate => at_least(right, 16, 20, mine.len()),
            Part::Done => wrong_yes <= 1 && at_least(right, 7, 10, mine.len()),
        };
    PartMarks { part, examples: mine.len(), caught, of_yes, wrong_yes, of_no, right, passed }
}

/// What a model may do for memory, from its marks.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Verdict {
    /// The set's version: marks from another version are not compared.
    pub set: u32,
    pub parts: Vec<PartMarks>,
    /// May replace items by itself (tidying).
    pub may_tidy: bool,
    /// May keep what was settled, and suggest names.
    pub may_keep: bool,
    /// May close open items that later work finished.
    pub may_close: bool,
}

pub(crate) fn verdict(set: u32, answers: &[Answered]) -> Verdict {
    let parts: Vec<PartMarks> = [Part::Judge, Part::Gate, Part::Done].into_iter().map(|p| mark(p, answers)).collect();
    let passed = |p: Part| parts.iter().any(|m| m.part == p && m.passed);
    Verdict { set, may_tidy: passed(Part::Judge), may_keep: passed(Part::Gate), may_close: passed(Part::Done), parts }
}

/// Is the check itself steady for a model? The same verdict in every run
/// (the examples shuffled each time), or it is not a measure.
pub(crate) fn steady(runs: &[Verdict]) -> bool {
    runs.len() >= 3 && runs.windows(2).all(|w| (w[0].may_tidy, w[0].may_keep, w[0].may_close) == (w[1].may_tidy, w[1].may_keep, w[1].may_close))
}

/// A model's answer read from its reply: the JSON object between the
/// first `{` and the last `}`, and in it the field that carries the
/// verdict. Anything else is no answer.
pub(crate) fn read_answer(part: Part, reply: &str) -> Option<bool> {
    let (a, b) = (reply.find('{')?, reply.rfind('}')?);
    let v: serde_json::Value = serde_json::from_str(reply.get(a..=b)?).ok()?;
    let said = v.get("verdict")?.as_str()?.trim();
    match (part, said) {
        (Part::Judge, "REPLACES") => Some(true),
        (Part::Judge, "BOTH STAND") => Some(false),
        (Part::Gate, "settled") => Some(true),
        (Part::Gate, "in_progress") => Some(false),
        (Part::Done, "DONE") => Some(true),
        (Part::Done, "STILL OPEN") => Some(false),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A part's answers: `caught` of `yes` expected-yes examples said yes,
    /// `wrong` of `no` expected-no examples said yes, the rest said no.
    fn answers(part: Part, yes: usize, caught: usize, no: usize, wrong: usize) -> Vec<Answered> {
        let mut out: Vec<Answered> = (0..yes).map(|i| Answered { part, expected_yes: true, said_yes: Some(i < caught) }).collect();
        out.extend((0..no).map(|i| Answered { part, expected_yes: false, said_yes: Some(i < wrong) }));
        out
    }

    #[test]
    fn a_wrong_replaces_is_the_strict_mark() {
        // 12 replacements, 18 pairs that both stand.
        assert!(mark(Part::Judge, &answers(Part::Judge, 12, 12, 18, 0)).passed);
        assert!(mark(Part::Judge, &answers(Part::Judge, 12, 10, 18, 1)).passed, "ten of twelve caught and one wrong: the mark");
        assert!(!mark(Part::Judge, &answers(Part::Judge, 12, 12, 18, 2)).passed, "two wrong replacements fail, whatever was caught");
        assert!(!mark(Part::Judge, &answers(Part::Judge, 12, 9, 18, 0)).passed, "nine of twelve is too few");
        let m = mark(Part::Judge, &answers(Part::Judge, 12, 11, 18, 6));
        assert_eq!((m.caught, m.of_yes, m.wrong_yes, m.of_no, m.right, m.examples), (11, 12, 6, 18, 23, 30));
        // A model that says "replaces" to everything catches every replacement and fails.
        assert!(!mark(Part::Judge, &answers(Part::Judge, 12, 12, 18, 18)).passed);
        // And one that never does harms nothing and is no use: it fails too.
        assert!(!mark(Part::Judge, &answers(Part::Judge, 12, 0, 18, 0)).passed);
    }

    #[test]
    fn the_gate_and_the_done_part_have_their_own_marks() {
        assert!(mark(Part::Gate, &answers(Part::Gate, 10, 8, 10, 2)).passed, "16 of 20");
        assert!(!mark(Part::Gate, &answers(Part::Gate, 10, 8, 10, 3)).passed, "15 of 20");
        assert!(mark(Part::Done, &answers(Part::Done, 5, 3, 5, 1)).passed, "7 of 10 with one wrong done");
        assert!(!mark(Part::Done, &answers(Part::Done, 5, 5, 5, 2)).passed, "two wrong dones fail");
        assert!(!mark(Part::Done, &answers(Part::Done, 5, 1, 5, 0)).passed, "6 of 10");
        // An answer that could not be read is wrong, and is not a harmful yes.
        let unread = vec![Answered { part: Part::Gate, expected_yes: false, said_yes: None }];
        let m = mark(Part::Gate, &unread);
        assert_eq!((m.right, m.wrong_yes, m.passed), (0, 0, false));
        // No examples of a part: not passed.
        assert!(!mark(Part::Judge, &[]).passed);
    }

    #[test]
    fn a_models_marks_say_what_it_may_do() {
        let good: Vec<Answered> = [answers(Part::Judge, 12, 11, 18, 1), answers(Part::Gate, 10, 9, 10, 1), answers(Part::Done, 5, 4, 5, 0)].concat();
        let v = verdict(2, &good);
        assert_eq!((v.may_tidy, v.may_keep, v.may_close, v.set), (true, true, true, 2));
        // Judging fails: it may still keep what was settled and close what was done, and never tidy.
        let weak: Vec<Answered> = [answers(Part::Judge, 12, 12, 18, 7), answers(Part::Gate, 10, 9, 10, 1), answers(Part::Done, 5, 5, 5, 0)].concat();
        let v = verdict(2, &weak);
        assert_eq!((v.may_tidy, v.may_keep, v.may_close), (false, true, true));
        assert_eq!(v.parts.iter().map(|p| p.passed).collect::<Vec<_>>(), vec![false, true, true]);
        // The same answers in another order give the same verdict.
        let mut shuffled = good.clone();
        shuffled.reverse();
        assert_eq!(verdict(2, &shuffled).may_tidy, verdict(2, &good).may_tidy);
        assert!(steady(&[verdict(2, &good), verdict(2, &shuffled), verdict(2, &good)]));
        assert!(!steady(&[verdict(2, &good), verdict(2, &weak), verdict(2, &good)]), "a verdict that changes between runs is not a measure");
        assert!(!steady(&[verdict(2, &good), verdict(2, &good)]), "three runs at least");
    }

    #[test]
    fn an_answer_is_read_from_the_reply_or_not_at_all() {
        assert_eq!(read_answer(Part::Judge, "{\"verdict\":\"REPLACES\",\"now\":\"Free delivery starts at $60.\"}"), Some(true));
        assert_eq!(read_answer(Part::Judge, "Sure. {\"verdict\": \"BOTH STAND\", \"now\": \"x\"} Hope that helps."), Some(false));
        assert_eq!(read_answer(Part::Gate, "{\"verdict\":\"settled\",\"quote\":\"we go with Hartwell\"}"), Some(true));
        assert_eq!(read_answer(Part::Gate, "{\"verdict\":\"in_progress\",\"quote\":\"\"}"), Some(false));
        assert_eq!(read_answer(Part::Done, "{\"verdict\":\"DONE\"}"), Some(true));
        // Another part's word, free text, broken JSON: no answer.
        assert_eq!(read_answer(Part::Gate, "{\"verdict\":\"REPLACES\"}"), None);
        assert_eq!(read_answer(Part::Judge, "I think the later one replaces it."), None);
        assert_eq!(read_answer(Part::Judge, "{\"verdict\":\"REPLACES\""), None);
        assert_eq!(read_answer(Part::Judge, "}{"), None);
    }
}
