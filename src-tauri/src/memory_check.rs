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

/// The shipped examples: made-up conversations of one shop, each passage
/// as long and as messy as a real one. The set carries its own version.
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

/// The shipped set's version.
pub(crate) fn set_version() -> u32 {
    static V: std::sync::OnceLock<u32> = std::sync::OnceLock::new();
    *V.get_or_init(|| shipped_set().v)
}

// ── what a model was found able to do, kept on this computer ────────────

const VERDICTS_FILE: &str = "memory-check.json";

/// A model's result on this computer: what it may do for memory.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Stored {
    /// The set's version the result is for.
    pub set: u32,
    /// The model file's hash when it was checked: a replaced file under
    /// the same name is checked again.
    #[serde(default)]
    pub sha256: Option<String>,
    pub may_tidy: bool,
    pub may_keep: bool,
    pub may_close: bool,
    /// Right answers on the judging part, in its worst run: between two
    /// of the person's devices, the better judge tidies.
    pub judge_right: u32,
    /// The marks of each run, as they are shown.
    #[serde(default)]
    pub runs: Vec<serde_json::Value>,
    pub at: i64,
}

/// What several runs of the check let a model do: only what every run
/// allowed, and nothing at all unless the runs agree (three at least).
pub(crate) fn settled(runs: &[Verdict], sha256: Option<String>, at: i64) -> Option<Stored> {
    let first = runs.first()?;
    let agree = steady(runs);
    let all = |f: fn(&Verdict) -> bool| agree && runs.iter().all(f);
    let judge_right = runs.iter().filter_map(|v| v.parts.iter().find(|p| p.part == Part::Judge).map(|p| p.right as u32)).min().unwrap_or(0);
    Some(Stored {
        set: first.set,
        sha256,
        may_tidy: all(|v| v.may_tidy),
        may_keep: all(|v| v.may_keep),
        may_close: all(|v| v.may_close),
        judge_right,
        runs: runs.iter().map(|v| serde_json::to_value(v).unwrap_or_default()).collect(),
        at,
    })
}

fn verdicts_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    app.path().app_data_dir().ok().map(|d| d.join(VERDICTS_FILE))
}

fn all_stored(app: &tauri::AppHandle) -> std::collections::HashMap<String, Stored> {
    verdicts_path(app).and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

/// Is a kept result still good? For this version of the set, and for the
/// file as it is now.
pub(crate) fn still_good(s: &Stored, set: u32, sha256_now: Option<&str>) -> bool {
    s.set == set && s.sha256.as_deref() == sha256_now
}

/// The result kept for a model, when it is for the shipped set and the
/// file as it is now.
pub(crate) fn stored(app: &tauri::AppHandle, model: &str) -> Option<Stored> {
    let s = all_stored(app).remove(model)?;
    still_good(&s, set_version(), crate::model_hash::get(app, model).as_deref()).then_some(s)
}

/// Keep a model's result.
pub(crate) fn store(app: &tauri::AppHandle, model: &str, result: Stored) {
    let mut all = all_stored(app);
    all.insert(model.to_string(), result);
    if let (Some(p), Ok(json)) = (verdicts_path(app), serde_json::to_vec_pretty(&all)) {
        let _ = std::fs::write(p, json);
    }
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

// ── the check, run by the app while the computer is idle ────────────────

const PROGRESS_FILE: &str = "memory-check-progress.json";
/// The job's name in the machine's list of held work.
const JOB: &str = "checking a model for memory work";
/// A whole check is this many runs of the set, each in another order.
pub(crate) const RUNS: usize = 3;

/// One answer of a check in progress.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Got {
    pub part: Part,
    pub expected_yes: bool,
    pub said_yes: Option<bool>,
}

/// A check that is under way: kept after every answer, so a computer
/// that is rarely idle still finishes it, a few examples at a time.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Progress {
    pub model: String,
    pub set: u32,
    #[serde(default)]
    pub sha256: Option<String>,
    /// The answers so far of each run, in the order that run asks.
    pub runs: Vec<Vec<Got>>,
}

/// Which example comes next: the run (0 to 2) and the place in its order.
/// `None` when every run is whole.
pub(crate) fn next_to_ask(p: &Progress, per_run: usize) -> Option<(usize, usize)> {
    (0..RUNS).find_map(|run| {
        let done = p.runs.get(run).map_or(0, Vec::len);
        (done < per_run).then_some((run, done))
    })
}

