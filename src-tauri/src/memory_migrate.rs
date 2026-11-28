//! The update path: what the stores from before memory was rebuilt become.
//!
//! The rule: what the person chose to keep is carried over and used; what
//! an AI guessed about them is set aside, used by no AI, and can be kept
//! one by one; nothing is asked, and nothing goes back through old
//! conversations in bulk.
//!
//! This file is the mapping, from the old stores as data to the rows that
//! are saved. Every row's id is derived from the old one and it keeps the
//! old dates, so two of the person's devices that both run the update
//! write the same rows, and running it twice changes nothing.

use crate::memory::MemoryFact;
use crate::memory_items::{Audience, Confirmed, ItemMeta, ItemState, ItemType, Source, SCOPE};
use crate::memory_rows::MemoryRow;
use crate::memory_subjects::{MadeFrom, Subject, ABOUT_YOU};
use crate::transcript_memory::TranscriptEmbedding;

/// The subject of what was saved with Remember before the update.
pub(crate) const KEPT_BEFORE: &str = "sub-kept-before-the-update";

fn id_for(store: &str, old: &str) -> String {
    let safe: String = old.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' { c } else { '-' }).take(80).collect();
    format!("mig-{store}-{safe}")
}

struct Old<'a> {
    id: String,
    text: &'a str,
    item_type: ItemType,
    state: ItemState,
    /// When it was learned or saved (ms), and last changed (ms).
    as_of: i64,
    updated_at: i64,
    saved_by: String,
    confirmed: bool,
    store: &'a str,
    subject: Option<String>,
    audience: Audience,
    /// The AI the old save was made for, or that learned it.
    for_ai: Option<String>,
}

fn item_row(o: Old) -> MemoryRow {
    let meta = ItemMeta {
        v: 1,
        item_type: o.item_type,
        state: o.state,
        replaced_by: None,
        now: None,
        reason: None,
        waiting_on: None,
        state_at: o.updated_at,
        state_by: o.saved_by.clone(),
        confirmed: o.confirmed.then(|| Confirmed { at: o.as_of, how: "kept".into() }),
        corrected_from: None,
        subject: o.subject,
        audience: o.audience,
        source: Source { kind: "legacy".into(), ptr: None, quote: None, doc: None, why: format!("migrated:{}", o.store), for_ai: o.for_ai },
        last_came_up: None,
        more: Default::default(),
    };
    MemoryRow {
        id: o.id,
        scope: SCOPE.to_string(),
        kind: match o.item_type {
            ItemType::Decision => "decision",
            ItemType::AboutPerson => "fact",
            _ => "note",
        }
        .to_string(),
        text: o.text.trim().to_string(),
        structured: Some(serde_json::json!({ "item": meta })),
        as_of: o.as_of,
        valid_to: None,
        saved_by: o.saved_by,
        provenance: format!("migrated:{}", o.store),
        pinned: false,
        topics: Vec::new(),
        replaces: Vec::new(),
        deleted: false,
        updated_at: o.updated_at,
        action_hash: None,
    }
}

