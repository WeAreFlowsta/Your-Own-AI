//! Tidying: when something newer replaces something memory holds, the
//! older item is marked replaced and kept as history, by a model that has
//! passed the built-in check, in idle time. Every change is shown and can
//! be undone, an undo sticks, undos pause tidying, and what the person
//! confirmed is never replaced without asking.
//!
//! This file is the rules: which pairs are judged, what a verdict
//! changes, the ledger of what was judged and undone (kept as rows, so it
//! is the same on every device), the pause, and the daily cap. The model
//! call that gives a verdict is made elsewhere and handed in.

use crate::memory_items::{Change, Item, ItemState, What};
use crate::memory_rows::MemoryRow;
use serde::{Deserialize, Serialize};

/// A new item is compared with at most this many current items.
pub(crate) const PAIRS_PER_ITEM: usize = 5;
/// Tidying makes at most this many changes a day.
pub(crate) const CHANGES_A_DAY: usize = 20;
/// Tidying pauses when this many of its last changes were undone.
const PAUSE_UNDONE: usize = 3;
const PAUSE_OF_LAST: usize = 5;

/// What the judging model said of an earlier and a later item.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Verdict {
    /// The later one changes what was true in the earlier one; with the
    /// sentence of what is true now.
    Replaces { now: String },
    BothStand,
}

/// One entry of the ledger, kept as a system row (`structured.sys` =
/// `undo`) so every device has it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Judged {
    /// The two items, the smaller id first.
    pub pair: (String, String),
    /// `both_stand` | `replaced` | `closed` | `undone` | `asked`.
    pub outcome: String,
    pub at: i64,
    /// For `undone`: the outcome that was undone (`replaced` or `closed`
    /// count toward the pause; undoing a keep does not).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub of: Option<String>,
}

fn pair_of(a: &str, b: &str) -> (String, String) {
    if a <= b { (a.to_string(), b.to_string()) } else { (b.to_string(), a.to_string()) }
}

/// The ledger entry as the row that is saved.
pub(crate) fn ledger_row(j: &Judged) -> MemoryRow {
    MemoryRow {
        id: format!("sys-judged-{}-{}-{}", j.pair.0, j.pair.1, j.at),
        scope: crate::memory_items::SCOPE.to_string(),
        kind: "note".to_string(),
        text: String::new(),
        structured: Some(serde_json::json!({ "sys": "undo", "judged": j })),
        as_of: j.at,
        valid_to: None,
        saved_by: "person".to_string(),
        provenance: "authored".to_string(),
        pinned: false,
        topics: Vec::new(),
        replaces: Vec::new(),
        deleted: false,
        updated_at: j.at,
        action_hash: None,
    }
}

/// The ledger, read from the scope's rows (from every device).
pub(crate) fn ledger(rows: &[MemoryRow]) -> Vec<Judged> {
    let mut out: Vec<Judged> = rows
        .iter()
        .filter(|r| !r.deleted)
        .filter_map(|r| {
            let s = r.structured.as_ref()?;
            (s.get("sys")?.as_str()? == "undo").then(|| serde_json::from_value(s.get("judged")?.clone()).ok()).flatten()
        })
        .collect();
    out.sort_by(|a, b| a.at.cmp(&b.at).then(a.pair.cmp(&b.pair)));
    out
}

/// May this pair be judged? Not when it was judged before (whatever the
/// outcome), and never again once the person undid what tidying did to it.
pub(crate) fn may_judge(ledger: &[Judged], a: &str, b: &str) -> bool {
    let pair = pair_of(a, b);
    !ledger.iter().any(|j| j.pair == pair)
}

/// The pairs to judge for a newly kept item: the current items closest to
/// it in meaning (`near`: item id and likeness, any order), the closest
/// first, at most five, leaving out pairs the ledger settles. Tidying
/// never depends on subjects.
pub(crate) fn pairs_to_judge<'a>(new: &Item, held: &'a [Item], near: &[(String, f32)], ledger: &[Judged]) -> Vec<&'a Item> {
    let mut near: Vec<&(String, f32)> = near.iter().collect();
    near.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal).then(a.0.cmp(&b.0)));
    near.into_iter()
        .filter_map(|(id, _)| held.iter().find(|i| &i.row.id == id))
        .filter(|i| i.row.id != new.row.id)
        .filter(|i| matches!(i.meta.state, ItemState::Current | ItemState::Open))
        .filter(|i| may_judge(ledger, &i.row.id, &new.row.id))
        .take(PAIRS_PER_ITEM)
        .collect()
}

/// What a verdict on a pair leads to.
#[derive(Debug, PartialEq)]
pub(crate) enum Outcome {
    /// The older item is marked replaced, the newer one carries what is
    /// true now. The rows to save, the change for the log, the ledger entry.
    Replaced { older: MemoryRow, newer: MemoryRow, change: Change, judged: Judged },
    /// The older item was confirmed by the person: nothing changes; the
    /// person is asked, and may leave the question unanswered for good.
    Ask { older: String, newer: String, now: String, judged: Judged },
    /// Both stand: nothing changes, and the pair is not judged again.
    Nothing { judged: Judged },
}

