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

// ── placing a kept item ─────────────────────────────────────────────────

/// The id of the subject a stretch of a conversation becomes: made from
/// the conversation's start and the stretch's first item, so two devices
/// that keep from the same place make the same subject.
pub(crate) fn conversation_subject_id(started_at: i64, first: i64) -> String {
    format!("sub-conv-{}", &blake3::hash(format!("{started_at}:{first}").as_bytes()).to_hex()[..16])
}

/// The name the person knows a project by: its folder's name.
fn project_name(app: &tauri::AppHandle, project: &str) -> String {
    crate::commands_holochain::project_memory_folders(app)
        .into_iter()
        .find(|folder| crate::project_identity::project_identity(std::path::Path::new(folder)) == project)
        .and_then(|folder| std::path::Path::new(&folder).file_name().map(|n| n.to_string_lossy().to_string()))
        .unwrap_or_else(|| "A project".to_string())
}

/// Give an item that has no subject its subject, saving the subject's row
/// when one is made or grows. An item with no place to tell from (kept by
/// a tool outside any conversation or project) stays without one. Never
/// fails a keep: on any trouble the item is returned as it came.
pub(crate) async fn settle(app: &tauri::AppHandle, key: &[u8; 32], row: MemoryRow) -> MemoryRow {
    use crate::memory_items::Audience;
    let Some(mut meta) = crate::memory_items::meta_of(&row) else { return row };
    if meta.subject.is_some() {
        return row;
    }
    let all = subjects(&crate::memory_rows::read_cache(app, key, crate::memory_items::SCOPE));
    let project_id = match &meta.audience {
        Audience::Project { project } => Some(project.clone()),
        _ => None,
    };
    let name = project_id.as_deref().map(|p| project_name(app, p));
    let started_at = meta.source.ptr.map(|p| p.c).filter(|c| *c > 0);
    if project_id.is_none() && started_at.is_none() {
        return row;
    }
    let title = started_at
        .and_then(|c| crate::transcript_search::cached_conversations(app).ok().and_then(|list| list.into_iter().find(|x| x.started_at == c).and_then(|x| x.title)))
        .unwrap_or_default();
    let k = Kept { as_of: row.as_of, about_person: false, project: project_id.as_deref().zip(name.as_deref()), conversation: (started_at.unwrap_or(0), &title) };
    let new_id = match &project_id {
        Some(p) => crate::memory_migrate::project_subject_id(p),
        None => conversation_subject_id(started_at.unwrap_or(0), row.as_of),
    };
    let placed = place(&all, &k, &new_id);
    if let Some(subject) = &placed.save {
        if let Err(e) = crate::memory_rows::save_and_cache(app, subject_row(subject, crate::memory_items::now_ms())).await {
            log::warn!("[memory] a subject could not be saved: {e}");
            return row;
        }
    }
    meta.subject = Some(placed.id);
    crate::memory_items::with_meta(row, &meta)
}

// ── what comes up together ──────────────────────────────────────────────

/// Which subjects came up in answers on this device: when each last did,
/// and how often two did in the same turn.
#[derive(Serialize, Deserialize, Default, Clone, Debug, PartialEq)]
pub(crate) struct CameUp {
    #[serde(default)]
    pub last: std::collections::BTreeMap<String, i64>,
    /// `a|b`, the lesser id first.
    #[serde(default)]
    pub pairs: std::collections::BTreeMap<String, u32>,
}

/// A turn was given items of these subjects.
pub(crate) fn note_came_up(mut held: CameUp, subjects: &[String], now: i64) -> CameUp {
    let mut ids: Vec<&String> = subjects.iter().collect();
    ids.sort();
    ids.dedup();
    for id in &ids {
        held.last.insert((*id).clone(), now);
    }
    for (i, a) in ids.iter().enumerate() {
        for b in &ids[i + 1..] {
            *held.pairs.entry(format!("{a}|{b}")).or_insert(0) += 1;
        }
    }
    held
}

const CAME_UP_FILE: &str = "memory-came-up.enc";

