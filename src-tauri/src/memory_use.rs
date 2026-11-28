//! Who may draw on what: a conversation's audience, its private mark and
//! the messages the person chose not to have used; forgetting and deleting
//! that says exactly what happens (deleting is in `memory_delete`).
//!
//! What the person decides about a conversation is kept as a system row
//! in memory's scope, named by the conversation's start (the same on
//! every device), so it survives the index being thrown away and rebuilt,
//! travels to the person's other devices, and comes back with a restore.
//! A change takes effect at once: nothing is read or imported again.

use crate::memory_items::{Audience, Item, ItemState, SCOPE};
use crate::memory_rows::MemoryRow;
use serde::{Deserialize, Serialize};

/// What the person decided about one conversation.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ConvUse {
    /// The conversation's start (microseconds): its name everywhere.
    pub started_at: i64,
    /// Absent: all the person's AIs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub audience: Option<Audience>,
    /// Kept out of memory: never searched, nothing kept from it.
    #[serde(default)]
    pub private: bool,
    /// Messages not to be used, by sequence.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded: Vec<u32>,
}

/// The decision as the row that is saved (one per conversation).
pub(crate) fn conv_row(u: &ConvUse, now: i64) -> MemoryRow {
    MemoryRow {
        id: format!("sys-conv-{}", u.started_at),
        scope: SCOPE.to_string(),
        kind: "note".to_string(),
        text: String::new(),
        structured: Some(serde_json::json!({ "sys": "conv", "conv": u })),
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
    }
}

/// Every conversation the person decided something about.
pub(crate) fn conv_uses(rows: &[MemoryRow]) -> Vec<ConvUse> {
    rows.iter()
        .filter(|r| !r.deleted)
        .filter_map(|r| {
            let s = r.structured.as_ref()?;
            (s.get("sys")?.as_str()? == "conv").then(|| serde_json::from_value(s.get("conv")?.clone()).ok()).flatten()
        })
        .collect()
}

/// One change the person makes to how a conversation is used.
#[derive(Deserialize, Debug, Default, Clone)]
#[serde(rename_all = "camelCase")]
pub struct ConvChange {
    pub started_at: i64,
    /// Keep it out of memory, or let it be used again.
    #[serde(default)]
    pub private: Option<bool>,
    /// Who may use it; "all" clears the choice.
    #[serde(default)]
    pub audience: Option<Audience>,
    /// A message not to be used.
    #[serde(default)]
    pub exclude: Option<u32>,
    /// A message to be used again.
    #[serde(default)]
    pub include: Option<u32>,
}

/// The decision after a change. What is not named stays as it was.
pub(crate) fn changed(uses: &[ConvUse], c: &ConvChange) -> ConvUse {
    let mut u = uses.iter().find(|u| u.started_at == c.started_at).cloned().unwrap_or(ConvUse { started_at: c.started_at, audience: None, private: false, excluded: Vec::new() });
    if let Some(p) = c.private {
        u.private = p;
    }
    if let Some(a) = &c.audience {
        u.audience = if matches!(a, Audience::All) { None } else { Some(a.clone()) };
    }
    if let Some(seq) = c.exclude {
        if !u.excluded.contains(&seq) {
            u.excluded.push(seq);
            u.excluded.sort_unstable();
        }
    }
    if let Some(seq) = c.include {
        u.excluded.retain(|s| *s != seq);
    }
    u
}

/// Does a decision say anything other than the default (all the person's
/// AIs, every message)? These are the ones the "Not used" list shows.
pub(crate) fn restricts(u: &ConvUse) -> bool {
    u.private || !u.excluded.is_empty() || u.audience.is_some()
}

fn rows_now(app: &tauri::AppHandle) -> Result<Vec<MemoryRow>, String> {
    use tauri::Manager;
    let key = app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get()?.data_key()?;
    Ok(crate::memory_rows::read_cache(app, &key, SCOPE))
}

/// What the person decided about one conversation (the default when nothing).
#[tauri::command]
pub fn memory_conversation_use(app: tauri::AppHandle, started_at: i64) -> Result<ConvUse, String> {
    Ok(changed(&conv_uses(&rows_now(&app)?), &ConvChange { started_at, ..Default::default() }))
}

