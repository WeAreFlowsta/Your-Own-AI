//! The memory item: one thing the person or an AI chose to keep, with a
//! date, a state, a subject, a pointer to the exchange it came from, a
//! "confirmed" mark, and who may use it.
//!
//! An item is a memory row in the scope `memory`. Its own fields live in
//! the row's free `structured` JSON under `item`, so the row's shape does
//! not change and an older app that edits a row keeps what it does not
//! understand. An item's state is on its own row: a replaced item stays
//! readable as history and never comes back by accident (the row's
//! `replaces` list is not used for items).
//!
//! This file is the item as data and the changes made to it. Each change
//! returns the row to save and the entry for the change log, with what an
//! undo would put back.

use crate::memory_block::Ptr;
use crate::memory_rows::MemoryRow;
use serde::{Deserialize, Serialize};

/// The scope every item, subject and system row of memory lives in.
pub(crate) const SCOPE: &str = "memory";

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemType {
    Decision,
    Done,
    Open,
    Detail,
    AboutPerson,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemState {
    Current,
    Replaced,
    Open,
    Done,
    NotNeeded,
    /// The person chose to forget it. Undoable; given to no AI.
    Forgotten,
    /// Learned by an AI before the update and not used: given to no AI.
    SetAside,
}

/// Who may use an item.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Audience {
    /// All the person's AIs.
    All,
    /// One AI, by its founding key.
    Ai { ai: String },
    /// Turns held in one project, by the project's identity.
    Project { project: String },
    /// AIs that run on the person's own devices.
    LocalAis,
    None,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Confirmed {
    pub at: i64,
    /// `pressed` | `corrected` | `kept`.
    pub how: String,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Source {
    /// `exchange` | `selection` | `note` | `legacy`.
    pub kind: String,
    #[serde(default)]
    pub ptr: Option<Ptr>,
    #[serde(default)]
    pub quote: Option<String>,
    #[serde(default)]
    pub doc: Option<serde_json::Value>,
    /// `pressed` | `said` | `settled` | `asked` | `tool` | `migrated:<store>`.
    pub why: String,
    #[serde(default)]
    pub for_ai: Option<String>,
}

/// An item's own fields.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ItemMeta {
    pub v: u8,
    #[serde(rename = "type")]
    pub item_type: ItemType,
    pub state: ItemState,
    #[serde(default)]
    pub replaced_by: Option<String>,
    /// The "what is true now" sentence a replacement judgment wrote.
    #[serde(default)]
    pub now: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    /// Open items: `person` or `ai:<founding key>`.
    #[serde(default)]
    pub waiting_on: Option<String>,
    /// When the state last changed (ms), and who or what changed it:
    /// `person` | `ai:<founding key>` | `tidy`.
    pub state_at: i64,
    pub state_by: String,
    #[serde(default)]
    pub confirmed: Option<Confirmed>,
    /// The wording before the person corrected it.
    #[serde(default)]
    pub corrected_from: Option<String>,
    /// A subject row's id.
    #[serde(default)]
    pub subject: Option<String>,
    pub audience: Audience,
    pub source: Source,
    /// When it last came up in an answer (ms), noted at most once a day.
    #[serde(default)]
    pub last_came_up: Option<i64>,
    /// Fields a later version adds ride through an edit untouched.
    #[serde(flatten)]
    pub more: serde_json::Map<String, serde_json::Value>,
}

/// An item: its row and its own fields, read together.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    pub row: MemoryRow,
    pub meta: ItemMeta,
}

/// The row's kind for an item of a type: inside the closed list of kinds
/// (the real type is in the item's own fields).
fn kind_for(t: ItemType) -> &'static str {
    match t {
        ItemType::Decision => "decision",
        ItemType::AboutPerson => "fact",
        _ => "note",
    }
}

/// A row's item fields, when it is an item.
pub(crate) fn meta_of(row: &MemoryRow) -> Option<ItemMeta> {
    serde_json::from_value(row.structured.as_ref()?.get("item")?.clone()).ok()
}

/// Write an item's fields back into its row, keeping every other key the
/// row's `structured` holds.
pub(crate) fn with_meta(mut row: MemoryRow, meta: &ItemMeta) -> MemoryRow {
    let mut structured = match row.structured.take() {
        Some(serde_json::Value::Object(o)) => o,
        _ => serde_json::Map::new(),
    };
    structured.insert("item".into(), serde_json::to_value(meta).unwrap_or_default());
    row.structured = Some(serde_json::Value::Object(structured));
    // Items never use the row's `replaces`: a reader drops any row another
    // row names there.
    row.replaces.clear();
    row
}

/// The items among a scope's rows (already reduced to the newest revision
/// of each): rows that carry item fields. A replaced or forgotten item is
/// still an item.
pub(crate) fn items(rows: &[MemoryRow]) -> Vec<Item> {
    rows.iter().filter(|r| r.scope == SCOPE && !r.deleted).filter_map(|r| meta_of(r).map(|meta| Item { row: r.clone(), meta })).collect()
}

/// What is being kept.
pub(crate) struct Keep<'a> {
    /// A new id for the item.
    pub id: String,
    /// The statement: the exchange's words, or the selection.
    pub text: &'a str,
    pub item_type: ItemType,
    /// The exchange it came from, and when that was (ms).
    pub ptr: Ptr,
    pub as_of: i64,
    /// `person` or `ai:<founding key>`: who kept it.
    pub by: &'a str,
    /// Absent: all the person's AIs.
    pub audience: Option<Audience>,
    /// `exchange` or `selection`.
    pub kind: &'a str,
    /// `pressed` | `said` | `settled` | `asked` | `tool`.
    pub why: &'a str,
    pub quote: Option<String>,
    pub subject: Option<String>,
    pub now: i64,
}

/// What kept an item the first time, when the same exchange is kept again.
pub(crate) fn already_kept<'a>(existing: &'a [Item], ptr: &Ptr, text: &str) -> Option<&'a Item> {
    existing.iter().find(|i| {
        let same_place = i.meta.source.ptr.as_ref().is_some_and(|p| p.c == ptr.c && p.s == ptr.s);
        same_place && i.row.text.trim() == text.trim() && !matches!(i.meta.state, ItemState::Forgotten)
    })
}

/// Keep an exchange (or a selection from it) as an item. `Err` carries the
/// item that already holds it.
pub(crate) fn keep<'a>(existing: &'a [Item], k: Keep) -> Result<(MemoryRow, Change), &'a Item> {
    if let Some(had) = already_kept(existing, &k.ptr, k.text) {
        return Err(had);
    }
    let by_person = k.by == "person";
    let meta = ItemMeta {
        v: 1,
        item_type: k.item_type,
        state: if k.item_type == ItemType::Open { ItemState::Open } else { ItemState::Current },
        replaced_by: None,
        now: None,
        reason: None,
        waiting_on: None,
        state_at: k.now,
        state_by: k.by.to_string(),
        // What the person keeps themselves is theirs to stand behind.
        confirmed: by_person.then(|| Confirmed { at: k.now, how: "kept".into() }),
        corrected_from: None,
        subject: k.subject,
        audience: k.audience.unwrap_or(Audience::All),
        source: Source { kind: k.kind.to_string(), ptr: Some(k.ptr), quote: k.quote, doc: None, why: k.why.to_string(), for_ai: None },
        last_came_up: None,
        more: Default::default(),
    };
    let row = with_meta(
        MemoryRow {
            id: k.id.clone(),
            scope: SCOPE.to_string(),
            kind: kind_for(k.item_type).to_string(),
            text: k.text.trim().to_string(),
            structured: None,
            as_of: k.as_of,
            valid_to: None,
            saved_by: k.by.to_string(),
            provenance: if by_person { "authored".into() } else { format!("kept:{}", k.why) },
            pinned: false,
            topics: Vec::new(),
            replaces: Vec::new(),
            deleted: false,
            updated_at: k.now,
            action_hash: None,
        },
        &meta,
    );
    let change = Change { id: format!("chg-{}-{}", k.now, k.id), at: k.now, what: What::Kept, items: vec![k.id], by: k.by.to_string(), undo: Undo::Forget, device: None };
    Ok((row, change))
}

/// May a turn use an item? `ai` is the AI's founding key, `project` the
/// project the turn is in, `online` whether the model runs elsewhere.
pub(crate) fn may_use(meta: &ItemMeta, ai: &str, project: Option<&str>, online: bool) -> bool {
    if !matches!(meta.state, ItemState::Current | ItemState::Open | ItemState::Done) {
        return false;
    }
    // A line of a note that is gone, or that the person chose not to have used.
    if !crate::memory_notes::usable(meta) {
        return false;
    }
    match &meta.audience {
        Audience::All => true,
        Audience::Ai { ai: only } => only == ai,
        Audience::Project { project: only } => project == Some(only.as_str()),
        Audience::LocalAis => !online,
        Audience::None => false,
    }
}

// ── changes, and the log of them ────────────────────────────────────────

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum What {
    Kept,
    Replaced,
    Closed,
    Forgotten,
    Confirmed,
    Corrected,
    AudienceChanged,
    /// Moved to another subject.
    Moved,
    Undone,
    /// A note an item points at was edited and read again.
    NoteChanged,
}

/// What an undo of a change puts back.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum Undo {
    /// The item was new: an undo forgets it.
    Forget,
    /// The item's fields and words as they were before.
    Restore { text: String, meta: Box<ItemMeta> },
    /// Nothing to undo (an undo is not undone; do the thing again).
    Nothing,
}

/// One entry of the change log: what memory changed, by itself or for the
/// person, and what an undo would restore.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Change {
    pub id: String,
    pub at: i64,
    pub what: What,
    pub items: Vec<String>,
    /// `person` | `ai:<founding key>` | `tidy`.
    pub by: String,
    pub undo: Undo,
    /// The device that made it, when it was not this one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub device: Option<String>,
}

fn changed(item: &Item, text: Option<&str>, meta: ItemMeta, what: What, by: &str, now: i64) -> (MemoryRow, Change) {
    let mut row = item.row.clone();
    if let Some(t) = text {
        row.text = t.trim().to_string();
    }
    row.updated_at = now;
    row.action_hash = None;
    let change = Change {
        id: format!("chg-{now}-{}", item.row.id),
        at: now,
        what,
        items: vec![item.row.id.clone()],
        by: by.to_string(),
        undo: Undo::Restore { text: item.row.text.clone(), meta: Box::new(item.meta.clone()) },
        device: None,
    };
    (with_meta(row, &meta), change)
}

/// The person says an item is right.
pub(crate) fn confirm(item: &Item, now: i64) -> (MemoryRow, Change) {
    let meta = ItemMeta { confirmed: Some(Confirmed { at: now, how: "pressed".into() }), ..item.meta.clone() };
    changed(item, None, meta, What::Confirmed, "person", now)
}

/// Keep, pressed on something an AI had learned before the update and
/// that was set aside: it is the person's now, current, and used again.
/// Anything else is left as it is.
pub(crate) fn keep_set_aside(item: &Item, now: i64) -> Option<(MemoryRow, Change)> {
    if item.meta.state != ItemState::SetAside {
        return None;
    }
    let meta = ItemMeta {
        state: ItemState::Current,
        state_at: now,
        state_by: "person".into(),
        confirmed: Some(Confirmed { at: now, how: "kept".into() }),
        subject: Some(crate::memory_subjects::ABOUT_YOU.to_string()),
        ..item.meta.clone()
    };
    Some(changed(item, None, meta, What::Kept, "person", now))
}

