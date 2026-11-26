//! Notes in memory. A note stays the person's own file, the one copy:
//! memory points at a line of it and follows the file.
//!
//! - A line of a note that answered a question becomes an item whose
//!   source is the note (the document, the passage, the line as it read),
//!   with the question it answered. It is never corrected or forgotten
//!   here: the note is changed where it lives.
//! - "Do not use this" has a reach (this line, this note, this folder)
//!   and is kept as a system row in memory's scope, so it is the same on
//!   every device, comes back with a restore, and can be taken back.
//! - When the file changes, the line is looked for again; when the file is
//!   gone, the item says so; when the folder cannot be reached, nothing
//!   is concluded.

use crate::memory_block::Ptr;
use crate::memory_items::{Audience, Item, ItemMeta, ItemState, ItemType, Source, SCOPE};
use crate::memory_rows::MemoryRow;
use serde::{Deserialize, Serialize};

/// The source kind of an item that points at a line of a note.
pub(crate) const KIND: &str = "document";

/// Where an item's line lives.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct NoteRef {
    pub doc_id: String,
    /// The passage the line was found in when it answered.
    pub idx: u32,
    /// The file's name, as shown.
    pub name: String,
    /// When the file last changed, as known when the line was last read (seconds).
    pub changed: i64,
    /// The file's own identity (the same on every device), when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_id: Option<String>,
    /// The question the line answered, in the person's words (shortened).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub asked: Option<String>,
    /// How the note stands, when it is not simply there: `deleted` | `line-gone`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state: Option<String>,
}

/// The note an item points at, when its source is a note.
pub(crate) fn note_of(meta: &ItemMeta) -> Option<NoteRef> {
    (meta.source.kind == KIND).then(|| meta.source.doc.clone()).flatten().and_then(|d| serde_json::from_value(d).ok())
}

// ── which line answered ─────────────────────────────────────────────────

const STOP: &[&str] = &[
    "the", "and", "for", "from", "that", "this", "with", "are", "was", "were", "has", "have", "had", "not", "but", "you", "your", "our", "its", "per", "all", "any", "can", "will", "which", "what", "when",
    "who", "how", "why", "there", "their", "they", "them", "been", "being", "into", "than", "then", "also", "each", "one", "two", "according", "document", "documents", "note", "notes", "changed", "last",
];

/// The words of a text that can tell one line from another: three letters
/// or more and not a common word, or anything with a digit in it.
fn telling(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !(c.is_alphanumeric() || c == '$' || c == '%' || c == '-'))
        .map(|w| w.trim_matches('-').trim_start_matches('$').to_string())
        .filter(|w| !w.is_empty())
        .filter(|w| w.chars().any(|c| c.is_ascii_digit()) || (w.chars().count() >= 3 && !STOP.contains(&w.as_str())))
        .collect()
}

/// A line of a passage as it is kept: markdown marks at its edges removed,
/// a table row's cells joined by a colon.
fn tidy_line(line: &str) -> String {
    let t = line.trim();
    if t.starts_with('|') {
        let cells: Vec<&str> = t.trim_matches('|').split('|').map(str::trim).filter(|c| !c.is_empty()).collect();
        if cells.iter().all(|c| c.chars().all(|ch| ch == '-' || ch == ':' || ch == ' ')) {
            return String::new();
        }
        return cells.join(": ");
    }
    t.trim_start_matches(|c: char| c == '#' || c == '-' || c == '*' || c == '>' || c == ' ').replace("**", "").trim().to_string()
}

/// The line of a passage that a reply's answer came from: the one sharing
/// the most telling words with the reply. A line with a figure in it must
/// share a figure, and any line must share at least two telling words, so a
/// reply that only mentions the subject points at nothing.
pub(crate) fn answering_line(passage: &str, reply: &str) -> Option<String> {
    let said: std::collections::HashSet<String> = telling(reply).into_iter().collect();
    let mut best: Option<(usize, String)> = None;
    for raw in passage.lines() {
        let line = tidy_line(raw);
        if line.chars().count() < 8 {
            continue;
        }
        let words: std::collections::HashSet<String> = telling(&line).into_iter().collect();
        let shared: Vec<&String> = words.iter().filter(|w| said.contains(*w)).collect();
        let figures = words.iter().filter(|w| w.chars().any(|c| c.is_ascii_digit())).count();
        let shared_figures = shared.iter().filter(|w| w.chars().any(|c| c.is_ascii_digit())).count();
        if shared.len() < 2 || (figures > 0 && shared_figures == 0) {
            continue;
        }
        // A figure in common tells more than a word in common.
        let score = shared.len() + 2 * shared_figures;
        if best.as_ref().map_or(true, |(s, _)| score > *s) {
            best = Some((score, line));
        }
    }
    best.map(|(_, line)| line.chars().take(400).collect())
}

