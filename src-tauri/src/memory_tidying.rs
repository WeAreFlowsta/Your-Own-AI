//! The tidying job. When something is kept, it waits here; when the
//! computer is idle, a model that passed the built-in check is asked, of
//! the new item and each current item close to it in meaning, whether the
//! later replaces the earlier. What it answers is applied by the rules in
//! `memory_tidy`: the older item is marked replaced and kept as history,
//! or (when the person confirmed it) the person is asked, or nothing
//! changes. Every change goes in the change log with its undo, and every
//! judgment in the ledger, so no pair is judged twice on any device.
//!
//! Nothing here runs during the person's turn, on a model that has not
//! passed, while tidying is off or paused, or on a device that does not
//! hold the lease.

use crate::memory_check::{Part, Passage};
use crate::memory_items::{Item, ItemState};
use crate::memory_tidy::{self as tidy, Outcome, Verdict};
use serde::Serialize;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;
use tauri::{AppHandle, Manager};

/// The job's name in the machine's list of held work.
const JOB: &str = "tidying memory";
/// Items waiting to be looked at, by id: this device's own list.
const QUEUE_FILE: &str = "memory-tidy-queue.json";
/// This device's name in the lease: random, and known to nothing else.
const DEVICE_FILE: &str = "memory-device-id";
/// Two items are judged only when they are at least this alike: a pair
/// about different matters is not worth a judgment, or its risk.
pub(crate) const PAIR_FLOOR: f32 = 0.5;

static WAKE: tokio::sync::Notify = tokio::sync::Notify::const_new();
/// One pass at a time.
static RUNNING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn queue_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(crate::profile::root(app)?.join(QUEUE_FILE))
}

fn read_queue(app: &AppHandle) -> Vec<String> {
    queue_path(app).ok().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write_queue(app: &AppHandle, queue: &[String]) {
    if let (Ok(p), Ok(json)) = (queue_path(app), serde_json::to_vec(queue)) {
        let _ = flowsta_hc_lines::files::write_atomic_private(&p, &json);
    }
}

/// No item waits to be tidied any more (all memory was deleted).
pub(crate) fn clear_queue(app: &AppHandle) {
    write_queue(app, &[]);
}

/// The queue with one more item, once.
pub(crate) fn queued(mut queue: Vec<String>, id: &str) -> Vec<String> {
    if !queue.iter().any(|q| q == id) {
        queue.push(id.to_string());
    }
    queue
}

/// An item was kept: it waits for tidying, and its vector is made now.
pub(crate) fn kept(app: &AppHandle, id: &str) {
    write_queue(app, &queued(read_queue(app), id));
    words_changed(app);
    WAKE.notify_one();
}

/// Something tidying waited for may have changed (a model passed the
/// check): look again now.
pub(crate) fn wake() {
    WAKE.notify_one();
}

/// An item's words changed, or a new one arrived: make the vectors that
/// are missing. One short text each, so it does not wait for idle time.
pub(crate) fn words_changed(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::memory_state::fill_vectors(&app).await {
            log::info!("[memory tidying] vectors not made yet: {e}");
        }
    });
}

fn device(app: &AppHandle) -> Result<String, String> {
    let path = crate::profile::root(app)?.join(DEVICE_FILE);
    if let Ok(id) = std::fs::read_to_string(&path) {
        if !id.trim().is_empty() {
            return Ok(id.trim().to_string());
        }
    }
    let id = {
        use rand::Rng as _;
        format!("{:016x}{:016x}", rand::thread_rng().gen::<u64>(), rand::thread_rng().gen::<u64>())
    };
    std::fs::write(&path, &id).map_err(|e| e.to_string())?;
    Ok(id)
}

/// What a pass did, or why it did nothing.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub ran: bool,
    pub why_not: Option<String>,
    pub model: Option<String>,
    /// Pairs put to the model.
    pub judged: u32,
    pub replaced: u32,
    /// The older item was confirmed by the person: they are asked.
    pub asked: u32,
    pub both_stand: u32,
    /// Open things a later item may have finished: put to the person.
    pub may_be_done: u32,
    /// Open things a later item did not finish.
    pub still_open: u32,
    /// Answers that could not be read (nothing changes).
    pub unreadable: u32,
    pub vectors_made: usize,
    pub left_in_queue: usize,
    /// Each judgment, for a test to read: the two items, the verdict.
    pub pairs: Vec<serde_json::Value>,
}

