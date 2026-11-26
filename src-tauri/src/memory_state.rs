//! What memory holds to be true now, beside a question: the note placed
//! after the question, and which found exchanges are left out because
//! memory marks them replaced.
//!
//! The note is made from the current items closest in meaning to the
//! question. A found exchange is left out when it is where a replaced
//! item came from, or when the note says what replaced it: the exchange
//! is older than the item that now stands and is about the same matter as
//! the item it replaced. What is left out is recorded with the reply, and
//! shown, so it can be put back.

use crate::memory_block::{self as mb, Held};
use crate::memory_items::{Item, ItemState};
use crate::memory_use::Asking;
use std::collections::HashMap;

/// A found exchange is "about the same matter" as a replaced item when it
/// is at least this alike: the bar an item meets to be in the note.
pub(crate) const SAME_MATTER: f32 = mb::NOTE_FLOOR;

/// The words an item is known by: what a replacement judgment wrote as
/// true now, else its own statement. A replaced item is known by what it
/// said.
pub(crate) fn sentence(item: &Item) -> &str {
    if item.meta.state != ItemState::Replaced {
        if let Some(now) = item.meta.now.as_deref().filter(|n| !n.trim().is_empty()) {
            return now;
        }
    }
    &item.row.text
}

/// A stamp of the words an item's vector is made from: other words, a
/// new vector.
pub(crate) fn stamp(item: &Item) -> i64 {
    use sha2::Digest;
    let d = sha2::Sha256::digest(sentence(item).as_bytes());
    i64::from_le_bytes(d[..8].try_into().unwrap_or([0; 8]))
}

/// Is an item one whose vector is kept? Those a note may be made from and
/// those a found exchange is compared with.
pub(crate) fn has_vector(item: &Item) -> bool {
    matches!(item.meta.state, ItemState::Current | ItemState::Open | ItemState::Replaced)
}

/// An item the note speaks for, and what it replaced.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Superseded {
    /// The replaced item, and the one that stands now.
    pub replaced: String,
    pub by: String,
    /// When what stands now became so (ms).
    pub since_ms: i64,
    /// The replaced item's vector.
    pub like: Vec<f32>,
}

/// What memory holds beside one question.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Stand {
    /// The note for after the question.
    pub note: Option<String>,
    /// The items the note was made from, newest first.
    pub note_items: Vec<String>,
    /// Where replaced items came from: the conversation's start, the
    /// message's sequence, and the replaced item.
    pub replaced_sources: Vec<(i64, u32, String)>,
    pub superseded: Vec<Superseded>,
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

/// The item that stands at the end of a chain of replacements (A replaced
/// by B, B by C: C). `None` when the chain leads to an item that is not
/// here.
fn stands_now<'a>(items: &'a [Item], replaced: &'a Item) -> Option<&'a Item> {
    let mut at = replaced;
    for _ in 0..32 {
        if at.meta.state != ItemState::Replaced {
            return Some(at);
        }
        let next = at.meta.replaced_by.as_deref()?;
        at = items.iter().find(|i| i.row.id == next)?;
    }
    None
}

/// What memory holds beside a question, for one AI's turn. `vectors`: each
/// item's vector by its id; an item without one is not in the note.
pub(crate) fn stand(items: &[Item], vectors: &HashMap<String, Vec<f32>>, question: &[f32], who: &Asking) -> Stand {
    let usable = |i: &Item| i.meta.state == ItemState::Current && crate::memory_items::may_use(&i.meta, who.ai, who.project, who.online);
    let current: Vec<&Item> = items.iter().filter(|i| usable(i)).collect();
    let held: Vec<Held> = current.iter().map(|i| Held { sentence: sentence(i).to_string(), as_of: i.row.as_of, confirmed: i.meta.confirmed.is_some() }).collect();
    let near: Vec<(usize, f32)> = current.iter().enumerate().filter_map(|(x, i)| vectors.get(&i.row.id).map(|v| (x, cosine(question, v)))).collect();
    let order = mb::note_order(&held, &near);
    let picked: Vec<Held> = order.iter().map(|x| held[*x].clone()).collect();
    let source = |i: &Item| i.meta.source.ptr.map(|p| (p.c, p.s));
    // An exchange that a current item also came from is not history.
    let live: Vec<(i64, u32)> = current.iter().filter_map(|i| source(i)).collect();
    let mut out = Stand { note: mb::state_note(&picked, true), note_items: order.iter().map(|x| current[*x].row.id.clone()).collect(), ..Default::default() };
    for r in items.iter().filter(|i| i.meta.state == ItemState::Replaced) {
        let by = stands_now(items, r);
        // What replaced it is not this AI's to use (or is itself gone):
        // then the older exchange is all this AI has, and it stays.
        if by.is_some_and(|b| !usable(b)) {
            continue;
        }
        if let Some((c, s)) = source(r).filter(|s| !live.contains(s)) {
            if !out.replaced_sources.iter().any(|(c2, s2, _)| (*c2, *s2) == (c, s)) {
                out.replaced_sources.push((c, s, r.row.id.clone()));
            }
        }
        if let (Some(b), Some(v)) = (by, vectors.get(&r.row.id)) {
            if out.note_items.contains(&b.row.id) {
                out.superseded.push(Superseded { replaced: r.row.id.clone(), by: b.row.id.clone(), since_ms: b.row.as_of, like: v.clone() });
            }
        }
    }
    out
}