/// Earlier and later by the date of what was said, not of when it was kept.
fn in_order<'a>(a: &'a Item, b: &'a Item) -> (&'a Item, &'a Item) {
    if (a.row.as_of, &a.row.id) <= (b.row.as_of, &b.row.id) { (a, b) } else { (b, a) }
}

/// The newer item's row with what is true now said on it.
fn says_now(newer: &Item, now_true: &str, now: i64) -> MemoryRow {
    let mut meta = newer.meta.clone();
    meta.now = Some(now_true.to_string());
    let mut row = newer.row.clone();
    row.updated_at = now;
    row.action_hash = None;
    let mut structured = match row.structured.take() {
        Some(serde_json::Value::Object(o)) => o,
        _ => serde_json::Map::new(),
    };
    structured.insert("item".into(), serde_json::to_value(&meta).unwrap_or_default());
    row.structured = Some(serde_json::Value::Object(structured));
    row
}

/// Apply a verdict to a pair of items.
pub(crate) fn apply(a: &Item, b: &Item, verdict: &Verdict, now: i64) -> Outcome {
    let (older, newer) = in_order(a, b);
    let pair = pair_of(&older.row.id, &newer.row.id);
    let judged = |outcome: &str| Judged { pair: pair.clone(), outcome: outcome.to_string(), at: now, of: None };
    match verdict {
        Verdict::BothStand => Outcome::Nothing { judged: judged("both_stand") },
        Verdict::Replaces { now: now_true } if older.meta.confirmed.is_some() => Outcome::Ask { older: older.row.id.clone(), newer: newer.row.id.clone(), now: now_true.clone(), judged: judged("asked") },
        Verdict::Replaces { now: now_true } => {
            let (older_row, mut change) = crate::memory_items::replace(older, &newer.row.id, None, "tidy", now);
            let newer_row = says_now(newer, now_true, now);
            change.items = vec![older.row.id.clone(), newer.row.id.clone()];
            Outcome::Replaced { older: older_row, newer: newer_row, change, judged: judged("replaced") }
        }
    }
}

/// The ledger entry for an undo of a change tidying made, so the pair is
/// never judged again on any device. `None` when the change is not one of
/// tidying's on a pair.
pub(crate) fn undone(change: &Change, now: i64) -> Option<Judged> {
    let of = match change.what {
        What::Replaced => "replaced",
        What::Closed => "closed",
        _ => return None,
    };
    if change.by != "tidy" {
        return None;
    }
    let pair = match change.items.as_slice() {
        [a, b] => pair_of(a, b),
        [a] => (a.clone(), String::new()),
        _ => return None,
    };
    Some(Judged { pair, outcome: "undone".to_string(), at: now, of: Some(of.to_string()) })
}

/// The ledger entry for "Not replaced" pressed on an item that tidying
/// replaced, wherever the change itself is logged: the pair is settled on
/// every device, and the undo counts toward the pause.
pub(crate) fn not_replaced_entry(replaced: &str, by: &str, now: i64) -> Judged {
    Judged { pair: pair_of(replaced, by), outcome: "undone".to_string(), at: now, of: Some("replaced".to_string()) }
}

/// Is tidying paused? When three of its last five changes (replacements
/// and closings, from any device) were undone. `started_again` is when the
/// person last pressed "Start again": the count begins after it.
pub(crate) fn paused(ledger: &[Judged], started_again: Option<i64>) -> bool {
    let since = started_again.unwrap_or(i64::MIN);
    let changes: Vec<&Judged> = ledger.iter().filter(|j| j.at > since && matches!(j.outcome.as_str(), "replaced" | "closed")).collect();
    let last: Vec<&&Judged> = changes.iter().rev().take(PAUSE_OF_LAST).collect();
    let was_undone = |c: &Judged| ledger.iter().any(|u| u.outcome == "undone" && u.pair == c.pair && u.at >= c.at && u.at > since && u.of.as_deref() == Some(c.outcome.as_str()));
    last.iter().filter(|c| was_undone(c)).count() >= PAUSE_UNDONE
}

/// How many more changes tidying may make today (`day_start`: the start
/// of today, ms).
pub(crate) fn left_today(ledger: &[Judged], day_start: i64) -> usize {
    let made = ledger.iter().filter(|j| j.at >= day_start && matches!(j.outcome.as_str(), "replaced" | "closed")).count();
    CHANGES_A_DAY.saturating_sub(made)
}

// ── open items that later work may have finished ────────────────────────

/// What the judging model said of an open item and a later one.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum DoneVerdict {
    Done,
    StillOpen,
}

/// What a verdict on an open item leads to: the ledger entry. A "done"
/// verdict never closes the item: it is put to the person ("May already
/// be done", with both quotes), who answers or leaves it. "Still open" is
/// remembered, so the same pair is not judged, or asked, again.
pub(crate) fn done_judged(open: &Item, later: &Item, verdict: &DoneVerdict, now: i64) -> Judged {
    Judged { pair: pair_of(&open.row.id, &later.row.id), outcome: match verdict { DoneVerdict::Done => "may_be_done", DoneVerdict::StillOpen => "still_open" }.to_string(), at: now, of: None }
}