/// Words the person wrote on the Memory page, about themselves: an item
/// that is theirs from the start, current, confirmed, for all their AIs,
/// under "About you". No exchange stands behind it.
pub(crate) fn keep_words(id: String, text: &str, now: i64) -> (MemoryRow, Change) {
    let meta = ItemMeta {
        v: 1,
        item_type: ItemType::AboutPerson,
        state: ItemState::Current,
        replaced_by: None,
        now: None,
        reason: None,
        waiting_on: None,
        state_at: now,
        state_by: "person".into(),
        confirmed: Some(Confirmed { at: now, how: "kept".into() }),
        corrected_from: None,
        subject: Some(crate::memory_subjects::ABOUT_YOU.to_string()),
        audience: Audience::All,
        source: Source { kind: "note".into(), ptr: None, quote: None, doc: None, why: "pressed".into(), for_ai: None },
        last_came_up: None,
        more: Default::default(),
    };
    let row = MemoryRow {
        id: id.clone(),
        scope: SCOPE.to_string(),
        kind: "fact".to_string(),
        text: text.trim().to_string(),
        structured: Some(serde_json::json!({ "item": meta })),
        as_of: now,
        valid_to: None,
        saved_by: "person".to_string(),
        provenance: "authored".to_string(),
        pinned: false,
        topics: Vec::new(),
        replaces: Vec::new(),
        deleted: false,
        updated_at: now,
        action_hash: None,
    };
    let change = Change { id: format!("chg-{now}-{id}"), at: now, what: What::Kept, items: vec![id], by: "person".into(), undo: Undo::Forget, device: None };
    (row, change)
}

/// The person corrects an item's words: theirs now, and confirmed.
pub(crate) fn correct(item: &Item, text: &str, now: i64) -> (MemoryRow, Change) {
    let meta = ItemMeta {
        confirmed: Some(Confirmed { at: now, how: "corrected".into() }),
        corrected_from: Some(item.meta.corrected_from.clone().unwrap_or_else(|| item.row.text.clone())),
        state_by: "person".into(),
        ..item.meta.clone()
    };
    changed(item, Some(text), meta, What::Corrected, "person", now)
}

/// A later item replaces this one. The older item stays, as history.
pub(crate) fn replace(item: &Item, by_item: &str, now_true: Option<&str>, by: &str, now: i64) -> (MemoryRow, Change) {
    let meta = ItemMeta { state: ItemState::Replaced, replaced_by: Some(by_item.to_string()), now: now_true.map(str::to_string), state_at: now, state_by: by.to_string(), ..item.meta.clone() };
    changed(item, None, meta, What::Replaced, by, now)
}

/// The person says a replaced item is not replaced: it is current again,
/// beside the one that was said to replace it. `None` when it is not
/// replaced.
pub(crate) fn not_replaced(item: &Item, now: i64) -> Option<(MemoryRow, Change)> {
    if item.meta.state != ItemState::Replaced {
        return None;
    }
    let meta = ItemMeta { state: ItemState::Current, replaced_by: None, state_at: now, state_by: "person".into(), ..item.meta.clone() };
    let (row, mut change) = changed(item, None, meta, What::Undone, "person", now);
    change.undo = Undo::Nothing;
    Some((row, change))
}

/// An open item is closed: done, or no longer needed.
pub(crate) fn close(item: &Item, done: bool, by: &str, now: i64) -> (MemoryRow, Change) {
    let meta = ItemMeta { state: if done { ItemState::Done } else { ItemState::NotNeeded }, state_at: now, state_by: by.to_string(), ..item.meta.clone() };
    changed(item, None, meta, What::Closed, by, now)
}

/// The person forgets an item. It can be brought back.
pub(crate) fn forget(item: &Item, now: i64) -> (MemoryRow, Change) {
    let meta = ItemMeta { state: ItemState::Forgotten, state_at: now, state_by: "person".into(), ..item.meta.clone() };
    changed(item, None, meta, What::Forgotten, "person", now)
}

/// The person changes who may use an item.
pub(crate) fn set_audience(item: &Item, audience: Audience, now: i64) -> (MemoryRow, Change) {
    let meta = ItemMeta { audience, ..item.meta.clone() };
    changed(item, None, meta, What::AudienceChanged, "person", now)
}

/// The person moves an item to another subject.
pub(crate) fn move_to(item: &Item, subject: &str, now: i64) -> (MemoryRow, Change) {
    let meta = ItemMeta { subject: Some(subject.to_string()), ..item.meta.clone() };
    changed(item, None, meta, What::Moved, "person", now)
}

/// Undo a change: the row to save, and the log's entry for the undo. The
/// item is taken as it is now, so an undo never loses what a later edit
/// of another kind put on it beyond the fields the change touched.
pub(crate) fn undo(change: &Change, item: &Item, now: i64) -> Option<(MemoryRow, Change)> {
    if !change.items.contains(&item.row.id) {
        return None;
    }
    let (text, meta) = match &change.undo {
        Undo::Nothing => return None,
        Undo::Forget => (item.row.text.clone(), ItemMeta { state: ItemState::Forgotten, state_at: now, state_by: "person".into(), ..item.meta.clone() }),
        Undo::Restore { text, meta } => (text.clone(), ItemMeta { more: item.meta.more.clone(), last_came_up: item.meta.last_came_up, ..(**meta).clone() }),
    };
    let mut row = item.row.clone();
    row.text = text;
    row.updated_at = now;
    row.action_hash = None;
    let entry = Change { id: format!("chg-{now}-undo-{}", item.row.id), at: now, what: What::Undone, items: vec![item.row.id.clone()], by: "person".into(), undo: Undo::Nothing, device: None };
    Some((with_meta(row, &meta), entry))
}

/// The change log as a list: newest first, with at most `keep` entries
/// (older ones are still in the rows' own history).
pub(crate) fn log_with(mut log: Vec<Change>, entry: Change, keep: usize) -> Vec<Change> {
    log.retain(|c| c.id != entry.id);
    log.insert(0, entry);
    log.truncate(keep);
    log
}

/// Two copies of the log as one: every change once, newest first, at most
/// `keep`. A change is never edited after it is written, so the same id
/// is the same change on both sides.
pub(crate) fn merged_log(mine: Vec<Change>, theirs: Vec<Change>, keep: usize) -> Vec<Change> {
    let mut seen = std::collections::HashSet::new();
    let mut all: Vec<Change> = mine.into_iter().chain(theirs).filter(|c| seen.insert(c.id.clone())).collect();
    all.sort_by(|a, b| b.at.cmp(&a.at));
    all.truncate(keep);
    all
}

// ── "remember that" ─────────────────────────────────────────────────────

/// What a message asks memory to keep, when it asks.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Remember {
    /// "remember that": the exchange before this message.
    TheExchangeBefore,
    /// "remember that the oven runs hot": this message's own words.
    This(String),
}

const REMEMBER_PHRASES: [&str; 5] = ["remember that", "remember this", "keep that", "don't forget that", "do not forget that"];

/// Does the message start by asking memory to keep something? A fixed
/// list of phrases, no model. A question ("do you remember that ...") is
/// not a request.
pub(crate) fn remember_phrase(message: &str) -> Option<Remember> {
    let text = message.trim();
    let folded: String = text.chars().map(|c| if c == '\u{2019}' { '\'' } else { c }).collect::<String>().to_lowercase();
    let folded = folded.trim_start_matches("please ").trim_start_matches("ok ").trim_start_matches("okay ").trim_start_matches(", ");
    let skipped = text.chars().count() - folded.chars().count();
    for phrase in REMEMBER_PHRASES {
        let Some(rest) = folded.strip_prefix(phrase) else { continue };
        if folded.ends_with('?') {
            return None;
        }
        let rest_trimmed = rest.trim_matches(|c: char| c.is_whitespace() || ".,:;!".contains(c));
        if rest_trimmed.is_empty() {
            return Some(Remember::TheExchangeBefore);
        }
        // The phrase runs into another word ("remember thatch roofs"): not it.
        if !rest.starts_with(|c: char| c.is_whitespace() || ",:".contains(c)) {
            return None;
        }
        // The words after the phrase, as the person wrote them.
        let from = skipped + phrase.chars().count();
        let own: String = text.chars().skip(from).collect();
        return Some(Remember::This(own.trim_matches(|c: char| c.is_whitespace() || ",:".contains(c)).to_string()));
    }
    None
}

// ── the stores, and the page's doors ────────────────────────────────────

const CHANGES_FILE: &str = "memory-changes.enc";
/// The change log on this device holds this many entries; older changes
/// are still in the rows' own history.
const CHANGES_KEPT: usize = 2000;

pub(crate) fn data_key(app: &tauri::AppHandle) -> Result<[u8; 32], String> {
    use tauri::Manager;
    app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get()?.data_key()
}

fn read_changes(app: &tauri::AppHandle, key: &[u8; 32]) -> Vec<Change> {
    let Ok(root) = crate::profile::root(app) else { return Vec::new() };
    let Ok(raw) = std::fs::read(root.join(CHANGES_FILE)) else { return Vec::new() };
    if raw.len() < 24 {
        return Vec::new();
    }
    crate::transcript_crypto::decrypt(key, &raw[..24], &raw[24..]).ok().and_then(|p| serde_json::from_slice(&p).ok()).unwrap_or_default()
}

