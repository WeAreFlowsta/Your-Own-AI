//! Who may draw on what: a conversation's audience, its private mark and
//! the messages the person chose not to have used; forgetting and deleting
//! that say exactly what happens.
//!
//! What the person decides about a conversation is kept as a system row
//! in memory's scope, named by the conversation's start (the same on
//! every device), so it survives the index being thrown away and rebuilt,
//! travels to the person's other devices, and comes back with a restore.
//! A change takes effect at once: nothing is read or imported again.

use crate::memory_block::GivenRecord;
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

// ── what a delete will do, said before it is done ───────────────────────

/// What deleting a conversation takes with it.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConversationDelete {
    /// Memory items made from it: deleted with it.
    pub items: Vec<String>,
    /// Replies in other conversations that were given passages from it:
    /// they are not rewritten.
    pub replies_quoting: usize,
}

/// `replies`: the records of what replies in OTHER conversations were
/// given, as this device holds them.
pub(crate) fn delete_preview(started_at: i64, items: &[Item], replies: &[GivenRecord]) -> ConversationDelete {
    let made_here: Vec<String> = items.iter().filter(|i| i.meta.source.ptr.is_some_and(|p| p.c == started_at)).map(|i| i.row.id.clone()).collect();
    let points_here = |g: &GivenRecord| g.exchanges.iter().any(|e| e.hit.c == started_at || e.before.is_some_and(|p| p.c == started_at) || e.after.is_some_and(|p| p.c == started_at));
    ConversationDelete { items: made_here, replies_quoting: replies.iter().filter(|g| points_here(g)).count() }
}

/// What deleting an AI does to the items it kept.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct AiDelete {
    /// For all the person's AIs (or a project, or local AIs): kept,
    /// without their source conversations.
    pub kept: Vec<String>,
    /// For that AI only: deleted.
    pub deleted: Vec<String>,
}

/// `ai`: the founding key of the AI being deleted.
pub(crate) fn ai_delete_plan(ai: &str, items: &[Item]) -> AiDelete {
    let its: Vec<&Item> = items.iter().filter(|i| i.row.saved_by == format!("ai:{ai}") || matches!(&i.meta.audience, Audience::Ai { ai: a } if a == ai)).collect();
    let (deleted, kept): (Vec<&Item>, Vec<&Item>) = its.into_iter().partition(|i| matches!(&i.meta.audience, Audience::Ai { ai: a } if a == ai));
    AiDelete { kept: kept.iter().map(|i| i.row.id.clone()).collect(), deleted: deleted.iter().map(|i| i.row.id.clone()).collect() }
}

/// "Delete all memory", memory only: every row of memory's scope goes
/// (items, subjects, the "Not used" list and memory's other rows). Rows of
/// other scopes (the person's rules) and the conversations are untouched.
pub(crate) fn delete_memory_only(rows: &[MemoryRow], now: i64) -> Vec<MemoryRow> {
    rows.iter()
        .filter(|r| r.scope == SCOPE && !r.deleted)
        .map(|r| MemoryRow { deleted: true, updated_at: now, action_hash: None, text: String::new(), structured: None, ..r.clone() })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_block::{GivenExchange, Ptr, SearchReport};
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
    fn delete_preview_counts() {
        let items = vec![item("it-1", C1, 2, "person", None), item("it-2", C1, 8, "ai:juno", None), item("it-3", C2, 0, "person", None)];
        let ptr = |c: i64, s: u32| Ptr { c, s, t: c + 1, a: 0, b: 9 };
        let given = |hits: Vec<(i64, u32)>, before: Option<(i64, u32)>| GivenRecord { v: 1, search: SearchReport { ran: true, ..Default::default() }, exchanges: hits.into_iter().map(|(c, s)| GivenExchange { before: before.map(|(c, s)| ptr(c, s)), hit: ptr(c, s), after: None, score: 0.5, quoted: false }).collect(), ..Default::default() };
        let replies = vec![given(vec![(C1, 2), (C2, 0)], None), given(vec![(C2, 1)], None), given(vec![(C2, 5)], Some((C1, 4))), given(vec![(C1, 9), (C1, 2)], None)];
        let p = delete_preview(C1, &items, &replies);
        assert_eq!(p, ConversationDelete { items: vec!["it-1".into(), "it-2".into()], replies_quoting: 3 });
        assert_eq!(delete_preview(999, &items, &replies), ConversationDelete { items: vec![], replies_quoting: 0 });
        assert_eq!(serde_json::to_value(&p).unwrap()["repliesQuoting"], 3);
    }

    #[test]
    fn items_for_all_survive_an_ai_delete() {
        let items = vec![
            item("for-all", C1, 0, "ai:atlas", None),
            item("atlas-only", C1, 1, "ai:atlas", Some(Audience::Ai { ai: "atlas".into() })),
            item("the-persons-for-atlas", C1, 2, "person", Some(Audience::Ai { ai: "atlas".into() })),
            item("project", C1, 3, "ai:atlas", Some(Audience::Project { project: "id:p1".into() })),
            item("junos", C2, 0, "ai:juno", Some(Audience::Ai { ai: "juno".into() })),
            item("the-persons", C2, 1, "person", None),
        ];
        let plan = ai_delete_plan("atlas", &items);
        assert_eq!(plan.kept, vec!["for-all", "project"]);
        assert_eq!(plan.deleted, vec!["atlas-only", "the-persons-for-atlas"]);
        assert!(!plan.kept.iter().chain(&plan.deleted).any(|id| id == "junos" || id == "the-persons"), "another AI's and the person's own are not touched");
    }

    #[test]
    fn delete_memory_only_keeps_conversations_and_rules() {
        let rule: MemoryRow = serde_json::from_value(serde_json::json!({ "id": "r1", "scope": "person", "kind": "rule", "text": "Prices are in dollars.", "as_of": 1, "saved_by": "person", "provenance": "authored", "updated_at": 1 })).unwrap();
        let project_rule: MemoryRow = serde_json::from_value(serde_json::json!({ "id": "r2", "scope": "project:id:p1", "kind": "rule", "text": "Bread only.", "as_of": 1, "saved_by": "person", "provenance": "authored", "updated_at": 1 })).unwrap();
        let rows = vec![item("it-1", C1, 0, "person", None).row, conv_row(&ConvUse { started_at: C1, audience: None, private: true, excluded: vec![] }, 3), rule.clone(), project_rule.clone()];
        let gone = delete_memory_only(&rows, 100);
        assert_eq!(gone.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["it-1", &format!("sys-conv-{C1}")[..]]);
        assert!(gone.iter().all(|r| r.deleted && r.text.is_empty() && r.structured.is_none()), "the deletion record holds none of the words");
        // After the deletions are saved, a reader sees the rules and nothing of memory.
        let after = crate::memory_rows::reduce([rows, gone].concat());
        assert_eq!(after.iter().map(|r| r.id.as_str()).collect::<Vec<_>>().len(), 2);
        assert!(after.contains(&rule) && after.contains(&project_rule));
    }
}