/// The open items that later work may have finished, each with the later
/// item that suggests it: what "May already be done" lists. An item the
/// person has since closed or reopened by hand is not listed.
pub(crate) fn may_be_done<'a>(held: &'a [Item], ledger: &[Judged]) -> Vec<(&'a Item, &'a Item)> {
    // The last word on a pair counts: "Still waiting", answered by the
    // person, comes after the suggestion and takes it off the list.
    let mut last: std::collections::BTreeMap<&(String, String), &Judged> = std::collections::BTreeMap::new();
    for j in ledger {
        if last.get(&j.pair).is_none_or(|have| have.at <= j.at) {
            last.insert(&j.pair, j);
        }
    }
    let mut latest: Vec<&Judged> = last.into_values().collect();
    latest.sort_by(|a, b| a.at.cmp(&b.at).then(a.pair.cmp(&b.pair)));
    latest
        .into_iter()
        .filter(|j| j.outcome == "may_be_done")
        .filter_map(|j| {
            let (a, b) = (held.iter().find(|i| i.row.id == j.pair.0)?, held.iter().find(|i| i.row.id == j.pair.1)?);
            let (open, later) = if a.meta.state == ItemState::Open { (a, b) } else { (b, a) };
            (open.meta.state == ItemState::Open).then_some((open, later))
        })
        .collect()
}

// ── one device tidies at a time ─────────────────────────────────────────

/// Which of the person's devices tidies, until when. A mechanism only:
/// nothing shows it, names its holder, or lets anyone change it.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Lease {
    pub device: String,
    /// How its best passing model did on the check (right answers on the
    /// judging part): the better judge tidies.
    pub score: u32,
    /// The lease runs out by itself (ms), so a device that is switched off
    /// or offline is replaced without anyone being asked.
    pub until: i64,
}

/// How long a lease runs before it must be renewed.
pub(crate) const LEASE_MS: i64 = 6 * 60 * 60 * 1000;

/// The lease as the one row that is saved.
pub(crate) fn lease_row(l: &Lease, now: i64) -> MemoryRow {
    MemoryRow { id: "sys-lease".to_string(), structured: Some(serde_json::json!({ "sys": "lease", "lease": l })), ..ledger_row(&Judged { pair: (String::new(), String::new()), outcome: String::new(), at: now, of: None }) }
}

fn sys<T: serde::de::DeserializeOwned>(rows: &[MemoryRow], kind: &str, key: &str) -> Option<T> {
    rows.iter().filter(|r| !r.deleted).find_map(|r| {
        let s = r.structured.as_ref()?;
        (s.get("sys")?.as_str()? == kind).then(|| serde_json::from_value(s.get(key)?.clone()).ok()).flatten()
    })
}

pub(crate) fn lease(rows: &[MemoryRow]) -> Option<Lease> {
    sys(rows, "lease", "lease")
}

/// What this device does about the lease. `my_score`: how this device's
/// best passing model did; `None` when it has none (then it never holds
/// the lease).
#[derive(Debug, PartialEq)]
pub(crate) enum LeaseMove {
    /// Write this lease (taken, or renewed).
    Hold(Lease),
    /// Another device holds it: apply what arrives, judge nothing.
    Leave,
}

pub(crate) fn lease_move(current: Option<&Lease>, device: &str, my_score: Option<u32>, now: i64) -> LeaseMove {
    let Some(score) = my_score else { return LeaseMove::Leave };
    let mine = Lease { device: device.to_string(), score, until: now + LEASE_MS };
    match current {
        // Mine already: renewed when it is past half its time.
        Some(l) if l.device == device => {
            if l.until - now < LEASE_MS / 2 || l.score != score { LeaseMove::Hold(mine) } else { LeaseMove::Hold(l.clone()) }
        }
        // Another device's, still valid: taken only by a better judge. A tie stays with the holder.
        Some(l) if l.until > now => {
            if score > l.score { LeaseMove::Hold(mine) } else { LeaseMove::Leave }
        }
        // None, or run out.
        _ => LeaseMove::Hold(mine),
    }
}

/// May this device judge now? Only the holder of a valid lease.
pub(crate) fn judges_here(current: Option<&Lease>, device: &str, now: i64) -> bool {
    current.is_some_and(|l| l.device == device && l.until > now)
}

/// Does any of the person's devices tidy? (The one thing a screen reads
/// from the lease: which form the tidying line takes.)
pub(crate) fn some_device_tidies(current: Option<&Lease>, now: i64) -> bool {
    current.is_some_and(|l| l.until > now)
}

/// Tidying and AI keeping turned off, for all the person's devices: one
/// row, so it travels and comes back with a restore. Turning it on again
/// writes a newer revision that clears it.
pub(crate) fn tidy_off_row(off: bool, now: i64) -> MemoryRow {
    MemoryRow { id: "sys-tidy-off".to_string(), structured: Some(serde_json::json!({ "sys": "tidy_off", "off": off })), ..ledger_row(&Judged { pair: (String::new(), String::new()), outcome: String::new(), at: now, of: None }) }
}

pub(crate) fn tidy_is_off(rows: &[MemoryRow]) -> bool {
    sys::<bool>(rows, "tidy_off", "off").unwrap_or(false)
}