fn write_changes(app: &tauri::AppHandle, key: &[u8; 32], log: &[Change]) -> Result<(), String> {
    let root = crate::profile::root(app)?;
    let (nonce, cipher) = crate::transcript_crypto::encrypt(key, &serde_json::to_vec(log).map_err(|e| e.to_string())?)?;
    let mut out = nonce.to_vec();
    out.extend_from_slice(&cipher);
    let path = root.join(CHANGES_FILE);
    flowsta_hc_lines::files::write_atomic_private(&path, &out).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

/// This device's list of changes, emptied (all memory was deleted).
pub(crate) fn clear_changes(app: &tauri::AppHandle) {
    if let Ok(root) = crate::profile::root(app) {
        let _ = std::fs::remove_file(root.join(CHANGES_FILE));
    }
}

/// The log is read, added to and written back by one caller at a time: a
/// change noted here while another device's log is folded in is not lost.
static CHANGES_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Note a change in this device's log.
pub(crate) fn log_change(app: &tauri::AppHandle, key: &[u8; 32], change: Change) -> Result<(), String> {
    let _one_at_a_time = CHANGES_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    write_changes(app, key, &log_with(read_changes(app, key), change, CHANGES_KEPT))
}

/// The change log as a backup carries it: the sealed file as it stands.
pub(crate) fn changes_for_backup(app: &tauri::AppHandle) -> Option<Vec<u8>> {
    std::fs::read(crate::profile::root(app).ok()?.join(CHANGES_FILE)).ok().filter(|raw| raw.len() > 24)
}

/// A backup's change log, or another device's, folded into this device's.
/// `made_on` names the other device when the log is another device's: the
/// changes this device did not have are marked as made there, so Recently
/// can say so. Returns how many changes this device did not have.
pub(crate) fn fold_changes(app: &tauri::AppHandle, key: &[u8; 32], raw: &[u8], made_on: Option<&str>) -> usize {
    if raw.len() < 24 {
        return 0;
    }
    let Some(theirs) = crate::transcript_crypto::decrypt(key, &raw[..24], &raw[24..]).ok().and_then(|p| serde_json::from_slice::<Vec<Change>>(&p).ok()) else { return 0 };
    let _one_at_a_time = CHANGES_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mine = read_changes(app, key);
    let (log, new) = folded_log(mine, theirs, made_on, CHANGES_KEPT);
    if new > 0 {
        let _ = write_changes(app, key, &log);
    }
    new
}

/// This device's log with another log folded in: a change already here is
/// left as it is; one that is new here is marked as made on `made_on`
/// (unless it already says where it was made).
pub(crate) fn folded_log(mine: Vec<Change>, theirs: Vec<Change>, made_on: Option<&str>, keep: usize) -> (Vec<Change>, usize) {
    let had: std::collections::HashSet<String> = mine.iter().map(|c| c.id.clone()).collect();
    let new: Vec<Change> = theirs
        .into_iter()
        .filter(|c| !had.contains(&c.id))
        .map(|mut c| {
            if c.device.is_none() {
                c.device = made_on.map(str::to_string);
            }
            c
        })
        .collect();
    let n = new.len();
    (merged_log(mine, new, keep), n)
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

fn held(app: &tauri::AppHandle, key: &[u8; 32]) -> Vec<Item> {
    items(&crate::memory_rows::read_cache(app, key, SCOPE))
}

/// An item as the page sees it.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct ItemView {
    pub id: String,
    pub text: String,
    pub as_of: i64,
    pub updated_at: i64,
    pub saved_by: String,
    pub item: ItemMeta,
}

pub(crate) fn view(row: &MemoryRow) -> Option<ItemView> {
    Some(ItemView { id: row.id.clone(), text: row.text.clone(), as_of: row.as_of, updated_at: row.updated_at, saved_by: row.saved_by.clone(), item: meta_of(row)? })
}

/// Save a changed item and note the change in the log.
async fn save(app: &tauri::AppHandle, key: &[u8; 32], row: MemoryRow, change: Change) -> Result<ItemView, String> {
    // An item with no subject yet is given the one its place tells.
    let row = crate::memory_subjects::settle(app, key, row).await;
    let saved = crate::memory_rows::save_and_cache(app, row).await?;
    log_change(app, key, change)?;
    view(&saved).ok_or_else(|| "the saved row is not an item".to_string())
}

/// What the page asks to keep.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeepInput {
    pub text: String,
    pub item_type: ItemType,
    pub ptr: Ptr,
    /// When the exchange was (ms).
    pub as_of: i64,
    /// Absent: the person.
    #[serde(default)]
    pub by: Option<String>,
    #[serde(default)]
    pub audience: Option<Audience>,
    /// `exchange` or `selection`.
    pub kind: String,
    /// `pressed` | `said` | `asked` | `tool`.
    pub why: String,
    #[serde(default)]
    pub subject: Option<String>,
}

/// The answer to a keep: the item, the change that can be undone, and
/// whether it was already kept (then nothing was written).
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeptAnswer {
    pub item: ItemView,
    pub change: Option<String>,
    pub already: bool,
    /// The item was open and is now marked done ("that's done", said in the chat).
    pub closed: bool,
}

/// Every item, forgotten and replaced ones too (the page decides what a
/// view shows).
#[tauri::command]
pub fn memory_items(app: tauri::AppHandle) -> Result<Vec<ItemView>, String> {
    let key = data_key(&app)?;
    Ok(held(&app, &key).iter().filter_map(|i| view(&i.row)).collect())
}

#[tauri::command]
pub async fn memory_keep(app: tauri::AppHandle, input: KeepInput) -> Result<KeptAnswer, String> {
    if input.text.trim().is_empty() {
        return Err("there is nothing to keep".into());
    }
    let key = data_key(&app)?;
    let have = held(&app, &key);
    let now = now_ms();
    let id = {
        use rand::Rng as _;
        format!("it-{:016x}{:016x}", rand::thread_rng().gen::<u64>(), rand::thread_rng().gen::<u64>())
    };
    // What is kept is kept without its secrets.
    let text = crate::secret_mask::hand_on(&input.text).text;
    let by = input.by.clone().unwrap_or_else(|| "person".to_string());
    let k = Keep { id, text: &text, item_type: input.item_type, ptr: input.ptr, as_of: input.as_of, by: &by, audience: input.audience, kind: &input.kind, why: &input.why, quote: None, subject: input.subject, now };
    match keep(&have, k) {
        Err(had) => Ok(KeptAnswer { item: view(&had.row).ok_or("not an item")?, change: None, already: true, closed: false }),
        Ok((row, change)) => {
            let change_id = change.id.clone();
            let item = save(&app, &key, row, change).await?;
            // A new item is looked at by tidying when the computer is idle.
            crate::memory_tidying::kept(&app, &item.id);
            Ok(KeptAnswer { item, change: Some(change_id), already: false, closed: false })
        }
    }
}

/// One message of a conversation, as a place to keep from.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Recorded {
    pub seq: u32,
    pub by_person: bool,
    /// When it was written (microseconds).
    pub at: i64,
    pub content: String,
}

/// Which recorded message a bubble is: the one at its sequence when the
/// page knows it (and it is by the same speaker), else the latest message
/// by that speaker with the same words.
pub(crate) fn recorded_for<'a>(entries: &'a [Recorded], seq: Option<u32>, by_person: bool, words: &str) -> Option<&'a Recorded> {
    if let Some(e) = seq.and_then(|s| entries.iter().find(|e| e.seq == s && e.by_person == by_person)) {
        return Some(e);
    }
    let same = |a: &str, b: &str| a.split_whitespace().eq(b.split_whitespace());
    entries.iter().rev().find(|e| e.by_person == by_person && !words.trim().is_empty() && same(&e.content, words))
}

/// What "remember that", said in the chat, asks to keep.
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SaidRemember {
    /// The exchange before this message.
    Before,
    /// This message.
    This,
}

/// Did the person ask, in fixed words, for something to be kept? The
/// phrase alone means the exchange before; the phrase followed by words
/// means this message. No model reads it: a message is taken this way
/// only when it begins with one of the phrases and is not a question.
pub(crate) fn said_remember(message: &str) -> Option<SaidRemember> {
    const PHRASES: [&str; 6] = ["remember that", "remember this", "keep that", "keep this", "don't forget that", "dont forget that"];
    // "Keep this conversation out of memory" asks for the opposite.
    if said_control(message).is_some() {
        return None;
    }
    let said = message.trim().to_lowercase().replace('\u{2019}', "'");
    if said.ends_with('?') {
        return None;
    }
    let said = ["please ", "ok ", "ok, ", "okay ", "okay, "].iter().find_map(|p| said.strip_prefix(p)).unwrap_or(&said).trim_start();
    let rest = PHRASES.iter().find_map(|p| said.strip_prefix(p))?;
    // "remember thatch roofs leak" is not the phrase.
    if rest.chars().next().is_some_and(|c| c.is_alphanumeric()) {
        return None;
    }
    // "Keep that out of it", "keep this private", "keep that to yourself":
    // not a request to keep something in memory.
    let after = rest.trim_start();
    if ["out of ", "private", "secret", "to yourself", "between us", "off the record"].iter().any(|p| after.starts_with(p)) {
        return None;
    }
    let more = rest.trim_matches(|c: char| !c.is_alphanumeric());
    // "please", "for me", "too" after the phrase add nothing to keep.
    Some(if more.is_empty() || matches!(more, "please" | "for me" | "too" | "as well" | "for later") { SaidRemember::Before } else { SaidRemember::This })
}

/// Did the person say, in fixed words, that the thing left open is done?
/// No model reads it: the message begins with one of the phrases, is not a
/// question, and says little else.
pub(crate) fn said_done(message: &str) -> bool {
    const PHRASES: [&str; 10] = ["that's done", "that is done", "thats done", "that's been done", "that has been done", "i've done that", "i have done that", "i did that", "done with that", "mark that as done"];
    let said = message.trim().to_lowercase().replace('\u{2019}', "'");
    if said.ends_with('?') {
        return false;
    }
    let said = ["please ", "ok ", "ok, ", "okay ", "okay, "].iter().find_map(|p| said.strip_prefix(p)).unwrap_or(&said).trim_start();
    let Some(rest) = PHRASES.iter().find_map(|p| said.strip_prefix(p)) else { return false };
    // "that's done badly" or a longer report is not the phrase: a later
    // item about it is what "May already be done" is for.
    let more = rest.trim_matches(|c: char| !c.is_alphanumeric());
    !rest.chars().next().is_some_and(|c| c.is_alphanumeric()) && (more.is_empty() || matches!(more, "now" | "thanks" | "thank you" | "already" | "today"))
}

/// Something the person can say in the chat in place of pressing a
/// control: the words are read on the device by a fixed rule, like
/// "remember that" and "that's done".
#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SaidControl {
    /// "That's right": what the AI just used or wrote is confirmed.
    Right,
    /// "Forget that": what was just kept or used is forgotten.
    Forget,
    /// "Keep this conversation out of memory."
    KeepOut,
    /// "Stop using memory on this computer": replies here no longer look
    /// in memory (a slow computer answers sooner without it).
    OffHere,
    /// "Use memory on this computer again."
    OnHere,
}

/// Where the phrases below say it applies. Without one of these the
/// words could mean keeping, or every device, and nothing is done.
const HERE: [&str; 3] = ["on this computer", "on this device", "here"];
const SAID_OFF_HERE: [&str; 10] = [
    "stop using memory", "stop using your memory", "turn memory off", "turn off memory", "turn off your memory", "switch memory off", "switch off memory",
    "don't use memory", "dont use memory", "do not use memory",
];
const SAID_ON_HERE: [&str; 12] = [
    "use memory again", "use your memory again", "start using memory again", "start using memory", "start using your memory again", "turn memory on", "turn memory back on", "turn on memory", "turn memory on again",
    "switch memory on", "switch memory back on", "switch on memory",
];

/// "Stop using memory on this computer", "turn memory back on here": one
/// of the phrases, then where, in either order of "again" ("use memory on
/// this computer again"), and nothing else.
fn said_here(said: &str) -> Option<SaidControl> {
    let said = said.trim_end_matches(|c: char| !c.is_alphanumeric());
    let said = said.strip_suffix(" please").unwrap_or(said);
    let (said, again) = match said.strip_suffix(" again") {
        Some(rest) => (rest, true),
        None => (said, false),
    };
    let what = HERE.iter().find_map(|h| said.strip_suffix(h))?.trim_end();
    let is = |phrases: &[&str]| phrases.iter().any(|p| *p == what || (again && p.strip_suffix(" again") == Some(what)));
    if is(&SAID_OFF_HERE) && !again {
        Some(SaidControl::OffHere)
    } else if is(&SAID_ON_HERE) {
        Some(SaidControl::OnHere)
    } else {
        None
    }
}

const SAID_RIGHT: [&str; 18] = [
    "that's right", "that is right", "thats right", "that's correct", "that is correct", "thats correct", "that's true", "that is true", "thats true",
    "that's still right", "that is still right", "that's still true", "that is still true", "that's still correct", "that is still correct",
    "you have that right", "you've got that right", "you got that right",
];
const SAID_FORGET: [&str; 20] = [
    "forget that", "forget this", "forget what i just said", "forget what i said", "forget i said that", "forget that i said that",
    "don't remember that", "dont remember that", "do not remember that", "don't keep that", "dont keep that", "do not keep that",
    "remove that from memory", "remove that from your memory", "delete that from memory", "delete that from your memory",
    "take that out of memory", "take that out of your memory", "don't use that again", "do not use that again",
];
const SAID_KEEP_OUT: [&str; 26] = [
    "keep this conversation out of memory", "keep this chat out of memory", "keep this out of memory", "keep this conversation out of your memory", "keep this chat out of your memory",
    "leave this conversation out of memory", "leave this chat out of memory", "leave this out of memory",
    "don't remember this conversation", "dont remember this conversation", "do not remember this conversation",
    "don't remember this chat", "dont remember this chat", "do not remember this chat",
    "don't use this conversation", "dont use this conversation", "do not use this conversation",
    "this conversation is private", "this chat is private", "make this conversation private", "make this chat private",
    "keep this conversation private", "keep this chat private", "this is off the record", "off the record", "this stays off the record",
];