fn read_came_up(app: &tauri::AppHandle, key: &[u8; 32]) -> CameUp {
    let Ok(root) = crate::profile::root(app) else { return CameUp::default() };
    let Ok(raw) = std::fs::read(root.join(CAME_UP_FILE)) else { return CameUp::default() };
    if raw.len() < 24 {
        return CameUp::default();
    }
    crate::transcript_crypto::decrypt(key, &raw[..24], &raw[24..]).ok().and_then(|p| serde_json::from_slice(&p).ok()).unwrap_or_default()
}

fn write_came_up(app: &tauri::AppHandle, key: &[u8; 32], held: &CameUp) {
    let (Ok(root), Ok(plain)) = (crate::profile::root(app), serde_json::to_vec(held)) else { return };
    let Ok((nonce, cipher)) = crate::transcript_crypto::encrypt(key, &plain) else { return };
    let mut out = nonce.to_vec();
    out.extend_from_slice(&cipher);
    let path = root.join(CAME_UP_FILE);
    if std::fs::write(&path, out).is_ok() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
    }
}

/// What came up together on this device, emptied (all memory was deleted).
pub(crate) fn clear_came_up(app: &tauri::AppHandle) {
    if let Ok(root) = crate::profile::root(app) {
        let _ = std::fs::remove_file(root.join(CAME_UP_FILE));
    }
}

/// An answer was given these items: their subjects came up.
pub(crate) fn came_up(app: &tauri::AppHandle, key: &[u8; 32], rows: &[MemoryRow], item_ids: &[String]) {
    if item_ids.is_empty() {
        return;
    }
    let all = subjects(rows);
    let ids: Vec<String> = crate::memory_items::items(rows)
        .iter()
        .filter(|i| item_ids.contains(&i.row.id))
        .filter_map(|i| i.meta.subject.as_deref().and_then(|s| standing(&all, s)).map(|s| s.id.clone()))
        .collect();
    if ids.is_empty() {
        return;
    }
    write_came_up(app, key, &note_came_up(read_came_up(app, key), &ids, crate::memory_items::now_ms()));
}

// ── the Map ─────────────────────────────────────────────────────────────