/// The person pressed "Start again" after a pause: one row, for all their
/// devices. The count of undone changes begins after it.
pub(crate) fn started_again_row(now: i64) -> MemoryRow {
    MemoryRow { id: "sys-tidy-started-again".to_string(), structured: Some(serde_json::json!({ "sys": "tidy_started", "at": now })), ..ledger_row(&Judged { pair: (String::new(), String::new()), outcome: String::new(), at: now, of: None }) }
}

pub(crate) fn started_again(rows: &[MemoryRow]) -> Option<i64> {
    sys::<i64>(rows, "tidy_started", "at")
}

/// A ledger entry's row for a question put to the person, carrying which
/// item is the older one and the sentence the judgment wrote, so the
/// question can be shown and answered on any device.
pub(crate) fn asked_row(j: &Judged, older: &str, now_true: &str) -> MemoryRow {
    let mut row = ledger_row(j);
    if let Some(serde_json::Value::Object(o)) = row.structured.as_mut() {
        o.insert("older".into(), serde_json::Value::String(older.to_string()));
        o.insert("now".into(), serde_json::Value::String(now_true.to_string()));
    }
    row
}

/// A question that waits for the person: something they confirmed, and a
/// later item the judgment read as changing it. Nothing is replaced until
/// they answer, and they may never answer.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct Question {
    /// The row that carries it.
    pub id: String,
    pub older: String,
    pub newer: String,
    /// What the judgment wrote as true now.
    pub now: String,
    pub at: i64,
}

/// The questions not answered yet, oldest first.
pub(crate) fn questions(rows: &[MemoryRow]) -> Vec<Question> {
    let all = ledger(rows);
    let mut out: Vec<Question> = rows
        .iter()
        .filter(|r| !r.deleted)
        .filter_map(|r| {
            let s = r.structured.as_ref()?;
            if s.get("sys")?.as_str()? != "undo" {
                return None;
            }
            let j: Judged = serde_json::from_value(s.get("judged")?.clone()).ok()?;
            if j.outcome != "asked" {
                return None;
            }
            let older = s.get("older")?.as_str()?.to_string();
            let newer = if j.pair.0 == older { j.pair.1.clone() } else { j.pair.0.clone() };
            let answered = all.iter().any(|a| a.outcome == "answered" && a.pair == j.pair && a.at >= j.at);
            (!answered).then(|| Question { id: r.id.clone(), older, newer, now: s.get("now").and_then(|v| v.as_str()).unwrap_or_default().to_string(), at: j.at })
        })
        .collect();
    out.sort_by(|a, b| a.at.cmp(&b.at).then(a.id.cmp(&b.id)));
    out
}

/// The person's answer to a question.
#[derive(Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    /// "Replace it": the confirmed item is replaced by the later one.
    Replace,
    /// "Both are true": nothing changes.
    Both,
    /// Ignore: nothing changes, and the question is not put again.
    Ignore,
}

/// What an answer leads to: rows to save, the change for the log when
/// something was replaced, and the ledger entry that closes the question
/// on every device.
pub(crate) struct Answered {
    pub rows: Vec<MemoryRow>,
    pub change: Option<Change>,
    pub judged: Judged,
}