/// Did the person say, in fixed words, one of the things above? No model
/// reads it: the message begins with one of the phrases (after a "yes",
/// "ok" or "please"), is not a question, and says little else. "That's
/// right about the first part but ..." is an ordinary message.
pub(crate) fn said_control(message: &str) -> Option<SaidControl> {
    let said = message.trim().to_lowercase().replace('\u{2019}', "'");
    if said.ends_with('?') || said.len() > 120 {
        return None;
    }
    let mut said = said.as_str();
    // "Yes, ok, that's right": the small words people begin with.
    'opening: loop {
        for p in ["please ", "ok ", "ok, ", "okay ", "okay, ", "yes ", "yes, ", "yep ", "yep, ", "yeah ", "yeah, ", "actually ", "actually, ", "and ", "so ", "hey ", "can you ", "could you "] {
            if let Some(rest) = said.strip_prefix(p) {
                said = rest.trim_start();
                continue 'opening;
            }
        }
        break;
    }
    let ends_it = |phrases: &[&str]| {
        phrases.iter().any(|p| {
            said.strip_prefix(p).is_some_and(|rest| {
                let more = rest.trim_matches(|c: char| !c.is_alphanumeric());
                !rest.chars().next().is_some_and(|c| c.is_alphanumeric()) && (more.is_empty() || matches!(more, "please" | "thanks" | "thank you" | "now" | "for me" | "yes" | "exactly"))
            })
        })
    };
    if let Some(here) = said_here(said) {
        Some(here)
    } else if ends_it(&SAID_KEEP_OUT) {
        Some(SaidControl::KeepOut)
    } else if ends_it(&SAID_FORGET) {
        Some(SaidControl::Forget)
    } else if ends_it(&SAID_RIGHT) {
        Some(SaidControl::Right)
    } else {
        None
    }
}

/// An AI wrote the item: it was settled in a conversation, a reply quoted
/// it, an AI's tool kept it, or it came over from what AIs had learned.
pub(crate) fn written_by_an_ai(item: &Item) -> bool {
    let why = item.meta.source.why.as_str();
    matches!(why, "settled" | "asked" | "tool") || why.starts_with("migrated")
}

/// What "that" can mean when the person says it at message `seq` of the
/// conversation that began at `started_at`: an item the reply before was
/// given in its note (`used`), or one kept from the exchange before. Only
/// items that stand now.
pub(crate) fn said_about<'a>(items: &'a [Item], started_at: i64, seq: u32, used: &[String]) -> Vec<&'a Item> {
    items
        .iter()
        .filter(|i| matches!(i.meta.state, ItemState::Current | ItemState::Open))
        .filter(|i| used.contains(&i.row.id) || i.meta.source.ptr.is_some_and(|p| p.c == started_at && p.s < seq && p.s + 2 >= seq))
        .collect()
}

/// What the app did with something said in the chat, for the line under
/// the message.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SaidAnswer {
    /// `confirmed` | `forgotten` | `kept_out` | `left_out`.
    pub did: &'static str,
    pub item: Option<ItemView>,
    /// The change an undo names, for an item.
    pub change: Option<String>,
    /// The conversation, and the messages left out, for the other two.
    pub started_at: i64,
    pub seqs: Vec<u32>,
}

/// One of the controls above, said in the chat. `used`: the items the
/// reply before this message was given in its note. "That" must mean one
/// thing: with two candidates nothing is done, and the page shows nothing.
/// "Forget that" with nothing kept leaves the exchange before out of
/// memory from now on. Every outcome can be taken back.
#[tauri::command]
pub async fn memory_said(app: tauri::AppHandle, input: KeepMessageInput, used: Vec<String>) -> Result<Option<SaidAnswer>, String> {
    let Some(what) = said_control(&input.text) else { return Ok(None) };
    // About this computer, not about a place in a conversation.
    if matches!(what, SaidControl::OffHere | SaidControl::OnHere) {
        let on = what == SaidControl::OnHere;
        if crate::memory_prompt::replies_use_memory(&app) == on {
            return Ok(None);
        }
        crate::memory_prompt::memory_in_replies_set(app.clone(), on)?;
        return Ok(Some(SaidAnswer { did: if on { "on_here" } else { "off_here" }, item: None, change: None, started_at: 0, seqs: Vec::new() }));
    }
    let ptr = place_of(&app, &input).await?;
    if what == SaidControl::KeepOut {
        crate::memory_use::memory_conversation_use_set(app.clone(), crate::memory_use::ConvChange { started_at: ptr.c, private: Some(true), ..Default::default() }).await?;
        return Ok(Some(SaidAnswer { did: "kept_out", item: None, change: None, started_at: ptr.c, seqs: Vec::new() }));
    }
    let key = data_key(&app)?;
    let have = held(&app, &key);
    let mut about = said_about(&have, ptr.c, ptr.s, &used);
    let now = now_ms();
    if what == SaidControl::Right {
        about.retain(|i| written_by_an_ai(i) && i.meta.confirmed.is_none());
        let [item] = about[..] else { return Ok(None) };
        let (row, change) = confirm(item, now);
        let id = change.id.clone();
        let item = save(&app, &key, row, change).await?;
        return Ok(Some(SaidAnswer { did: "confirmed", item: Some(item), change: Some(id), started_at: ptr.c, seqs: Vec::new() }));
    }
    match about[..] {
        [item] => {
            let (row, change) = forget(item, now);
            let id = change.id.clone();
            let item = save(&app, &key, row, change).await?;
            crate::memory_tidying::words_changed(&app);
            Ok(Some(SaidAnswer { did: "forgotten", item: Some(item), change: Some(id), started_at: ptr.c, seqs: Vec::new() }))
        }
        [] => {
            // Nothing was kept: the exchange before is what "that" means.
            let k = crate::transcript_search::row_key(&app, &input.conversation)?;
            let mut before: Vec<u32> = crate::transcript_search::cached_entries(&app, &k, 0)?.into_iter().filter(|e| e.seq < ptr.s && (e.role == "user" || e.role == "assistant")).map(|e| e.seq).collect();
            let seqs: Vec<u32> = before.split_off(before.len().saturating_sub(2));
            if seqs.is_empty() {
                return Ok(None);
            }
            for seq in &seqs {
                crate::memory_use::memory_conversation_use_set(app.clone(), crate::memory_use::ConvChange { started_at: ptr.c, exclude: Some(*seq), ..Default::default() }).await?;
            }
            Ok(Some(SaidAnswer { did: "left_out", item: None, change: None, started_at: ptr.c, seqs }))
        }
        _ => Ok(None),
    }
}

/// The one thing a conversation left open (by the conversation's start).
/// None when it left nothing open, or more than one: "that" must mean one
/// thing for it to be closed without asking.
pub(crate) fn open_here(items: &[Item], started_at: i64) -> Option<&Item> {
    let mut open = items.iter().filter(|i| i.meta.state == ItemState::Open && i.meta.source.ptr.is_some_and(|p| p.c == started_at));
    match (open.next(), open.next()) {
        (Some(one), None) => Some(one),
        _ => None,
    }
}

/// "That's done", said in the chat: when the conversation left exactly one
/// thing open, it is marked done, as the button on the Open board does,
/// and can be taken back. Anything else changes nothing.
#[tauri::command]
pub async fn memory_done_said(app: tauri::AppHandle, input: KeepMessageInput) -> Result<Option<KeptAnswer>, String> {
    if !said_done(&input.text) {
        return Ok(None);
    }
    let ptr = place_of(&app, &input).await?;
    let key = data_key(&app)?;
    let have = held(&app, &key);
    let Some(item) = open_here(&have, ptr.c) else { return Ok(None) };
    let (row, change) = close(item, true, "person", now_ms());
    let change_id = change.id.clone();
    let item = save(&app, &key, row, change).await?;
    crate::memory_tidying::words_changed(&app);
    Ok(Some(KeptAnswer { item, change: Some(change_id), already: false, closed: true }))
}

/// An item's words are at most this long; the pointer leads to the rest.
const ITEM_CHARS: usize = 1000;

/// The words an item gets: the selection when there is one, else the
/// message, shortened to what an item holds.
pub(crate) fn words_to_keep(message: &str, selection: Option<&str>) -> String {
    let words = selection.map(str::trim).filter(|s| !s.is_empty()).unwrap_or(message.trim());
    words.chars().take(ITEM_CHARS).collect::<String>().trim().to_string()
}

/// What the page says when Remember is pressed on a message.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KeepMessageInput {
    /// The open conversation's id.
    pub conversation: String,
    /// The chain that holds it (the AI's key), for a conversation this
    /// device has not read into its text cache yet.
    #[serde(default)]
    pub agent_key: Option<String>,
    /// The message's sequence, when the page knows it.
    #[serde(default)]
    pub seq: Option<u32>,
    /// `user` or `assistant`.
    pub role: String,
    /// The message as the bubble shows it.
    pub text: String,
    /// The selected words, for Remember on a selection.
    #[serde(default)]
    pub selection: Option<String>,
    #[serde(default)]
    pub audience: Option<Audience>,
    /// Absent: `pressed`. `said` for "remember that".
    #[serde(default)]
    pub why: Option<String>,
}

/// Where a bubble's message is recorded: read from this device's text of
/// the conversation, else (a conversation not read here yet) from its
/// records.
pub(crate) async fn place_of(app: &tauri::AppHandle, input: &KeepMessageInput) -> Result<Ptr, String> {
    use tauri::Manager;
    let by_person = input.role == "user";
    let k = crate::transcript_search::row_key(app, &input.conversation)?;
    if let Some(conv) = crate::transcript_search::cached_conversations(app)?.into_iter().find(|c| c.k == k) {
        let entries: Vec<Recorded> = crate::transcript_search::cached_entries(app, &k, 0)?.into_iter().map(|e| Recorded { seq: e.seq, by_person: e.role == "user", at: e.at, content: e.content }).collect();
        if let Some(e) = recorded_for(&entries, input.seq, by_person, &input.text) {
            return Ok(Ptr { c: conv.started_at, s: e.seq, t: e.at, a: 0, b: 0 });
        }
    }
    let agent = input.agent_key.as_deref().filter(|a| !a.is_empty()).ok_or("this conversation is still being saved; try again in a moment")?;
    let started_at = crate::conversation_cache::read_cache(app, agent)?.into_iter().find(|c| c.hash == input.conversation).map(|c| c.started_at).ok_or("that conversation is not in the list yet; try again in a moment")?;
    let entries: Vec<Recorded> = crate::commands_holochain::get_conversation_transcript(agent.to_string(), input.conversation.clone(), app.state())
        .await?
        .into_iter()
        .map(|e| Recorded { seq: e.sequence, by_person: e.role == "user", at: e.timestamp, content: e.content })
        .collect();
    let e = recorded_for(&entries, input.seq, by_person, &input.text).ok_or("that message is not saved yet; try again in a moment")?;
    Ok(Ptr { c: started_at, s: e.seq, t: e.at, a: 0, b: 0 })
}