/// Whose view of the Map: every AI and everywhere when both are absent.
#[derive(Deserialize, Default, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct MapView {
    /// One AI, by its founding key: only what it may use is counted.
    #[serde(default)]
    pub ai: Option<String>,
    /// One project, by its identity: its subjects, and About you.
    #[serde(default)]
    pub project: Option<String>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapSubject {
    pub id: String,
    /// This subject's id and the ids of every subject merged into it: an
    /// item belongs here when its subject is one of these.
    pub ids: Vec<String>,
    pub name: String,
    pub name_accepted: bool,
    /// `about_you` | `project` | `conversation` | `by_hand`.
    pub kind: &'static str,
    /// The project it was made from.
    pub project: Option<String>,
    pub items: usize,
    pub open: usize,
    /// `all` | `ai` | `project` | `local` | `mixed`: who its items are for.
    pub audience: &'static str,
    /// When something was last kept in it (ms).
    pub last_kept: i64,
    /// When it last came up in an answer on this device (ms).
    pub last_came_up: Option<i64>,
    /// A stretch of a conversation: the conversation's title as it is now
    /// and the stretch's first and last item (ms). While the person has
    /// not named the subject, the page names it from these, with the dates
    /// as the person's own clock has them.
    pub title: Option<String>,
    pub first: Option<i64>,
    pub last: Option<i64>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapLink {
    pub a: String,
    pub b: String,
    /// How many turns were given items of both.
    pub times: u32,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MapProject {
    pub id: String,
    pub name: String,
    pub items: usize,
    /// Which of the Map's project colors it has; none when all are taken.
    pub color: Option<u8>,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct MemoryMap {
    pub subjects: Vec<MapSubject>,
    pub links: Vec<MapLink>,
    /// Every project that has a subject, most kept first, whatever the view.
    pub projects: Vec<MapProject>,
    /// Kept items that sit in no subject.
    pub unplaced: usize,
}

/// How many projects have a color of their own on the Map.
pub(crate) const PROJECT_COLORS: u8 = 6;

/// Which color each project has. A project keeps its color once given; a
/// free color goes to the project with the most kept that has none.
pub(crate) fn color_slots(held: &std::collections::BTreeMap<String, u8>, ranked: &[String]) -> std::collections::BTreeMap<String, u8> {
    let mut out = held.clone();
    for project in ranked {
        if out.contains_key(project) {
            continue;
        }
        let Some(free) = (0..PROJECT_COLORS).find(|slot| !out.values().any(|v| v == slot)) else { break };
        out.insert(project.clone(), free);
    }
    out
}

const COLORS_ROW: &str = "sys-map-colors";

fn colors_row(colors: &std::collections::BTreeMap<String, u8>, now: i64) -> MemoryRow {
    MemoryRow { id: COLORS_ROW.to_string(), text: String::new(), structured: Some(serde_json::json!({ "sys": "map_colors", "colors": colors })), ..subject_row(&Subject { id: String::new(), name: String::new(), name_accepted: false, from: MadeFrom::ByHand, merged_into: None }, now) }
}

pub(crate) fn colors(rows: &[MemoryRow]) -> std::collections::BTreeMap<String, u8> {
    rows.iter()
        .filter(|r| !r.deleted && r.id == COLORS_ROW)
        .find_map(|r| serde_json::from_value(r.structured.as_ref()?.get("colors")?.clone()).ok())
        .unwrap_or_default()
}

fn about_you() -> Subject {
    Subject { id: ABOUT_YOU.to_string(), name: "About you".to_string(), name_accepted: true, from: MadeFrom::AboutYou, merged_into: None }
}

/// The Map: each standing subject with what it holds for this view, the
/// lines between subjects that came up together, and the projects. About
/// you is always on it; any other subject is on it only while it holds
/// something.
pub(crate) fn map_of(all: &[Subject], items: &[crate::memory_items::Item], view: &MapView, came: &CameUp, colors: &std::collections::BTreeMap<String, u8>, titles: &std::collections::HashMap<i64, String>) -> MemoryMap {
    use crate::memory_items::{Audience, ItemState};
    let mut all: Vec<Subject> = all.to_vec();
    if !all.iter().any(|s| s.from == MadeFrom::AboutYou) {
        all.push(about_you());
    }
    let stands = |id: &str| standing(&all, id).map(|s| s.id.clone());
    let live = |m: &crate::memory_items::ItemMeta| matches!(m.state, ItemState::Current | ItemState::Open | ItemState::Done);
    let counted = |m: &crate::memory_items::ItemMeta| match &view.ai {
        Some(ai) => crate::memory_items::may_use(m, ai, view.project.as_deref(), false),
        None => live(m),
    };
    let project_of = |s: &Subject| match &s.from {
        MadeFrom::Project { project } => Some(project.clone()),
        _ => None,
    };
    let mut subjects_out = Vec::new();
    for s in all.iter().filter(|s| s.merged_into.is_none()) {
        let ids: Vec<String> = all.iter().filter(|x| stands(&x.id).as_deref() == Some(s.id.as_str())).map(|x| x.id.clone()).collect();
        let mine: Vec<&crate::memory_items::Item> = items.iter().filter(|i| i.meta.subject.as_ref().is_some_and(|id| ids.contains(id)) && counted(&i.meta)).collect();
        let is_you = s.from == MadeFrom::AboutYou;
        if let Some(p) = &view.project {
            let for_project = mine.iter().any(|i| matches!(&i.meta.audience, Audience::Project { project } if project == p));
            if !(is_you || project_of(s).as_deref() == Some(p.as_str()) || for_project) {
                continue;
            }
        }
        if mine.is_empty() && !is_you {
            continue;
        }
        let kinds: Vec<&'static str> = mine
            .iter()
            .map(|i| match i.meta.audience {
                Audience::All => "all",
                Audience::Ai { .. } => "ai",
                Audience::Project { .. } => "project",
                Audience::LocalAis => "local",
                Audience::None => "all",
            })
            .collect();
        let audience = match kinds.first() {
            None => "all",
            Some(first) if kinds.iter().all(|k| k == first) => first,
            Some(_) => "mixed",
        };
        subjects_out.push(MapSubject {
            id: s.id.clone(),
            name: s.name.clone(),
            name_accepted: s.name_accepted,
            kind: match s.from {
                MadeFrom::AboutYou => "about_you",
                MadeFrom::Project { .. } => "project",
                MadeFrom::Conversation { .. } => "conversation",
                MadeFrom::ByHand => "by_hand",
            },
            project: project_of(s),
            items: mine.len(),
            open: mine.iter().filter(|i| i.meta.state == ItemState::Open).count(),
            audience,
            last_kept: mine.iter().map(|i| i.row.as_of).max().unwrap_or(0),
            last_came_up: ids.iter().filter_map(|id| came.last.get(id).copied()).max(),
            title: match &s.from {
                MadeFrom::Conversation { started_at, .. } => titles.get(started_at).cloned(),
                _ => None,
            },
            first: match s.from {
                MadeFrom::Conversation { first, .. } => Some(first),
                _ => None,
            },
            last: match s.from {
                MadeFrom::Conversation { last, .. } => Some(last),
                _ => None,
            },
            ids,
        });
    }
    // About you first, then the most kept.
    subjects_out.sort_by(|a, b| (b.kind == "about_you").cmp(&(a.kind == "about_you")).then(b.items.cmp(&a.items)).then(a.name.cmp(&b.name)));

    let shown = |id: &str| subjects_out.iter().any(|s| s.id == id);
    let mut times: std::collections::BTreeMap<(String, String), u32> = std::collections::BTreeMap::new();
    for (pair, n) in &came.pairs {
        let Some((a, b)) = pair.split_once('|') else { continue };
        let (Some(a), Some(b)) = (stands(a), stands(b)) else { continue };
        if a == b || !shown(&a) || !shown(&b) {
            continue;
        }
        let key = if a < b { (a, b) } else { (b, a) };
        *times.entry(key).or_insert(0) += n;
    }
    let links = times.into_iter().map(|((a, b), times)| MapLink { a, b, times }).collect();

    let mut projects: Vec<MapProject> = all
        .iter()
        .filter(|s| s.merged_into.is_none())
        .filter_map(|s| {
            let project = project_of(s)?;
            let n = items.iter().filter(|i| live(&i.meta) && i.meta.subject.as_deref().and_then(|id| stands(id)).as_deref() == Some(s.id.as_str())).count();
            (n > 0).then(|| MapProject { color: colors.get(&project).copied(), id: project, name: s.name.clone(), items: n })
        })
        .collect();
    projects.sort_by(|a, b| b.items.cmp(&a.items).then(a.name.cmp(&b.name)));
    let unplaced = items.iter().filter(|i| live(&i.meta) && i.meta.subject.as_deref().and_then(|id| stands(id)).is_none()).count();
    MemoryMap { subjects: subjects_out, links, projects, unplaced }
}

fn data_key(app: &tauri::AppHandle) -> Result<[u8; 32], String> {
    use tauri::Manager;
    app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get()?.data_key()
}

/// The Map for a view. A project that has no color yet is given a free
/// one here, and keeps it.
#[tauri::command]
pub async fn memory_map(app: tauri::AppHandle, view: Option<MapView>) -> Result<MemoryMap, String> {
    let key = data_key(&app)?;
    let rows = crate::memory_rows::read_cache(&app, &key, crate::memory_items::SCOPE);
    let (all, items, came) = (subjects(&rows), crate::memory_items::items(&rows), read_came_up(&app, &key));
    let view = view.unwrap_or_default();
    let held = colors(&rows);
    let titles: std::collections::HashMap<i64, String> = crate::transcript_search::cached_conversations(&app)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|c| c.title.filter(|t| !t.trim().is_empty()).map(|t| (c.started_at, t)))
        .collect();
    let ranked: Vec<String> = map_of(&all, &items, &MapView::default(), &came, &held, &titles).projects.into_iter().map(|p| p.id).collect();
    let now_colors = color_slots(&held, &ranked);
    if now_colors != held {
        if let Err(e) = crate::memory_rows::save_and_cache(&app, colors_row(&now_colors, crate::memory_items::now_ms())).await {
            log::warn!("[memory] the Map's project colors could not be saved: {e}");
        }
    }
    Ok(map_of(&all, &items, &view, &came, &now_colors, &titles))
}

/// The person names a subject: the name is theirs from now on and never
/// changes by itself.
#[tauri::command]
pub async fn memory_subject_rename(app: tauri::AppHandle, id: String, name: String) -> Result<(), String> {
    let name: String = name.trim().chars().take(80).collect();
    if name.is_empty() {
        return Err("a subject needs a name".into());
    }
    let key = data_key(&app)?;
    let all = subjects(&crate::memory_rows::read_cache(&app, &key, crate::memory_items::SCOPE));
    let subject = match standing(&all, &id) {
        Some(s) => s.clone(),
        None if id == ABOUT_YOU => about_you(),
        None => return Err("that subject is not here any more".into()),
    };
    crate::memory_rows::save_and_cache(&app, subject_row(&Subject { name, name_accepted: true, ..subject }, crate::memory_items::now_ms())).await?;
    Ok(())
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

    fn item(id: &str, subject: Option<&str>, audience: crate::memory_items::Audience, as_of: i64) -> crate::memory_items::Item {
        use crate::memory_block::Ptr;
        use crate::memory_items::{keep, meta_of, with_meta, Item, ItemType, Keep};
        let (row, _) = keep(&[], Keep { id: id.into(), text: &format!("Item {id}."), item_type: ItemType::Detail, ptr: Ptr { c: 5, s: 1, t: as_of * 1000, a: 0, b: 0 }, as_of, by: "person", audience: Some(audience), kind: "exchange", why: "pressed", quote: None, subject: subject.map(str::to_string), now: as_of }).ok().unwrap();
        let meta = meta_of(&row).unwrap();
        Item { row: with_meta(row, &meta), meta }
    }

    fn sub(id: &str, name: &str, from: MadeFrom) -> Subject {
        Subject { id: id.into(), name: name.into(), name_accepted: false, from, merged_into: None }
    }

    #[test]
    fn about_you_is_on_the_map_before_anything_is_kept() {
        let map = map_of(&[], &[], &MapView::default(), &CameUp::default(), &Default::default(), &Default::default());
        assert_eq!(map.subjects.len(), 1);
        assert_eq!((map.subjects[0].id.as_str(), map.subjects[0].name.as_str(), map.subjects[0].kind, map.subjects[0].items), (ABOUT_YOU, "About you", "about_you", 0));
        assert!(map.links.is_empty() && map.projects.is_empty());
    }

    #[test]
    fn a_subject_is_on_the_map_while_it_holds_something_and_counts_follow_merges() {
        use crate::memory_items::Audience;
        let all = vec![
            sub("s-host", "Hosting", MadeFrom::Conversation { started_at: 5, first: 1, last: 2 }),
            Subject { merged_into: Some("s-host".into()), ..sub("s-dns", "DNS", MadeFrom::Conversation { started_at: 6, first: 1, last: 2 }) },
            sub("s-empty", "Market stall", MadeFrom::ByHand),
        ];
        let items = vec![item("a", Some("s-host"), Audience::All, 10), item("b", Some("s-dns"), Audience::Ai { ai: "atlas".into() }, 30), item("c", None, Audience::All, 5)];
        let map = map_of(&all, &items, &MapView::default(), &CameUp::default(), &Default::default(), &Default::default());
        let names: Vec<&str> = map.subjects.iter().map(|s| s.name.as_str()).collect();
        assert_eq!(names, vec!["About you", "Hosting"], "About you first; the merged and the empty subject are not circles");
        let host = &map.subjects[1];
        assert_eq!((host.items, host.audience, host.last_kept), (2, "mixed", 30));
        assert_eq!(host.ids, vec!["s-host".to_string(), "s-dns".to_string()]);
        assert_eq!(map.unplaced, 1, "an item with no subject is counted, not lost");
        // The page names an unnamed stretch from the conversation's title as it is now.
        let titles = std::collections::HashMap::from([(5i64, "Hosting move".to_string())]);
        let titled = map_of(&all, &items, &MapView::default(), &CameUp::default(), &Default::default(), &titles);
        assert_eq!((titled.subjects[1].title.as_deref(), titled.subjects[1].first, titled.subjects[1].last), (Some("Hosting move"), Some(1), Some(2)));
        assert_eq!(titled.subjects[0].title, None);
    }

    #[test]
    fn an_ai_is_shown_only_what_it_may_use_and_a_project_only_its_own_and_about_you() {
        use crate::memory_items::Audience;
        let all = vec![about_you(), sub("s-bakery", "Bakery site", MadeFrom::Project { project: "p1".into() }), sub("s-trip", "Trip", MadeFrom::Conversation { started_at: 5, first: 1, last: 2 })];
        let items = vec![
            item("y", Some(ABOUT_YOU), Audience::All, 1),
            item("p", Some("s-bakery"), Audience::Project { project: "p1".into() }, 2),
            item("t", Some("s-trip"), Audience::Ai { ai: "atlas".into() }, 3),
        ];
        let ids = |v: &MapView| map_of(&all, &items, v, &CameUp::default(), &Default::default(), &Default::default()).subjects.into_iter().map(|s| s.id).collect::<Vec<_>>();
        assert_eq!(ids(&MapView { ai: Some("juno".into()), project: None }), vec![ABOUT_YOU.to_string()], "Juno may use neither Atlas's item nor the project's outside the project");
        assert_eq!(ids(&MapView { ai: Some("atlas".into()), project: None }), vec![ABOUT_YOU.to_string(), "s-trip".to_string()]);
        assert_eq!(ids(&MapView { ai: None, project: Some("p1".into()) }), vec![ABOUT_YOU.to_string(), "s-bakery".to_string()]);
        let projects = map_of(&all, &items, &MapView { ai: Some("juno".into()), project: None }, &CameUp::default(), &Default::default(), &Default::default()).projects;
        assert_eq!(projects.iter().map(|p| (p.id.as_str(), p.name.as_str(), p.items)).collect::<Vec<_>>(), vec![("p1", "Bakery site", 1)], "the list of projects does not change with the view");
    }

    #[test]
    fn subjects_that_come_up_in_one_turn_are_joined_and_a_merge_folds_the_count() {
        use crate::memory_items::Audience;
        let came = note_came_up(CameUp::default(), &["s-b".to_string(), "s-a".to_string(), "s-a".to_string()], 100);
        let came = note_came_up(came, &["s-a".to_string(), "s-b".to_string(), "s-c".to_string()], 200);
        assert_eq!(came.pairs.get("s-a|s-b"), Some(&2), "a pair counts once a turn, whatever the order");
        assert_eq!((came.last.get("s-a"), came.last.get("s-c")), (Some(&200), Some(&200)));
        let conv = |n: i64| MadeFrom::Conversation { started_at: n, first: 1, last: 2 };
        let all = vec![sub("s-a", "A", conv(1)), sub("s-b", "B", conv(2)), Subject { merged_into: Some("s-a".into()), ..sub("s-c", "C", conv(3)) }];
        let items = vec![item("1", Some("s-a"), Audience::All, 1), item("2", Some("s-b"), Audience::All, 1), item("3", Some("s-c"), Audience::All, 1)];
        let map = map_of(&all, &items, &MapView::default(), &came, &Default::default(), &Default::default());
        assert_eq!(map.links, vec![MapLink { a: "s-a".into(), b: "s-b".into(), times: 3 }], "C was merged into A: its turn with B counts for A, and its turn with A joins nothing");
        assert_eq!(map.subjects.iter().find(|s| s.id == "s-a").unwrap().last_came_up, Some(200));
    }

    #[test]
    fn a_project_keeps_its_color_and_a_free_one_goes_to_the_most_kept() {
        let ranked: Vec<String> = (1..=8).map(|n| format!("p{n}")).collect();
        let first = color_slots(&Default::default(), &ranked[..2]);
        assert_eq!((first.get("p1"), first.get("p2")), (Some(&0), Some(&1)));
        // Later the order changes and more projects exist: nothing moves.
        let mut later_ranked = ranked.clone();
        later_ranked.reverse();
        let later = color_slots(&first, &later_ranked);
        assert_eq!((later.get("p1"), later.get("p2")), (Some(&0), Some(&1)));
        assert_eq!(later.len(), usize::from(PROJECT_COLORS), "six projects have a color; the rest share the quiet one");
        assert_eq!(later.get("p8"), Some(&2));
        assert!(later.get("p3").is_none() && later.get("p4").is_none());
        assert_eq!(colors(&[colors_row(&later, 5)]), later, "the colors are a row, so every device shows the same");
    }

    #[test]
    fn the_same_stretch_of_a_conversation_is_the_same_subject_on_every_device() {
        assert_eq!(conversation_subject_id(777, 1_000), conversation_subject_id(777, 1_000));
        assert_ne!(conversation_subject_id(777, 1_000), conversation_subject_id(777, 2_000));
        assert!(conversation_subject_id(777, 1_000).starts_with("sub-conv-"));
    }
}