/// Change how a conversation is used. It takes effect with the next
/// question: nothing is read or imported again.
#[tauri::command]
pub async fn memory_conversation_use_set(app: tauri::AppHandle, change: ConvChange) -> Result<ConvUse, String> {
    if change.started_at <= 0 {
        return Err("This conversation is still being saved. Try again in a moment.".into());
    }
    let next = changed(&conv_uses(&rows_now(&app)?), &change);
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    crate::memory_rows::save_and_cache(&app, conv_row(&next, now)).await?;
    Ok(next)
}

/// Every conversation with a decision other than the default.
#[tauri::command]
pub fn memory_conversations_not_used(app: tauri::AppHandle) -> Result<Vec<ConvUse>, String> {
    Ok(conv_uses(&rows_now(&app)?).into_iter().filter(restricts).collect())
}

/// A turn, as far as use goes.
#[derive(Clone, Copy)]
pub(crate) struct Asking<'a> {
    /// The AI's founding key.
    pub ai: &'a str,
    /// The project the turn is in.
    pub project: Option<&'a str>,
    /// The model runs somewhere other than the person's own devices.
    pub online: bool,
}

/// May this turn be given a message of a conversation?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum May {
    Use,
    /// It may not, because of who the conversation is for: said to the
    /// person as "cannot use N conversations", with a way to allow it.
    Withheld,
    /// It is kept out of memory altogether (private, or that message
    /// excluded): never found, never counted.
    Never,
}

fn audience_allows(a: &Audience, who: &Asking) -> bool {
    match a {
        Audience::All => true,
        Audience::Ai { ai } => ai == who.ai,
        Audience::Project { project } => who.project == Some(project.as_str()),
        Audience::LocalAis => !who.online,
        Audience::None => false,
    }
}

/// May a turn be given the message at `seq` of the conversation that
/// started at `started_at`? The default for a conversation is all the
/// person's AIs.
pub(crate) fn may_use_message(uses: &[ConvUse], started_at: i64, seq: u32, who: &Asking) -> May {
    let Some(u) = uses.iter().find(|u| u.started_at == started_at) else { return May::Use };
    if u.private || u.excluded.contains(&seq) {
        return May::Never;
    }
    match &u.audience {
        Some(a) if !audience_allows(a, who) => May::Withheld,
        _ => May::Use,
    }
}

/// Is a conversation kept out of memory (so nothing is queued for
/// keeping from it and its passages leave the index)?
pub(crate) fn is_private(uses: &[ConvUse], started_at: i64) -> bool {
    uses.iter().any(|u| u.started_at == started_at && u.private)
}

