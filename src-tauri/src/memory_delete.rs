//! Deleting, said exactly: what goes with a conversation, with an AI, and
//! with all of memory. Each has a count the person is shown before they
//! decide, taken from the same rule the delete then follows.
//!
//! An item that is deleted is a row marked deleted, with none of its words,
//! so it goes on every device and is not brought back by a read of the
//! records; its earlier copies are then removed from the record, each
//! device removing the ones it wrote. Forgetting an
//! item is a different thing (it can be undone, from Recently) and lives
//! with the items.

use crate::memory_items::{Audience, Item, ItemState};
use crate::memory_rows::MemoryRow;
use serde::Serialize;

/// The row as it is saved to delete it: its id, its place, the mark and
/// the ids it replaced (so nothing it replaced comes back), and none of its
/// words.
pub(crate) fn deleted(row: &MemoryRow, now: i64) -> MemoryRow {
    MemoryRow { deleted: true, updated_at: now, action_hash: None, text: String::new(), structured: None, topics: Vec::new(), ..row.clone() }
}

/// The items made from a conversation, by its start.
pub(crate) fn items_from(items: &[Item], started_at: i64) -> Vec<&Item> {
    items.iter().filter(|i| i.meta.source.ptr.is_some_and(|p| p.c == started_at)).collect()
}

/// What deleting an AI does to the items it kept: those for that AI alone
/// go; those for all the person's AIs (or a project) stay, without the
/// conversations they came from.
#[derive(Debug, Default, PartialEq)]
pub(crate) struct AiPlan {
    /// Ids of the items that stay.
    pub stay: Vec<String>,
    /// Ids of the items that are deleted.
    pub go: Vec<String>,
}

/// `founding`: the AI's founding key. An item is "kept by" the AI when the
/// AI saved it, or when it is for that AI alone.
pub(crate) fn ai_plan(items: &[Item], founding: &str) -> AiPlan {
    let mut plan = AiPlan::default();
    let by = format!("ai:{founding}");
    for i in items {
        let only_theirs = matches!(&i.meta.audience, Audience::Ai { ai } if ai == founding);
        if only_theirs {
            plan.go.push(i.row.id.clone());
        } else if i.row.saved_by == by {
            plan.stay.push(i.row.id.clone());
        }
    }
    plan
}

/// An item that stays after its AI was deleted says so: the conversation
/// it came from went with the AI.
pub(crate) fn source_gone(item: &Item, ai_name: &str, now: i64) -> MemoryRow {
    let mut meta = item.meta.clone();
    meta.more.insert("source_gone".into(), serde_json::json!({ "with": ai_name }));
    let mut row = crate::memory_items::with_meta(item.row.clone(), &meta);
    row.updated_at = now;
    row.action_hash = None;
    row
}

const DELETED_ALL: &str = "sys-deleted-all";

/// Every row of memory's own scope, marked deleted (items, subjects, the
/// "Not used" list, what was heard for rule offers, the Map's colors), and
/// the one row that says when. Conversations and rules live elsewhere and
/// are untouched.
pub(crate) fn all_deleted(rows: &[MemoryRow], now: i64) -> Vec<MemoryRow> {
    let mut out: Vec<MemoryRow> = rows.iter().filter(|r| !r.deleted && r.id != DELETED_ALL).map(|r| deleted(r, now)).collect();
    out.push(MemoryRow {
        id: DELETED_ALL.to_string(),
        scope: crate::memory_items::SCOPE.to_string(),
        kind: "note".to_string(),
        text: String::new(),
        structured: Some(serde_json::json!({ "sys": "deleted_all", "at": now })),
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
    });
    out
}

/// When the person deleted all memory, if they did (ms).
pub(crate) fn deleted_all_at(rows: &[MemoryRow]) -> Option<i64> {
    rows.iter().filter(|r| !r.deleted && r.id == DELETED_ALL).find_map(|r| r.structured.as_ref()?.get("at")?.as_i64())
}

/// What "Delete all memory" would delete, counted.
#[derive(Serialize, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AllPreview {
    /// Kept items of every state the person can see (not ones already forgotten).
    pub items: usize,
    pub subjects: usize,
    /// Conversations and messages on the "Not used" list.
    pub not_used: usize,
}