/// A judgment read from the model's reply: the JSON object between the
/// first `{` and the last `}`. Anything else is no answer.
pub(crate) fn read_verdict(reply: &str) -> Option<Verdict> {
    let (a, b) = (reply.find('{')?, reply.rfind('}')?);
    let v: serde_json::Value = serde_json::from_str(reply.get(a..=b)?).ok()?;
    match v.get("verdict")?.as_str()?.trim() {
        "REPLACES" => Some(Verdict::Replaces { now: v.get("now").and_then(|n| n.as_str()).unwrap_or("").split_whitespace().collect::<Vec<_>>().join(" ") }),
        "BOTH STAND" => Some(Verdict::BothStand),
        _ => None,
    }
}

/// The passage a judgment reads for an item: the exchange it came from,
/// as it is on this device, with its date; the item's own words when the
/// exchange is not here.
fn passage_of(app: &AppHandle, item: &Item) -> Passage {
    let said = item.meta.source.ptr.map(|p| crate::memory_reading::exchange_at(app, p.c, p.s)).unwrap_or_default();
    Passage { date: crate::memory_block::day(item.row.as_of * 1000), text: passage_or_words(&said, &item.row.text) }
}

pub(crate) fn passage_or_words(said: &[crate::memory_block::Said], words: &str) -> String {
    if said.is_empty() {
        format!("They said: {}", words.split_whitespace().collect::<Vec<_>>().join(" "))
    } else {
        crate::memory_block::passage_text(said)
    }
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0f64, 0f64, 0f64);
    for (x, y) in a.iter().zip(b) {
        dot += f64::from(*x) * f64::from(*y);
        na += f64::from(*x) * f64::from(*x);
        nb += f64::from(*y) * f64::from(*y);
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    (dot / (na.sqrt() * nb.sqrt())) as f32
}

/// The items alike enough to a new one to be judged against it: each
/// one's id and likeness.
pub(crate) fn near(new: &str, items: &[Item], vectors: &HashMap<String, Vec<f32>>) -> Vec<(String, f32)> {
    let Some(mine) = vectors.get(new) else { return Vec::new() };
    items.iter().filter(|i| i.row.id != new).filter_map(|i| vectors.get(&i.row.id).map(|v| (i.row.id.clone(), cosine(mine, v)))).filter(|(_, s)| *s >= PAIR_FLOOR).collect()
}

/// What still waits after a pass over `taken`: `waiting`, and whatever was
/// kept while the pass ran.
pub(crate) fn still_waiting(taken: &[String], mut waiting: Vec<String>, queue_now: Vec<String>) -> Vec<String> {
    for id in queue_now {
        if !taken.contains(&id) && !waiting.contains(&id) {
            waiting.push(id);
        }
    }
    waiting
}

fn leave_waiting(app: &AppHandle, taken: &[String], waiting: Vec<String>) -> usize {
    let left = still_waiting(taken, waiting, read_queue(app));
    write_queue(app, &left);
    left.len()
}

fn refuse(mut report: Report, why: &str) -> Report {
    report.why_not = Some(why.to_string());
    report
}

/// One pass over the queue. `force` (dev builds' tests only) runs it now,
/// on whatever model is loaded.
pub(crate) async fn pass(app: &AppHandle, force: bool) -> Report {
    let _one = RUNNING.lock().await;
    let queue = read_queue(app);
    let report = Report { left_in_queue: queue.len(), ..Default::default() };
    if queue.is_empty() {
        return refuse(report, "nothing new was kept");
    }
    match run(app, force, queue, report.clone()).await {
        Ok(r) => r,
        Err(e) => refuse(report, &e),
    }
}