/// The item for a line of a note that answered a question.
pub(crate) fn note_item(id: String, line: &str, note: NoteRef, question: Option<Ptr>, now: i64) -> MemoryRow {
    let as_of = note.changed * 1000;
    let meta = ItemMeta {
        v: 1,
        item_type: ItemType::Detail,
        state: ItemState::Current,
        replaced_by: None,
        now: None,
        reason: None,
        waiting_on: None,
        state_at: now,
        state_by: "search".into(),
        confirmed: None,
        corrected_from: None,
        subject: None,
        audience: Audience::All,
        source: Source { kind: KIND.into(), ptr: question, quote: Some(line.to_string()), doc: serde_json::to_value(&note).ok(), why: "answered".into(), for_ai: None },
        last_came_up: Some(now),
        more: Default::default(),
    };
    MemoryRow {
        id,
        scope: SCOPE.to_string(),
        kind: "fact".to_string(),
        text: line.to_string(),
        structured: Some(serde_json::json!({ "item": meta })),
        as_of,
        valid_to: None,
        saved_by: "search".to_string(),
        provenance: "authored".to_string(),
        pinned: false,
        topics: Vec::new(),
        replaces: Vec::new(),
        deleted: false,
        updated_at: now,
        action_hash: None,
    }
}

/// The item already kept for this line of this note, if there is one.
pub(crate) fn already<'a>(items: &'a [Item], doc_id: &str, line: &str) -> Option<&'a Item> {
    items.iter().find(|i| i.meta.state != ItemState::Forgotten && i.row.text == line && note_of(&i.meta).is_some_and(|n| n.doc_id == doc_id))
}

// ── following the file ──────────────────────────────────────────────────

/// How a note stands now, as the library knows it.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum NoteNow {
    /// The file is there: its text and when it last changed (seconds).
    Here { text: String, changed: i64 },
    /// The library no longer holds the document: the note was deleted.
    Deleted,
    /// The file or its folder cannot be reached right now.
    Unreachable,
}

fn squeezed(text: &str) -> String {
    text.lines().map(tidy_line).collect::<Vec<_>>().join(" ").split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase()
}

/// An item after its note was looked at again: `None` when nothing about
/// it changes. A folder that cannot be reached changes nothing: an
/// unplugged drive is not a deleted note.
pub(crate) fn followed(item: &Item, now_is: &NoteNow, now: i64) -> Option<MemoryRow> {
    let note = note_of(&item.meta)?;
    let mut next = note.clone();
    match now_is {
        NoteNow::Unreachable => return None,
        NoteNow::Deleted => next.state = Some("deleted".into()),
        NoteNow::Here { text, changed } => {
            let there = squeezed(text).contains(&squeezed(&item.row.text));
            next.state = (!there).then(|| "line-gone".to_string());
            next.changed = *changed;
        }
    }
    if next == note {
        return None;
    }
    let mut meta = item.meta.clone();
    meta.source.doc = serde_json::to_value(&next).ok();
    let mut row = crate::memory_items::with_meta(item.row.clone(), &meta);
    if next.state.is_none() {
        row.as_of = next.changed * 1000;
    }
    row.updated_at = now;
    Some(row)
}

/// May an item whose source is a note be given to an AI? Not when the
/// note is gone or the line was taken out of it.
pub(crate) fn usable(meta: &ItemMeta) -> bool {
    note_of(meta).map_or(true, |n| n.state.is_none())
}

// ── "Do not use this" ───────────────────────────────────────────────────

/// What the person chose not to have used, and how far it reaches.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct DocUse {
    /// `line` | `note` | `folder`.
    pub reach: String,
    /// The document (line and note).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub doc_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content_id: Option<String>,
    /// The line's words (line).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    /// The folder's path (folder).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// The file's or the folder's name, as the list shows it.
    pub name: String,
    pub at: i64,
}

fn use_key(u: &DocUse) -> String {
    let what = match u.reach.as_str() {
        "folder" => u.folder.clone().unwrap_or_default(),
        "line" => format!("{}\n{}", u.doc_id.clone().unwrap_or_default(), squeezed(u.line.as_deref().unwrap_or(""))),
        _ => u.doc_id.clone().unwrap_or_default(),
    };
    blake3::hash(format!("{}\n{what}", u.reach).as_bytes()).to_hex()[..24].to_string()
}

/// The choice as the row that is saved; `deleted` takes it back.
pub(crate) fn doc_row(u: &DocUse, now: i64, deleted: bool) -> MemoryRow {
    MemoryRow {
        id: format!("sys-doc-{}", use_key(u)),
        scope: SCOPE.to_string(),
        kind: "note".to_string(),
        text: String::new(),
        structured: (!deleted).then(|| serde_json::json!({ "sys": "doc", "doc": u })),
        as_of: now,
        valid_to: None,
        saved_by: "person".to_string(),
        provenance: "authored".to_string(),
        pinned: false,
        topics: Vec::new(),
        replaces: Vec::new(),
        deleted,
        updated_at: now,
        action_hash: None,
    }
}

