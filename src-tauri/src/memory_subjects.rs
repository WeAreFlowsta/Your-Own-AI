//! Subjects: what memory's items are grouped under on the Map.
//!
//! A subject comes from structure, with no model: the project an item was
//! kept in, else the conversation, split where more than three days pass
//! between one kept item and the next. What is about the person goes to
//! one fixed subject. A subject's name is a suggestion until the person
//! accepts or changes it; with no model allowed to suggest one, the name
//! is the conversation's or project's own with its dates.
//!
//! Two subjects that look like one are offered for merging, never merged
//! by themselves: by how close their items are in meaning once what all
//! items share is taken out.
//!
//! A subject is a system row in memory's scope, so it is the same on
//! every device and comes back with a restore.

use crate::memory_rows::MemoryRow;
use serde::{Deserialize, Serialize};

/// A conversation's items become a new subject when more than this passes
/// between neighbors.
const SPLIT_AFTER_MS: i64 = 3 * 86_400_000;
/// Merge offers are made down to this score, for groups of at least this
/// many items.
pub(crate) const OFFER_FLOOR: f32 = 0.2;
const OFFER_GROUP_AT_LEAST: usize = 3;
/// The one subject for what is about the person.
pub(crate) const ABOUT_YOU: &str = "sub-about-you";

/// What a subject was made from.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MadeFrom {
    /// A project, by its identity.
    Project { project: String },
    /// A stretch of one conversation (named by its start), from the first
    /// item's time to the last's (ms).
    Conversation { started_at: i64, first: i64, last: i64 },
    AboutYou,
    /// The person made it by hand.
    ByHand,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Subject {
    pub id: String,
    pub name: String,
    /// The person accepted or wrote the name: it never changes by itself.
    pub name_accepted: bool,
    pub from: MadeFrom,
    /// When this subject was merged into another: that one's id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub merged_into: Option<String>,
}

/// The subject as the row that is saved.
pub(crate) fn subject_row(s: &Subject, now: i64) -> MemoryRow {
    MemoryRow {
        id: s.id.clone(),
        scope: crate::memory_items::SCOPE.to_string(),
        kind: "note".to_string(),
        text: s.name.clone(),
        structured: Some(serde_json::json!({ "sys": "subject", "subject": s })),
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

/// The subjects among a scope's rows.
pub(crate) fn subjects(rows: &[MemoryRow]) -> Vec<Subject> {
    rows.iter()
        .filter(|r| !r.deleted)
        .filter_map(|r| {
            let s = r.structured.as_ref()?;
            (s.get("sys")?.as_str()? == "subject").then(|| serde_json::from_value(s.get("subject")?.clone()).ok()).flatten()
        })
        .collect()
}

/// Follow merges to the subject that stands now.
pub(crate) fn standing<'a>(all: &'a [Subject], id: &str) -> Option<&'a Subject> {
    let mut at = all.iter().find(|s| s.id == id)?;
    for _ in 0..all.len() {
        match at.merged_into.as_deref().and_then(|next| all.iter().find(|s| s.id == next)) {
            Some(next) => at = next,
            None => break,
        }
    }
    Some(at)
}

/// Where a newly kept item belongs.
pub(crate) struct Placed {
    /// The subject's id.
    pub id: String,
    /// The subject to save: a new one, or an existing one whose stretch
    /// of the conversation grew. `None`: an existing subject as it is.
    pub save: Option<Subject>,
}

/// What is known of the item being placed.
pub(crate) struct Kept<'a> {
    /// When the exchange it came from was (ms).
    pub as_of: i64,
    pub about_person: bool,
    /// The project it was kept in, with the name the person knows it by.
    pub project: Option<(&'a str, &'a str)>,
    /// The conversation it came from: its start (the name it has
    /// everywhere) and its title.
    pub conversation: (i64, &'a str),
}

