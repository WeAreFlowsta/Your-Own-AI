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

/// The shipped examples, version 2: made-up conversations of one shop,
/// each passage as long and as messy as a real one.
pub(crate) const SET: &str = include_str!("memory_check_set.json");

/// One side of an example: a dated passage of a conversation.
#[derive(Deserialize, Clone, Debug)]
pub(crate) struct Passage {
    pub date: String,
    pub text: String,
}

/// An earlier and a later passage: does the later replace the earlier?
#[derive(Deserialize, Clone, Debug)]
pub(crate) struct PairExample {
    pub id: String,
    pub expect: String,
    pub earlier: Passage,
    pub later: Passage,
}

/// One exchange: was something settled in it?
#[derive(Deserialize, Clone, Debug)]
pub(crate) struct ExchangeExample {
    pub id: String,
    pub expect: String,
    pub exchange: Passage,
}

/// An open thing and a later passage: was it done?
#[derive(Deserialize, Clone, Debug)]
pub(crate) struct DoneExample {
    pub id: String,
    pub expect: String,
    pub open: Passage,
    pub later: Passage,
}

#[derive(Deserialize, Clone, Debug)]
pub(crate) struct CheckSet {
    pub v: u32,
    pub pairs: Vec<PairExample>,
    pub exchanges: Vec<ExchangeExample>,
    pub done: Vec<DoneExample>,
}

pub(crate) fn shipped_set() -> CheckSet {
    serde_json::from_str(SET).expect("the shipped check set parses")
}

// ── the questions, in their fixed form ──────────────────────────────────

/// The replacement judgment. The words are measured: answers were read
/// with exactly this text.
pub(crate) const JUDGE_SYSTEM: &str = "You keep a person's memory tidy. You are shown an EARLIER passage and a LATER passage from their conversations, each dated. Decide whether the later passage changes what was true in the earlier one.\nverdict REPLACES: the later passage changes, reverses, closes or updates something stated in the earlier one.\nverdict BOTH STAND: they are about different things, or the later one does not change the earlier one.\n\"now\": one sentence, what is true now.";

/// "Was something settled": measured text.
pub(crate) const GATE_SYSTEM: &str = "You are shown one dated exchange between a person and their AI. Answer one question: by the END of this exchange, has something been settled that will still matter in a month?\n\"settled\": the person made a choice, or work was reported finished, or a name, number, path or setting was established, or the person stated how they want things done.\n\"in_progress\": the AI is still checking, proposing, asking or working; or the person only asked for something; or it is a log, a test message or passing talk.\nGive the verdict and, if settled, the short quote that settles it.";

/// "Done by later work": the same shape as the replacement judgment.
pub(crate) const DONE_SYSTEM: &str = "You keep a person's memory tidy. You are shown something they LEFT OPEN in an earlier passage, and a LATER passage from their conversations, each dated. Decide whether the later passage shows that the open thing was finished.\nverdict DONE: the later passage says or shows that the open thing itself was done.\nverdict STILL OPEN: it was only planned, promised, asked about or chased; or only part of it was done; or it was dropped; or the later passage is about something else.\n\"quote\": the short quote from the later passage that shows it, or an empty string.";

pub(crate) fn judge_user(earlier: &Passage, later: &Passage) -> String {
    format!("EARLIER passage, {}:\n{}\n\nLATER passage, {}:\n{}", earlier.date, earlier.text, later.date, later.text)
}

pub(crate) fn gate_user(exchange: &Passage) -> String {
    format!("Exchange, {}:\n{}", exchange.date, exchange.text)
}

pub(crate) fn done_user(open: &Passage, later: &Passage) -> String {
    format!("LEFT OPEN, {}:\n{}\n\nLATER passage, {}:\n{}", open.date, open.text, later.date, later.text)
}

/// The form a reply is held to, per part.
pub(crate) fn schema(part: Part) -> serde_json::Value {
    let (verdicts, other) = match part {
        Part::Judge => (["REPLACES", "BOTH STAND"], "now"),
        Part::Gate => (["settled", "in_progress"], "quote"),
        Part::Done => (["DONE", "STILL OPEN"], "quote"),
    };
    serde_json::json!({ "type": "object", "properties": { "verdict": { "type": "string", "enum": verdicts }, other: { "type": "string" } }, "required": ["verdict", other] })
}

/// One example as it is asked: its id, its part, what is expected, and
/// the two texts of the request.
pub(crate) struct Asked {
    pub id: String,
    pub part: Part,
    pub expected_yes: bool,
    pub system: &'static str,
    pub user: String,
}

/// Every example of a set, in an order that depends on `seed` only (each
/// run shuffles, so a verdict that depends on the order shows itself).
pub(crate) fn asked(set: &CheckSet, seed: u64) -> Vec<Asked> {
    let mut all: Vec<Asked> = Vec::new();
    for p in &set.pairs {
        all.push(Asked { id: p.id.clone(), part: Part::Judge, expected_yes: p.expect == "REPLACES", system: JUDGE_SYSTEM, user: judge_user(&p.earlier, &p.later) });
    }
    for e in &set.exchanges {
        all.push(Asked { id: e.id.clone(), part: Part::Gate, expected_yes: e.expect == "settled", system: GATE_SYSTEM, user: gate_user(&e.exchange) });
    }
    for d in &set.done {
        all.push(Asked { id: d.id.clone(), part: Part::Done, expected_yes: d.expect == "DONE", system: DONE_SYSTEM, user: done_user(&d.open, &d.later) });
    }
    let mut state = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    for i in (1..all.len()).rev() {
        state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        all.swap(i, ((state >> 33) % (i as u64 + 1)) as usize);
    }
    all
}