/// Every note line, note and folder the person chose not to have used.
pub(crate) fn doc_uses(rows: &[MemoryRow]) -> Vec<(String, DocUse)> {
    rows.iter()
        .filter(|r| !r.deleted)
        .filter_map(|r| {
            let s = r.structured.as_ref()?;
            (s.get("sys")?.as_str()? == "doc").then(|| serde_json::from_value(s.get("doc")?.clone()).ok()).flatten().map(|u| (r.id.clone(), u))
        })
        .collect()
}

fn under(path: &str, folder: &str) -> bool {
    let folder = folder.trim_end_matches(['/', '\\']);
    path.strip_prefix(folder).is_some_and(|rest| rest.starts_with(['/', '\\']))
}

/// Is a passage one the person chose not to have used?
pub(crate) fn not_used(uses: &[DocUse], doc_id: &str, content_id: Option<&str>, path: Option<&str>, passage: &str) -> bool {
    let same = |u: &DocUse| u.doc_id.as_deref() == Some(doc_id) || (content_id.is_some() && u.content_id.as_deref() == content_id);
    uses.iter().any(|u| match u.reach.as_str() {
        "folder" => u.folder.as_deref().zip(path).is_some_and(|(f, p)| under(p, f)),
        "note" => same(u),
        "line" => same(u) && u.line.as_deref().is_some_and(|l| !l.trim().is_empty() && squeezed(passage).contains(&squeezed(l))),
        _ => false,
    })
}

/// Is an item's own line covered by a choice? (The item is then given to no AI.)
pub(crate) fn item_not_used(uses: &[DocUse], meta: &ItemMeta, text: &str, path: Option<&str>) -> bool {
    note_of(meta).is_some_and(|n| not_used(uses, &n.doc_id, n.content_id.as_deref(), path, text))
}

#[cfg(test)]
mod tests {
    use super::*;

    const HANDBOOK: &str = "## The shop at a glance\n\n| What | Now |\n|---|---|\n| Opening hours | Tuesday to Saturday, 8:30 to 17:30 |\n| Workshop labour rate | $95 an hour |\n| Loan bikes | 4 |\n| Warranty on repairs | 90 days |";

    fn note() -> NoteRef {
        NoteRef { doc_id: "d1".into(), idx: 0, name: "handbook.md".into(), changed: 1_760_954_400, content_id: Some("c1".into()), asked: Some("What is the workshop labour rate?".into()), state: None }
    }

    fn item(line: &str) -> Item {
        let row = note_item("it-n1".into(), line, note(), None, 5);
        crate::memory_items::items(&[row]).remove(0)
    }

    #[test]
    fn the_line_that_answered_is_the_one_sharing_the_answers_words() {
        let reply = "According to handbook.md (last changed 2026-10-20), the workshop labour rate is $95 an hour.";
        assert_eq!(answering_line(HANDBOOK, reply).as_deref(), Some("Workshop labour rate: $95 an hour"));
        let reply = "The warranty on our repairs is 90 days.";
        assert_eq!(answering_line(HANDBOOK, reply).as_deref(), Some("Warranty on repairs: 90 days"));
        // The reply gave a different figure: no line of this passage answered it.
        assert_eq!(answering_line(HANDBOOK, "The workshop labour rate is $70 an hour."), None);
        // The reply only names the subject.
        assert_eq!(answering_line(HANDBOOK, "I could not find the workshop details."), None);
        // Prose: the sentence line with the figure.
        let prose = "# Insurance\n\nInsurer: Marlow Mutual. The policy renews on March 14 each year.\n\n- The excess on any claim is $750.\n- Customers' bikes in our care are covered up to $6,000 each.";
        assert_eq!(answering_line(prose, "Based on insurance.md, the excess on any claim is $750.").as_deref(), Some("The excess on any claim is $750."));
    }

    #[test]
    fn a_note_item_points_at_its_note_and_is_not_confirmed() {
        let i = item("Workshop labour rate: $95 an hour");
        assert_eq!(i.meta.source.kind, KIND);
        assert_eq!(i.meta.confirmed, None);
        assert_eq!(i.row.as_of, 1_760_954_400_000);
        assert_eq!(note_of(&i.meta), Some(note()));
        assert!(usable(&i.meta));
        assert!(already(&[i.clone()], "d1", "Workshop labour rate: $95 an hour").is_some());
        assert!(already(&[i], "d2", "Workshop labour rate: $95 an hour").is_none());
    }