/// The subject of a newly kept item, with no model. `new_id` is used when
/// a subject is made.
pub(crate) fn place(all: &[Subject], k: &Kept, new_id: &str) -> Placed {
    let found = |want: &dyn Fn(&Subject) -> bool| all.iter().find(|s| want(s)).and_then(|s| standing(all, &s.id)).cloned();
    if k.about_person {
        return match found(&|s| s.from == MadeFrom::AboutYou) {
            Some(s) => Placed { id: s.id, save: None },
            None => Placed { id: ABOUT_YOU.to_string(), save: Some(Subject { id: ABOUT_YOU.to_string(), name: "About you".to_string(), name_accepted: true, from: MadeFrom::AboutYou, merged_into: None }) },
        };
    }
    if let Some((project, name)) = k.project {
        return match found(&|s| matches!(&s.from, MadeFrom::Project { project: p } if p == project)) {
            Some(s) => Placed { id: s.id, save: None },
            None => Placed { id: new_id.to_string(), save: Some(Subject { id: new_id.to_string(), name: name.to_string(), name_accepted: false, from: MadeFrom::Project { project: project.to_string() }, merged_into: None }) },
        };
    }
    let (started_at, title) = k.conversation;
    // The stretch of this conversation the item falls in, or within three
    // days of: it joins that one, and the stretch grows to hold it.
    let near = all.iter().find(|s| matches!(&s.from, MadeFrom::Conversation { started_at: c, first, last } if *c == started_at && k.as_of >= first - SPLIT_AFTER_MS && k.as_of <= last + SPLIT_AFTER_MS));
    if let Some(s) = near {
        let MadeFrom::Conversation { first, last, .. } = s.from else { unreachable!() };
        let grown = MadeFrom::Conversation { started_at, first: first.min(k.as_of), last: last.max(k.as_of) };
        let now_named = if s.name_accepted { s.name.clone() } else { fallback_name(title, first.min(k.as_of), last.max(k.as_of)) };
        let changed = grown != s.from || now_named != s.name;
        let grown_subject = Subject { from: grown, name: now_named, ..s.clone() };
        // The item goes where that stretch stands now (it may have been merged).
        let id = standing(all, &s.id).map(|x| x.id.clone()).unwrap_or_else(|| s.id.clone());
        return Placed { id, save: changed.then_some(grown_subject) };
    }
    Placed { id: new_id.to_string(), save: Some(Subject { id: new_id.to_string(), name: fallback_name(title, k.as_of, k.as_of), name_accepted: false, from: MadeFrom::Conversation { started_at, first: k.as_of, last: k.as_of }, merged_into: None }) }
}

const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];

fn month_day(ms: i64) -> (i32, usize, u32) {
    let d = crate::memory_block::day(ms * 1000);
    let part = |i: usize| d.split('-').nth(i).and_then(|p| p.parse::<i64>().ok()).unwrap_or(1);
    (part(0) as i32, (part(1) as usize).clamp(1, 12), part(2) as u32)
}

/// A subject's name when no model may suggest one: the conversation's or
/// project's own name and the dates its items span ("Bakery site, March 2
/// to 9").
pub(crate) fn fallback_name(title: &str, first_ms: i64, last_ms: i64) -> String {
    let title: String = title.trim().chars().take(40).collect();
    let title = if title.is_empty() { "A conversation".to_string() } else { title };
    let ((y1, m1, d1), (y2, m2, d2)) = (month_day(first_ms), month_day(last_ms));
    let dates = if (y1, m1, d1) == (y2, m2, d2) {
        format!("{} {d1}", MONTHS[m1 - 1])
    } else if (y1, m1) == (y2, m2) {
        format!("{} {d1} to {d2}", MONTHS[m1 - 1])
    } else if y1 == y2 {
        format!("{} {d1} to {} {d2}", MONTHS[m1 - 1], MONTHS[m2 - 1])
    } else {
        format!("{} {d1}, {y1} to {} {d2}, {y2}", MONTHS[m1 - 1], MONTHS[m2 - 1])
    };
    format!("{title}, {dates}")
}

// ── merge offers ────────────────────────────────────────────────────────

/// Two subjects that look like one.
#[derive(Serialize, Clone, Debug, PartialEq)]
pub struct MergeOffer {
    pub a: String,
    pub b: String,
    pub score: f32,
}

fn unit(v: &mut [f32]) {
    let n = v.iter().map(|x| f64::from(*x) * f64::from(*x)).sum::<f64>().sqrt();
    if n > 0.0 {
        for x in v.iter_mut() {
            *x = (f64::from(*x) / n) as f32;
        }
    }
}