async fn run(app: &AppHandle, force: bool, queue: Vec<String>, mut report: Report) -> Result<Report, String> {
    let key = app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get()?.data_key()?;
    let scope = crate::memory_items::SCOPE;
    if tidy::tidy_is_off(&crate::memory_rows::read_cache(app, &key, scope)) {
        return Ok(refuse(report, "tidying is turned off"));
    }
    let llm = app.state::<crate::llm::LLMState>();
    let Some(model) = llm.current_model.lock().await.clone() else { return Ok(refuse(report, "no model is loaded")) };
    report.model = Some(model.clone());
    let checked = crate::memory_check::stored(app, &model);
    if !force && !checked.as_ref().is_some_and(|c| c.may_tidy) {
        return Ok(refuse(report, "the loaded model has not passed the check"));
    }
    if !force && !crate::machine::may_run(crate::machine::Work::Heavy, JOB) {
        return Ok(refuse(report, crate::machine::held_reason(JOB).unwrap_or("the computer is busy")));
    }
    let may_close = checked.as_ref().is_some_and(|c| c.may_close);
    let done = judge_queue(app, force, &key, &llm, queue, checked.map(|c| c.judge_right), may_close, report).await;
    crate::machine::done(JOB);
    done
}

async fn judge_queue(app: &AppHandle, force: bool, key: &[u8; 32], llm: &crate::llm::LLMState, queue: Vec<String>, score: Option<u32>, may_close: bool, mut report: Report) -> Result<Report, String> {
    let scope = crate::memory_items::SCOPE;
    // What other devices judged, undid or turned off since: read first.
    let _ = crate::memory_rows::list_memory_rows(app.clone(), None, scope.to_string(), app.state()).await;
    let rows = crate::memory_rows::read_cache(app, key, scope);
    if tidy::tidy_is_off(&rows) {
        return Ok(refuse(report, "tidying is turned off"));
    }
    let mut ledger = tidy::ledger(&rows);
    if tidy::paused(&ledger, tidy::started_again(&rows)) {
        return Ok(refuse(report, "tidying is paused"));
    }
    // One device tidies at a time.
    let me = device(app)?;
    let held = tidy::lease(&rows);
    match tidy::lease_move(held.as_ref(), &me, score.or(force.then_some(0)), now_ms()) {
        tidy::LeaseMove::Leave => return Ok(refuse(report, "another device tidies")),
        tidy::LeaseMove::Hold(l) => {
            if held.as_ref() != Some(&l) {
                crate::memory_rows::save_and_cache(app, tidy::lease_row(&l, now_ms())).await?;
            }
        }
    }
    report.ran = true;
    report.vectors_made = crate::memory_state::fill_vectors(app).await.unwrap_or(0);
    let vectors: HashMap<String, Vec<f32>> = crate::memory_reading::item_vectors(app).into_iter().map(|(id, (_, q))| (id, crate::memory_index::numbers(&q))).collect();
    let mut items = crate::memory_items::items(&rows);
    let day_start = {
        let now = now_ms();
        now - now.rem_euclid(86_400_000)
    };
    let mut left = tidy::left_today(&ledger, day_start);
    let mut waiting: Vec<String> = Vec::new();
    for (at, id) in queue.iter().enumerate() {
        // The day's changes are made, or the person is back: the rest waits.
        if left == 0 || (!force && !crate::machine::would_run(crate::machine::Work::Heavy)) {
            waiting.extend(queue[at..].iter().cloned());
            break;
        }
        let Some(mut new) = items.iter().find(|i| &i.row.id == id).cloned() else { continue };
        if !matches!(new.meta.state, ItemState::Current | ItemState::Open) {
            continue;
        }
        if !vectors.contains_key(id) {
            // Its vector could not be made yet: next time.
            waiting.push(id.clone());
            continue;
        }
        let others: Vec<String> = tidy::pairs_to_judge(&new, &items, &near(id, &items, &vectors), &ledger).into_iter().map(|i| i.row.id.clone()).collect();
        // The day's last change was made with pairs of this item still to judge.
        let mut unfinished = false;
        for other_id in others {
            // As it is now: an earlier pair of this pass may have changed it.
            let Some(other) = items.iter().find(|i| i.row.id == other_id).cloned() else { continue };
            if !matches!(other.meta.state, ItemState::Current | ItemState::Open) || !matches!(new.meta.state, ItemState::Current | ItemState::Open) {
                continue;
            }
            let (earlier, later) = if (other.row.as_of, &other.row.id) <= (new.row.as_of, &new.row.id) { (&other, &new) } else { (&new, &other) };
            // Something left open, and something later: the question is
            // whether it was done, never whether one replaces the other.
            // The answer only ever proposes: the person closes it.
            if earlier.meta.state == ItemState::Open && later.meta.state == ItemState::Current {
                if !force && !may_close {
                    // This model may not judge it; the pair stays for one that may.
                    continue;
                }
                let user = crate::memory_check::done_user(&passage_of(app, earlier), &passage_of(app, later));
                let reply = match crate::llm::fixed_form_once(llm, crate::memory_check::DONE_SYSTEM, &user, crate::memory_check::schema(Part::Done), 0.2, 300, None).await {
                    Ok(r) => r,
                    Err(e) => {
                        log::info!("[memory tidying] a judgment was not answered: {e}");
                        waiting.extend(queue[at..].iter().cloned());
                        report.left_in_queue = leave_waiting(app, &queue, waiting);
                        return Ok(report);
                    }
                };
                report.judged += 1;
                let Some(done) = crate::memory_check::read_answer(Part::Done, &reply) else {
                    report.unreadable += 1;
                    continue;
                };
                let verdict = if done { tidy::DoneVerdict::Done } else { tidy::DoneVerdict::StillOpen };
                let judged = tidy::done_judged(earlier, later, &verdict, now_ms());
                report.pairs.push(serde_json::json!({ "earlier": earlier.row.id, "later": later.row.id, "verdict": if done { "DONE" } else { "STILL OPEN" } }));
                crate::memory_rows::save_and_cache(app, tidy::ledger_row(&judged)).await?;
                ledger.push(judged);
                if done {
                    report.may_be_done += 1;
                } else {
                    report.still_open += 1;
                }
                continue;
            }
            let user = crate::memory_check::judge_user(&passage_of(app, earlier), &passage_of(app, later));
            let reply = match crate::llm::fixed_form_once(llm, crate::memory_check::JUDGE_SYSTEM, &user, crate::memory_check::schema(Part::Judge), 0.2, 300, None).await {
                Ok(r) => r,
                Err(e) => {
                    // The engine is gone or busy: the item waits.
                    log::info!("[memory tidying] a judgment was not answered: {e}");
                    waiting.extend(queue[at..].iter().cloned());
                    report.left_in_queue = leave_waiting(app, &queue, waiting);
                    return Ok(report);
                }
            };
            report.judged += 1;
            let Some(verdict) = read_verdict(&reply) else {
                report.unreadable += 1;
                continue;
            };
            let verdict = match verdict {
                // What the judgment wrote is kept without secrets, like every item.
                Verdict::Replaces { now } => Verdict::Replaces { now: crate::secret_mask::hand_on(&now).text },
                v => v,
            };
            let now = now_ms();
            let said = if matches!(verdict, Verdict::Replaces { .. }) { "REPLACES" } else { "BOTH STAND" };
            report.pairs.push(serde_json::json!({ "earlier": earlier.row.id, "later": later.row.id, "verdict": said }));
            match tidy::apply(&other, &new, &verdict, now) {
                Outcome::Replaced { older, newer, change, judged } => {
                    let older = crate::memory_rows::save_and_cache(app, older).await?;
                    let newer = crate::memory_rows::save_and_cache(app, newer).await?;
                    crate::memory_items::log_change(app, key, change)?;
                    crate::memory_rows::save_and_cache(app, tidy::ledger_row(&judged)).await?;
                    ledger.push(judged);
                    report.replaced += 1;
                    left = left.saturating_sub(1);
                    for row in [older, newer] {
                        let Some(meta) = crate::memory_items::meta_of(&row) else { continue };
                        let item = Item { row, meta };
                        if item.row.id == new.row.id {
                            new = item.clone();
                        }
                        if let Some(slot) = items.iter_mut().find(|i| i.row.id == item.row.id) {
                            *slot = item;
                        }
                    }
                    if left == 0 {
                        unfinished = true;
                        break;
                    }
                }
                Outcome::Ask { older, now: now_true, judged, .. } => {
                    crate::memory_rows::save_and_cache(app, tidy::asked_row(&judged, &older, &now_true)).await?;
                    ledger.push(judged);
                    report.asked += 1;
                }
                Outcome::Nothing { judged } => {
                    crate::memory_rows::save_and_cache(app, tidy::ledger_row(&judged)).await?;
                    ledger.push(judged);
                    report.both_stand += 1;
                }
            }
        }
        if unfinished {
            waiting.push(id.clone());
        }
    }
    report.left_in_queue = leave_waiting(app, &queue, waiting);
    // The words of what now stands may have changed.
    report.vectors_made += crate::memory_state::fill_vectors(app).await.unwrap_or(0);
    Ok(report)
}