/// Exchanges a forgotten item came from are not given either: the places
/// (conversation start, sequence) of every forgotten item's source.
pub(crate) fn forgotten_sources(items: &[Item]) -> Vec<(i64, u32)> {
    items.iter().filter(|i| i.meta.state == ItemState::Forgotten).filter_map(|i| i.meta.source.ptr.map(|p| (p.c, p.s))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_block::Ptr;
    use crate::memory_items::{forget, keep, may_use, meta_of, set_audience, ItemType, Keep};

    const C1: i64 = 1_768_896_000_000_000;
    const C2: i64 = 1_769_000_000_000_000;

    fn who<'a>(ai: &'a str, project: Option<&'a str>, online: bool) -> Asking<'a> {
        Asking { ai, project, online }
    }

    fn item(id: &str, conv: i64, seq: u32, by: &str, audience: Option<Audience>) -> Item {
        let (row, _) = keep(&[], Keep { id: id.into(), text: &format!("Item {id}."), item_type: ItemType::Detail, ptr: Ptr { c: conv, s: seq, t: conv + 1, a: 0, b: 5 }, as_of: 1, by, audience, kind: "exchange", why: "settled", quote: None, subject: None, now: 5 }).ok().unwrap();
        let meta = meta_of(&row).unwrap();
        Item { row, meta }
    }

    #[test]
    fn audience_change_needs_no_reindex() {
        // The decision is a row; the index is not touched. Before: everyone.
        let mut uses: Vec<ConvUse> = Vec::new();
        assert_eq!(may_use_message(&uses, C1, 3, &who("juno", None, false)), May::Use);
        // "Atlas only", pressed on an import.
        uses.push(ConvUse { started_at: C1, audience: Some(Audience::Ai { ai: "atlas".into() }), private: false, excluded: vec![] });
        assert_eq!(may_use_message(&uses, C1, 3, &who("juno", None, false)), May::Withheld);
        assert_eq!(may_use_message(&uses, C1, 3, &who("atlas", None, false)), May::Use);
        // "Let Juno use them": one row later, at once.
        uses[0].audience = None;
        assert_eq!(may_use_message(&uses, C1, 3, &who("juno", None, false)), May::Use);
        // As rows, read back the same; never read as items.
        let rows = vec![conv_row(&ConvUse { started_at: C1, audience: Some(Audience::LocalAis), private: false, excluded: vec![4] }, 9)];
        assert_eq!(conv_uses(&rows)[0].excluded, vec![4]);
        assert!(crate::memory_items::items(&rows).is_empty());
        // A newer decision about the same conversation replaces the older one.
        let both = crate::memory_rows::reduce(vec![conv_row(&ConvUse { started_at: C1, audience: None, private: false, excluded: vec![] }, 5), conv_row(&ConvUse { started_at: C1, audience: None, private: true, excluded: vec![] }, 9)]);
        assert_eq!((both.len(), conv_uses(&both)[0].private), (1, true));
    }

    #[test]
    fn local_ais_only_blocks_an_online_ai() {
        let uses = vec![ConvUse { started_at: C1, audience: Some(Audience::LocalAis), private: false, excluded: vec![] }];
        assert_eq!(may_use_message(&uses, C1, 0, &who("juno", None, false)), May::Use);
        assert_eq!(may_use_message(&uses, C1, 0, &who("juno", None, true)), May::Withheld);
        let none = vec![ConvUse { started_at: C1, audience: Some(Audience::None), private: false, excluded: vec![] }];
        assert_eq!(may_use_message(&none, C1, 0, &who("juno", None, false)), May::Withheld);
        // The same value on an item.
        let i = item("it-1", C1, 0, "person", Some(Audience::LocalAis));
        assert!(may_use(&i.meta, "juno", None, false) && !may_use(&i.meta, "juno", None, true));
    }

    #[test]
    fn a_private_conversation_has_no_passages_and_no_queue_entries() {
        let uses = vec![ConvUse { started_at: C1, audience: None, private: true, excluded: vec![] }];
        // Never found, by anyone, and not counted as withheld (it is not "cannot use": it is not there).
        for w in [who("juno", None, false), who("atlas", Some("id:p1"), true)] {
            assert_eq!(may_use_message(&uses, C1, 2, &w), May::Never);
        }
        assert!(is_private(&uses, C1) && !is_private(&uses, C2));
        assert_eq!(may_use_message(&uses, C2, 2, &who("juno", None, false)), May::Use);
    }

    #[test]
    fn excluded_message_is_never_given() {
        let uses = vec![ConvUse { started_at: C1, audience: None, private: false, excluded: vec![4, 5] }];
        assert_eq!(may_use_message(&uses, C1, 4, &who("juno", None, false)), May::Never);
        assert_eq!(may_use_message(&uses, C1, 5, &who("juno", None, false)), May::Never);
        assert_eq!(may_use_message(&uses, C1, 6, &who("juno", None, false)), May::Use);
    }

    #[test]
    fn forget_blocks_the_item_and_its_exchange() {
        let kept = item("it-1", C1, 7, "person", None);
        let (gone, _) = forget(&kept, 9);
        let gone = Item { meta: meta_of(&gone).unwrap(), row: gone };
        assert!(!may_use(&gone.meta, "juno", None, false));
        assert_eq!(forgotten_sources(&[gone, item("it-2", C2, 1, "person", None)]), vec![(C1, 7)]);
    }

    #[test]
    fn a_project_item_is_given_in_its_project_and_nowhere_else() {
        let i = item("it-1", C1, 0, "person", Some(Audience::Project { project: "id:bakery".into() }));
        // Any of the person's AIs, while working in that project.
        assert!(may_use(&i.meta, "juno", Some("id:bakery"), false) && may_use(&i.meta, "atlas", Some("id:bakery"), true));
        assert!(!may_use(&i.meta, "juno", Some("id:recipes"), false) && !may_use(&i.meta, "juno", None, false));
        // The same for a conversation held for a project.
        let uses = vec![ConvUse { started_at: C1, audience: Some(Audience::Project { project: "id:bakery".into() }), private: false, excluded: vec![] }];
        assert_eq!(may_use_message(&uses, C1, 0, &who("juno", Some("id:bakery"), false)), May::Use);
        assert_eq!(may_use_message(&uses, C1, 0, &who("juno", None, false)), May::Withheld);
    }

    #[test]
    fn widening_a_project_item_takes_effect_at_once() {
        let i = item("it-1", C1, 0, "person", Some(Audience::Project { project: "id:bakery".into() }));
        assert!(!may_use(&i.meta, "juno", None, false));
        let (wider, change) = set_audience(&i, Audience::All, 9);
        assert!(may_use(&meta_of(&wider).unwrap(), "juno", None, false));
        // And it can be put back.
        let wider_item = Item { meta: meta_of(&wider).unwrap(), row: wider };
        let (back, _) = crate::memory_items::undo(&change, &wider_item, 12).unwrap();
        assert!(!may_use(&meta_of(&back).unwrap(), "juno", None, false));
    }

    #[test]
    fn a_change_names_one_thing_and_leaves_the_rest() {
        let start = 1_700_000_000_000_000;
        let one = changed(&[], &ConvChange { started_at: start, private: Some(true), ..Default::default() });
        assert!(one.private && one.excluded.is_empty() && one.audience.is_none());
        let two = changed(&[one.clone()], &ConvChange { started_at: start, exclude: Some(4), ..Default::default() });
        assert!(two.private, "the private mark stays");
        assert_eq!(two.excluded, vec![4]);
        let three = changed(&[two], &ConvChange { started_at: start, private: Some(false), exclude: Some(2), ..Default::default() });
        assert!(!three.private);
        assert_eq!(three.excluded, vec![2, 4]);
        let four = changed(&[three.clone()], &ConvChange { started_at: start, include: Some(4), audience: Some(Audience::LocalAis), ..Default::default() });
        assert_eq!(four.excluded, vec![2]);
        assert_eq!(four.audience, Some(Audience::LocalAis));
        let five = changed(&[four], &ConvChange { started_at: start, audience: Some(Audience::All), include: Some(2), ..Default::default() });
        assert!(!restricts(&five), "back to the default: all the person's AIs, every message");
        assert!(restricts(&three));
        // Another conversation is untouched by any of it.
        assert!(!restricts(&changed(&[three], &ConvChange { started_at: start + 1, ..Default::default() })));
    }

    #[test]
    fn a_change_is_read_back_from_its_row() {
        let u = changed(&[], &ConvChange { started_at: 7, private: Some(true), exclude: Some(3), ..Default::default() });
        let rows = vec![conv_row(&u, 100)];
        assert_eq!(conv_uses(&rows), vec![u.clone()]);
        let who = Asking { ai: "a", project: None, online: false };
        assert_eq!(may_use_message(&conv_uses(&rows), 7, 0, &who), May::Never);
        let back = changed(&conv_uses(&rows), &ConvChange { started_at: 7, private: Some(false), ..Default::default() });
        let rows = crate::memory_rows::reduce(vec![conv_row(&u, 100), conv_row(&back, 200)]);
        assert_eq!(may_use_message(&conv_uses(&rows), 7, 0, &who), May::Use);
        assert_eq!(may_use_message(&conv_uses(&rows), 7, 3, &who), May::Never, "the one message stays out");
    }
}