/// Merge offers, best first. `items`: each item's subject and vector.
/// What all items share is taken out first (the mean of every vector is
/// subtracted from each, and each is scaled to length 1); a subject's
/// center is the sum of its items' vectors, scaled to length 1; only
/// subjects of three items or more are compared; a pair's score is the
/// likeness of the two centers. Pairs the person declined are left out.
pub(crate) fn merge_offers(items: &[(String, Vec<f32>)], declined: &[(String, String)]) -> Vec<MergeOffer> {
    let Some(dims) = items.first().map(|i| i.1.len()).filter(|d| *d > 0) else { return Vec::new() };
    let usable: Vec<&(String, Vec<f32>)> = items.iter().filter(|i| i.1.len() == dims).collect();
    let mut mean = vec![0f64; dims];
    for (_, v) in &usable {
        for (m, x) in mean.iter_mut().zip(v) {
            *m += f64::from(*x);
        }
    }
    for m in mean.iter_mut() {
        *m /= usable.len() as f64;
    }
    let mut groups: std::collections::BTreeMap<&str, (Vec<f32>, usize)> = Default::default();
    for (subject, v) in &usable {
        let mut c: Vec<f32> = v.iter().zip(&mean).map(|(x, m)| (f64::from(*x) - m) as f32).collect();
        unit(&mut c);
        let g = groups.entry(subject.as_str()).or_insert_with(|| (vec![0.0; dims], 0));
        for (s, x) in g.0.iter_mut().zip(&c) {
            *s += x;
        }
        g.1 += 1;
    }
    let centers: Vec<(&str, Vec<f32>)> = groups
        .into_iter()
        .filter(|(_, (_, n))| *n >= OFFER_GROUP_AT_LEAST)
        .map(|(id, (mut sum, _))| {
            unit(&mut sum);
            (id, sum)
        })
        .collect();
    let was_declined = |a: &str, b: &str| declined.iter().any(|(x, y)| (x == a && y == b) || (x == b && y == a));
    let mut offers = Vec::new();
    for i in 0..centers.len() {
        for j in i + 1..centers.len() {
            let score: f32 = centers[i].1.iter().zip(&centers[j].1).map(|(x, y)| x * y).sum();
            if score >= OFFER_FLOOR && !was_declined(centers[i].0, centers[j].0) {
                offers.push(MergeOffer { a: centers[i].0.to_string(), b: centers[j].0.to_string(), score });
            }
        }
    }
    offers.sort_by(|x, y| y.score.partial_cmp(&x.score).unwrap_or(std::cmp::Ordering::Equal).then(x.a.cmp(&y.a)).then(x.b.cmp(&y.b)));
    offers
}

/// Merge `from` into `into`: the row to save. Items keep their subject id;
/// readers follow the merge (`standing`), so an undo is one row.
pub(crate) fn merged(from: &Subject, into: &str) -> Subject {
    Subject { merged_into: Some(into.to_string()), ..from.clone() }
}

#[cfg(test)]
mod tests {
    use super::*;

    const DAY: i64 = 86_400_000;
    const MARCH_2: i64 = 1_772_409_600_000; // 2026-03-02 00:00 UTC