    #[test]
    fn a_changed_note_updates_its_item_row() {
        let i = item("Workshop labour rate: $95 an hour");
        // The file was edited elsewhere and the line still reads the same: its date moves.
        let later = followed(&i, &NoteNow::Here { text: format!("{HANDBOOK}\n| Deposit | 25 percent |"), changed: 1_761_000_000 }, 9).unwrap();
        assert_eq!(later.as_of, 1_761_000_000_000);
        let m = crate::memory_items::meta_of(&later).unwrap();
        assert_eq!(note_of(&m).unwrap().state, None);
        assert!(usable(&m));
        // Nothing changed: nothing is written.
        assert_eq!(followed(&i, &NoteNow::Here { text: HANDBOOK.into(), changed: 1_760_954_400 }, 9), None);
        // The line was changed in the note: the item says so and is no longer given.
        let gone = followed(&i, &NoteNow::Here { text: HANDBOOK.replace("$95", "$105"), changed: 1_761_000_000 }, 9).unwrap();
        let m = crate::memory_items::meta_of(&gone).unwrap();
        assert_eq!(note_of(&m).unwrap().state.as_deref(), Some("line-gone"));
        assert!(!usable(&m));
        assert_eq!(gone.text, "Workshop labour rate: $95 an hour");
        // The line comes back (the edit was undone): the item is usable again.
        let again = crate::memory_items::items(&[gone]).remove(0);
        let back = followed(&again, &NoteNow::Here { text: HANDBOOK.into(), changed: 1_761_100_000 }, 12).unwrap();
        assert!(usable(&crate::memory_items::meta_of(&back).unwrap()));
    }

    #[test]
    fn an_unreachable_folder_is_not_a_deletion() {
        let i = item("Workshop labour rate: $95 an hour");
        assert_eq!(followed(&i, &NoteNow::Unreachable, 9), None);
        let deleted = followed(&i, &NoteNow::Deleted, 9).unwrap();
        let m = crate::memory_items::meta_of(&deleted).unwrap();
        assert_eq!(note_of(&m).unwrap().state.as_deref(), Some("deleted"));
        assert!(!usable(&m));
        // Deleted, then unreachable: still nothing concluded from the second.
        let d = crate::memory_items::items(&[deleted]).remove(0);
        assert_eq!(followed(&d, &NoteNow::Unreachable, 12), None);
    }

    #[test]
    fn reach_of_do_not_use() {
        let line = DocUse { reach: "line".into(), doc_id: Some("d1".into()), content_id: None, line: Some("Workshop labour rate: $95 an hour".into()), folder: None, name: "handbook.md".into(), at: 1 };
        let whole = DocUse { reach: "note".into(), doc_id: Some("d2".into()), content_id: Some("c2".into()), line: None, folder: None, name: "pricing-review-2025.md".into(), at: 1 };
        let folder = DocUse { reach: "folder".into(), doc_id: None, content_id: None, line: None, folder: Some("/home/pat/Notes/old".into()), name: "old".into(), at: 1 };
        let uses = vec![line.clone(), whole.clone(), folder.clone()];
        // The line: only the passage of that note that holds it.
        assert!(not_used(&uses, "d1", None, Some("/home/pat/Notes/handbook.md"), HANDBOOK));
        assert!(!not_used(&uses, "d1", None, Some("/home/pat/Notes/handbook.md"), "## Collecting\n\nA bike not collected within 21 days gets a reminder text."));
        assert!(!not_used(&uses, "d9", None, Some("/home/pat/Notes/other.md"), HANDBOOK));
        // The note: every passage of it, and the same file under another id.
        assert!(not_used(&uses, "d2", None, None, "anything"));
        assert!(not_used(&uses, "d7", Some("c2"), None, "anything"));
        // The folder: everything under it, and nothing beside it.
        assert!(not_used(&uses, "d5", None, Some("/home/pat/Notes/old/2019/plan.md"), "anything"));
        assert!(!not_used(&uses, "d5", None, Some("/home/pat/Notes/older/plan.md"), "anything"));
        assert!(!not_used(&uses, "d5", None, None, "anything"));
        // Saved as rows, read back, and taken back.
        let rows: Vec<MemoryRow> = uses.iter().map(|u| doc_row(u, 5, false)).collect();
        assert_eq!(doc_uses(&rows).into_iter().map(|(_, u)| u).collect::<Vec<_>>(), uses);
        assert_eq!(rows[0].id, doc_row(&line, 9, true).id);
        assert!(doc_uses(&[doc_row(&line, 9, true)]).is_empty());
        // An item whose line is covered is not given.
        let i = item("Workshop labour rate: $95 an hour");
        assert!(item_not_used(&uses, &i.meta, &i.row.text, None));
        assert!(!item_not_used(&[whole, folder], &i.meta, &i.row.text, Some("/home/pat/Notes/handbook.md")));
    }
}