/// One run of the check on the loaded model: every example asked once.
/// Answers with the verdict and, per example, what was said.
pub(crate) async fn run(state: &crate::llm::LLMState, seed: u64) -> Result<(Verdict, Vec<serde_json::Value>), String> {
    let set = shipped_set();
    let mut answers = Vec::new();
    let mut said = Vec::new();
    for a in asked(&set, seed) {
        let reply = crate::llm::fixed_form_once(state, a.system, &a.user, schema(a.part), 0.2, 300, Some(seed)).await?;
        let said_yes = read_answer(a.part, &reply);
        said.push(serde_json::json!({ "id": a.id, "part": a.part, "expected_yes": a.expected_yes, "said_yes": said_yes, "reply": reply.chars().take(300).collect::<String>() }));
        answers.push(Answered { part: a.part, expected_yes: a.expected_yes, said_yes });
    }
    Ok((verdict(set.v, &answers), said))
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
    fn the_shipped_set_is_the_size_and_shape_the_marks_assume() {
        let set = shipped_set();
        assert_eq!(set.v, 2);
        assert_eq!((set.pairs.len(), set.exchanges.len(), set.done.len()), (30, 20, 10));
        assert_eq!(set.pairs.iter().filter(|p| p.expect == "REPLACES").count(), 12);
        assert_eq!(set.pairs.iter().filter(|p| p.expect == "BOTH STAND").count(), 18);
        assert_eq!(set.exchanges.iter().filter(|e| e.expect == "settled").count(), 10);
        assert_eq!(set.exchanges.iter().filter(|e| e.expect == "in_progress").count(), 10);
        assert!(set.done.iter().all(|d| d.expect == "DONE" || d.expect == "STILL OPEN"));
        // Every expected answer is one the reader understands, and ids are not repeated.
        for p in &set.pairs {
            assert!(read_answer(Part::Judge, &format!("{{\"verdict\":\"{}\"}}", p.expect)).is_some(), "{}", p.id);
        }
        for e in &set.exchanges {
            assert!(read_answer(Part::Gate, &format!("{{\"verdict\":\"{}\"}}", e.expect)).is_some(), "{}", e.id);
        }
        for d in &set.done {
            assert!(read_answer(Part::Done, &format!("{{\"verdict\":\"{}\"}}", d.expect)).is_some(), "{}", d.id);
        }
        let mut ids: Vec<&str> = set.pairs.iter().map(|p| p.id.as_str()).chain(set.exchanges.iter().map(|e| e.id.as_str())).chain(set.done.iter().map(|d| d.id.as_str())).collect();
        let n = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n);
        // As long as real passages are: a side of a pair 600 characters or more, an exchange 800 or more.
        for p in &set.pairs {
            for side in [&p.earlier, &p.later] {
                assert!(side.text.chars().count() >= 600, "{}: {} characters", p.id, side.text.chars().count());
                assert!(side.date.len() == 10 && side.text.contains("They said: "));
            }
            assert!(p.earlier.date < p.later.date, "{}: the earlier passage is the earlier one", p.id);
        }
        for e in &set.exchanges {
            assert!(e.exchange.text.chars().count() >= 800, "{}", e.id);
        }
        // Two languages.
        assert!(set.exchanges.iter().filter(|e| e.exchange.text.contains("español") || e.exchange.text.contains("verano") || e.exchange.text.contains("festivo")).count() >= 3);
        // Made-up text only: nothing in it is taken for a password or key.
        for text in set.pairs.iter().flat_map(|p| [&p.earlier.text, &p.later.text]).chain(set.exchanges.iter().map(|e| &e.exchange.text)) {
            assert_eq!(crate::secret_mask::hand_on(text).hidden, 0, "{}", &text[..80.min(text.len())]);
        }
    }

    #[test]
    fn every_example_is_asked_once_in_an_order_the_seed_sets() {
        let set = shipped_set();
        let (a, b, c) = (asked(&set, 1), asked(&set, 1), asked(&set, 2));
        assert_eq!(a.len(), 60);
        assert_eq!(a.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), b.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), "the same seed, the same order");
        assert_ne!(a.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), c.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), "another seed, another order");
        let mut ids: Vec<&str> = c.iter().map(|x| x.id.as_str()).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 60);
        // The parts are mixed together, not asked in blocks.
        assert!(a.windows(2).filter(|w| w[0].part != w[1].part).count() > 15);
        let one = a.iter().find(|x| x.id == "R-labour-rate").unwrap();
        assert!(one.expected_yes && one.user.starts_with("EARLIER passage, 2025-11-14:\n") && one.user.contains("\n\nLATER passage, 2026-03-02:\n"));
        assert_eq!(schema(Part::Judge)["properties"]["verdict"]["enum"], serde_json::json!(["REPLACES", "BOTH STAND"]));
        assert_eq!(schema(Part::Gate)["required"], serde_json::json!(["verdict", "quote"]));
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