/// The facts file. Times in it are microseconds.
///
/// - The name keeps working: a kept item about the person.
/// - Notes (the old Remember, for all AIs) are carried over and used, each
///   marked as kept by the person with its old date; the pieces of one
///   long save are joined.
/// - Every other fact an AI learned is set aside.
/// - The portrait and the library portrait are retired: no row.
pub(crate) fn from_facts(facts: &[MemoryFact]) -> Vec<MemoryRow> {
    let ms = |micros: i64| micros / 1000;
    let live: Vec<&MemoryFact> = facts.iter().filter(|f| f.valid_to.is_none()).collect();
    let mut rows = Vec::new();
    // The name: the one the old prompt used (the strongest, then newest).
    let mut names: Vec<&&MemoryFact> = live.iter().filter(|f| f.entry_kind == "fact" && crate::memory_prompt::is_name_predicate(&f.predicate)).collect();
    names.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal).then(b.updated_at.cmp(&a.updated_at)));
    let name_id = names.first().map(|f| f.id.clone());
    if let Some(f) = names.first() {
        rows.push(item_row(Old {
            id: "mig-name".to_string(),
            text: &format!("Their name is {}.", f.value),
            item_type: ItemType::AboutPerson,
            state: ItemState::Current,
            as_of: ms(f.valid_from),
            updated_at: ms(f.updated_at),
            saved_by: "person".into(),
            confirmed: true,
            store: "facts",
            subject: Some(ABOUT_YOU.to_string()),
            audience: Audience::All,
            for_ai: None,
        }));
    }
    // Notes: one item per save (pieces that share a group are one save).
    let mut groups: Vec<(String, Vec<&MemoryFact>)> = Vec::new();
    for f in live.iter().filter(|f| f.entry_kind == "note") {
        let key = f.group_id.clone().unwrap_or_else(|| f.id.clone());
        match groups.iter_mut().find(|g| g.0 == key) {
            Some(g) => g.1.push(f),
            None => groups.push((key, vec![f])),
        }
    }
    for (key, mut pieces) in groups {
        pieces.sort_by(|a, b| a.created_at.cmp(&b.created_at).then(a.id.cmp(&b.id)));
        let text = pieces.iter().map(|p| p.value.trim()).collect::<Vec<_>>().join("\n");
        rows.push(item_row(Old {
            id: id_for("note", &key),
            text: &text,
            item_type: ItemType::Detail,
            state: ItemState::Current,
            as_of: ms(pieces[0].created_at),
            updated_at: ms(pieces.iter().map(|p| p.updated_at).max().unwrap_or(0)),
            saved_by: "person".into(),
            confirmed: true,
            store: "note",
            subject: Some(KEPT_BEFORE.to_string()),
            audience: Audience::All,
            for_ai: pieces[0].source_ai_id.clone(),
        }));
    }
    // What an AI learned: set aside, with its dates and the AI.
    for f in live.iter().filter(|f| f.entry_kind == "fact" && Some(&f.id) != name_id.as_ref()) {
        let said_by_them = f.provenance == "authored";
        rows.push(item_row(Old {
            id: id_for("fact", &f.id),
            text: &format!("{}: {}", f.predicate.replace('_', " "), f.value),
            item_type: ItemType::AboutPerson,
            // A fact the person typed in themselves is theirs: kept.
            state: if said_by_them { ItemState::Current } else { ItemState::SetAside },
            as_of: ms(f.valid_from),
            updated_at: ms(f.updated_at),
            saved_by: if said_by_them { "person".into() } else { f.source_ai_id.as_ref().map(|a| format!("ai:{a}")).unwrap_or_else(|| "ai:unknown".into()) },
            confirmed: said_by_them,
            store: "fact",
            subject: Some(ABOUT_YOU.to_string()),
            audience: Audience::All,
            for_ai: f.source_ai_id.clone(),
        }));
    }
    rows
}

/// Knowledge the person gave one AI by hand: carried over and used, for
/// all their AIs now, with the AI it was saved for kept on the item.
/// Passages of a document given to an AI are not items (the document is in
/// the library, with that AI, as before); an AI's index of earlier turns
/// is not carried (whole conversations are searched now).
pub(crate) fn from_knowledge(ai_founding: &str, entries: &[TranscriptEmbedding]) -> Vec<MemoryRow> {
    entries
        .iter()
        .filter(|e| e.kind == "authored" && e.source.is_none() && !e.text.trim().is_empty())
        .map(|e| {
            item_row(Old {
                id: id_for("knowledge", &e.id),
                text: &e.text,
                item_type: ItemType::Detail,
                state: ItemState::Current,
                as_of: e.created_at / 1000,
                updated_at: e.created_at / 1000,
                saved_by: "person".into(),
                confirmed: true,
                store: "knowledge",
                subject: Some(KEPT_BEFORE.to_string()),
                audience: Audience::All,
                for_ai: Some(ai_founding.to_string()),
            })
        })
        .collect()
}

/// Some older rows carry a time in microseconds where milliseconds were
/// meant: anything this large is one of those.
fn as_ms(t: i64) -> i64 {
    if t > 100_000_000_000_000 { t / 1000 } else { t }
}

/// A row that mirrors a line of the facts file (the row keeps the fact's
/// id and its fields), read back as that fact. Facts learned on another
/// device reach this one only as such rows.
pub(crate) fn fact_from_row(r: &MemoryRow) -> Option<MemoryFact> {
    let s = r.structured.as_ref()?;
    let predicate = s.get("predicate")?.as_str()?;
    let value = s.get("value")?.as_str()?;
    let text = |k: &str| s.get(k).and_then(|v| v.as_str()).map(str::to_string);
    let micros = |ms: i64| as_ms(ms) * 1000;
    serde_json::from_value(serde_json::json!({
        "id": r.id, "subject": text("subject").unwrap_or_else(|| "user".into()), "predicate": predicate, "value": value,
        "confidence": s.get("confidence").and_then(|c| c.as_f64()).unwrap_or(0.5),
        "valid_from": micros(r.as_of), "valid_to": r.valid_to.map(micros),
        "source_action_hash": text("source_action_hash"), "source_ai_id": text("source_ai_id"),
        "owner_scope": "personal", "key_scope": "personal",
        "entry_kind": text("entry_kind").unwrap_or_else(|| if r.kind == "note" { "note".into() } else { "fact".into() }),
        "provenance": text("provenance").unwrap_or_else(|| "extracted".into()),
        "group_id": text("group_id"),
        "created_at": micros(r.as_of), "updated_at": micros(r.updated_at),
    }))
    .ok()
}