// ── open things later work may have finished ────────────────────────────

/// An open item with the later item that suggests it was done.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MayBeDone {
    pub open: crate::memory_items::ItemView,
    pub later: crate::memory_items::ItemView,
}

/// What "May already be done" lists, oldest open item first.
#[tauri::command]
pub fn memory_may_be_done(app: AppHandle) -> Result<Vec<MayBeDone>, String> {
    let key = app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get()?.data_key()?;
    let rows = crate::memory_rows::read_cache(&app, &key, crate::memory_items::SCOPE);
    let items = crate::memory_items::items(&rows);
    let mut out: Vec<MayBeDone> = tidy::may_be_done(&items, &tidy::ledger(&rows))
        .into_iter()
        .filter_map(|(open, later)| Some(MayBeDone { open: crate::memory_items::view(&open.row)?, later: crate::memory_items::view(&later.row)? }))
        .collect();
    out.sort_by_key(|m| m.open.as_of);
    Ok(out)
}

/// "Still waiting", answered by the person: the suggestion goes, and the
/// same two items are not put together again.
#[tauri::command]
pub async fn memory_still_waiting(app: AppHandle, open: String, later: String) -> Result<(), String> {
    let key = app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get()?.data_key()?;
    let rows = crate::memory_rows::read_cache(&app, &key, crate::memory_items::SCOPE);
    let items = crate::memory_items::items(&rows);
    let (Some(o), Some(l)) = (items.iter().find(|i| i.row.id == open), items.iter().find(|i| i.row.id == later)) else { return Err("no such item".into()) };
    let judged = tidy::done_judged(o, l, &tidy::DoneVerdict::StillOpen, now_ms());
    crate::memory_rows::save_and_cache(&app, tidy::ledger_row(&judged)).await?;
    Ok(())
}