pub(crate) fn answer(q: &Question, older: &Item, newer: &Item, a: Answer, now: i64) -> Answered {
    let judged = |of: &str| Judged { pair: pair_of(&q.older, &q.newer), outcome: "answered".to_string(), at: now, of: Some(of.to_string()) };
    match a {
        Answer::Replace => {
            let (older_row, mut change) = crate::memory_items::replace(older, &newer.row.id, None, "person", now);
            change.items = vec![older.row.id.clone(), newer.row.id.clone()];
            Answered { rows: vec![older_row, says_now(newer, &q.now, now)], change: Some(change), judged: judged("replaced") }
        }
        Answer::Both => Answered { rows: Vec::new(), change: None, judged: judged("both_stand") },
        Answer::Ignore => Answered { rows: Vec::new(), change: None, judged: judged("ignored") },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_block::Ptr;
    use crate::memory_items::{keep, meta_of, undo, ItemType, Keep};

    const DAY: i64 = 86_400_000;
    const NOW: i64 = 1_772_000_000_000;

    fn item(id: &str, text: &str, as_of: i64, by: &str) -> Item {
        let (row, _) = keep(&[], Keep { id: id.into(), text, item_type: ItemType::Decision, ptr: Ptr { c: as_of * 1000, s: 0, t: as_of * 1000, a: 0, b: 10 }, as_of, by, audience: None, kind: "exchange", why: "settled", quote: None, subject: None, now: as_of + 1000 }).ok().unwrap();
        let meta = meta_of(&row).unwrap();
        Item { row, meta }
    }

    fn as_item(row: MemoryRow) -> Item {
        let meta = meta_of(&row).unwrap();
        Item { row, meta }
    }

    fn replaces() -> Verdict {
        Verdict::Replaces { now: "Free delivery starts at $60.".into() }
    }

    #[test]
    fn replaces_marks_the_older_item_and_keeps_it() {
        let old = item("it-40", "Free delivery starts at $40.", NOW - 50 * DAY, "ai:juno");
        let new = item("it-60", "Free delivery starts at $60.", NOW - DAY, "ai:juno");
        // Whichever way round the pair is handed in, the older by date is the one replaced.
        for (a, b) in [(&old, &new), (&new, &old)] {
            let Outcome::Replaced { older, newer, change, judged } = apply(a, b, &replaces(), NOW) else { panic!("not replaced") };
            let (o, n) = (meta_of(&older).unwrap(), meta_of(&newer).unwrap());
            assert_eq!((older.id.as_str(), o.state, o.replaced_by.as_deref(), o.state_by.as_str()), ("it-40", ItemState::Replaced, Some("it-60"), "tidy"));
            assert!(!older.deleted && older.text == "Free delivery starts at $40.", "it stays, as history");
            assert_eq!((newer.id.as_str(), n.state, n.now.as_deref()), ("it-60", ItemState::Current, Some("Free delivery starts at $60.")));
            assert_eq!((change.what, change.by.as_str(), change.items.clone()), (What::Replaced, "tidy", vec!["it-40".to_string(), "it-60".to_string()]));
            assert_eq!((judged.pair.clone(), judged.outcome.as_str()), (("it-40".to_string(), "it-60".to_string()), "replaced"));
        }
    }

    #[test]
    fn both_stand_pair_is_not_judged_twice() {
        let a = item("it-a", "We buy flour from Hartwell Mills.", NOW - 9 * DAY, "ai:juno");
        let b = item("it-b", "Hartwell delivers on the first Monday.", NOW - 2 * DAY, "ai:juno");
        let Outcome::Nothing { judged } = apply(&a, &b, &Verdict::BothStand, NOW) else { panic!() };
        let rows = vec![ledger_row(&judged)];
        let led = ledger(&rows);
        assert_eq!(led, vec![judged]);
        assert!(!may_judge(&led, "it-b", "it-a") && !may_judge(&led, "it-a", "it-b"));
        assert!(may_judge(&led, "it-a", "it-c"));
        // So the pair is not offered again when another item arrives near both.
        let held = vec![a.clone(), b.clone()];
        let near = vec![("it-a".to_string(), 0.8), ("it-b".to_string(), 0.7)];
        assert!(pairs_to_judge(&b, &held, &near, &led).is_empty());
        let c = item("it-c", "Hartwell raised its price.", NOW, "ai:juno");
        assert_eq!(pairs_to_judge(&c, &held, &near, &led).iter().map(|i| i.row.id.as_str()).collect::<Vec<_>>(), vec!["it-a", "it-b"]);
    }

    #[test]
    fn the_closest_five_current_items_are_judged() {
        let new = item("it-new", "Free delivery starts at $60.", NOW, "ai:juno");
        let mut held: Vec<Item> = (0..8).map(|i| item(&format!("it-{i}"), &format!("Item {i}."), NOW - (i + 1) * DAY, "ai:juno")).collect();
        // One of the near ones is already replaced: it is not judged.
        let (gone, _) = crate::memory_items::replace(&held[1], "it-x", None, "tidy", NOW);
        held[1] = as_item(gone);
        held.push(new.clone());
        let near: Vec<(String, f32)> = (0..8).map(|i| (format!("it-{i}"), 0.9 - i as f32 * 0.05)).chain([("it-new".to_string(), 1.0), ("it-gone-from-here".to_string(), 0.99)]).collect();
        let picked: Vec<&str> = pairs_to_judge(&new, &held, &near, &[]).iter().map(|i| i.row.id.as_str()).collect();
        assert_eq!(picked, vec!["it-0", "it-2", "it-3", "it-4", "it-5"]);
    }

    #[test]
    fn an_undone_pair_is_never_judged_again() {
        let old = item("it-40", "Free delivery starts at $40.", NOW - 50 * DAY, "ai:juno");
        let new = item("it-60", "Free delivery starts at $60.", NOW - DAY, "ai:juno");
        let Outcome::Replaced { older, change, judged, .. } = apply(&old, &new, &replaces(), NOW) else { panic!() };
        // The person presses "Not replaced".
        let (back, _) = undo(&change, &as_item(older), NOW + 5).unwrap();
        assert_eq!(meta_of(&back).unwrap().state, ItemState::Current);
        let entry = undone(&change, NOW + 5).unwrap();
        assert_eq!((entry.outcome.as_str(), entry.of.as_deref(), entry.pair.clone()), ("undone", Some("replaced"), judged.pair.clone()));
        // The ledger travels as rows: read on another device, the pair is settled there too.
        let rows = vec![ledger_row(&judged), ledger_row(&entry)];
        let on_another_device = ledger(&rows);
        assert!(!may_judge(&on_another_device, "it-60", "it-40"));
        assert!(pairs_to_judge(&new, &[as_item(back), new.clone()], &[("it-40".to_string(), 0.9)], &on_another_device).is_empty());
        // The row is a system row: never read as an item.
        assert!(crate::memory_items::items(&rows).is_empty());
    }

    #[test]
    fn a_confirmed_item_becomes_an_ask() {
        let old = item("it-small", "Use the small plan.", NOW - 30 * DAY, "person");
        assert!(old.meta.confirmed.is_some(), "what the person kept is confirmed");
        let new = item("it-medium", "We moved to the medium plan.", NOW - DAY, "ai:juno");
        let out = apply(&old, &new, &Verdict::Replaces { now: "They use the medium plan.".into() }, NOW);
        let Outcome::Ask { older, newer, now, judged } = out else { panic!("it was replaced without asking") };
        assert_eq!((older.as_str(), newer.as_str(), now.as_str(), judged.outcome.as_str()), ("it-small", "it-medium", "They use the medium plan.", "asked"));
        // Asked once: the pair is not judged, and so not asked, again.
        assert!(!may_judge(&[judged], "it-small", "it-medium"));
        // A confirmed NEWER item does not stop an unconfirmed older one being replaced.
        let old = item("it-a", "Saturdays 8 to 2.", NOW - 30 * DAY, "ai:juno");
        let new = item("it-b", "Closed on Saturdays from March.", NOW - DAY, "person");
        assert!(matches!(apply(&old, &new, &replaces(), NOW), Outcome::Replaced { .. }));
    }

    fn change_at(n: i64) -> Judged {
        Judged { pair: (format!("a{n}"), format!("b{n}")), outcome: "replaced".into(), at: NOW + n, of: None }
    }

    fn undo_of(c: &Judged, at: i64) -> Judged {
        Judged { pair: c.pair.clone(), outcome: "undone".into(), at, of: Some(c.outcome.clone()) }
    }

    #[test]
    fn three_of_five_undos_pause() {
        let changes: Vec<Judged> = (1..=5).map(change_at).collect();
        let mut led = changes.clone();
        assert!(!paused(&led, None));
        led.push(undo_of(&changes[0], NOW + 10));
        led.push(undo_of(&changes[2], NOW + 11));
        assert!(!paused(&led, None), "two of five");
        led.push(undo_of(&changes[4], NOW + 12));
        assert!(paused(&led, None), "three of five");
        // "Start again" clears the count.
        assert!(!paused(&led, Some(NOW + 20)));
        // Old undos fall out of the last five as tidying goes on.
        let mut later = led.clone();
        later.extend((30..=34).map(change_at));
        assert!(!paused(&later, None));
        // Fewer than five changes in all: three undone still pauses.
        let few: Vec<Judged> = (1..=3).map(change_at).collect();
        let mut led: Vec<Judged> = few.clone();
        led.extend(few.iter().enumerate().map(|(i, c)| undo_of(c, NOW + 50 + i as i64)));
        assert!(paused(&led, None));
    }

    #[test]
    fn undos_from_another_device_count_toward_the_pause() {
        // The desktop tidied; the undos were pressed on the laptop. Both
        // devices read the same rows, so both see the pause.
        let changes: Vec<Judged> = (1..=5).map(change_at).collect();
        let mut rows: Vec<MemoryRow> = changes.iter().map(ledger_row).collect();
        rows.extend(changes.iter().take(3).enumerate().map(|(i, c)| ledger_row(&undo_of(c, NOW + 100 + i as i64))));
        rows.reverse(); // they arrive in any order
        assert!(paused(&ledger(&rows), None));
    }

    #[test]
    fn undo_of_a_kept_does_not_count() {
        let kept = Change { id: "c1".into(), at: NOW, what: What::Kept, items: vec!["it-1".into()], by: "ai:juno".into(), undo: crate::memory_items::Undo::Forget, device: None };
        assert_eq!(undone(&kept, NOW + 1), None);
        // Nor does undoing something the person did themselves.
        let own = Change { what: What::Replaced, by: "person".into(), items: vec!["it-1".into(), "it-2".into()], ..kept.clone() };
        assert_eq!(undone(&own, NOW + 1), None);
        // A closing by tidying does.
        let closed = Change { what: What::Closed, by: "tidy".into(), ..kept };
        assert_eq!(undone(&closed, NOW + 1).unwrap().of.as_deref(), Some("closed"));
        let changes: Vec<Judged> = (1..=5).map(change_at).collect();
        let mut led = changes.clone();
        // Three "undone" entries that are not of a replacement or closing change nothing.
        led.extend(changes.iter().take(3).map(|c| Judged { of: Some("kept".into()), ..undo_of(c, NOW + 60) }));
        assert!(!paused(&led, None));
    }

    #[test]
    fn middle_of_a_chain() {
        // A replaced by B, B replaced by C. "Not replaced" on A.
        let a = item("it-a", "Free delivery starts at $40.", NOW - 60 * DAY, "ai:juno");
        let b = item("it-b", "Free delivery starts at $50.", NOW - 30 * DAY, "ai:juno");
        let c = item("it-c", "Free delivery starts at $60.", NOW - DAY, "ai:juno");
        let Outcome::Replaced { older: a_replaced, newer: b_now, change: a_by_b, .. } = apply(&a, &b, &Verdict::Replaces { now: "$50.".into() }, NOW) else { panic!() };
        let Outcome::Replaced { older: b_replaced, newer: c_now, .. } = apply(&as_item(b_now), &c, &replaces(), NOW + 10) else { panic!() };
        let (a_back, _) = undo(&a_by_b, &as_item(a_replaced), NOW + 20).unwrap();
        // A is current again beside C; B stays replaced by C.
        assert_eq!(meta_of(&a_back).unwrap().state, ItemState::Current);
        let b_meta = meta_of(&b_replaced).unwrap();
        assert_eq!((b_meta.state, b_meta.replaced_by.as_deref()), (ItemState::Replaced, Some("it-c")));
        assert_eq!(meta_of(&c_now).unwrap().state, ItemState::Current);
        // And A is not paired with B again; with C it can still be judged (they were never a pair).
        let led = vec![Judged { pair: pair_of("it-a", "it-b"), outcome: "replaced".into(), at: NOW, of: None }, undone(&a_by_b, NOW + 20).unwrap()];
        assert!(!may_judge(&led, "it-a", "it-b") && may_judge(&led, "it-a", "it-c"));
    }

    #[test]
    fn done_verdict_only_proposes() {
        use crate::memory_items::{keep, Keep};
        use crate::memory_block::Ptr;
        let (row, _) = keep(&[], Keep { id: "it-open".into(), text: "Ask the fitter about a second deck.", item_type: ItemType::Open, ptr: Ptr { c: 1, s: 0, t: 1, a: 0, b: 5 }, as_of: NOW - 9 * DAY, by: "ai:juno", audience: None, kind: "exchange", why: "settled", quote: None, subject: None, now: NOW - 9 * DAY }).ok().unwrap();
        let open = as_item(row);
        let later = item("it-later", "The fitter quoted for the second deck on Tuesday.", NOW - DAY, "ai:juno");
        let judged = done_judged(&open, &later, &DoneVerdict::Done, NOW);
        assert_eq!(judged.outcome, "may_be_done");
        // The item itself is untouched: still open, still given.
        assert_eq!(open.meta.state, ItemState::Open);
        let held = vec![open.clone(), later.clone()];
        let listed = may_be_done(&held, &[judged.clone()]);
        assert_eq!(listed.iter().map(|(o, l)| (o.row.id.as_str(), l.row.id.as_str())).collect::<Vec<_>>(), vec![("it-open", "it-later")], "listed with the later item that suggests it");
        // It does not use up the day's changes, and does not count toward the pause.
        assert_eq!(left_today(&[judged.clone()], NOW - (NOW % DAY)), CHANGES_A_DAY);
        assert!(!paused(&[judged.clone()], None));
        // "Still waiting", answered by the person, takes it off the list and the pair is not asked again.
        let answered = vec![judged.clone(), done_judged(&open, &later, &DoneVerdict::StillOpen, NOW + 1)];
        assert!(may_be_done(&held, &answered).is_empty() && !may_judge(&answered, "it-open", "it-later"));
        // Once the person closes it, it is no longer listed.
        let (closed, _) = crate::memory_items::close(&open, true, "person", NOW + 5);
        assert!(may_be_done(&[as_item(closed), later], &[judged]).is_empty());
    }

    #[test]
    fn still_waiting_is_remembered_so_the_same_pair_is_not_asked_again() {
        let open = item("it-open", "Ask the fitter about a second deck.", NOW - 9 * DAY, "ai:juno");
        let later = item("it-later", "The fitter is on holiday until May.", NOW - DAY, "ai:juno");
        let judged = done_judged(&open, &later, &DoneVerdict::StillOpen, NOW);
        assert_eq!(judged.outcome, "still_open");
        let led = ledger(&[ledger_row(&judged)]);
        assert!(!may_judge(&led, "it-open", "it-later"));
        assert!(may_be_done(&[open.clone(), later], &led).is_empty());
        // Another later item can still suggest it.
        assert!(may_judge(&led, "it-open", "it-newer"));
    }

    #[test]
    fn one_device_tidies_at_a_time_and_nobody_is_asked() {
        // No lease: a device with a passing model takes it; one without never does.
        let LeaseMove::Hold(first) = lease_move(None, "desktop", Some(27), NOW) else { panic!() };
        assert_eq!((first.device.as_str(), first.score, first.until), ("desktop", 27, NOW + LEASE_MS));
        assert_eq!(lease_move(None, "old-laptop", None, NOW), LeaseMove::Leave);
        assert!(judges_here(Some(&first), "desktop", NOW + 5) && !judges_here(Some(&first), "laptop", NOW + 5));
        // A tie stays with the holder; a better judge takes it; a worse one leaves it.
        assert_eq!(lease_move(Some(&first), "laptop", Some(27), NOW + 5), LeaseMove::Leave);
        assert_eq!(lease_move(Some(&first), "laptop", Some(25), NOW + 5), LeaseMove::Leave);
        assert!(matches!(lease_move(Some(&first), "laptop", Some(29), NOW + 5), LeaseMove::Hold(l) if l.device == "laptop"));
        // The holder renews past half its time, and not before.
        assert_eq!(lease_move(Some(&first), "desktop", Some(27), NOW + 5), LeaseMove::Hold(first.clone()));
        assert!(matches!(lease_move(Some(&first), "desktop", Some(27), NOW + LEASE_MS / 2 + 1), LeaseMove::Hold(l) if l.until == NOW + LEASE_MS / 2 + 1 + LEASE_MS));
        // The desktop is switched off: the lease runs out and the laptop takes it, with the same or a worse model.
        let after = NOW + LEASE_MS + 1;
        assert!(!judges_here(Some(&first), "desktop", after) && !some_device_tidies(Some(&first), after));
        assert!(matches!(lease_move(Some(&first), "laptop", Some(20), after), LeaseMove::Hold(l) if l.device == "laptop"));
        // It travels as one row; the newer revision is the lease.
        let LeaseMove::Hold(second) = lease_move(Some(&first), "laptop", Some(29), NOW + 5) else { panic!() };
        let rows = crate::memory_rows::reduce(vec![lease_row(&first, NOW), lease_row(&second, NOW + 5)]);
        assert_eq!((rows.len(), lease(&rows)), (1, Some(second)));
        assert!(crate::memory_items::items(&rows).is_empty());
    }

    #[test]
    fn turning_tidying_off_is_one_row_for_all_devices() {
        assert!(!tidy_is_off(&[]));
        let off = tidy_off_row(true, NOW);
        assert!(tidy_is_off(&[off.clone()]));
        // Turned on again on another device: the newer revision clears it everywhere.
        let rows = crate::memory_rows::reduce(vec![off, tidy_off_row(false, NOW + 9)]);
        assert_eq!((rows.len(), tidy_is_off(&rows)), (1, false));
    }

    #[test]
    fn catch_up_is_capped() {
        let day_start = NOW - (NOW % DAY);
        let mut led: Vec<Judged> = Vec::new();
        assert_eq!(left_today(&led, day_start), 20);
        led.extend((0..18).map(|n| Judged { at: day_start + 1000 + n, ..change_at(n) }));
        // What was only looked at (both stand, asked) does not use up the day.
        led.extend((100..140).map(|n| Judged { outcome: "both_stand".into(), at: day_start + 5000 + n, ..change_at(n) }));
        assert_eq!(left_today(&led, day_start), 2);
        led.extend((50..60).map(|n| Judged { at: day_start + 9000 + n, ..change_at(n) }));
        assert_eq!(left_today(&led, day_start), 0);
        // Yesterday's changes do not count against today.
        assert_eq!(left_today(&led, day_start + DAY), 20);
    }

    #[test]
    fn start_again_and_a_question_travel_as_rows() {
        assert_eq!(started_again(&[]), None);
        let rows = vec![started_again_row(NOW), tidy_off_row(false, NOW)];
        assert_eq!(started_again(&rows), Some(NOW));
        assert!(ledger(&rows).is_empty(), "neither is a ledger entry");
        let j = Judged { pair: pair_of("it-a", "it-b"), outcome: "asked".into(), at: NOW, of: None };
        let row = asked_row(&j, "it-b", "They moved to the medium plan.");
        assert_eq!(row.structured.as_ref().unwrap()["now"], "They moved to the medium plan.");
        assert_eq!(ledger(&[row]), vec![j], "the entry reads the same with the sentence beside it");
    }

    #[test]
    fn a_question_waits_until_it_is_answered_and_only_replace_changes_anything() {
        let small = item("it-b", "Use the small plan.", 10, "person");
        assert!(small.meta.confirmed.is_some());
        let medium = item("it-a", "We moved to the medium plan.", 20, "ai:atlas");
        let Outcome::Ask { older, now: now_true, judged, .. } = apply(&small, &medium, &Verdict::Replaces { now: "They are on the medium plan.".into() }, NOW) else { panic!("a confirmed item is asked about") };
        let asked = asked_row(&judged, &older, &now_true);
        let open = questions(&[asked.clone()]);
        assert_eq!(open.len(), 1);
        assert_eq!((open[0].older.as_str(), open[0].newer.as_str(), open[0].now.as_str()), ("it-b", "it-a", "They are on the medium plan."));
        assert!(!may_judge(&ledger(&[asked.clone()]), "it-a", "it-b"), "a pair asked about is not judged again");

        // "Both are true" and Ignore change no item and close the question.
        for a in [Answer::Both, Answer::Ignore] {
            let got = answer(&open[0], &small, &medium, a, NOW + 5);
            assert!(got.rows.is_empty() && got.change.is_none());
            assert!(questions(&[asked.clone(), ledger_row(&got.judged)]).is_empty());
            assert!(!paused(&ledger(&[asked.clone(), ledger_row(&got.judged)]), None));
        }

        // "Replace it": the confirmed item is replaced, by the person; the
        // later item says what is true now; the change can be undone.
        let got = answer(&open[0], &small, &medium, Answer::Replace, NOW + 5);
        let older_meta = meta_of(&got.rows[0]).unwrap();
        assert_eq!((older_meta.state.clone(), older_meta.replaced_by.as_deref(), older_meta.state_by.as_str()), (ItemState::Replaced, Some("it-a"), "person"));
        assert_eq!(meta_of(&got.rows[1]).unwrap().now.as_deref(), Some("They are on the medium plan."));
        let change = got.change.as_ref().unwrap();
        assert_eq!((change.what.clone(), change.by.as_str(), change.items.clone()), (What::Replaced, "person", vec!["it-b".to_string(), "it-a".to_string()]));
        assert!(undone(change, NOW + 9).is_none(), "the person's own replacement, undone, does not count against tidying");
        assert!(questions(&[asked, ledger_row(&got.judged)]).is_empty());
    }
}