/// The facts to carry over: this device's facts file, and the facts that
/// are here only as rows (learned on another of the person's devices).
/// The file's own copy of a fact stands where both hold it.
pub(crate) fn all_facts(file: &[MemoryFact], person_rows: &[MemoryRow]) -> Vec<MemoryFact> {
    let mut out: Vec<MemoryFact> = file.to_vec();
    for r in person_rows.iter().filter(|r| !r.deleted && r.scope == "person") {
        if let Some(f) = fact_from_row(r) {
            if !out.iter().any(|x| x.id == f.id) {
                out.push(f);
            }
        }
    }
    out
}

/// Decision, fact and note rows of the person's or an AI's scope: each
/// becomes an item, marked by who saved it. Rules, commands and locations
/// stay where they are; episodes are not carried. Rows that mirror the
/// facts file are carried as facts (`all_facts`), once. A note the person
/// gave one AI by hand is here as a row with the same id it has in that
/// AI's own file: it gets the id `from_knowledge` gives it, so it is one
/// item whichever store it is read from.
pub(crate) fn from_rows(rows: &[MemoryRow]) -> Vec<MemoryRow> {
    rows.iter()
        .filter(|r| !r.deleted && r.scope != SCOPE && matches!(r.kind.as_str(), "decision" | "fact" | "note") && !r.text.trim().is_empty())
        .filter(|r| fact_from_row(r).is_none() && !r.provenance.starts_with("migrated:"))
        .map(|r| {
            let by_person = r.saved_by == "person";
            let for_ai = r.scope.strip_prefix("ai:").map(str::to_string);
            let given_by_hand = by_person && for_ai.is_some() && r.kind == "note" && r.provenance == "authored";
            item_row(Old {
                id: if given_by_hand { id_for("knowledge", &r.id) } else { id_for("row", &r.id) },
                text: &r.text,
                item_type: match r.kind.as_str() {
                    "decision" => ItemType::Decision,
                    "fact" => ItemType::AboutPerson,
                    _ => ItemType::Detail,
                },
                // A fact about the person that an AI wrote down is a guess
                // like the others: set aside.
                state: if r.kind == "fact" && !by_person { ItemState::SetAside } else { ItemState::Current },
                as_of: as_ms(r.as_of),
                updated_at: if given_by_hand { as_ms(r.as_of) } else { as_ms(r.updated_at) },
                saved_by: r.saved_by.clone(),
                confirmed: by_person,
                store: if given_by_hand { "knowledge" } else { "row" },
                subject: if r.kind == "fact" { Some(ABOUT_YOU.to_string()) } else { Some(KEPT_BEFORE.to_string()) },
                audience: match &for_ai {
                    // What an AI kept for itself stays with that AI; what
                    // the person saved is for all of them.
                    Some(ai) if !by_person => Audience::Ai { ai: ai.clone() },
                    _ => Audience::All,
                },
                for_ai,
            })
        })
        .collect()
}

/// A project's memory text: each line becomes an item of that project,
/// marked as kept, for turns in that project. `at` is when the text was
/// last written (ms). The text itself stays in the record as history.
pub(crate) fn from_project_text(project: &str, text: &str, at: i64) -> Vec<MemoryRow> {
    let subject = project_subject_id(project);
    let mut seen: Vec<String> = Vec::new();
    text.lines()
        .map(|l| l.trim().trim_start_matches(['-', '*', '•']).trim())
        .filter(|l| !l.is_empty() && !l.starts_with('#') && !l.starts_with('[') && l.chars().any(char::is_alphanumeric))
        .filter(|l| {
            let key = l.to_lowercase();
            let new = !seen.contains(&key);
            seen.push(key);
            new
        })
        .map(|l| {
            let print = blake3::hash(format!("{project}\n{}", l.to_lowercase()).as_bytes()).to_hex();
            item_row(Old {
                id: id_for("project", &print[..24]),
                text: l,
                item_type: ItemType::Detail,
                state: ItemState::Current,
                as_of: at,
                updated_at: at,
                saved_by: "person".into(),
                confirmed: true,
                store: "project",
                subject: Some(subject.clone()),
                audience: Audience::Project { project: project.to_string() },
                for_ai: None,
            })
        })
        .collect()
}

/// The id of a project's subject: the same on every device.
pub(crate) fn project_subject_id(project: &str) -> String {
    format!("sub-project-{}", &blake3::hash(project.as_bytes()).to_hex()[..16])
}