/// Is a found exchange left out? Answers with the replaced item it is
/// left out for. `conversation` and `seq` say where its found message is,
/// `at_micros` when it was written, and `alike` how alike it is to each of
/// `stand.superseded`, in their order (`None`: not known).
pub(crate) fn left_out<'a>(stand: &'a Stand, conversation: i64, seq: u32, at_micros: i64, alike: &[Option<f32>]) -> Option<&'a str> {
    // Where a replaced item came from: that message, or the other half of
    // its exchange.
    if let Some((_, _, item)) = stand.replaced_sources.iter().find(|(c, s, _)| *c == conversation && s.abs_diff(seq) <= 1) {
        return Some(item);
    }
    stand.superseded.iter().zip(alike).find(|(sup, a)| at_micros / 1000 < sup.since_ms && a.is_some_and(|a| a >= SAME_MATTER)).map(|(sup, _)| sup.replaced.as_str())
}

/// `stand`, from this device's stores.
pub(crate) fn stand_for(app: &tauri::AppHandle, question: &[f32], memory_rows: &[crate::memory_rows::MemoryRow], who: &Asking) -> Stand {
    // A measured run holds the search to conversations begun before a
    // moment (development builds only); items kept from later ones are
    // left out by the same bound, so the run is not handed its own
    // questions as memory.
    let until = crate::memory_reading::SEARCH_UNTIL.load(std::sync::atomic::Ordering::Relaxed);
    let items: Vec<Item> = crate::memory_items::items(memory_rows).into_iter().filter(|i| i.meta.source.ptr.is_none_or(|p| p.c <= until)).collect();
    if items.is_empty() {
        return Stand::default();
    }
    let vectors: HashMap<String, Vec<f32>> = crate::memory_reading::item_vectors(app).into_iter().map(|(id, (_, q))| (id, crate::memory_index::numbers(&q))).collect();
    stand(&items, &vectors, question, who)
}