pub(crate) fn all_preview(rows: &[MemoryRow]) -> AllPreview {
    let items = crate::memory_items::items(rows);
    let all = crate::memory_subjects::subjects(rows);
    let live = |i: &&Item| matches!(i.meta.state, ItemState::Current | ItemState::Open | ItemState::Done | ItemState::Replaced);
    let held: Vec<&Item> = items.iter().filter(live).collect();
    // The same count the Map gives: every subject that holds something, and
    // "About you" among them (an item in no subject is drawn there).
    let subjects = held
        .iter()
        .map(|i| i.meta.subject.as_deref().and_then(|id| crate::memory_subjects::standing(&all, id)).map_or(crate::memory_subjects::ABOUT_YOU, |s| s.id.as_str()))
        .collect::<std::collections::BTreeSet<&str>>()
        .len();
    AllPreview { items: held.len(), subjects, not_used: crate::memory_use::conv_uses(rows).iter().filter(|u| crate::memory_use::restricts(u)).count() }
}

fn data_key(app: &tauri::AppHandle) -> Result<[u8; 32], String> {
    use tauri::Manager;
    app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get()?.data_key()
}

fn rows_now(app: &tauri::AppHandle) -> Result<([u8; 32], Vec<MemoryRow>), String> {
    let key = data_key(app)?;
    let rows = crate::memory_rows::read_cache(app, &key, crate::memory_items::SCOPE);
    Ok((key, rows))
}

/// How many memory items were made from a conversation: shown before it is deleted.
#[tauri::command]
pub fn memory_delete_preview(app: tauri::AppHandle, started_at: i64) -> Result<usize, String> {
    let (_, rows) = rows_now(&app)?;
    Ok(items_from(&crate::memory_items::items(&rows), started_at).len())
}

/// A conversation was deleted: the items made from it are deleted too.
#[tauri::command]
pub async fn memory_conversation_deleted(app: tauri::AppHandle, started_at: i64) -> Result<usize, String> {
    let (_, rows) = rows_now(&app)?;
    let items = crate::memory_items::items(&rows);
    let now = crate::memory_items::now_ms();
    let mut n = 0;
    for i in items_from(&items, started_at) {
        crate::memory_rows::save_and_cache(&app, deleted(&i.row, now)).await?;
        n += 1;
    }
    if n > 0 {
        crate::memory_tidying::words_changed(&app);
        crate::memory_rows::removal_after_delete(&app, crate::memory_items::SCOPE);
    }
    Ok(n)
}

/// What deleting an AI does to what it kept.
#[derive(Serialize, Debug, PartialEq)]
pub struct AiPreview {
    pub stay: usize,
    pub go: usize,
}

#[tauri::command]
pub fn memory_ai_delete_preview(app: tauri::AppHandle, founding: String) -> Result<AiPreview, String> {
    let (_, rows) = rows_now(&app)?;
    let plan = ai_plan(&crate::memory_items::items(&rows), &founding);
    Ok(AiPreview { stay: plan.stay.len(), go: plan.go.len() })
}

/// An AI was deleted: its own items go, the rest stay and say where their
/// conversations went.
#[tauri::command]
pub async fn memory_ai_deleted(app: tauri::AppHandle, founding: String, name: String) -> Result<AiPreview, String> {
    let (_, rows) = rows_now(&app)?;
    let items = crate::memory_items::items(&rows);
    let plan = ai_plan(&items, &founding);
    let now = crate::memory_items::now_ms();
    for i in items.iter().filter(|i| plan.go.contains(&i.row.id)) {
        crate::memory_rows::save_and_cache(&app, deleted(&i.row, now)).await?;
    }
    for i in items.iter().filter(|i| plan.stay.contains(&i.row.id)) {
        crate::memory_rows::save_and_cache(&app, source_gone(i, &name, now)).await?;
    }
    crate::memory_tidying::words_changed(&app);
    if !plan.go.is_empty() {
        crate::memory_rows::removal_after_delete(&app, crate::memory_items::SCOPE);
    }
    Ok(AiPreview { stay: plan.stay.len(), go: plan.go.len() })
}

#[tauri::command]
pub fn memory_delete_all_preview(app: tauri::AppHandle) -> Result<AllPreview, String> {
    let (_, rows) = rows_now(&app)?;
    Ok(all_preview(&rows))
}

/// Delete all memory: every row of memory's scope, and this device's own
/// files of it (the list of changes, what came up together, what waits to
/// be looked at). Conversations and rules stay. Returns how many rows went.
#[tauri::command]
pub async fn memory_delete_all(app: tauri::AppHandle) -> Result<usize, String> {
    let (_, rows) = rows_now(&app)?;
    let now = crate::memory_items::now_ms();
    let to_write = all_deleted(&rows, now);
    let n = to_write.len().saturating_sub(1);
    for row in to_write {
        crate::memory_rows::save_and_cache(&app, row).await?;
    }
    crate::memory_items::clear_changes(&app);
    crate::memory_subjects::clear_came_up(&app);
    crate::memory_keeping::clear_queue(&app);
    crate::memory_tidying::clear_queue(&app);
    crate::memory_tidying::words_changed(&app);
    crate::memory_rows::removal_after_delete(&app, crate::memory_items::SCOPE);
    log::info!("[memory] all memory deleted: {n} row(s)");
    Ok(n)
}