/// Remember, pressed on a message (or on words selected in it): the
/// exchange is kept as an item that points at it. A second press answers
/// with the item that already holds it.
/// The person wrote something about themselves on the Memory page.
/// The same words kept before are not kept twice.
#[tauri::command]
pub async fn memory_keep_words(app: tauri::AppHandle, text: String) -> Result<KeptAnswer, String> {
    // What is kept is kept without its secrets.
    let text = crate::secret_mask::hand_on(text.trim()).text;
    if text.is_empty() {
        return Err("there is nothing to keep".into());
    }
    let key = data_key(&app)?;
    let plain = |t: &str| t.trim().trim_end_matches('.').to_lowercase();
    if let Some(had) = held(&app, &key).into_iter().find(|i| i.meta.state == ItemState::Current && i.meta.source.kind == "note" && plain(&i.row.text) == plain(&text)) {
        return Ok(KeptAnswer { item: view(&had.row).ok_or("not an item")?, change: None, already: true, closed: false });
    }
    let id = {
        use rand::Rng as _;
        format!("it-{:016x}{:016x}", rand::thread_rng().gen::<u64>(), rand::thread_rng().gen::<u64>())
    };
    let (row, change) = keep_words(id, &text, now_ms());
    let change_id = change.id.clone();
    let item = save(&app, &key, row, change).await?;
    crate::memory_tidying::kept(&app, &item.id);
    Ok(KeptAnswer { item, change: Some(change_id), already: false, closed: false })
}

#[tauri::command]
pub async fn memory_keep_message(app: tauri::AppHandle, input: KeepMessageInput) -> Result<KeptAnswer, String> {
    let text = words_to_keep(&input.text, input.selection.as_deref());
    if text.is_empty() {
        return Err("there is nothing to keep".into());
    }
    let ptr = place_of(&app, &input).await?;
    let kind = if input.selection.as_deref().is_some_and(|s| !s.trim().is_empty()) { "selection" } else { "exchange" };
    memory_keep(app, KeepInput { text, item_type: ItemType::Detail, ptr, as_of: ptr.t / 1000, by: None, audience: input.audience, kind: kind.to_string(), why: input.why.unwrap_or_else(|| "pressed".to_string()), subject: None }).await
}

/// A finished reply quoted, word for word, exchanges memory had found for
/// the question: each such exchange is kept, once, because it answered
/// something the person asked. No model is involved. `ai` is the founding
/// key of the AI that replied. Nothing is kept from a private
/// conversation, or while the person has keeping turned off; an exchange
/// that was kept before (and perhaps undone) is not kept again.
#[tauri::command]
pub async fn memory_keep_quoted(app: tauri::AppHandle, reply: String, given: crate::memory_block::GivenRecord, texts: Vec<String>, ai: Option<String>) -> Result<Vec<ItemView>, String> {
    let key = data_key(&app)?;
    let rows = crate::memory_rows::read_cache(&app, &key, SCOPE);
    if crate::memory_tidy::tidy_is_off(&rows) {
        return Ok(Vec::new());
    }
    let uses = crate::memory_use::conv_uses(&rows);
    let mut have = items(&rows);
    let by = ai.filter(|k| !k.is_empty()).map(|k| format!("ai:{k}")).unwrap_or_else(|| "memory".to_string());
    let mut out = Vec::new();
    for (ptr, quote) in crate::memory_quotes::quoted_exactly(&reply, &given, &texts) {
        if crate::memory_use::is_private(&uses, ptr.c) || have.iter().any(|i| i.meta.source.ptr.is_some_and(|p| p.c == ptr.c && p.s == ptr.s)) {
            continue;
        }
        // The same words kept before, from another conversation where the
        // thing was also said: once is enough, and what was undone is not
        // kept again.
        let words = |t: &str| t.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
        if have.iter().any(|i| words(&i.row.text) == words(&quote)) {
            continue;
        }
        // The item is for whoever the conversation it came from is for.
        let audience = uses.iter().find(|u| u.started_at == ptr.c).and_then(|u| u.audience.clone());
        let text = crate::secret_mask::hand_on(&quote).text;
        let now = now_ms();
        let id = {
            use rand::Rng as _;
            format!("it-{:016x}{:016x}", rand::thread_rng().gen::<u64>(), rand::thread_rng().gen::<u64>())
        };
        let k = Keep { id, text: &text, item_type: ItemType::Detail, ptr, as_of: ptr.t / 1000, by: &by, audience, kind: "exchange", why: "asked", quote: Some(text.clone()), subject: None, now };
        let Some((row, change)) = keep(&have, k).ok() else { continue };
        if let Some(meta) = meta_of(&row) {
            have.push(Item { row: row.clone(), meta });
        }
        let item = save(&app, &key, row, change).await?;
        crate::memory_tidying::kept(&app, &item.id);
        out.push(item);
    }
    Ok(out)
}

/// A change the person makes to one item.
#[derive(Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum ItemEdit {
    Confirm,
    /// Keep something that was set aside at the update.
    Keep,
    Correct { text: String },
    Forget,
    Audience { audience: Audience },
    Close { done: bool },
    /// To another subject, by its id.
    Move { subject: String },
}

#[tauri::command]
pub async fn memory_item_change(app: tauri::AppHandle, id: String, edit: ItemEdit) -> Result<ItemView, String> {
    let key = data_key(&app)?;
    let have = held(&app, &key);
    let item = have.iter().find(|i| i.row.id == id).ok_or("no such item")?;
    let now = now_ms();
    let (row, change) = match edit {
        ItemEdit::Confirm => confirm(item, now),
        ItemEdit::Keep => keep_set_aside(item, now).ok_or("only something set aside can be kept this way")?,
        ItemEdit::Correct { text } => correct(item, &crate::secret_mask::hand_on(&text).text, now),
        ItemEdit::Forget => forget(item, now),
        ItemEdit::Audience { audience } => set_audience(item, audience, now),
        ItemEdit::Close { done } => close(item, done, "person", now),
        ItemEdit::Move { subject } => {
            let all = crate::memory_subjects::subjects(&crate::memory_rows::read_cache(&app, &key, SCOPE));
            let to = crate::memory_subjects::standing(&all, &subject).map(|s| s.id.clone()).or_else(|| (subject == crate::memory_subjects::ABOUT_YOU).then(|| subject.clone())).ok_or("that subject is not here any more")?;
            move_to(item, &to, now)
        }
    };
    let saved = save(&app, &key, row, change).await?;
    // Other words need another vector.
    crate::memory_tidying::words_changed(&app);
    Ok(saved)
}

/// "Remove all of these", on the list of what was learned before the
/// update and set aside: every set-aside row is deleted. The person
/// started it and was asked once. Answers how many were removed.
#[tauri::command]
pub async fn memory_set_aside_remove_all(app: tauri::AppHandle) -> Result<u32, String> {
    let key = data_key(&app)?;
    let now = now_ms();
    let mut removed = 0u32;
    for item in held(&app, &key).into_iter().filter(|i| i.meta.state == ItemState::SetAside) {
        let mut row = item.row.clone();
        row.deleted = true;
        row.updated_at = now;
        row.action_hash = None;
        crate::memory_rows::save_and_cache(&app, row).await?;
        removed += 1;
    }
    Ok(removed)
}

/// "Not replaced", pressed where a reply's panel shows what was left out:
/// the item is current again, and tidying never judges that pair again.
#[tauri::command]
pub async fn memory_not_replaced(app: tauri::AppHandle, id: String) -> Result<ItemView, String> {
    let key = data_key(&app)?;
    let have = held(&app, &key);
    let item = have.iter().find(|i| i.row.id == id).ok_or("no such item")?;
    let now = now_ms();
    let (row, change) = not_replaced(item, now).ok_or("that item is not replaced")?;
    let saved = save(&app, &key, row, change).await?;
    if let (Some(by), "tidy") = (item.meta.replaced_by.as_deref(), item.meta.state_by.as_str()) {
        crate::memory_rows::save_and_cache(&app, crate::memory_tidy::ledger_row(&crate::memory_tidy::not_replaced_entry(&id, by, now))).await?;
    }
    Ok(saved)
}

/// "This replaces ...", by hand: the person says which earlier item a
/// later one replaces. The earlier item stays as history, marked replaced
/// by the person, and the change can be undone from Recent like any
/// other. Answers the earlier item as it is now.
#[tauri::command]
pub async fn memory_replaces(app: tauri::AppHandle, newer: String, older: String) -> Result<ItemView, String> {
    if newer == older {
        return Err("an item cannot replace itself".into());
    }
    let key = data_key(&app)?;
    let have = held(&app, &key);
    let later = have.iter().find(|i| i.row.id == newer).ok_or("no such item")?;
    let earlier = have.iter().find(|i| i.row.id == older).ok_or("no such item")?;
    if !replaceable_by_hand(earlier, later) {
        return Err("only something current can be replaced, by something current".into());
    }
    let (row, change) = replace(earlier, &later.row.id, None, "person", now_ms());
    save(&app, &key, row, change).await
}

/// Both stand now: the earlier one is in use, and so is the one that
/// takes its place.
pub(crate) fn replaceable_by_hand(earlier: &Item, later: &Item) -> bool {
    let stands = |i: &Item| matches!(i.meta.state, ItemState::Current | ItemState::Open);
    earlier.row.id != later.row.id && stands(earlier) && stands(later)
}

/// A question as the page shows it: the item the person confirmed, the
/// later item that seems to change it, and what would be true then.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct QuestionView {
    pub id: String,
    pub older: ItemView,
    pub newer: ItemView,
    pub now: String,
    /// See `memory_tidy::Question::kept`.
    pub kept: bool,
}

/// The questions that wait for the person. One about an item that is no
/// longer current (forgotten, or replaced another way) has gone.
#[tauri::command]
pub fn memory_questions(app: tauri::AppHandle) -> Result<Vec<QuestionView>, String> {
    let key = data_key(&app)?;
    let rows = crate::memory_rows::read_cache(&app, &key, SCOPE);
    let have = held(&app, &key);
    let live = |id: &str| have.iter().find(|i| i.row.id == id && matches!(i.meta.state, ItemState::Current | ItemState::Open));
    Ok(crate::memory_tidy::questions(&rows)
        .into_iter()
        .filter_map(|q| Some(QuestionView { older: view(&live(&q.older)?.row)?, newer: view(&live(&q.newer)?.row)?, id: q.id, now: q.now, kept: q.kept }))
        .collect())
}

/// The person answers a question: "Replace it" replaces the item they had
/// confirmed; "Both are true" and Ignore change nothing. Each closes the
/// question on every device.
#[tauri::command]
pub async fn memory_question_answer(app: tauri::AppHandle, id: String, answer: crate::memory_tidy::Answer) -> Result<(), String> {
    let key = data_key(&app)?;
    let rows = crate::memory_rows::read_cache(&app, &key, SCOPE);
    let q = crate::memory_tidy::questions(&rows).into_iter().find(|q| q.id == id).ok_or("that question is already answered")?;
    let have = held(&app, &key);
    let older = have.iter().find(|i| i.row.id == q.older).ok_or("no such item")?;
    let newer = have.iter().find(|i| i.row.id == q.newer).ok_or("no such item")?;
    let got = crate::memory_tidy::answer(&q, older, newer, answer, now_ms());
    let mut rows = got.rows.into_iter();
    if let (Some(first), Some(change)) = (rows.next(), got.change) {
        save(&app, &key, first, change).await?;
    }
    for row in rows {
        crate::memory_rows::save_and_cache(&app, row).await?;
    }
    crate::memory_rows::save_and_cache(&app, crate::memory_tidy::ledger_row(&got.judged)).await?;
    Ok(())
}