/// The subjects the migrated items sit in.
pub(crate) fn subjects_for(rows: &[MemoryRow], projects: &[(String, String)]) -> Vec<Subject> {
    let in_subject = |id: &str| rows.iter().any(|r| crate::memory_items::meta_of(r).is_some_and(|m| m.subject.as_deref() == Some(id)));
    let mut out = Vec::new();
    if in_subject(ABOUT_YOU) {
        out.push(Subject { id: ABOUT_YOU.to_string(), name: "About you".into(), name_accepted: true, from: MadeFrom::AboutYou, merged_into: None });
    }
    if in_subject(KEPT_BEFORE) {
        out.push(Subject { id: KEPT_BEFORE.to_string(), name: "Kept before the update".into(), name_accepted: false, from: MadeFrom::ByHand, merged_into: None });
    }
    for (project, name) in projects {
        let id = project_subject_id(project);
        if in_subject(&id) {
            out.push(Subject { id, name: name.clone(), name_accepted: false, from: MadeFrom::Project { project: project.clone() }, merged_into: None });
        }
    }
    out
}

/// Of the rows the update would write, those not already written: an
/// item the person has changed since (kept a set-aside fact, corrected a
/// save) is never written over.
pub(crate) fn still_to_write(wanted: Vec<MemoryRow>, held: &[MemoryRow]) -> Vec<MemoryRow> {
    wanted.into_iter().filter(|w| !held.iter().any(|h| h.id == w.id)).collect()
}

/// The old stores of one profile, as data.
#[derive(Default)]
pub(crate) struct Stores {
    pub facts: Vec<MemoryFact>,
    /// The person's rows and each AI's rows (not memory's own scope).
    pub rows: Vec<MemoryRow>,
    /// What the person gave each AI by hand: (the AI's founding key, its entries).
    pub knowledge: Vec<(String, Vec<TranscriptEmbedding>)>,
    /// Each project's memory text: (identity, name, text, when written in ms).
    pub projects: Vec<(String, String, String, i64)>,
}

/// Where each old thing went, in counts: the page "What changed in the
/// update" reads this.
#[derive(serde::Serialize, serde::Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// Saved by the person before the update, carried over and used.
    pub kept_by_you: u32,
    /// Learned by an AI, set aside and used by no AI.
    pub set_aside: u32,
    /// The person's name goes on being used.
    pub name: bool,
    /// Lines of project memory texts, now items of their projects.
    pub project_lines: u32,
    pub projects: u32,
    /// The short description of the person was in the old store.
    pub portrait_retired: bool,
    /// Rows written by this run (0 on a second run).
    pub written: u32,
    /// Tidying starts off because learning was paused before.
    pub tidying_starts_off: bool,
    /// When the update's pass finished on this device (ms).
    pub done_at: i64,
}

/// Every row the update wants written, the subjects they sit in, and the
/// counts. No reading and no writing here.
pub(crate) fn plan(stores: &Stores) -> (Vec<MemoryRow>, Vec<Subject>, Report) {
    let mut rows = from_facts(&all_facts(&stores.facts, &stores.rows));
    for (founding, entries) in &stores.knowledge {
        rows.extend(from_knowledge(founding, entries));
    }
    rows.extend(from_rows(&stores.rows));
    let mut report = Report::default();
    for (project, _, text, at) in &stores.projects {
        let lines = from_project_text(project, text, *at);
        if !lines.is_empty() {
            report.projects += 1;
            report.project_lines += lines.len() as u32;
        }
        rows.extend(lines);
    }
    // One item per id: the same save can be read from two stores.
    let mut seen = std::collections::HashSet::new();
    rows.retain(|r| seen.insert(r.id.clone()));
    for r in &rows {
        let Some(meta) = crate::memory_items::meta_of(r) else { continue };
        match meta.state {
            ItemState::SetAside => report.set_aside += 1,
            _ if r.id == "mig-name" => report.name = true,
            _ if matches!(meta.audience, Audience::Project { .. }) && r.provenance == "migrated:project" => {}
            _ => report.kept_by_you += 1,
        }
    }
    report.portrait_retired = stores.facts.iter().any(|f| f.entry_kind == "synthesis" && f.valid_to.is_none());
    let names: Vec<(String, String)> = stores.projects.iter().map(|(id, name, _, _)| (id.clone(), name.clone())).collect();
    let subjects = subjects_for(&rows, &names);
    (rows, subjects, report)
}

const REPORT_FILE: &str = "memory-update.json";

fn report_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    crate::profile::root(app).ok().map(|r| r.join(REPORT_FILE))
}

/// Has the update's pass finished on this profile? From then on the
/// older stores are given to no AI.
pub(crate) fn update_done(app: &tauri::AppHandle) -> bool {
    report_path(app).is_some_and(|p| p.exists())
}

/// Development builds' tests only: put the profile back to before the
/// update's pass (the rows it wrote stay; a new pass writes none twice).
pub(crate) fn forget_report(app: &tauri::AppHandle) {
    if let Some(p) = report_path(app) {
        let _ = std::fs::remove_file(p);
    }
}

/// The update's report on this device, once its pass has finished.
#[tauri::command]
pub fn memory_update_report(app: tauri::AppHandle) -> Option<Report> {
    serde_json::from_slice(&std::fs::read(report_path(&app)?).ok()?).ok()
}