/// Examples answered so far, and how many a whole check asks.
pub(crate) fn asked_so_far(p: &Progress, per_run: usize) -> (usize, usize) {
    (p.runs.iter().take(RUNS).map(|r| r.len().min(per_run)).sum(), RUNS * per_run)
}

/// The result of a check whose runs are whole.
pub(crate) fn result_of(p: &Progress, at: i64) -> Option<Stored> {
    let runs: Vec<Verdict> = p.runs.iter().take(RUNS).map(|r| verdict(p.set, &r.iter().map(|g| Answered { part: g.part, expected_yes: g.expected_yes, said_yes: g.said_yes }).collect::<Vec<_>>())).collect();
    settled(&runs, p.sha256.clone(), at)
}

fn progress_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    use tauri::Manager;
    app.path().app_data_dir().ok().map(|d| d.join(PROGRESS_FILE))
}

fn read_progress(app: &tauri::AppHandle) -> Option<Progress> {
    progress_path(app).and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok())
}

fn write_progress(app: &tauri::AppHandle, p: &Progress) {
    if let (Some(path), Ok(json)) = (progress_path(app), serde_json::to_vec(p)) {
        let _ = std::fs::write(path, json);
    }
}

/// What the check line shows.
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct CheckStatus {
    /// The model that is loaded on this computer, when one is.
    pub model: Option<String>,
    /// Its result, when it has been checked.
    pub result: Option<Stored>,
    /// A check under way for it: answers so far, and how many there are.
    pub done: usize,
    pub total: usize,
}

#[tauri::command]
pub async fn memory_check_status(app: tauri::AppHandle) -> Result<CheckStatus, String> {
    use tauri::Manager;
    let model = app.state::<crate::llm::LLMState>().current_model.lock().await.clone();
    let Some(m) = model.clone() else { return Ok(CheckStatus::default()) };
    let per_run = {
        let set = shipped_set();
        set.pairs.len() + set.exchanges.len() + set.done.len()
    };
    let (done, total) = read_progress(&app).filter(|p| p.model == m && p.set == set_version()).map_or((0, RUNS * per_run), |p| asked_so_far(&p, per_run));
    Ok(CheckStatus { model, result: stored(&app, &m), done, total })
}

/// Development builds only: the replacement judgment of every pair of the
/// shipped set, asked once with another wording and form, so a wording
/// can be measured before it is made the app's. The form's `verdict`
/// field is read as the shipped one is. Answers, per pair: its id, what
/// was expected, what was said, and the reply.
#[cfg(debug_assertions)]
pub(crate) async fn judge_trial(state: &crate::llm::LLMState, system: &str, schema: serde_json::Value, seed: u64, max_tokens: u32) -> Result<Vec<serde_json::Value>, String> {
    let set = shipped_set();
    let mut out = Vec::new();
    for a in asked(&set, seed).into_iter().filter(|a| a.part == Part::Judge) {
        let reply = crate::llm::fixed_form_once(state, system, &a.user, schema.clone(), 0.2, max_tokens, Some(seed)).await?;
        out.push(serde_json::json!({ "id": a.id, "expected_yes": a.expected_yes, "said_yes": read_answer(Part::Judge, &reply), "reply": reply }));
    }
    Ok(out)
}

/// How long the check waits for a model file's hash before it runs without one.
const HASH_WAIT: std::time::Duration = std::time::Duration::from_secs(1800);
/// The model whose hash is waited for, and since when.
static HASH_WAITED: std::sync::Mutex<Option<(String, std::time::Instant)>> = std::sync::Mutex::new(None);

/// Has this model's hash been waited for long enough? The wait starts the
/// first time it is asked about a model, and again for another model.
fn waited_long_enough_for_a_hash(model: &str, now: std::time::Instant) -> bool {
    let mut held = HASH_WAITED.lock().unwrap_or_else(|e| e.into_inner());
    match held.as_ref() {
        Some((m, since)) if m == model => now.duration_since(*since) >= HASH_WAIT,
        _ => {
            *held = Some((model.to_string(), now));
            false
        }
    }
}