// ── what the tidying line shows, and the person's switches ──────────────

/// Which form the tidying line takes.
pub(crate) fn state_of(off: bool, paused: bool, local_model: bool, result: Option<&crate::memory_check::Stored>, elsewhere: bool) -> &'static str {
    if off {
        // The person turned it off, for all their devices.
        "off"
    } else if paused {
        "paused"
    } else if result.is_some_and(|r| r.may_tidy) {
        "on"
    } else if elsewhere {
        // Another of the person's devices tidies; this one shows its changes.
        "elsewhere"
    } else if !local_model {
        "no-local-model"
    } else if result.is_none() {
        "checking"
    } else {
        "not-passed"
    }
}

/// What the tidying line shows.
#[derive(Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct TidyStatus {
    /// `on` | `off` | `paused` | `checking` | `not-passed` |
    /// `no-local-model` | `elsewhere`.
    pub state: String,
    /// The model loaded on this computer, by its file name.
    pub model: Option<String>,
    /// It passed "was something settled": it keeps what is settled even
    /// when it may not replace anything.
    pub may_keep: bool,
    /// The check's marks, as they are shown ("13 of 30 ...").
    pub result: Option<crate::memory_check::Stored>,
    /// A check under way: answers so far, of how many.
    pub done: usize,
    pub total: usize,
    /// Items waiting to be looked at.
    pub queued: usize,
}