/// When all memory was deleted, for the Map's empty words.
#[tauri::command]
pub fn memory_deleted_all_at(app: tauri::AppHandle) -> Option<i64> {
    rows_now(&app).ok().and_then(|(_, rows)| deleted_all_at(&rows))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_block::Ptr;
    use crate::memory_items::{items, keep, meta_of, ItemType, Keep};

    fn item(id: &str, by: &str, audience: Audience, conversation: i64) -> MemoryRow {
        let (row, _) = keep(&[], Keep { id: id.into(), text: &format!("Item {id}."), item_type: ItemType::Detail, ptr: Ptr { c: conversation, s: 2, t: 1, a: 0, b: 0 }, as_of: 1, by, audience: Some(audience), kind: "exchange", why: "pressed", quote: None, subject: Some("sub-a".into()), now: 1 }).ok().unwrap();
        row
    }

    #[test]
    fn delete_preview_counts() {
        let rows = vec![item("a", "person", Audience::All, 100), item("b", "person", Audience::All, 100), item("c", "person", Audience::All, 200)];
        let all = items(&rows);
        assert_eq!(items_from(&all, 100).len(), 2);
        assert_eq!(items_from(&all, 200).len(), 1);
        assert_eq!(items_from(&all, 300).len(), 0);
        let gone = deleted(&rows[0], 9);
        assert!(gone.deleted && gone.updated_at == 9 && gone.id == "a" && gone.action_hash.is_none());
        assert!(gone.text.is_empty() && gone.structured.is_none() && gone.topics.is_empty(), "the saved mark carries none of the item's words");
        // A deleted row is no item any more.
        assert_eq!(items(&[gone, rows[1].clone()]).len(), 1);
    }

    #[test]
    fn items_for_all_survive_an_ai_delete() {
        let rows = vec![
            item("for-all", "ai:atlas", Audience::All, 100),
            item("only-atlas", "ai:atlas", Audience::Ai { ai: "atlas".into() }, 100),
            item("person-for-atlas", "person", Audience::Ai { ai: "atlas".into() }, 100),
            item("junos", "ai:juno", Audience::All, 200),
            item("persons", "person", Audience::All, 200),
        ];
        let all = items(&rows);
        let plan = ai_plan(&all, "atlas");
        assert_eq!(plan, AiPlan { stay: vec!["for-all".into()], go: vec!["only-atlas".into(), "person-for-atlas".into()] }, "what was for Atlas alone goes; what Atlas kept for everyone stays; others' items are not in the count");
        let stays = source_gone(&all[0], "Atlas", 7);
        let meta = meta_of(&stays).unwrap();
        assert_eq!(meta.more["source_gone"], serde_json::json!({ "with": "Atlas" }));
        assert_eq!((meta.state, stays.text.as_str()), (ItemState::Current, "Item for-all."), "the item is as it was, and says where its conversation went");
    }

    #[test]
    fn delete_memory_only_keeps_conversations_and_rules() {
        let mut rows = vec![item("a", "person", Audience::All, 100), item("b", "ai:atlas", Audience::All, 100)];
        rows.push(crate::memory_subjects::subject_row(&crate::memory_subjects::Subject { id: "sub-a".into(), name: "Suppliers".into(), name_accepted: true, from: crate::memory_subjects::MadeFrom::ByHand, merged_into: None }, 1));
        assert_eq!(all_preview(&rows), AllPreview { items: 2, subjects: 1, not_used: 0 });
        assert_eq!(deleted_all_at(&rows), None);
        let written = all_deleted(&rows, 50);
        assert_eq!(written.len(), 4, "every row of memory's scope, and the row that says when");
        assert!(written.iter().take(3).all(|r| r.deleted && r.scope == crate::memory_items::SCOPE));
        // Only memory's own scope is written: a rule is a row of the person's scope, a conversation is no row at all.
        assert!(written.iter().all(|r| r.scope == crate::memory_items::SCOPE));
        let after = crate::memory_rows::reduce(written);
        assert_eq!(all_preview(&after), AllPreview { items: 0, subjects: 0, not_used: 0 });
        assert_eq!(deleted_all_at(&after), Some(50));
        // Done a second time, nothing is left to delete but the date moves.
        let again = all_deleted(&after, 60);
        assert_eq!((again.len(), deleted_all_at(&crate::memory_rows::reduce(again))), (1, Some(60)));
    }
}