/// One stretch of the check: as many examples as the computer's idle time
/// gives. Answers how long to wait before looking again.
async fn stretch(app: &tauri::AppHandle) -> std::time::Duration {
    use std::time::Duration;
    use tauri::Manager;
    let llm = app.state::<crate::llm::LLMState>();
    let Some(model) = llm.current_model.lock().await.clone() else { return Duration::from_secs(300) };
    // Only a model that runs on this computer is checked, or ever judges.
    if !model.to_lowercase().ends_with(".gguf") || stored(app, &model).is_some() {
        return Duration::from_secs(600);
    }
    if !crate::machine::may_run(crate::machine::Work::Heavy, JOB) {
        return Duration::from_secs(120);
    }
    let set = shipped_set();
    let per_run = set.pairs.len() + set.exchanges.len() + set.done.len();
    // A result is kept for the file as it is, by its hash, and the hash is
    // made some minutes after launch. A check begun before it is known
    // would keep a result that reads as stale the moment the hash exists,
    // and the whole check would run a second time. So it waits for the
    // hash, and goes without one only when none came in half an hour.
    let sha256 = crate::model_hash::get(app, &model);
    if sha256.is_none() && !waited_long_enough_for_a_hash(&model, std::time::Instant::now()) {
        return Duration::from_secs(60);
    }
    let mut progress = read_progress(app).filter(|p| p.model == model && p.set == set.v && p.sha256 == sha256).unwrap_or_else(|| Progress { model: model.clone(), set: set.v, sha256: sha256.clone(), runs: Vec::new() });
    let mut order: Option<(usize, Vec<Asked>)> = None;
    while let Some((run, at)) = next_to_ask(&progress, per_run) {
        // The person is back, or another model was loaded: the rest waits.
        if !crate::machine::would_run(crate::machine::Work::Heavy) || llm.current_model.lock().await.as_deref() != Some(model.as_str()) {
            break;
        }
        if order.as_ref().is_none_or(|(r, _)| *r != run) {
            order = Some((run, asked(&set, run as u64 + 1)));
        }
        let Some(a) = order.as_ref().and_then(|(_, all)| all.get(at)) else { break };
        let Ok(reply) = crate::llm::fixed_form_once(&llm, a.system, &a.user, schema(a.part), 0.2, 300, Some(run as u64 + 1)).await else { break };
        while progress.runs.len() <= run {
            progress.runs.push(Vec::new());
        }
        progress.runs[run].push(Got { part: a.part, expected_yes: a.expected_yes, said_yes: read_answer(a.part, &reply) });
        write_progress(app, &progress);
    }
    crate::machine::done(JOB);
    if next_to_ask(&progress, per_run).is_some() {
        return Duration::from_secs(60);
    }
    let at = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    if let Some(result) = result_of(&progress, at) {
        log::info!("[memory check] {model}: may tidy {}, may keep {}, may close {}", result.may_tidy, result.may_keep, result.may_close);
        store(app, &model, result);
        if let Some(p) = progress_path(app) {
            let _ = std::fs::remove_file(p);
        }
        // What waited for a checked model can go on.
        crate::memory_tidying::wake();
    }
    Duration::from_secs(600)
}