#[tauri::command]
pub async fn memory_tidy_status(app: AppHandle) -> Result<TidyStatus, String> {
    let key = app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get()?.data_key()?;
    let rows = crate::memory_rows::read_cache(&app, &key, crate::memory_items::SCOPE);
    let check = crate::memory_check::memory_check_status(app.clone()).await?;
    let local = check.model.as_deref().is_some_and(|m| m.to_lowercase().ends_with(".gguf"));
    let me = device(&app).unwrap_or_default();
    let elsewhere = tidy::lease(&rows).is_some_and(|l| l.device != me && l.until > now_ms());
    let state = state_of(tidy::tidy_is_off(&rows), tidy::paused(&tidy::ledger(&rows), tidy::started_again(&rows)), local, check.result.as_ref(), elsewhere);
    Ok(TidyStatus { state: state.to_string(), model: check.model, may_keep: check.result.as_ref().is_some_and(|r| r.may_keep), result: check.result, done: check.done, total: check.total, queued: read_queue(&app).len() })
}

/// Turn tidying (and keeping by an AI) off or on, for all the person's
/// devices.
#[tauri::command]
pub async fn memory_tidy_set(app: AppHandle, off: bool) -> Result<(), String> {
    crate::memory_rows::save_and_cache(&app, tidy::tidy_off_row(off, now_ms())).await?;
    if !off {
        wake();
    }
    Ok(())
}

/// "Start again" after a pause: the count of undone changes begins anew.
#[tauri::command]
pub async fn memory_tidy_start_again(app: AppHandle) -> Result<(), String> {
    crate::memory_rows::save_and_cache(&app, tidy::started_again_row(now_ms())).await?;
    wake();
    Ok(())
}