/// Make the vectors that are missing, or made from other words than the
/// item has now. Answers how many were made.
pub(crate) async fn fill_vectors(app: &tauri::AppHandle) -> Result<usize, String> {
    use tauri::Manager;
    let key = app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get()?.data_key()?;
    let items = crate::memory_items::items(&crate::memory_rows::read_cache(app, &key, crate::memory_items::SCOPE));
    let have = crate::memory_reading::item_vectors(app);
    let todo: Vec<&Item> = items.iter().filter(|i| has_vector(i)).filter(|i| have.get(&i.row.id).is_none_or(|(s, _)| *s != stamp(i))).collect();
    let mut made = 0;
    for chunk in todo.chunks(4) {
        let texts: Vec<String> = chunk.iter().map(|i| format!("{}{}", crate::embedding_model::DOC_PREFIX, sentence(i))).collect();
        let vectors = crate::llm::embed_texts(app.clone(), app.state(), texts, crate::embedding_model::FILE.to_string()).await.map_err(|e| format!("the memory model: {e}"))?;
        for (i, v) in chunk.iter().zip(vectors) {
            if !v.is_empty() {
                crate::memory_reading::put_item_vector(app, &i.row.id, stamp(i), &v)?;
                made += 1;
            }
        }
    }
    Ok(made)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_block::Ptr;
    use crate::memory_items::{keep, meta_of, replace, Audience, ItemType, Keep};

    const DAY: i64 = 86_400_000;
    const T0: i64 = 1_768_000_000_000;

    /// An item said on day `day`, from message `seq` of the conversation
    /// that started that day.
    fn item(id: &str, text: &str, day: i64, seq: u32, by: &str, audience: Option<Audience>) -> Item {
        let as_of = T0 + day * DAY;
        let (row, _) = keep(&[], Keep { id: id.into(), text, item_type: ItemType::Decision, ptr: Ptr { c: as_of * 1000, s: seq, t: as_of * 1000, a: 0, b: 10 }, as_of, by, audience, kind: "exchange", why: "settled", quote: None, subject: None, now: as_of + 1000 })
            .ok()
            .unwrap();
        let meta = meta_of(&row).unwrap();
        Item { row, meta }
    }

    fn as_item(row: crate::memory_rows::MemoryRow) -> Item {
        let meta = meta_of(&row).unwrap();
        Item { row, meta }
    }

    /// `older` replaced by `newer`, which now carries what is true.
    fn replaced(older: &Item, newer: &Item, now_true: &str) -> (Item, Item) {
        let (row, _) = replace(older, &newer.row.id, None, "tidy", newer.row.as_of + 5000);
        let mut n = newer.clone();
        n.meta.now = Some(now_true.to_string());
        (as_item(row), n)
    }

    /// A made-up vector along one of 8 axes, leaning a little to the next.
    fn v(axis: usize, lean: f32) -> Vec<f32> {
        let mut x = vec![0.0f32; 8];
        x[axis] = 1.0;
        x[(axis + 1) % 8] = lean;
        x
    }

    fn who() -> Asking<'static> {
        Asking { ai: "atlas", project: None, online: false }
    }

    fn vectors(of: &[(&str, Vec<f32>)]) -> HashMap<String, Vec<f32>> {
        of.iter().map(|(id, x)| (id.to_string(), x.clone())).collect()
    }

    #[test]
    fn the_note_is_made_from_the_closest_current_items_newest_first() {
        let delivery = item("it-delivery", "Free delivery starts at $60", 40, 2, "person", None);
        let fee = item("it-fee", "Orders under $60 keep the $6 fee", 30, 4, "ai:atlas", None);
        let oven = item("it-oven", "The oven runs 10 degrees hot", 50, 6, "person", None);
        let items = vec![fee.clone(), oven.clone(), delivery.clone()];
        let vs = vectors(&[("it-delivery", v(0, 0.1)), ("it-fee", v(0, 0.4)), ("it-oven", v(5, 0.0))]);
        let s = stand(&items, &vs, &v(0, 0.0), &who());
        assert_eq!(s.note_items, vec!["it-delivery", "it-fee"], "close enough, newest first; the oven is another matter");
        let note = s.note.unwrap();
        assert!(note.starts_with("[From memory, the state record for this, current as of "), "{note}");
        assert!(note.contains("Free delivery starts at $60. Orders under $60 keep the $6 fee. (an AI's note, not confirmed by them)"), "{note}");
        assert!(!note.contains("oven"));
        // Nothing close: no note, and nothing is left out.
        let far = stand(&items, &vs, &v(3, 0.0), &who());
        assert_eq!((far.note, far.note_items.len(), far.replaced_sources.len()), (None, 0, 0));
        // An item with no vector yet is not in the note.
        let bare = stand(&items, &vectors(&[("it-fee", v(0, 0.4))]), &v(0, 0.0), &who());
        assert_eq!(bare.note_items, vec!["it-fee"]);
    }

    #[test]
    fn the_note_says_what_is_true_now_when_a_judgment_wrote_it() {
        let old = item("it-40", "Free delivery over $40", 10, 2, "ai:atlas", None);
        let new = item("it-60", "ok make it 60 from Monday", 40, 2, "person", None);
        let (old, new) = replaced(&old, &new, "Free delivery starts at $60 from March 9.");
        assert_eq!(sentence(&new), "Free delivery starts at $60 from March 9.");
        assert_eq!(sentence(&old), "Free delivery over $40", "a replaced item is known by what it said");
        assert_ne!(stamp(&new), stamp(&item("it-60", "ok make it 60 from Monday", 40, 2, "person", None)), "other words, another stamp");
        let s = stand(&[old.clone(), new.clone()], &vectors(&[("it-40", v(0, 0.2)), ("it-60", v(0, 0.1))]), &v(0, 0.0), &who());
        assert_eq!(s.note_items, vec!["it-60"], "a replaced item is never in the note");
        assert!(s.note.unwrap().contains("Free delivery starts at $60 from March 9."));
        // Where the replaced item came from is left out, and so is its matter before the new date.
        assert_eq!(s.replaced_sources, vec![(old.row.as_of * 1000, 2, "it-40".to_string())]);
        assert_eq!(s.superseded.len(), 1);
        assert_eq!((s.superseded[0].replaced.as_str(), s.superseded[0].by.as_str(), s.superseded[0].since_ms), ("it-40", "it-60", new.row.as_of));
    }

    #[test]
    fn replaced_sources_and_older_exchanges_on_the_same_matter_are_left_out() {
        let old = item("it-40", "Free delivery over $40", 10, 2, "ai:atlas", None);
        let new = item("it-60", "Free delivery over $60", 40, 2, "person", None);
        let (old, new) = replaced(&old, &new, "Free delivery starts at $60.");
        let s = stand(&[old.clone(), new.clone()], &vectors(&[("it-40", v(0, 0.2)), ("it-60", v(0, 0.1))]), &v(0, 0.0), &who());
        let (old_conv, new_conv) = (old.row.as_of * 1000, new.row.as_of * 1000);
        // The message the replaced item came from, and the reply next to it.
        assert_eq!(left_out(&s, old_conv, 2, old_conv, &[None]), Some("it-40"));
        assert_eq!(left_out(&s, old_conv, 3, old_conv + 5, &[None]), Some("it-40"));
        assert_eq!(left_out(&s, old_conv, 9, old_conv + 9, &[None]), None, "another part of that conversation stays, unless it is about the same matter");
        // Another old conversation about the same matter, from before the new date.
        let between = (T0 + 20 * DAY) * 1000;
        assert_eq!(left_out(&s, between, 0, between, &[Some(0.8)]), Some("it-40"));
        assert_eq!(left_out(&s, between, 0, between, &[Some(0.3)]), None, "about something else: it stays");
        assert_eq!(left_out(&s, between, 0, between, &[None]), None, "not known to be alike: it stays");
        // The exchange that now stands, and anything after it, stays however alike.
        assert_eq!(left_out(&s, new_conv, 2, new_conv, &[Some(0.95)]), None);
        assert_eq!(left_out(&s, new_conv + 1, 0, (T0 + 60 * DAY) * 1000, &[Some(0.95)]), None);
    }

    #[test]
    fn an_older_exchange_stays_when_this_ai_may_not_use_what_replaced_it() {
        let old = item("it-40", "Free delivery over $40", 10, 2, "ai:atlas", None);
        let new = item("it-60", "Free delivery over $60", 40, 2, "person", Some(Audience::Ai { ai: "juno".into() }));
        let (old, new) = replaced(&old, &new, "Free delivery starts at $60.");
        let vs = vectors(&[("it-40", v(0, 0.2)), ("it-60", v(0, 0.1))]);
        let atlas = stand(&[old.clone(), new.clone()], &vs, &v(0, 0.0), &who());
        assert_eq!((atlas.note, atlas.replaced_sources.len(), atlas.superseded.len()), (None, 0, 0));
        let juno = stand(&[old, new], &vs, &v(0, 0.0), &Asking { ai: "juno", project: None, online: false });
        assert_eq!((juno.note.is_some(), juno.replaced_sources.len()), (true, 1));
    }

    #[test]
    fn a_chain_of_replacements_leads_to_what_stands_now() {
        let a = item("it-a", "Closed Mondays", 10, 2, "ai:atlas", None);
        let b = item("it-b", "Closed Mondays and Tuesdays", 20, 2, "ai:atlas", None);
        let c = item("it-c", "Open every day", 30, 2, "person", None);
        let (a, b) = replaced(&a, &b, "Closed Mondays and Tuesdays.");
        let (b, c) = replaced(&b, &c, "The shop is open every day.");
        let vs = vectors(&[("it-a", v(0, 0.3)), ("it-b", v(0, 0.2)), ("it-c", v(0, 0.1))]);
        let s = stand(&[a.clone(), b.clone(), c.clone()], &vs, &v(0, 0.0), &who());
        assert_eq!(s.note_items, vec!["it-c"]);
        assert_eq!(s.replaced_sources.len(), 2, "both older sources are history");
        assert!(s.superseded.iter().all(|x| x.by == "it-c" && x.since_ms == c.row.as_of), "each is measured against what stands now");
        // A source that a current item also came from is not history.
        let mut twin = item("it-twin", "Deliveries arrive on Thursdays", 10, 2, "person", None);
        twin.meta.source.ptr = a.meta.source.ptr;
        let s = stand(&[a.clone(), b, c, twin], &vs, &v(0, 0.0), &who());
        assert!(!s.replaced_sources.iter().any(|(c, q, _)| (*c, *q) == (a.row.as_of * 1000, 2)));
    }
}
