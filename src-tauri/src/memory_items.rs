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
fn with_meta(mut row: MemoryRow, meta: &ItemMeta) -> MemoryRow {
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
    Undone,
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

fn data_key(app: &tauri::AppHandle) -> Result<[u8; 32], String> {
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
    std::fs::write(&path, out).map_err(|e| e.to_string())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    Ok(())
}

fn now_ms() -> i64 {
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

fn view(row: &MemoryRow) -> Option<ItemView> {
    Some(ItemView { id: row.id.clone(), text: row.text.clone(), as_of: row.as_of, updated_at: row.updated_at, saved_by: row.saved_by.clone(), item: meta_of(row)? })
}

/// Save a changed item and note the change in the log.
async fn save(app: &tauri::AppHandle, key: &[u8; 32], row: MemoryRow, change: Change) -> Result<ItemView, String> {
    let saved = crate::memory_rows::save_and_cache(app, row).await?;
    write_changes(app, key, &log_with(read_changes(app, key), change, CHANGES_KEPT))?;
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
        Err(had) => Ok(KeptAnswer { item: view(&had.row).ok_or("not an item")?, change: None, already: true }),
        Ok((row, change)) => {
            let change_id = change.id.clone();
            Ok(KeptAnswer { item: save(&app, &key, row, change).await?, change: Some(change_id), already: false })
        }
    }
}

/// A change the person makes to one item.
#[derive(Deserialize)]
#[serde(tag = "do", rename_all = "snake_case")]
pub enum ItemEdit {
    Confirm,
    Correct { text: String },
    Forget,
    Audience { audience: Audience },
    Close { done: bool },
}

#[tauri::command]
pub async fn memory_item_change(app: tauri::AppHandle, id: String, edit: ItemEdit) -> Result<ItemView, String> {
    let key = data_key(&app)?;
    let have = held(&app, &key);
    let item = have.iter().find(|i| i.row.id == id).ok_or("no such item")?;
    let now = now_ms();
    let (row, change) = match edit {
        ItemEdit::Confirm => confirm(item, now),
        ItemEdit::Correct { text } => correct(item, &crate::secret_mask::hand_on(&text).text, now),
        ItemEdit::Forget => forget(item, now),
        ItemEdit::Audience { audience } => set_audience(item, audience, now),
        ItemEdit::Close { done } => close(item, done, "person", now),
    };
    save(&app, &key, row, change).await
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
    for id in &change.items {
        let Some(item) = have.iter().find(|i| &i.row.id == id) else { continue };
        if let Some((row, entry)) = undo(&change, item, now) {
            out.push(save(&app, &key, row, entry).await?);
        }
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
}