/// Start the standing job: a pass when something was kept and at times
/// after, each only if something waits.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        // The conductor, the stores and the first machine reading come up first.
        tokio::time::sleep(Duration::from_secs(120)).await;
        loop {
            if !read_queue(&app).is_empty() {
                let r = pass(&app, false).await;
                match &r.why_not {
                    Some(why) => log::info!("[memory tidying] waiting: {why} ({} item(s) queued)", r.left_in_queue),
                    None => log::info!("[memory tidying] {} pair(s) judged: {} replaced, {} asked, {} stand, {} unread; {} item(s) still queued", r.judged, r.replaced, r.asked, r.both_stand, r.unreadable, r.left_in_queue),
                }
            }
            let _ = tokio::time::timeout(Duration::from_secs(15 * 60), WAKE.notified()).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_block::{Ptr, Said};
    use crate::memory_items::{keep, meta_of, ItemType, Keep};

    fn item(id: &str, text: &str, as_of: i64) -> Item {
        let (row, _) = keep(&[], Keep { id: id.into(), text, item_type: ItemType::Decision, ptr: Ptr { c: as_of * 1000, s: 0, t: as_of * 1000, a: 0, b: 10 }, as_of, by: "ai:atlas", audience: None, kind: "exchange", why: "settled", quote: None, subject: None, now: as_of + 1000 })
            .ok()
            .unwrap();
        let meta = meta_of(&row).unwrap();
        Item { row, meta }
    }

    #[test]
    fn a_judgment_is_read_from_the_reply_or_not_at_all() {
        assert_eq!(read_verdict("{\"verdict\":\"REPLACES\",\"now\":\"Free delivery  starts\\n at $60.\"}"), Some(Verdict::Replaces { now: "Free delivery starts at $60.".into() }));
        assert_eq!(read_verdict("Here it is: {\"verdict\": \"BOTH STAND\", \"now\": \"\"} done"), Some(Verdict::BothStand));
        assert_eq!(read_verdict("{\"verdict\":\"REPLACES\"}"), Some(Verdict::Replaces { now: String::new() }), "a missing sentence is an empty one");
        assert_eq!(read_verdict("{\"verdict\":\"MAYBE\",\"now\":\"x\"}"), None);
        assert_eq!(read_verdict("REPLACES"), None);
        assert_eq!(read_verdict("{\"verdict\": 3}"), None);
    }

    #[test]
    fn only_items_alike_enough_are_put_beside_a_new_one() {
        let items = vec![item("it-new", "Free delivery over $60", 30), item("it-old", "Free delivery over $40", 10), item("it-oven", "The oven runs hot", 20), item("it-bare", "No vector yet", 5)];
        let vectors: HashMap<String, Vec<f32>> = [("it-new", vec![1.0, 0.0, 0.0]), ("it-old", vec![0.9, 0.3, 0.0]), ("it-oven", vec![0.1, 0.0, 1.0])].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        let close = near("it-new", &items, &vectors);
        assert_eq!(close.len(), 1);
        assert_eq!(close[0].0, "it-old");
        assert!(close[0].1 > 0.9);
        assert!(near("it-bare", &items, &vectors).is_empty(), "an item with no vector is beside nothing");
        // And the rules take it from there: the close one is the pair to judge.
        let pairs = tidy::pairs_to_judge(&items[0], &items, &close, &[]);
        assert_eq!(pairs.iter().map(|i| i.row.id.as_str()).collect::<Vec<_>>(), vec!["it-old"]);
    }

    #[test]
    fn a_passage_is_the_exchange_or_the_items_own_words() {
        let said = vec![Said { by_person: true, text: "ok make it\n60 from Monday".into(), seq: 4, at: 1 }, Said { by_person: false, text: "Set: $60 from Monday March 9.".into(), seq: 5, at: 2 }];
        assert_eq!(passage_or_words(&said, "ignored"), "They said: ok make it 60 from Monday\nAI: Set: $60 from Monday March 9.");
        assert_eq!(passage_or_words(&[], "Free  delivery\nover $60"), "They said: Free delivery over $60");
    }

    #[test]
    fn the_tidying_line_takes_one_form() {
        use crate::memory_check::Stored;
        let result = |tidy: bool, keep: bool| Stored { set: 3, sha256: None, may_tidy: tidy, may_keep: keep, may_close: false, judge_right: 20, runs: Vec::new(), at: 1 };
        let (passed, failed) = (result(true, true), result(false, true));
        assert_eq!(state_of(false, false, true, Some(&passed), false), "on");
        assert_eq!(state_of(true, false, true, Some(&passed), false), "off", "the person's switch comes first");
        assert_eq!(state_of(false, true, true, Some(&passed), false), "paused");
        assert_eq!(state_of(false, false, true, None, false), "checking");
        assert_eq!(state_of(false, false, true, Some(&failed), false), "not-passed");
        assert_eq!(state_of(false, false, false, None, false), "no-local-model");
        // Another of the person's devices tidies: this one says so only when it cannot itself.
        assert_eq!(state_of(false, false, true, Some(&failed), true), "elsewhere");
        assert_eq!(state_of(false, false, false, None, true), "elsewhere");
        assert_eq!(state_of(false, false, true, Some(&passed), true), "on");
    }

    #[test]
    fn an_item_is_queued_once() {
        let q = queued(queued(queued(Vec::new(), "it-a"), "it-b"), "it-a");
        assert_eq!(q, vec!["it-a", "it-b"]);
    }

    #[test]
    fn what_was_kept_during_a_pass_still_waits_after_it() {
        let taken = vec!["it-a".to_string(), "it-b".to_string()];
        // The pass finished both; one more was kept while it ran.
        assert_eq!(still_waiting(&taken, Vec::new(), vec!["it-a".into(), "it-b".into(), "it-c".into()]), vec!["it-c"]);
        // The pass left one of its own waiting: it stays, once, with the new one.
        assert_eq!(still_waiting(&taken, vec!["it-b".into()], vec!["it-a".into(), "it-b".into(), "it-c".into()]), vec!["it-b", "it-c"]);
        assert!(still_waiting(&taken, Vec::new(), taken.clone()).is_empty());
    }
}