/// Start the standing job: the loaded model is checked once, in idle time.
pub fn start(app: &tauri::AppHandle) {
    // A development build runs it only when asked to (`YOAI_MEMORY_CHECK`
    // set, by a test of the check itself): three runs of sixty judgments
    // would sit in the middle of whatever else is being measured.
    if cfg!(debug_assertions) && std::env::var("YOAI_MEMORY_CHECK").is_err() {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(180)).await;
        loop {
            let wait = stretch(&app).await;
            tokio::time::sleep(wait).await;
        }
    });
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
    fn the_check_waits_for_a_files_hash_and_then_goes_without() {
        let t0 = std::time::Instant::now();
        assert!(!waited_long_enough_for_a_hash("wait-test-a.gguf", t0), "the wait starts");
        assert!(!waited_long_enough_for_a_hash("wait-test-a.gguf", t0 + HASH_WAIT - std::time::Duration::from_secs(1)));
        assert!(waited_long_enough_for_a_hash("wait-test-a.gguf", t0 + HASH_WAIT));
        // Another model starts its own wait.
        assert!(!waited_long_enough_for_a_hash("wait-test-b.gguf", t0 + HASH_WAIT));
        // A result kept with no hash is stale once the file has one: the reason for the wait.
        let s = settled(&[], None, 0);
        assert!(s.is_none());
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
    fn a_model_may_do_only_what_every_run_allowed_and_only_when_the_runs_agree() {
        let good = verdict(3, &[answers(Part::Judge, 12, 12, 18, 0), answers(Part::Gate, 10, 9, 10, 1), answers(Part::Done, 5, 5, 5, 0)].concat());
        let fewer = verdict(3, &[answers(Part::Judge, 12, 10, 18, 1), answers(Part::Gate, 10, 9, 10, 1), answers(Part::Done, 5, 5, 5, 0)].concat());
        let no_judge = verdict(3, &[answers(Part::Judge, 12, 12, 18, 4), answers(Part::Gate, 10, 9, 10, 1), answers(Part::Done, 5, 5, 5, 0)].concat());
        let s = settled(&[good.clone(), fewer.clone(), good.clone()], Some("abc".into()), 5).unwrap();
        assert_eq!((s.set, s.may_tidy, s.may_keep, s.may_close, s.runs.len(), s.at), (3, true, true, true, 3, 5));
        assert_eq!(s.judge_right, 27, "the worst run's count: 10 caught and 17 left alone");
        // Runs that disagree settle nothing, on any part.
        let s = settled(&[good.clone(), no_judge.clone(), good.clone()], None, 5).unwrap();
        assert_eq!((s.may_tidy, s.may_keep, s.may_close), (false, false, false));
        // Runs that agree on failing the judging still allow what passed.
        let s = settled(&[no_judge.clone(), no_judge.clone(), no_judge.clone()], None, 5).unwrap();
        assert_eq!((s.may_tidy, s.may_keep, s.may_close), (false, true, true));
        // Fewer than three runs settle nothing; no run at all is no result.
        assert!(!settled(&[good.clone(), good.clone()], None, 5).unwrap().may_tidy);
        assert!(settled(&[], None, 5).is_none());
        // A result is for one version of the set and one file.
        let s = settled(&[good.clone(), good.clone(), good], Some("abc".into()), 5).unwrap();
        assert!(still_good(&s, 3, Some("abc")));
        assert!(!still_good(&s, 4, Some("abc")), "another set: checked again");
        assert!(!still_good(&s, 3, Some("def")), "another file under the same name: checked again");
        assert!(!still_good(&s, 3, None));
        assert_eq!(set_version(), shipped_set().v);
    }

    #[test]
    fn a_check_goes_on_from_where_it_stopped_and_ends_in_a_result() {
        let got = |part, expected_yes, said: bool| Got { part, expected_yes, said_yes: Some(said) };
        // A set of 4 examples a run: two pairs, one exchange, one done-pair.
        let run = |wrong_replaces: bool| vec![got(Part::Judge, true, true), got(Part::Judge, false, wrong_replaces), got(Part::Gate, true, true), got(Part::Done, false, false)];
        let mut p = Progress { model: "m.gguf".into(), set: 3, sha256: Some("abc".into()), runs: Vec::new() };
        assert_eq!(next_to_ask(&p, 4), Some((0, 0)));
        assert_eq!(asked_so_far(&p, 4), (0, 12));
        p.runs.push(run(false)[..3].to_vec());
        assert_eq!(next_to_ask(&p, 4), Some((0, 3)), "stopped mid-run: the next example of that run");
        p.runs[0] = run(false);
        assert_eq!(next_to_ask(&p, 4), Some((1, 0)));
        p.runs.push(run(false));
        p.runs.push(run(false)[..1].to_vec());
        assert_eq!((next_to_ask(&p, 4), asked_so_far(&p, 4)), (Some((2, 1)), (9, 12)));
        p.runs[2] = run(false);
        assert_eq!(next_to_ask(&p, 4), None);
        // What was kept reads back the same.
        let back: Progress = serde_json::from_slice(&serde_json::to_vec(&p).unwrap()).unwrap();
        assert_eq!(back, p);
        let r = result_of(&p, 9).unwrap();
        assert_eq!((r.set, r.sha256.as_deref(), r.runs.len(), r.at), (3, Some("abc"), 3, 9));
        assert!(r.may_keep && r.may_close);
        // A run that disagrees with the others settles nothing.
        p.runs[1] = vec![got(Part::Judge, true, true), got(Part::Judge, false, false), got(Part::Gate, true, false), got(Part::Done, false, false)];
        let r = result_of(&p, 9).unwrap();
        assert!(!r.may_keep && !r.may_tidy && !r.may_close);
    }

    #[test]
    fn the_shipped_set_is_the_size_and_shape_the_marks_assume() {
        let set = shipped_set();
        assert_eq!(set.v, 3);
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