/// What memory changed lately, newest first.
#[tauri::command]
pub fn memory_changes(app: tauri::AppHandle) -> Result<Vec<Change>, String> {
    let key = data_key(&app)?;
    Ok(read_changes(&app, &key))
}

#[tauri::command]
pub async fn memory_undo(app: tauri::AppHandle, change_id: String) -> Result<Vec<ItemView>, String> {
    let key = data_key(&app)?;
    let log = read_changes(&app, &key);
    let change = log.iter().find(|c| c.id == change_id).ok_or("that change is no longer in the list")?.clone();
    let have = held(&app, &key);
    let now = now_ms();
    let mut out = Vec::new();
    // What a change restores belongs to the item it changed, which is
    // named first; a second item (the one that replaced it) is only named.
    let changed = if matches!(change.undo, Undo::Restore { .. }) { &change.items[..change.items.len().min(1)] } else { &change.items[..] };
    for id in changed {
        let Some(item) = have.iter().find(|i| &i.row.id == id) else { continue };
        if let Some((row, entry)) = undo(&change, item, now) {
            out.push(save(&app, &key, row, entry).await?);
        }
    }
    // An undo of what tidying did is kept where every device reads it: the
    // pair is never judged again, and undos count toward the pause.
    if !out.is_empty() {
        if let Some(j) = crate::memory_tidy::undone(&change, now) {
            crate::memory_rows::save_and_cache(&app, crate::memory_tidy::ledger_row(&j)).await?;
        }
        crate::memory_tidying::words_changed(&app);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: i64 = 1_772_000_000_000;

    fn ptr(seq: u32) -> Ptr {
        Ptr { c: 1_768_896_000_000_000, s: seq, t: 1_768_896_090_000_000, a: 0, b: 58 }
    }

    fn kept(existing: &[Item], id: &str, text: &str, seq: u32, by: &str) -> (MemoryRow, Change) {
        keep(existing, Keep { id: id.into(), text, item_type: ItemType::Decision, ptr: ptr(seq), as_of: 1_768_896_090_000, by, audience: None, kind: "exchange", why: "pressed", quote: None, subject: Some("sub-suppliers".into()), now: NOW })
            .map_err(|had| had.row.id.clone())
            .expect("kept")
    }

    fn item_of(row: MemoryRow) -> Item {
        let meta = meta_of(&row).expect("an item");
        Item { row, meta }
    }

    #[test]
    fn keep_points_at_the_exchange() {
        let (row, change) = kept(&[], "it-1", "  We buy flour from Hartwell Mills.  ", 2, "person");
        assert_eq!((row.scope.as_str(), row.kind.as_str(), row.text.as_str()), ("memory", "decision", "We buy flour from Hartwell Mills."));
        assert_eq!(row.as_of, 1_768_896_090_000, "the item is dated by its exchange, not by when it was kept");
        let meta = meta_of(&row).unwrap();
        assert_eq!(meta.source.ptr, Some(ptr(2)));
        assert_eq!((meta.source.kind.as_str(), meta.source.why.as_str()), ("exchange", "pressed"));
        assert_eq!((meta.state, meta.subject.as_deref()), (ItemState::Current, Some("sub-suppliers")));
        assert_eq!(meta.confirmed.as_ref().map(|c| c.how.as_str()), Some("kept"), "what the person keeps is confirmed by them");
        assert_eq!((change.what, change.items.clone(), change.by.as_str(), &change.undo), (What::Kept, vec!["it-1".to_string()], "person", &Undo::Forget));
        // Kept by an AI: not confirmed by the person, and the row says who.
        let (row, _) = kept(&[], "it-2", "The oven runs hot.", 4, "ai:uhCAkJuno");
        let meta = meta_of(&row).unwrap();
        assert_eq!((row.saved_by.as_str(), meta.confirmed, meta.state_by.as_str()), ("ai:uhCAkJuno", None, "ai:uhCAkJuno"));
        // Something left to do starts open.
        let (row, _) = keep(&[], Keep { id: "it-3".into(), text: "Ask the fitter about a second deck.", item_type: ItemType::Open, ptr: ptr(6), as_of: 1, by: "person", audience: None, kind: "exchange", why: "said", quote: None, subject: None, now: NOW }).ok().unwrap();
        assert_eq!((meta_of(&row).unwrap().state, row.kind.as_str()), (ItemState::Open, "note"));
    }

    #[test]
    fn a_move_changes_only_the_subject_and_undo_puts_it_back() {
        let (row, _) = keep(&[], Keep { id: "it-1".into(), text: "The oven is a Rofco B40.", item_type: ItemType::Detail, ptr: ptr(3), as_of: 1, by: "person", audience: None, kind: "exchange", why: "pressed", quote: None, subject: Some("sub-a".into()), now: 5 }).ok().unwrap();
        let item = Item { meta: meta_of(&row).unwrap(), row };
        let (moved, change) = move_to(&item, "sub-b", 9);
        let meta = meta_of(&moved).unwrap();
        assert_eq!((meta.subject.as_deref(), moved.text.as_str(), meta.state), (Some("sub-b"), "The oven is a Rofco B40.", ItemState::Current));
        assert_eq!(change.what, What::Moved);
        let now_item = Item { meta, row: moved };
        let (back, _) = undo(&change, &now_item, 12).unwrap();
        assert_eq!(meta_of(&back).unwrap().subject.as_deref(), Some("sub-a"));
    }

    #[test]
    fn selection_keeps_the_selection_text() {
        let (row, _) = keep(&[], Keep { id: "it-1".into(), text: "forty 12.5 kg bags", item_type: ItemType::Detail, ptr: Ptr { a: 31, b: 49, ..ptr(3) }, as_of: 1, by: "person", audience: None, kind: "selection", why: "pressed", quote: None, subject: None, now: NOW }).ok().unwrap();
        let meta = meta_of(&row).unwrap();
        assert_eq!(row.text, "forty 12.5 kg bags");
        assert_eq!((meta.source.kind.as_str(), meta.source.ptr.unwrap().a, meta.source.ptr.unwrap().b), ("selection", 31, 49));
    }

    #[test]
    fn default_audience_is_all_ais() {
        let (row, _) = kept(&[], "it-1", "We buy flour from Hartwell Mills.", 2, "person");
        let meta = meta_of(&row).unwrap();
        assert_eq!(meta.audience, Audience::All);
        assert!(may_use(&meta, "uhCAkJuno", None, false) && may_use(&meta, "uhCAkAtlas", Some("id:p1"), true));
        // Each other audience.
        let with = |a: Audience| ItemMeta { audience: a, ..meta.clone() };
        let only = with(Audience::Ai { ai: "uhCAkJuno".into() });
        assert!(may_use(&only, "uhCAkJuno", None, false) && !may_use(&only, "uhCAkAtlas", None, false));
        let project = with(Audience::Project { project: "id:p1".into() });
        assert!(may_use(&project, "uhCAkAtlas", Some("id:p1"), false) && !may_use(&project, "uhCAkAtlas", Some("id:p2"), false) && !may_use(&project, "uhCAkAtlas", None, false));
        let local = with(Audience::LocalAis);
        assert!(may_use(&local, "uhCAkAtlas", None, false) && !may_use(&local, "uhCAkAtlas", None, true));
        assert!(!may_use(&with(Audience::None), "uhCAkJuno", None, false));
        // As it is stored.
        assert_eq!(serde_json::to_value(Audience::Ai { ai: "k".into() }).unwrap(), serde_json::json!({ "kind": "ai", "ai": "k" }));
        assert_eq!(serde_json::to_value(Audience::All).unwrap(), serde_json::json!({ "kind": "all" }));
        // A replaced, forgotten or set-aside item is given to no AI.
        for state in [ItemState::Replaced, ItemState::Forgotten, ItemState::SetAside, ItemState::NotNeeded] {
            assert!(!may_use(&ItemMeta { state, ..meta.clone() }, "uhCAkJuno", None, false), "{state:?}");
        }
    }

    #[test]
    fn already_kept() {
        let (row, _) = kept(&[], "it-1", "We buy flour from Hartwell Mills.", 2, "person");
        let have = vec![item_of(row)];
        let again = keep(&have, Keep { id: "it-2".into(), text: "We buy flour from Hartwell Mills. ", item_type: ItemType::Decision, ptr: ptr(2), as_of: 1, by: "person", audience: None, kind: "exchange", why: "said", quote: None, subject: None, now: NOW + 5 });
        assert_eq!(again.err().map(|i| i.row.id.as_str()), Some("it-1"));
        // Another part of the same exchange is another item; so is the same words from another exchange.
        assert!(keep(&have, Keep { id: "it-3".into(), text: "forty 12.5 kg bags", item_type: ItemType::Detail, ptr: ptr(2), as_of: 1, by: "person", audience: None, kind: "selection", why: "pressed", quote: None, subject: None, now: NOW }).is_ok());
        assert!(keep(&have, Keep { id: "it-4".into(), text: "We buy flour from Hartwell Mills.", item_type: ItemType::Decision, ptr: ptr(9), as_of: 1, by: "person", audience: None, kind: "exchange", why: "pressed", quote: None, subject: None, now: NOW }).is_ok());
        // A forgotten item does not stand in the way of keeping it again.
        let (gone, _) = forget(&have[0], NOW + 9);
        assert!(keep(&[item_of(gone)], Keep { id: "it-5".into(), text: "We buy flour from Hartwell Mills.", item_type: ItemType::Decision, ptr: ptr(2), as_of: 1, by: "person", audience: None, kind: "exchange", why: "pressed", quote: None, subject: None, now: NOW }).is_ok());
    }

    #[test]
    fn thats_done_is_read_from_fixed_words_and_closes_the_one_open_thing() {
        for yes in ["That's done.", "that\u{2019}s done", "OK, that is done now", "I did that", "Please mark that as done", "I've done that, thanks"] {
            assert!(said_done(yes), "{yes}");
        }
        for no in ["Is that done?", "That's done badly", "That's done, and the fitter also wants a deposit by Friday", "The quote is done", "Done"] {
            assert!(!said_done(no), "{no}");
        }
        let at = |id: &str, t: ItemType, c: i64| {
            let (row, _) = keep(&[], Keep { id: id.into(), text: &format!("Item {id}."), item_type: t, ptr: Ptr { c, s: 0, t: c, a: 0, b: 0 }, as_of: 1, by: "person", audience: None, kind: "exchange", why: "pressed", quote: None, subject: None, now: 1 }).ok().unwrap();
            Item { meta: meta_of(&row).unwrap(), row }
        };
        let items = vec![at("open-1", ItemType::Open, 100), at("detail", ItemType::Detail, 100), at("open-2", ItemType::Open, 200), at("open-3", ItemType::Open, 200)];
        assert_eq!(open_here(&items, 100).map(|i| i.row.id.as_str()), Some("open-1"), "the one thing this conversation left open");
        assert!(open_here(&items, 200).is_none(), "two open things: which one is not guessed");
        assert!(open_here(&items, 300).is_none());
    }

    #[test]
    fn remember_that_is_read_from_fixed_words() {
        for alone in ["remember that", "Remember this.", "please remember that!", "OK, keep that", "Don\u{2019}t forget that", "remember that please"] {
            assert_eq!(said_remember(alone), Some(SaidRemember::Before), "{alone}");
        }
        for with_words in ["Remember that the oven is serviced on Mondays.", "keep this: flour comes on Tuesdays", "don't forget that Sam is allergic to nuts"] {
            assert_eq!(said_remember(with_words), Some(SaidRemember::This), "{with_words}");
        }
        for not in ["Do you remember that?", "remember that time we talked about ovens?", "I remember that well", "remember thatch roofs leak", "What should I remember?", "keep going", ""] {
            assert_eq!(said_remember(not), None, "{not}");
        }
        // Asking for something to be kept out is not asking for it to be kept.
        for out in ["Keep this conversation out of memory.", "keep this out of memory", "Keep this chat private", "keep that out of it", "Keep this private.", "keep that to yourself", "Keep this between us"] {
            assert_eq!(said_remember(out), None, "{out}");
        }
    }

    #[test]
    fn controls_said_in_the_chat_are_read_from_fixed_words() {
        for (said, what) in [
            ("That's right.", SaidControl::Right),
            ("yes, that\u{2019}s correct", SaidControl::Right),
            ("Yep that is still true, thanks", SaidControl::Right),
            ("Forget that", SaidControl::Forget),
            ("Please forget what I just said.", SaidControl::Forget),
            ("ok, don't remember that", SaidControl::Forget),
            ("Keep this conversation out of memory", SaidControl::KeepOut),
            ("Stop using memory on this computer", SaidControl::OffHere),
            ("please turn memory off on this device.", SaidControl::OffHere),
            ("Don't use memory here", SaidControl::OffHere),
            ("ok, switch off memory on this computer please", SaidControl::OffHere),
            ("Use memory on this computer again", SaidControl::OnHere),
            ("use memory again on this computer", SaidControl::OnHere),
            ("Turn memory back on on this computer", SaidControl::OnHere),
            ("start using memory here again", SaidControl::OnHere),
            ("This is off the record.", SaidControl::KeepOut),
            ("please make this chat private", SaidControl::KeepOut),
            ("Can you keep this chat out of memory please", SaidControl::KeepOut),
        ] {
            assert_eq!(said_control(said), Some(what), "{said}");
        }
        for ordinary in [
            "Is that right?",
            "That's right about the price but the date is wrong",
            "That's rightly a concern",
            "Forget it",
            "Never mind",
            "Forget that idea, let's use the blue one instead",
            "Forget thatch, use slate",
            "Why is this conversation private?",
            "How do I keep this out of memory on my laptop when it is full",
            "Stop using memory",
            "Turn memory off",
            "How do I stop using memory on this computer?",
            "Stop using memory on this computer for the oven question",
            "Stop using so much memory on this computer",
            "Use memory on this computer again and again and tell me what you find",
            "Right",
            "",
        ] {
            assert_eq!(said_control(ordinary), None, "{ordinary}");
        }
    }

    #[test]
    fn that_means_what_the_reply_before_used_or_what_the_exchange_before_kept() {
        let at = |id: &str, c: i64, s: u32, why: &'static str| {
            let (row, _) = keep(&[], Keep { id: id.into(), text: &format!("Item {id}."), item_type: ItemType::Detail, ptr: Ptr { c, s, t: c, a: 0, b: 0 }, as_of: 1, by: "ai:uhCAkJuno", audience: None, kind: "exchange", why, quote: None, subject: None, now: 1 }).ok().unwrap();
            Item { meta: meta_of(&row).unwrap(), row }
        };
        let items = vec![at("here-before", 100, 3, "settled"), at("here-long-ago", 100, 0, "settled"), at("elsewhere", 200, 3, "settled"), at("used", 300, 9, "pressed")];
        let ids = |v: Vec<&Item>| v.into_iter().map(|i| i.row.id.clone()).collect::<Vec<_>>();
        // Said at message 5 of the conversation that began at 100.
        assert_eq!(ids(said_about(&items, 100, 5, &[])), ["here-before"]);
        assert_eq!(ids(said_about(&items, 100, 5, &["used".to_string()])), ["here-before", "used"], "two things: the caller does nothing");
        assert!(said_about(&items, 100, 9, &[]).is_empty(), "an exchange long before is not \"that\"");
        assert!(written_by_an_ai(&items[0]) && !written_by_an_ai(&items[3]));
        // What is forgotten is no longer "that".
        let (gone, _) = forget(&items[0], NOW);
        let after = vec![Item { meta: meta_of(&gone).unwrap(), row: gone }];
        assert!(said_about(&after, 100, 5, &[]).is_empty());
    }

    #[test]
    fn replaced_by_hand_is_history_and_can_be_undone() {
        let (old, _) = kept(&[], "it-1", "The shop opens at nine.", 2, "person");
        let (new, _) = kept(&[], "it-2", "The shop opens at eight.", 3, "person");
        let (old, new) = (item_of(old), item_of(new));
        assert!(replaceable_by_hand(&old, &new));
        assert!(!replaceable_by_hand(&old, &old), "nothing replaces itself");
        let (replaced, change) = replace(&old, &new.row.id, None, "person", NOW + 10);
        let after = item_of(replaced);
        assert_eq!((after.meta.state, after.meta.replaced_by.as_deref(), after.meta.state_by.as_str()), (ItemState::Replaced, Some("it-2"), "person"));
        assert!(!replaceable_by_hand(&after, &new), "what is replaced already is not replaced again");
        assert!(!replaceable_by_hand(&new, &after), "and replaces nothing itself");
        let (back, _) = undo(&change, &after, NOW + 20).unwrap();
        assert_eq!(item_of(back).meta.state, ItemState::Current);
    }

    #[test]
    fn undo_restores_and_is_logged() {
        let (row, kept_change) = kept(&[], "it-1", "Free delivery starts at $40.", 2, "ai:uhCAkJuno");
        let item = item_of(row);
        // Tidying replaces it; the person undoes that.
        let (replaced, change) = replace(&item, "it-2", Some("Free delivery starts at $60."), "tidy", NOW + 10);
        let after = item_of(replaced);
        assert_eq!((after.meta.state, after.meta.replaced_by.as_deref(), after.meta.state_by.as_str()), (ItemState::Replaced, Some("it-2"), "tidy"));
        let (back, entry) = undo(&change, &after, NOW + 20).unwrap();
        let back = item_of(back);
        assert_eq!((back.meta.state, back.meta.replaced_by.clone(), back.meta.now.clone()), (ItemState::Current, None, None));
        assert_eq!(back.row.updated_at, NOW + 20);
        assert_eq!((entry.what, entry.by.as_str(), &entry.undo), (What::Undone, "person", &Undo::Nothing));
        // Undoing a keep forgets the item (it stays, as history).
        let (gone, _) = undo(&kept_change, &item, NOW + 30).unwrap();
        assert_eq!(item_of(gone).meta.state, ItemState::Forgotten);
        // A correction undone puts the old words back.
        let (fixed, change) = correct(&item, "Free delivery starts at $45.", NOW + 40);
        let fixed = item_of(fixed);
        assert_eq!((fixed.row.text.as_str(), fixed.meta.corrected_from.as_deref(), fixed.meta.confirmed.as_ref().map(|c| c.how.as_str())), ("Free delivery starts at $45.", Some("Free delivery starts at $40."), Some("corrected")));
        let (back, _) = undo(&change, &fixed, NOW + 50).unwrap();
        assert_eq!((back.text.as_str(), meta_of(&back).unwrap().confirmed), ("Free delivery starts at $40.", None));
        // An undo is not itself undone, and a change of another item does nothing here.
        assert!(undo(&entry, &back_item(&item), NOW).is_none());
        let (other, _) = kept(&[], "it-9", "Something else.", 7, "person");
        assert!(undo(&change, &item_of(other), NOW).is_none());
        // The log: newest first, capped.
        let log = log_with(log_with(vec![], kept_change.clone(), 2), change.clone(), 2);
        assert_eq!(log.iter().map(|c| c.what).collect::<Vec<_>>(), vec![What::Corrected, What::Kept]);
        assert_eq!(log_with(log, entry, 2).iter().map(|c| c.what).collect::<Vec<_>>(), vec![What::Undone, What::Corrected]);
    }

    fn back_item(i: &Item) -> Item {
        i.clone()
    }

    #[test]
    fn a_replaced_item_survives_reduce() {
        let (old, _) = kept(&[], "it-1", "Free delivery starts at $40.", 2, "person");
        let (new, _) = kept(&[], "it-2", "Free delivery starts at $60.", 9, "person");
        let (old_replaced, _) = replace(&item_of(old.clone()), "it-2", None, "tidy", NOW + 10);
        // Every revision, as a reader gets them.
        let reduced = crate::memory_rows::reduce(vec![old, new, old_replaced]);
        let all = items(&reduced);
        assert_eq!(all.len(), 2, "the replaced item is still there, as history");
        let was = all.iter().find(|i| i.row.id == "it-1").unwrap();
        assert_eq!((was.meta.state, was.meta.replaced_by.as_deref()), (ItemState::Replaced, Some("it-2")));
        assert!(was.row.replaces.is_empty() && all.iter().all(|i| i.row.replaces.is_empty()));
    }

    #[test]
    fn an_edit_does_not_revive_a_replaced_item() {
        let (old, _) = kept(&[], "it-1", "Free delivery starts at $40.", 2, "person");
        let (replaced, _) = replace(&item_of(old), "it-2", None, "tidy", NOW + 10);
        let replaced = item_of(replaced);
        // The person fixes a typo in the old item, and changes who may use it.
        let (edited, _) = correct(&replaced, "Free delivery started at $40.", NOW + 20);
        let edited = item_of(edited);
        assert_eq!((edited.meta.state, edited.meta.replaced_by.as_deref()), (ItemState::Replaced, Some("it-2")));
        let (moved, _) = set_audience(&edited, Audience::LocalAis, NOW + 30);
        assert_eq!(meta_of(&moved).unwrap().state, ItemState::Replaced);
        assert!(!may_use(&meta_of(&moved).unwrap(), "uhCAkJuno", None, false));
    }

    #[test]
    fn the_guard_test_still_passes() {
        // An item adds no field to the row: everything of its own is under `structured.item`.
        let (row, _) = kept(&[], "it-1", "We buy flour from Hartwell Mills.", 2, "person");
        let v = serde_json::to_value(&row).unwrap();
        let allowed = ["id", "scope", "kind", "text", "structured", "as_of", "valid_to", "saved_by", "provenance", "pinned", "topics", "replaces", "deleted", "updated_at", "action_hash"];
        for k in v.as_object().unwrap().keys() {
            assert!(allowed.contains(&k.as_str()), "unexpected field on the row: {k}");
        }
        assert!(crate::memory_rows::KINDS.contains(&row.kind.as_str()));
        for t in [ItemType::Decision, ItemType::Done, ItemType::Open, ItemType::Detail, ItemType::AboutPerson] {
            assert!(crate::memory_rows::KINDS.contains(&kind_for(t)));
        }
    }

    #[test]
    fn what_a_later_version_adds_rides_through_an_edit() {
        let (row, _) = kept(&[], "it-1", "We buy flour from Hartwell Mills.", 2, "person");
        // A newer app wrote two things this version does not know: one in
        // the item, one beside it in the row's free fields.
        let mut v = serde_json::to_value(&row).unwrap();
        v["structured"]["item"]["importance"] = serde_json::json!(3);
        v["structured"]["other"] = serde_json::json!({ "x": 1 });
        let newer: MemoryRow = serde_json::from_value(v).unwrap();
        let (confirmed, change) = confirm(&item_of(newer), NOW + 5);
        assert_eq!(confirmed.structured.as_ref().unwrap()["item"]["importance"], 3);
        assert_eq!(confirmed.structured.as_ref().unwrap()["other"]["x"], 1);
        assert_eq!(meta_of(&confirmed).unwrap().confirmed.unwrap().how, "pressed");
        // And an undo keeps it too.
        let (back, _) = undo(&change, &item_of(confirmed), NOW + 9).unwrap();
        assert_eq!(back.structured.as_ref().unwrap()["item"]["importance"], 3);
        // A row that is not an item (a rule in the same scope, a system row) is not read as one.
        let mut sys = row.clone();
        sys.structured = Some(serde_json::json!({ "sys": "subject", "name": "Suppliers" }));
        assert!(meta_of(&sys).is_none() && items(&[sys]).is_empty());
    }

    #[test]
    fn remember_that_is_a_fixed_list_of_phrases() {
        assert_eq!(remember_phrase("remember that"), Some(Remember::TheExchangeBefore));
        assert_eq!(remember_phrase("  Remember this.  "), Some(Remember::TheExchangeBefore));
        assert_eq!(remember_phrase("please keep that!"), Some(Remember::TheExchangeBefore));
        assert_eq!(remember_phrase("Don\u{2019}t forget that"), Some(Remember::TheExchangeBefore));
        assert_eq!(remember_phrase("Remember that the Oven runs 10 degrees hot."), Some(Remember::This("the Oven runs 10 degrees hot.".into())));
        assert_eq!(remember_phrase("ok remember that: Dana is the packaging rep"), Some(Remember::This("Dana is the packaging rep".into())));
        // Not a request.
        assert_eq!(remember_phrase("do you remember that we changed suppliers?"), None);
        assert_eq!(remember_phrase("remember that?"), None);
        assert_eq!(remember_phrase("I remember that day."), None);
        assert_eq!(remember_phrase("remember thatch roofs need care"), None);
        assert_eq!(remember_phrase("what should I remember"), None);
        assert_eq!(remember_phrase(""), None);
    }

    #[test]
    fn an_edit_from_the_page_reads_as_written() {
        let e: ItemEdit = serde_json::from_value(serde_json::json!({ "do": "audience", "audience": { "kind": "project", "project": "id:p1" } })).unwrap();
        assert!(matches!(e, ItemEdit::Audience { audience: Audience::Project { .. } }));
        let e: ItemEdit = serde_json::from_value(serde_json::json!({ "do": "correct", "text": "x" })).unwrap();
        assert!(matches!(e, ItemEdit::Correct { .. }));
        let k: KeepInput = serde_json::from_value(serde_json::json!({ "text": "t", "itemType": "about_person", "ptr": { "c": 1, "s": 2, "t": 3, "a": 0, "b": 1 }, "asOf": 5, "kind": "exchange", "why": "pressed" })).unwrap();
        assert_eq!((k.item_type, k.by, k.audience), (ItemType::AboutPerson, None, None));
    }

    #[test]
    fn closing_and_forgetting() {
        let (row, _) = keep(&[], Keep { id: "it-1".into(), text: "Ask the fitter about a second deck.", item_type: ItemType::Open, ptr: ptr(6), as_of: 1, by: "ai:uhCAkJuno", audience: None, kind: "exchange", why: "settled", quote: Some("ask the fitter".into()), subject: None, now: NOW }).ok().unwrap();
        let open = item_of(row);
        assert!(may_use(&open.meta, "uhCAkJuno", None, false));
        let (done, change) = close(&open, true, "tidy", NOW + 5);
        assert_eq!((meta_of(&done).unwrap().state, change.what), (ItemState::Done, What::Closed));
        let (dropped, _) = close(&open, false, "person", NOW + 5);
        assert_eq!(meta_of(&dropped).unwrap().state, ItemState::NotNeeded);
        let (gone, change) = forget(&open, NOW + 6);
        assert_eq!((meta_of(&gone).unwrap().state, change.what, gone.deleted), (ItemState::Forgotten, What::Forgotten, false));
        // Forgetting is undone to exactly what it was.
        let (back, _) = undo(&change, &item_of(gone), NOW + 7).unwrap();
        assert_eq!(meta_of(&back).unwrap(), ItemMeta { ..open.meta.clone() });
    }

    #[test]
    fn not_replaced_makes_an_item_current_again_beside_the_other() {
        let (old, _) = kept(&[], "it-40", "Free delivery starts at $40.", 2, "ai:uhCAkJuno");
        let old = item_of(old);
        assert!(not_replaced(&old, NOW).is_none(), "an item that is not replaced has nothing to put back");
        let (replaced, _) = replace(&old, "it-60", None, "tidy", NOW + 1);
        let replaced = item_of(replaced);
        assert!(!may_use(&replaced.meta, "uhCAkJuno", None, false));
        let (back, change) = not_replaced(&replaced, NOW + 9).unwrap();
        let meta = meta_of(&back).unwrap();
        assert_eq!((meta.state, meta.replaced_by.clone(), meta.state_by.as_str(), meta.state_at), (ItemState::Current, None, "person", NOW + 9));
        assert!(may_use(&meta, "uhCAkJuno", None, false));
        assert_eq!((change.what, change.by.as_str(), change.undo.clone()), (What::Undone, "person", Undo::Nothing));
        assert_eq!(back.text, "Free delivery starts at $40.");
        // The pair is settled for tidying on every device, and counts as an undo of its change.
        let entry = crate::memory_tidy::not_replaced_entry("it-40", "it-60", NOW + 9);
        assert!(!crate::memory_tidy::may_judge(&[entry.clone()], "it-60", "it-40"));
        assert_eq!((entry.outcome.as_str(), entry.of.as_deref()), ("undone", Some("replaced")));
    }

    #[test]
    fn a_bubble_is_found_by_its_sequence_or_by_its_words() {
        let entries = vec![
            Recorded { seq: 0, by_person: true, at: 10, content: "raise the threshold to $60".into() },
            Recorded { seq: 1, by_person: false, at: 11, content: "Updated.  From Monday,\nfree delivery starts at $60.".into() },
            Recorded { seq: 2, by_person: true, at: 12, content: "Thanks.".into() },
            Recorded { seq: 3, by_person: false, at: 13, content: "You're welcome.".into() },
            Recorded { seq: 4, by_person: true, at: 14, content: "Thanks.".into() },
        ];
        assert_eq!(recorded_for(&entries, Some(1), false, "anything").map(|e| e.at), Some(11));
        // A sequence that is the other speaker's is not trusted: the words decide.
        assert_eq!(recorded_for(&entries, Some(1), true, "Thanks.").map(|e| e.seq), Some(4), "the latest message with those words");
        assert_eq!(recorded_for(&entries, None, false, "Updated. From Monday, free delivery starts at $60.").map(|e| e.seq), Some(1), "spacing does not matter");
        assert_eq!(recorded_for(&entries, None, true, "You're welcome."), None, "the AI said that, not the person");
        assert_eq!(recorded_for(&entries, None, true, "  "), None);
        assert_eq!(recorded_for(&entries, Some(9), false, "never said"), None);
    }

    #[test]
    fn the_words_kept_are_the_selection_or_the_message_within_what_an_item_holds() {
        assert_eq!(words_to_keep("  the whole reply  ", None), "the whole reply");
        assert_eq!(words_to_keep("the whole reply", Some(" whole ")), "whole");
        assert_eq!(words_to_keep("the whole reply", Some("   ")), "the whole reply", "an empty selection is no selection");
        assert_eq!(words_to_keep(&"é".repeat(3000), None).chars().count(), 1000);
        let k: KeepMessageInput = serde_json::from_value(serde_json::json!({ "conversation": "h", "role": "assistant", "text": "t", "seq": 3, "agentKey": "k" })).unwrap();
        assert_eq!((k.seq, k.agent_key.as_deref(), k.selection, k.why), (Some(3), Some("k"), None, None));
    }

    #[test]
    fn a_change_made_on_another_device_says_so_and_ours_do_not() {
        let c = |id: &str, at: i64| Change { id: id.into(), at, what: What::Confirmed, items: vec!["it-1".into()], by: "person".into(), undo: Undo::Nothing, device: None };
        let mine = vec![c("here-1", 30), c("both", 20)];
        let theirs = vec![c("both", 20), c("there-1", 40), Change { device: Some("the laptop".into()), ..c("third", 10) }];
        let (log, new) = folded_log(mine.clone(), theirs.clone(), Some("the desktop"), 100);
        assert_eq!(new, 2);
        let of = |id: &str| log.iter().find(|x| x.id == id).unwrap().device.clone();
        assert_eq!((of("here-1"), of("both")), (None, None), "what this device did is not said to be elsewhere, even when the other device lists it too");
        assert_eq!(of("there-1").as_deref(), Some("the desktop"));
        assert_eq!(of("third").as_deref(), Some("the laptop"), "a change the other device took from a third keeps where it was made");
        assert_eq!(log.iter().map(|x| x.id.as_str()).collect::<Vec<_>>(), vec!["there-1", "here-1", "both", "third"], "newest first");
        // Folded again, nothing is added and nothing changes.
        let (again, new) = folded_log(log.clone(), theirs, Some("the desktop"), 100);
        assert_eq!((new, again), (0, log));
        // A restore of this device's own backup marks nothing.
        let (own, _) = folded_log(Vec::new(), mine, None, 100);
        assert!(own.iter().all(|x| x.device.is_none()));
    }

    #[test]
    fn a_restored_log_adds_what_this_device_lacks_and_repeats_nothing() {
        let c = |id: &str, at: i64| Change { id: id.into(), at, what: What::Kept, items: vec![], by: "person".into(), undo: Undo::Forget, device: None };
        let mine = vec![c("c", 30), c("a", 10)];
        let theirs = vec![c("d", 40), c("b", 20), c("a", 10)];
        let ids = |log: &[Change]| log.iter().map(|x| x.id.clone()).collect::<Vec<_>>();
        assert_eq!(ids(&merged_log(mine.clone(), theirs.clone(), 10)), vec!["d", "c", "b", "a"]);
        assert_eq!(ids(&merged_log(mine.clone(), theirs, 2)), vec!["d", "c"], "the newest are the ones held");
        assert_eq!(ids(&merged_log(mine.clone(), vec![], 10)), ids(&mine));
    }

    #[test]
    fn words_the_person_wrote_about_themselves_are_theirs_and_are_given() {
        let (row, change) = keep_words("it-w".into(), "  Vegetarian since 2019.  ", 5_000);
        let item = items(&[row]).pop().expect("an item");
        assert_eq!(item.row.text, "Vegetarian since 2019.");
        assert_eq!((item.meta.state, item.meta.item_type), (ItemState::Current, ItemType::AboutPerson));
        assert!(item.meta.confirmed.is_some() && item.meta.source.ptr.is_none());
        assert_eq!(item.meta.subject.as_deref(), Some(crate::memory_subjects::ABOUT_YOU));
        assert_eq!((change.what, change.by.as_str(), change.undo.clone()), (What::Kept, "person", Undo::Forget));
    }
}