/// One pass of the update. Reads the old stores, writes the rows that are
/// not written yet, and saves the report. Ids come from the old data, so
/// a pass that stopped halfway is finished by the next one, a second pass
/// writes nothing, and two devices write the same rows. The old stores
/// are left as they are. `learning_was_paused`: the device's old switch.
pub(crate) async fn run(app: &tauri::AppHandle, learning_was_paused: bool) -> Result<Report, String> {
    use tauri::Manager;
    let hc = app.try_state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().ok_or("the records are not open yet")?;
    let manager = hc.get()?;
    let key = manager.data_key()?;
    let agents: Vec<String> = manager.ai_agents().await;
    let agent = agents.first().cloned().ok_or("no AI has a place in the records yet")?;

    let mut stores = Stores { facts: crate::memory::read_facts(app, &key).unwrap_or_default(), ..Default::default() };
    stores.rows = crate::memory_rows::rows_live(app, &manager, &agent, "person").await?;
    let mut foundings: Vec<String> = Vec::new();
    for a in &agents {
        let founding = crate::commands_holochain::founding_key_for(app, a);
        if !foundings.contains(&founding) {
            stores.rows.extend(crate::memory_rows::rows_live(app, &manager, a, &format!("ai:{founding}")).await.unwrap_or_default());
            foundings.push(founding.clone());
        }
        stores.knowledge.push((founding, crate::transcript_memory::read_for(app, &key, a).unwrap_or_default()));
    }
    for folder in crate::commands_holochain::project_memory_folders(app) {
        let info = match crate::commands_holochain::project_memory_read_records_info(app, &folder).await {
            Ok(info) if !info.content.trim().is_empty() => info,
            Ok(_) => continue,
            Err(e) => return Err(format!("a project's memory could not be read ({e}); the update tries again later")),
        };
        let path = std::path::Path::new(&folder);
        let name = path.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_else(|| folder.clone());
        stores.projects.push((crate::project_identity::project_identity(path), name, info.content, info.updated_at / 1000));
    }

    let (wanted, subjects, mut report) = plan(&stores);
    let held = crate::memory_rows::rows_live(app, &manager, &agent, SCOPE).await?;
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0);
    let mut to_write = still_to_write(wanted, &held);
    to_write.extend(still_to_write(subjects.iter().map(|s| crate::memory_subjects::subject_row(s, now)).collect(), &held));
    // The device's old switch, kept on the safe side for every device: a
    // row that says tidying is off, unless one was written already.
    let tidy_row_held = held.iter().any(|r| r.id == "sys-tidy-off");
    if learning_was_paused && !tidy_row_held {
        to_write.push(crate::memory_tidy::tidy_off_row(true, now));
    }
    report.tidying_starts_off = learning_was_paused || crate::memory_tidy::tidy_is_off(&held);
    for row in to_write {
        crate::memory_rows::save_and_cache(app, row).await?;
        report.written += 1;
    }
    if report.written > 0 {
        // What was carried over is found by meaning like any item.
        crate::memory_tidying::words_changed(app);
    }
    report.done_at = now;
    if let Some(path) = report_path(app) {
        // The first pass's count of rows written stays: a later pass writes none.
        let first = memory_update_report(app.clone());
        let kept = Report { written: first.as_ref().map_or(report.written, |f| f.written + report.written), done_at: first.as_ref().map_or(now, |f| f.done_at), ..report.clone() };
        let _ = std::fs::write(path, serde_json::to_vec(&kept).map_err(|e| e.to_string())?);
        report = kept;
    }
    log::info!("[memory] update: {} kept by the person, {} set aside, {} project line(s) in {} project(s), {} row(s) written", report.kept_by_you, report.set_aside, report.project_lines, report.projects, report.written);
    Ok(report)
}