    fn conv_item(as_of: i64) -> Kept<'static> {
        Kept { as_of, about_person: false, project: None, conversation: (777, "Bakery site") }
    }

    #[test]
    fn a_conversations_items_are_one_subject_until_three_days_pass() {
        let mut all: Vec<Subject> = Vec::new();
        let mut put = |all: &mut Vec<Subject>, k: &Kept, id: &str| {
            let p = place(all, k, id);
            if let Some(s) = p.save {
                all.retain(|x| x.id != s.id);
                all.push(s);
            }
            p.id
        };
        let first = put(&mut all, &conv_item(MARCH_2), "sub-1");
        assert_eq!((first.as_str(), all[0].name.as_str(), all[0].name_accepted), ("sub-1", "Bakery site, March 2", false));
        // Two days later, then three days after that: the same subject, its stretch and name growing.
        assert_eq!(put(&mut all, &conv_item(MARCH_2 + 2 * DAY), "sub-2"), "sub-1");
        assert_eq!(put(&mut all, &conv_item(MARCH_2 + 5 * DAY), "sub-3"), "sub-1");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "Bakery site, March 2 to 7");
        assert_eq!(all[0].from, MadeFrom::Conversation { started_at: 777, first: MARCH_2, last: MARCH_2 + 5 * DAY });
        // More than three days after the last: a new subject of the same conversation.
        assert_eq!(put(&mut all, &conv_item(MARCH_2 + 5 * DAY + 3 * DAY + 1), "sub-4"), "sub-4");
        assert_eq!(all.len(), 2);
        // An item from between the two stretches' edges joins the one it is within three days of.
        assert_eq!(put(&mut all, &conv_item(MARCH_2 - DAY), "sub-5"), "sub-1");
        assert_eq!(all.iter().find(|s| s.id == "sub-1").unwrap().name, "Bakery site, March 1 to 7");
        // Another conversation is another subject, whatever the dates.
        let other = Kept { conversation: (888, "Newsletter tool"), ..conv_item(MARCH_2) };
        assert_eq!(put(&mut all, &other, "sub-6"), "sub-6");
        // An item inside a stretch changes nothing: nothing to save.
        assert!(place(&all, &conv_item(MARCH_2 + DAY), "sub-7").save.is_none());
    }

    #[test]
    fn a_project_and_the_person_each_have_one_subject() {
        let mut all: Vec<Subject> = Vec::new();
        let k = Kept { project: Some(("id:p1", "Bakery site")), ..conv_item(MARCH_2) };
        let p = place(&all, &k, "sub-1");
        let made = p.save.unwrap();
        assert_eq!((p.id.as_str(), made.name.as_str(), &made.from), ("sub-1", "Bakery site", &MadeFrom::Project { project: "id:p1".into() }));
        all.push(made);
        // Months later, from another conversation in the same project: the same subject.
        let later = Kept { as_of: MARCH_2 + 90 * DAY, conversation: (999, "Checkout page"), ..k };
        let p = place(&all, &later, "sub-2");
        assert_eq!((p.id.as_str(), p.save), ("sub-1", None));
        // Another project is another subject.
        assert_eq!(place(&all, &Kept { project: Some(("id:p2", "Recipe book")), ..conv_item(MARCH_2) }, "sub-3").id, "sub-3");
        // What is about the person: the one fixed subject, in a project or not.
        let about = Kept { about_person: true, project: Some(("id:p1", "Bakery site")), ..conv_item(MARCH_2) };
        let p = place(&all, &about, "sub-4");
        assert_eq!((p.id.as_str(), p.save.as_ref().map(|s| (s.name.as_str(), s.name_accepted))), (ABOUT_YOU, Some(("About you", true))));
        all.push(p.save.unwrap());
        assert!(place(&all, &about, "sub-5").save.is_none());
    }

    #[test]
    fn an_accepted_name_never_changes_by_itself() {
        let mut s = Subject { id: "sub-1".into(), name: "Delivery prices".into(), name_accepted: true, from: MadeFrom::Conversation { started_at: 777, first: MARCH_2, last: MARCH_2 }, merged_into: None };
        let p = place(&[s.clone()], &conv_item(MARCH_2 + 2 * DAY), "x");
        assert_eq!(p.save.as_ref().unwrap().name, "Delivery prices", "the stretch grows; the name the person gave stays");
        s.name_accepted = false;
        s.name = "Bakery site, March 2".into();
        assert_eq!(place(&[s], &conv_item(MARCH_2 + 2 * DAY), "x").save.unwrap().name, "Bakery site, March 2 to 4");
    }

    #[test]
    fn fallback_names_carry_their_dates() {
        assert_eq!(fallback_name("Bakery site", MARCH_2, MARCH_2 + 7 * DAY), "Bakery site, March 2 to 9");
        assert_eq!(fallback_name("Bakery site", MARCH_2, MARCH_2), "Bakery site, March 2");
        assert_eq!(fallback_name("Bakery site", MARCH_2 - 3 * DAY, MARCH_2 + DAY), "Bakery site, February 27 to March 3");
        assert_eq!(fallback_name("Bakery site", MARCH_2 - 70 * DAY, MARCH_2), "Bakery site, December 22, 2025 to March 2, 2026");
        assert_eq!(fallback_name("  ", MARCH_2, MARCH_2), "A conversation, March 2");
        assert_eq!(fallback_name(&"t".repeat(90), MARCH_2, MARCH_2).chars().count(), 40 + ", March 2".len());
    }

    /// A made-up item vector: everything shares the first axis; a subject's
    /// own direction is one of the others, with a little of a neighbor.
    fn v(own: usize, lean: usize, wobble: f32) -> Vec<f32> {
        let mut x = vec![0.0f32; 8];
        x[0] = 2.0;
        x[own] = 1.0;
        x[lean] += wobble;
        x
    }

    #[test]
    fn subjects_that_look_like_one_are_offered_best_first() {
        let mut items: Vec<(String, Vec<f32>)> = Vec::new();
        // Two subjects about the same thing (axis 1), one about another (axis 3),
        // one with only two items (too small to judge), one that leans toward the first.
        for (subject, own, lean, n) in [("sub-delivery-jan", 1, 2, 4), ("sub-delivery-mar", 1, 4, 4), ("sub-oven", 3, 5, 5), ("sub-small", 1, 2, 2), ("sub-fees", 6, 1, 3)] {
            for i in 0..n {
                items.push((subject.to_string(), v(own, lean, 0.1 + 0.05 * i as f32)));
            }
        }
        let offers = merge_offers(&items, &[]);
        assert_eq!((offers[0].a.as_str(), offers[0].b.as_str()), ("sub-delivery-jan", "sub-delivery-mar"));
        assert!(offers[0].score > 0.8, "{}", offers[0].score);
        assert!(offers.iter().all(|o| o.score >= OFFER_FLOOR));
        assert!(offers.iter().all(|o| o.a != "sub-small" && o.b != "sub-small"), "a subject of two items is not judged");
        assert!(!offers.iter().any(|o| (o.a.as_str(), o.b.as_str()) == ("sub-delivery-jan", "sub-oven")), "unrelated subjects are not offered");
        assert!(offers.windows(2).all(|w| w[0].score >= w[1].score), "best first");
        // What every item shares (the first axis) is taken out: without that, everything would look alike.
        let raw_likeness = |a: &[f32], b: &[f32]| a.iter().zip(b).map(|(x, y)| x * y).sum::<f32>() / (a.iter().map(|x| x * x).sum::<f32>().sqrt() * b.iter().map(|x| x * x).sum::<f32>().sqrt());
        assert!(raw_likeness(&v(1, 2, 0.1), &v(3, 5, 0.1)) > 0.7);
        // A declined offer never returns, whichever way round it was declined.
        let declined = vec![("sub-delivery-mar".to_string(), "sub-delivery-jan".to_string())];
        assert!(!merge_offers(&items, &declined).iter().any(|o| o.a == "sub-delivery-jan" && o.b == "sub-delivery-mar"));
        assert!(merge_offers(&[], &[]).is_empty());
    }

    #[test]
    fn a_merge_is_one_row_and_readers_follow_it() {
        let a = Subject { id: "sub-a".into(), name: "Delivery, January".into(), name_accepted: false, from: MadeFrom::Conversation { started_at: 1, first: 5, last: 9 }, merged_into: None };
        let b = Subject { id: "sub-b".into(), name: "Delivery prices".into(), name_accepted: true, from: MadeFrom::ByHand, merged_into: None };
        let c = Subject { id: "sub-c".into(), name: "Pricing".into(), name_accepted: true, from: MadeFrom::ByHand, merged_into: None };
        let all = vec![merged(&a, "sub-b"), merged(&b, "sub-c"), c.clone()];
        assert_eq!(standing(&all, "sub-a").map(|s| s.id.as_str()), Some("sub-c"), "a merge of a merge is followed");
        assert_eq!(standing(&all, "sub-c").map(|s| s.id.as_str()), Some("sub-c"));
        assert_eq!(standing(&all, "sub-nowhere"), None);
        // A new item of the merged conversation's stretch goes to the subject that stands.
        let p = place(&all, &Kept { as_of: 7, about_person: false, project: None, conversation: (1, "Delivery") }, "x");
        assert_eq!(p.id, "sub-c");
        // As rows: saved, read back, never read as items.
        let rows: Vec<MemoryRow> = all.iter().map(|s| subject_row(s, 100)).collect();
        assert_eq!(subjects(&rows), all);
        assert!(crate::memory_items::items(&rows).is_empty());
        assert!(crate::memory_rows::KINDS.contains(&rows[0].kind.as_str()));
        // Undoing the merge is the one row back as it was.
        assert_eq!(standing(&[a.clone(), b, c], "sub-a").map(|s| s.id.as_str()), Some("sub-a"));
    }
}