/// Run the update's pass now (the page calls it once the records are
/// open, and again after a restart until a report is saved).
#[tauri::command]
pub async fn memory_update_run(app: tauri::AppHandle, learning_was_paused: Option<bool>) -> Result<Report, String> {
    run(&app, learning_was_paused.unwrap_or(false)).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_items::{items, may_use, meta_of};

    const T: i64 = 1_740_000_000_000_000; // micros

    fn fact(id: &str, predicate: &str, value: &str, kind: &str, ai: Option<&str>) -> MemoryFact {
        serde_json::from_value(serde_json::json!({
            "id": id, "subject": "user", "predicate": predicate, "value": value, "confidence": 0.9, "valid_from": T, "source_ai_id": ai,
            "owner_scope": "personal", "key_scope": "personal", "entry_kind": kind, "provenance": if kind == "note" { "authored" } else { "extracted" },
            "created_at": T, "updated_at": T + 5_000_000,
        }))
        .unwrap()
    }

    #[test]
    fn what_an_ai_learned_is_set_aside_and_the_name_keeps_working() {
        let facts = vec![
            fact("f1", "name_is", "Marta", "fact", Some("ai-juno")),
            fact("f2", "runs_business", "Crumb & Kettle, a bakery", "fact", Some("ai-juno")),
            fact("f3", "lives_in", "Port Albury", "fact", Some("ai-atlas")),
            fact("f4", "synthesis", "Marta runs a small bakery.", "synthesis", None),
            fact("f5", "library", "Recipes and letters.", "library", None),
            { let mut f = fact("f6", "old_supplier", "Dunmore Grain", "fact", None); f.valid_to = Some(T + 1); f },
        ];
        let rows = from_facts(&facts);
        let all = items(&rows);
        assert_eq!(all.len(), 3, "the name, and two set-aside facts; the portraits and the closed fact make no row");
        let name = all.iter().find(|i| i.row.id == "mig-name").unwrap();
        assert_eq!((name.row.text.as_str(), name.meta.state, name.meta.confirmed.is_some(), name.meta.subject.as_deref()), ("Their name is Marta.", ItemState::Current, true, Some(ABOUT_YOU)));
        assert!(may_use(&name.meta, "any", None, true));
        let aside = all.iter().find(|i| i.row.id == "mig-fact-f2").unwrap();
        assert_eq!((aside.row.text.as_str(), aside.meta.state, aside.meta.confirmed.clone()), ("runs business: Crumb & Kettle, a bakery", ItemState::SetAside, None));
        assert_eq!((aside.row.saved_by.as_str(), aside.meta.source.for_ai.as_deref(), aside.meta.source.why.as_str()), ("ai:ai-juno", Some("ai-juno"), "migrated:fact"));
        assert_eq!((aside.row.as_of, aside.row.updated_at), (T / 1000, T / 1000 + 5000), "its old dates, in milliseconds");
        assert!(!may_use(&aside.meta, "ai-juno", None, false), "a set-aside fact is given to no AI");
        assert!(!rows.iter().any(|r| r.text.contains("small bakery") || r.text.contains("Dunmore")));
        // Keep on one makes it current and confirmed, and it is used again.
        let (kept, _) = crate::memory_items::keep_set_aside(aside, 9).unwrap();
        let meta = meta_of(&kept).unwrap();
        assert_eq!((meta.state, meta.subject.as_deref(), meta.confirmed.as_ref().map(|c| c.how.as_str())), (ItemState::Current, Some(ABOUT_YOU), Some("kept")));
        assert!(crate::memory_items::keep_set_aside(name, 9).is_none(), "Keep is for what was set aside");
        assert!(may_use(&meta, "ai-atlas", None, false));
    }

    #[test]
    fn what_the_person_saved_is_carried_over_and_used() {
        let mut long_a = fact("n2", "note", "The oven manual, part one.", "note", Some("ai-atlas"));
        long_a.group_id = Some("g1".into());
        let mut long_b = fact("n3", "note", "The oven manual, part two.", "note", Some("ai-atlas"));
        long_b.group_id = Some("g1".into());
        long_b.created_at = T + 1;
        let facts = vec![fact("n1", "note", "The oven runs 10 degrees hot.", "note", None), long_b, long_a];
        let rows = from_facts(&facts);
        let all = items(&rows);
        assert_eq!(all.len(), 2, "the two pieces of one long save are one item");
        let short = all.iter().find(|i| i.row.id == "mig-note-n1").unwrap();
        assert_eq!((short.row.saved_by.as_str(), short.meta.state, short.meta.confirmed.as_ref().map(|c| c.how.as_str()), short.meta.audience.clone()), ("person", ItemState::Current, Some("kept"), Audience::All));
        assert_eq!((short.meta.source.kind.as_str(), short.meta.source.ptr, short.meta.subject.as_deref()), ("legacy", None, Some(KEPT_BEFORE)));
        let long = all.iter().find(|i| i.row.id == "mig-note-g1").unwrap();
        assert_eq!(long.row.text, "The oven manual, part one.\nThe oven manual, part two.");
        assert_eq!(long.meta.source.for_ai.as_deref(), Some("ai-atlas"), "saved for Atlas, now for all the person's AIs");
        // Knowledge given to one AI by hand: the same, with the AI it was for.
        let entry = |id: &str, kind: &str, text: &str, doc: bool| -> TranscriptEmbedding {
            serde_json::from_value(serde_json::json!({ "id": id, "conversation_hash": "authored", "role": "authored", "text": text, "vector": [0.1], "kind": kind, "created_at": T, "source": if doc { serde_json::json!({ "doc_id": "d1", "filename": "manual.pdf", "size_bytes": 10 }) } else { serde_json::Value::Null } })).unwrap()
        };
        let given = from_knowledge("uhCAkAtlas", &[entry("k1", "authored", "Loaves are priced per 800 grams.", false), entry("k2", "authored", "A passage of a document.", true), entry("k3", "episodic", "an earlier turn", false)]);
        assert_eq!(given.len(), 1, "a document's passages and earlier turns are not items");
        let m = meta_of(&given[0]).unwrap();
        assert_eq!((given[0].id.as_str(), m.audience, m.source.for_ai.as_deref(), m.confirmed.is_some()), ("mig-knowledge-k1", Audience::All, Some("uhCAkAtlas"), true));
    }

    fn old_row(id: &str, scope: &str, kind: &str, text: &str, saved_by: &str) -> MemoryRow {
        serde_json::from_value(serde_json::json!({ "id": id, "scope": scope, "kind": kind, "text": text, "as_of": 1000, "saved_by": saved_by, "provenance": "authored", "updated_at": 2000 })).unwrap()
    }

    #[test]
    fn old_rows_become_items_marked_by_who_saved_them() {
        let rows = vec![
            old_row("r1", "person", "decision", "Free delivery starts at $60.", "person"),
            old_row("r2", "ai:uhCAkJuno", "note", "They like short answers.", "ai:uhCAkJuno"),
            old_row("r3", "person", "rule", "Prices are always in dollars.", "person"),
            old_row("r4", "ai:uhCAkJuno", "episode", "Conversation: flour (2 questions)", "ai:uhCAkJuno"),
            old_row("r5", "person", "command", "Print labels: labels --all", "person"),
            crate::memory_rows::fact_to_row(&fact("f2", "runs_business", "Crumb & Kettle", "fact", Some("ai-juno"))).unwrap(),
        ];
        let out = from_rows(&rows);
        assert_eq!(out.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["mig-row-r1", "mig-row-r2"], "rules, commands and episodes stay where they are; a mirror of the facts file is carried as a fact");
        let (mine, theirs) = (meta_of(&out[0]).unwrap(), meta_of(&out[1]).unwrap());
        assert_eq!((mine.item_type, mine.confirmed.is_some(), mine.audience), (ItemType::Decision, true, Audience::All));
        assert_eq!((theirs.confirmed, theirs.audience, out[1].saved_by.as_str()), (None, Audience::Ai { ai: "uhCAkJuno".into() }, "ai:uhCAkJuno"));
        assert_eq!((out[0].as_of, out[0].updated_at), (1000, 2000));
    }

    #[test]
    fn a_fact_or_a_note_is_one_item_whichever_store_it_is_read_from() {
        // The facts file on this device, and the same facts as the rows that mirror them.
        let facts = vec![fact("f1", "name_is", "Marta", "fact", None), fact("f2", "runs_business", "a bakery", "fact", Some("ai-juno")), fact("n1", "note", "The oven runs hot.", "note", None)];
        let mirrors: Vec<MemoryRow> = facts.iter().filter_map(crate::memory_rows::fact_to_row).collect();
        assert_eq!(mirrors.len(), 3);
        // Carried once: the rows are not also carried as rows.
        assert!(from_rows(&mirrors).is_empty());
        let both = from_facts(&all_facts(&facts, &mirrors));
        assert_eq!(both.len(), from_facts(&facts).len());
        // A device that has the facts only as rows writes the same ids, states and words.
        let only_rows = from_facts(&all_facts(&[], &mirrors));
        let key = |rows: &[MemoryRow]| { let mut k: Vec<(String, String, String)> = rows.iter().map(|r| (r.id.clone(), r.text.clone(), format!("{:?}", meta_of(r).unwrap().state))).collect(); k.sort(); k };
        assert_eq!(key(&only_rows), key(&from_facts(&facts)));
        assert_eq!(only_rows.iter().find(|r| r.id == "mig-fact-f2").unwrap().as_of, T / 1000);
        // Knowledge given to one AI by hand: in that AI's file and as its row, one item.
        let entry: TranscriptEmbedding = serde_json::from_value(serde_json::json!({ "id": "k1", "conversation_hash": "authored", "role": "authored", "text": "Loaves are priced per 800 grams.", "vector": [0.1], "kind": "authored", "created_at": T })).unwrap();
        let from_file = from_knowledge("uhCAkAtlas", &[entry.clone()]);
        let from_row = from_rows(&[crate::memory_rows::knowledge_to_row(&entry, "uhCAkAtlas")]);
        assert_eq!(serde_json::to_string(&from_file).unwrap(), serde_json::to_string(&from_row).unwrap());
        // A fact an AI wrote straight into its own rows is set aside too.
        let guess = old_row("r9", "ai:uhCAkJuno", "fact", "They own a van.", "ai:uhCAkJuno");
        assert_eq!(meta_of(&from_rows(&[guess])[0]).unwrap().state, ItemState::SetAside);
    }

    #[test]
    fn each_line_of_a_projects_memory_text_is_a_project_item() {
        let text = "# Bakery site\n\n- Deploy with: firebase deploy --only hosting\n- The order form's timezone is Pacific/Auckland.\n* Tests live in e2e/.\n\n[Project memory - durable notes]\n- deploy with: firebase deploy --only hosting\n---\n";
        let rows = from_project_text("id:p1", text, 5000);
        assert_eq!(rows.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(), vec!["Deploy with: firebase deploy --only hosting", "The order form's timezone is Pacific/Auckland.", "Tests live in e2e/."], "headings, markers, rules and a repeated line make no item");
        let m = meta_of(&rows[0]).unwrap();
        assert_eq!((m.audience.clone(), m.confirmed.is_some(), m.subject.as_deref()), (Audience::Project { project: "id:p1".into() }, true, Some(project_subject_id("id:p1").as_str())));
        assert!(may_use(&m, "any-ai", Some("id:p1"), false) && !may_use(&m, "any-ai", Some("id:p2"), false) && !may_use(&m, "any-ai", None, false));
        // The same line in another project is another item.
        assert_ne!(from_project_text("id:p2", text, 5000)[0].id, rows[0].id);
        let subjects = subjects_for(&rows, &[("id:p1".into(), "Bakery site".into()), ("id:p9".into(), "Unused".into())]);
        assert_eq!(subjects.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["Bakery site"]);
    }

    #[test]
    fn the_report_accounts_for_every_old_thing_once() {
        let stores = Stores {
            facts: vec![
                fact("f1", "name_is", "Marta", "fact", Some("ai-juno")),
                fact("f2", "runs_business", "Crumb & Kettle, a bakery", "fact", Some("ai-juno")),
                fact("f3", "lives_in", "Port Albury", "fact", Some("ai-atlas")),
                fact("f4", "synthesis", "Marta runs a small bakery.", "synthesis", None),
                fact("n1", "note", "The flour supplier is Mill and Sons.", "note", None),
            ],
            projects: vec![("proj-a".into(), "Bakery site".into(), "- Deploy with make ship\n- The menu lives in data/menu.json\n".into(), T / 1000), ("proj-b".into(), "Empty".into(), "\n".into(), T / 1000)],
            ..Default::default()
        };
        let (rows, subjects, report) = plan(&stores);
        assert_eq!((report.name, report.set_aside, report.kept_by_you, report.project_lines, report.projects, report.portrait_retired), (true, 2, 1, 2, 1, true));
        assert_eq!(rows.len() as u32, 1 + report.set_aside + report.kept_by_you + report.project_lines, "every row is counted in exactly one place");
        assert!(subjects.iter().any(|s| s.name == "Bakery site"));
        // A second device with the same stores plans the same rows.
        let (again, _, _) = plan(&stores);
        assert_eq!(rows.iter().map(|r| &r.id).collect::<Vec<_>>(), again.iter().map(|r| &r.id).collect::<Vec<_>>());
        assert!(still_to_write(again, &rows).is_empty());
    }

    #[test]
    fn two_devices_write_the_same_rows_and_a_second_run_writes_nothing() {
        let facts = vec![fact("f1", "name_is", "Marta", "fact", None), fact("f2", "runs_business", "a bakery", "fact", Some("ai-juno")), fact("n1", "note", "The oven runs hot.", "note", None)];
        let on_the_laptop = from_facts(&facts);
        let on_the_desktop = from_facts(&facts);
        assert_eq!(serde_json::to_string(&on_the_laptop).unwrap(), serde_json::to_string(&on_the_desktop).unwrap(), "byte for byte, dates included");
        assert_eq!(crate::memory_rows::reduce([on_the_laptop.clone(), on_the_desktop].concat()).len(), 3, "nothing doubles");
        // A second run, after the person kept the set-aside fact: nothing is written over.
        let mut held = on_the_laptop.clone();
        let kept = held.iter_mut().find(|r| r.id == "mig-fact-f2").unwrap();
        kept.text = "runs a bakery called Crumb & Kettle".into();
        assert!(still_to_write(from_facts(&facts), &held).is_empty());
        // A fact learned on another device since arrives by itself.
        let more = [facts.clone(), vec![fact("f9", "has_pet", "a dog", "fact", Some("ai-juno"))]].concat();
        assert_eq!(still_to_write(from_facts(&more), &held).iter().map(|r| r.id.as_str()).collect::<Vec<_>>(), vec!["mig-fact-f9"]);
        // Every migrated row is an item of memory's scope, inside the closed list of kinds.
        for r in &on_the_laptop {
            assert!(r.scope == SCOPE && crate::memory_rows::KINDS.contains(&r.kind.as_str()) && meta_of(r).is_some());
        }
        let subjects = subjects_for(&on_the_laptop, &[]);
        assert_eq!(subjects.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(), vec![ABOUT_YOU, KEPT_BEFORE]);
    }
}
