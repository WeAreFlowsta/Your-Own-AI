//! Everything memory adds to a request, built in one place for every path
//! that sends one: the chat page, the local port (as a persona and for an
//! agent), and project and tools sessions.
//!
//! `block` is the text itself, from what the stores hold; it reads nothing
//! and asks nothing, so the same input always gives the same text.

use crate::memory::MemoryFact;
use crate::memory_rows::MemoryRow;
use crate::transcript_memory::TranscriptEmbedding;

/// The always-given facts stop at this size, counted as the lines are
/// written (about 25 short facts).
pub(crate) const FACTS_BUDGET: usize = 2048;
/// How close a saved note must be to the question to be given.
const NOTE_FLOOR: f32 = 0.25;
const NOTES_AT_MOST: usize = 3;
/// How close an earlier turn or a piece of given knowledge must be, how far
/// under the best one the others may fall, and how many are given. On the
/// embedding model's scale right passages score 0.21 to 0.75 and the best
/// wrong one up to 0.32, so the floor only drops noise and the margin does
/// the separating: first-person turns cluster close together, and without
/// the margin a strong match drags in an unrelated second.
const RECALL_FLOOR: f32 = 0.25;
const RECALL_MARGIN: f32 = 0.12;
const RECALL_AT_MOST: usize = 3;

const NAME_PREDICATES: [&str; 7] = ["name_is", "name", "full_name", "first_name", "goes_by", "has_name", "is_named"];

pub(crate) fn is_name_predicate(predicate: &str) -> bool {
    NAME_PREDICATES.contains(&predicate.trim().to_lowercase().as_str())
}

/// What the stores hold for one turn.
pub(crate) struct Held<'a> {
    pub facts: &'a [MemoryFact],
    /// The rows of the scopes in play, in the order the scopes are given:
    /// the person's, then the AI's.
    pub rows: &'a [MemoryRow],
    /// The rows of the project the turn is in, when it is in one.
    pub project_rows: &'a [MemoryRow],
    /// The AI's index of earlier turns and given knowledge; `None` when the
    /// turn names no AI.
    pub recall: Option<&'a [TranscriptEmbedding]>,
    /// The question, embedded; `None` when there is no question or no
    /// embedding to be had, and then nothing is retrieved.
    pub question_vector: Option<&'a [f32]>,
    /// The open conversation, whose own turns are not "earlier".
    pub conversation: Option<&'a str>,
}

/// What a block carried, for the line under a reply.
#[derive(serde::Serialize, Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct Counts {
    pub rules: usize,
    pub commands: usize,
    pub locations: usize,
    pub facts: usize,
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    // A vector of another length belongs to another model: no likeness.
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0f64, 0f64, 0f64);
    for (x, y) in a.iter().zip(b) {
        dot += f64::from(*x) * f64::from(*y);
        na += f64::from(*x) * f64::from(*x);
        nb += f64::from(*y) * f64::from(*y);
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom == 0.0 { 0.0 } else { (dot / denom) as f32 }
}

fn by_score<T>(scored: &mut [(T, f32)]) {
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
}

/// The closest entries: over the floor, within the margin of the best.
fn closest<'a>(pool: impl Iterator<Item = &'a TranscriptEmbedding>, question: &[f32]) -> Vec<&'a str> {
    let mut scored: Vec<(&TranscriptEmbedding, f32)> = pool.map(|e| (e, cosine(question, &e.vector))).filter(|(_, s)| *s >= RECALL_FLOOR).collect();
    by_score(&mut scored);
    let Some(best) = scored.first().map(|s| s.1) else { return Vec::new() };
    let floor = RECALL_FLOOR.max(best - RECALL_MARGIN);
    scored.into_iter().filter(|(_, s)| *s >= floor).take(RECALL_AT_MOST).map(|(e, _)| e.text.as_str()).collect()
}

fn listed<'a>(lines: impl IntoIterator<Item = &'a str>) -> String {
    lines.into_iter().map(|t| format!("- {t}")).collect::<Vec<_>>().join("\n")
}

/// The rules given with every turn stop at this size; over it, pinned rows
/// and then the newest are kept, the person's and the AI's before a
/// project's.
pub(crate) const RULES_BUDGET: usize = 2048;

fn is_rule_kind(kind: &str) -> bool {
    matches!(kind, "rule" | "command" | "location")
}

fn layer_text(rows: &[&MemoryRow]) -> String {
    let of = |kind: &str| rows.iter().filter(|r| r.kind == kind).map(|r| r.text.as_str()).collect::<Vec<_>>();
    let mut lines: Vec<String> = Vec::new();
    for (heading, texts) in [
        ("Rules from them, in their words - follow them in every answer:", of("rule")),
        ("Commands they use here:", of("command")),
        ("Where things are:", of("location")),
    ] {
        if !texts.is_empty() {
            lines.push(heading.to_string());
            lines.push(listed(texts));
        }
    }
    lines.join("\n")
}

pub(crate) const PROJECT_HEADING: &str = "Rules for this project:";

/// The rows of `all` that are among `kept`, in the order of `all`.
fn in_order<'a>(kept: &[&'a MemoryRow], all: &[&'a MemoryRow]) -> Vec<&'a MemoryRow> {
    all.iter().copied().filter(|r| kept.iter().any(|k| std::ptr::eq(*k, *r))).collect()
}

/// The rules layer: the person's words, first, on every turn. The rows of
/// the person and the AI, then the rows of the project the turn is in.
fn rules_layer(rows: &[MemoryRow], project_rows: &[MemoryRow]) -> (String, Counts) {
    let mut own: Vec<&MemoryRow> = rows.iter().filter(|r| is_rule_kind(&r.kind)).collect();
    let mut project: Vec<&MemoryRow> = project_rows.iter().filter(|r| is_rule_kind(&r.kind)).collect();
    let text_of = |own: &[&MemoryRow], project: &[&MemoryRow]| {
        let (a, b) = (layer_text(own), layer_text(project));
        match (a.is_empty(), b.is_empty()) {
            (_, true) => a,
            (true, false) => format!("{PROJECT_HEADING}\n{b}"),
            (false, false) => format!("{a}\n{PROJECT_HEADING}\n{b}"),
        }
    };
    if text_of(&own, &project).len() > RULES_BUDGET {
        // Which rows stay: pinned, then the newest; the person's and the
        // AI's before the project's. They are still given in their order.
        let mut wanted: Vec<(bool, &MemoryRow)> = Vec::new();
        for (of_project, group) in [(false, &own), (true, &project)] {
            let mut g: Vec<&MemoryRow> = group.to_vec();
            g.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.updated_at.cmp(&a.updated_at)));
            wanted.extend(g.into_iter().map(|r| (of_project, r)));
        }
        let (mut keep_own, mut keep_project): (Vec<&MemoryRow>, Vec<&MemoryRow>) = (Vec::new(), Vec::new());
        for (of_project, r) in wanted {
            let (mut o, mut p) = (keep_own.clone(), keep_project.clone());
            if of_project { p.push(r) } else { o.push(r) }
            let (o, p) = (in_order(&o, &own), in_order(&p, &project));
            if text_of(&o, &p).len() > RULES_BUDGET {
                break;
            }
            keep_own = o;
            keep_project = p;
        }
        own = keep_own;
        project = keep_project;
    }
    let count = |kind: &str| own.iter().chain(project.iter()).filter(|r| r.kind == kind).count();
    let counts = Counts { rules: count("rule"), commands: count("command"), locations: count("location"), facts: 0 };
    (text_of(&own, &project), counts)
}

/// The memory block of a turn: what the stores hold, with any password or
/// key in it hidden. What memory says is said in other conversations than
/// the one a secret was given in.
pub(crate) fn block(held: &Held) -> (String, Counts) {
    let (text, counts) = block_as_held(held);
    (crate::secret_mask::hand_on(&text).text, counts)
}

fn block_as_held(held: &Held) -> (String, Counts) {
    let live: Vec<&MemoryFact> = held.facts.iter().filter(|f| f.valid_to.is_none()).collect();
    let kind = |f: &MemoryFact, k: &str| f.entry_kind == k;

    // Always given: the facts, strongest then newest, until the budget.
    let mut ranked: Vec<&MemoryFact> = live.iter().copied().filter(|f| !kind(f, "note") && !kind(f, "synthesis") && !kind(f, "library")).collect();
    ranked.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap_or(std::cmp::Ordering::Equal).then(b.updated_at.cmp(&a.updated_at)));
    let fact_line = |f: &MemoryFact| format!("- {}: {}", f.predicate.replace('_', " "), f.value);
    let mut given: Vec<&MemoryFact> = Vec::new();
    let mut size = 0usize;
    for f in ranked {
        let line = fact_line(f).encode_utf16().count() + 1;
        if !given.is_empty() && size + line > FACTS_BUDGET {
            break;
        }
        given.push(f);
        size += line;
    }
    let name = given.iter().copied().find(|f| is_name_predicate(&f.predicate));
    let others: Vec<&MemoryFact> = given.iter().copied().filter(|f| !name.is_some_and(|n| std::ptr::eq(n, *f))).collect();

    let (rules, mut counts) = rules_layer(held.rows, held.project_rows);
    counts.facts = others.len();
    let name_line = match name {
        Some(f) => format!("Their name is {}.", f.value),
        None => "Their name is unknown. Do not assume or allude to it.".to_string(),
    };
    let mut block = if rules.is_empty() { name_line } else { format!("{rules}\n{name_line}") };
    if let Some(f) = live.iter().find(|f| kind(f, "synthesis")) {
        block.push_str(&format!("\nAbout them, from what they've shared over time (use naturally; never recite): {}", f.value));
    }
    if let Some(f) = live.iter().find(|f| kind(f, "library")) {
        block.push_str(&format!("\nFrom the documents they write and keep in their library (use naturally; never recite): {}", f.value));
    }
    if !others.is_empty() {
        let lines = others.iter().map(|f| fact_line(f)).collect::<Vec<_>>().join("\n");
        block.push_str(&format!("\nThings you've learned about them from past conversations (use naturally where relevant; do not recite this list or claim certainty):\n{lines}"));
    }

    // Retrieved for this question: saved notes, then the AI's own.
    let Some(question) = held.question_vector else { return (block, counts) };
    let mut notes: Vec<(&MemoryFact, f32)> = live
        .iter()
        .copied()
        .filter(|f| kind(f, "note"))
        .filter_map(|f| f.embedding.as_deref().filter(|v| !v.is_empty()).map(|v| (f, cosine(question, v))))
        .filter(|(_, s)| *s >= NOTE_FLOOR)
        .collect();
    by_score(&mut notes);
    if !notes.is_empty() {
        block.push_str(&format!("\nRelevant notes they've saved (use naturally if helpful):\n{}", listed(notes.iter().take(NOTES_AT_MOST).map(|(n, _)| n.value.as_str()))));
    }
    if let Some(recall) = held.recall {
        let authored = closest(recall.iter().filter(|e| e.kind == "authored" && !e.vector.is_empty()), question);
        let earlier = closest(recall.iter().filter(|e| e.kind != "authored" && held.conversation.is_none_or(|c| e.conversation_hash != c)), question);
        if !authored.is_empty() {
            block.push_str(&format!("\nKnowledge this AI has been given (use if relevant):\n{}", listed(authored)));
        }
        if !earlier.is_empty() {
            block.push_str(&format!("\nFrom earlier conversations with this person (use naturally if relevant):\n{}", listed(earlier)));
        }
    }
    (block, counts)
}

/// Which path a turn takes to a model.
#[derive(serde::Deserialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Surface {
    Chat,
    ApiPersona,
    ApiAgent,
    Session,
    ToolsSession,
}

/// One turn, as the path that sends it knows it.
#[derive(serde::Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TurnInput {
    /// The AI's id in the profile; without one nothing of an AI's is given.
    #[serde(default)]
    pub ai_id: Option<String>,
    /// The AI's founding key: its own rules live under it.
    #[serde(default)]
    pub founding_key: Option<String>,
    pub surface: Surface,
    /// The project's identity, on any surface, when the turn is in one.
    #[serde(default)]
    pub project: Option<String>,
    /// The open conversation, when there is one.
    #[serde(default)]
    pub conversation: Option<String>,
    #[serde(default)]
    pub question: String,
    /// The question already embedded by the sender. Absent with
    /// `embed_here`, the question is embedded here; absent without it,
    /// nothing is retrieved.
    #[serde(default)]
    pub question_vector: Option<Vec<f32>>,
    #[serde(default)]
    pub embed_here: bool,
    /// The window the reply is made in, in tokens, and whether the model
    /// is an online one: they set how much the search block may hold.
    #[serde(default)]
    pub window_tokens: Option<u32>,
    #[serde(default)]
    pub online: bool,
    /// What the turn has left after the system prompt, the history and
    /// the reply's reserve, when the sender has counted it.
    #[serde(default)]
    pub room_tokens: Option<u32>,
    /// The person's question before this one, in the open conversation.
    #[serde(default)]
    pub previous_question: Option<String>,
    /// The first sequence of the open conversation that is being sent with
    /// this turn: messages from there on are not "found".
    #[serde(default)]
    pub on_wire_from: Option<u32>,
    /// Build the system part only: no search (the early call of a turn,
    /// made before the model and its window are known).
    #[serde(default)]
    pub skip_search: bool,
}

/// What memory adds to a turn.
#[derive(serde::Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TurnMemory {
    /// Rules, the name line, what is known about the person, and what was
    /// retrieved for the question: the memory part of the system prompt.
    pub system_part: String,
    /// Text placed in the person's turn ahead of the question.
    pub before_question: Option<String>,
    /// Text placed in the person's turn after the question.
    pub after_question: Option<String>,
    pub counts: Counts,
    /// A saved note has no embedding yet (the sender embeds it for the
    /// next turn).
    pub bare_notes: bool,
    /// Knowledge given to the AI has no embedding yet.
    pub bare_knowledge: bool,
    /// What the reply is given, to be recorded with it.
    pub given: Option<crate::memory_block::GivenRecord>,
    /// The passages given, as text, one per exchange in `given` and then
    /// the dated lines. For this turn only, in memory: the reply's
    /// quotations are checked against them and the panel shows them.
    pub given_texts: Vec<String>,
}

/// `Today is YYYY-MM-DD.` for a date.
pub(crate) fn date_line(year: i32, month: u32, day: u32) -> String {
    format!("Today is {year:04}-{month:02}-{day:02}.")
}

/// Today's date where this device is.
#[cfg(unix)]
pub(crate) fn local_date() -> (i32, u32, u32) {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0) as libc::time_t;
    // SAFETY: `localtime_r` writes only into the `tm` it is given.
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    unsafe { libc::localtime_r(&now, &mut tm) };
    (tm.tm_year + 1900, (tm.tm_mon + 1) as u32, tm.tm_mday as u32)
}

#[cfg(windows)]
pub(crate) fn local_date() -> (i32, u32, u32) {
    // SAFETY: `GetLocalTime` writes only into the struct it is given.
    let mut t: windows_sys::Win32::Foundation::SYSTEMTIME = unsafe { std::mem::zeroed() };
    unsafe { windows_sys::Win32::System::SystemInformation::GetLocalTime(&mut t) };
    (i32::from(t.wYear), u32::from(t.wMonth), u32::from(t.wDay))
}

/// The scopes whose rules a turn is given, in the order they are given.
pub(crate) fn rule_scopes(founding_key: Option<&str>) -> Vec<String> {
    let mut scopes = vec!["person".to_string()];
    if let Some(k) = founding_key.filter(|k| !k.is_empty()) {
        scopes.push(format!("ai:{k}"));
    }
    scopes
}

fn project_scope(project: Option<&str>) -> Option<String> {
    project.filter(|p| !p.is_empty()).map(|p| format!("project:{p}"))
}

/// The rows of each scope were last asked of the records at these times: a
/// rule saved on another device reaches the cache within a turn or two.
static ROWS_ASKED: std::sync::Mutex<Vec<(String, std::time::Instant)>> = std::sync::Mutex::new(Vec::new());

fn refresh_rows_soon(app: &tauri::AppHandle, scope: &str) {
    {
        let mut asked = ROWS_ASKED.lock().unwrap_or_else(|e| e.into_inner());
        if asked.iter().any(|(s, at)| s == scope && at.elapsed() < std::time::Duration::from_secs(60)) {
            return;
        }
        asked.retain(|(s, _)| s != scope);
        asked.push((scope.to_string(), std::time::Instant::now()));
    }
    let (app, scope) = (app.clone(), scope.to_string());
    tauri::async_runtime::spawn(async move {
        use tauri::Manager;
        let state = app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>();
        let _ = crate::memory_rows::list_memory_rows(app.clone(), None, scope, state).await;
    });
}

/// What changes when a rule changes: the cached rows of the scopes in
/// play. Two turns with the same answer here were given the same rules.
pub(crate) fn rules_revision(app: &tauri::AppHandle, founding_key: Option<&str>, project: Option<&str>) -> String {
    let mut scopes = rule_scopes(founding_key);
    scopes.extend(project_scope(project));
    scopes
        .iter()
        .map(|scope| {
            let stamp = crate::memory_rows::cache_path(app, scope)
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|m| (m.len(), m.modified().ok().and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok()).map(|d| d.as_nanos()).unwrap_or(0)));
            format!("{scope}={stamp:?}")
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// Everything memory adds to one turn, on any path. Never fails a turn:
/// with the stores closed it gives the name-unknown line and the date.
pub(crate) async fn for_turn(app: &tauri::AppHandle, input: TurnInput) -> TurnMemory {
    use tauri::Manager;
    let key = app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get().and_then(|m| m.data_key()).ok();
    let facts = key.as_ref().and_then(|k| crate::memory::read_facts(app, k).ok()).unwrap_or_default();
    let mut rows: Vec<MemoryRow> = Vec::new();
    let mut project_rows: Vec<MemoryRow> = Vec::new();
    if let Some(k) = &key {
        for scope in rule_scopes(input.founding_key.as_deref()) {
            rows.extend(crate::memory_rows::read_cache(app, k, &scope));
            refresh_rows_soon(app, &scope);
        }
        if let Some(scope) = project_scope(input.project.as_deref()) {
            project_rows = crate::memory_rows::read_cache(app, k, &scope);
            refresh_rows_soon(app, &scope);
        }
    }
    let ai_id = input.ai_id.as_deref().filter(|a| !a.is_empty());
    let recall = match (&key, ai_id) {
        (Some(k), Some(ai)) => Some(crate::transcript_memory::read_for(app, k, ai).unwrap_or_default()),
        _ => None,
    };
    let asked = !input.question.trim().is_empty();
    let live_note = |f: &&crate::memory::MemoryFact| f.valid_to.is_none() && f.entry_kind == "note";
    let has_notes = facts.iter().filter(live_note).any(|f| f.embedding.as_ref().is_some_and(|v| !v.is_empty()));
    // A turn typed to an AI gets what memory found beside the question; an
    // agent's step still gets it in the system part.
    let beside = matches!(input.surface, Surface::Chat | Surface::ApiPersona);
    let question_vector = match (asked && !beside, input.question_vector.clone()) {
        (false, _) => None,
        (true, Some(v)) => Some(v),
        (true, None) if input.embed_here && (has_notes || recall.is_some()) => crate::inference_memory::embed_question(app, &input.question).await,
        (true, None) => None,
    };
    let (text, counts) = block(&Held {
        facts: &facts,
        rows: &rows,
        project_rows: &project_rows,
        recall: recall.as_deref(),
        question_vector: question_vector.as_deref(),
        conversation: input.conversation.as_deref().filter(|c| !c.is_empty()),
    });
    let (y, m, d) = local_date();
    let today = date_line(y, m, d);
    let (before_question, given, given_texts) = if beside && asked && !input.skip_search {
        let (text, mut record, texts) = found_beside(app, &input, key.is_some(), &facts, recall.as_deref(), &today).await;
        if let Some(r) = record.as_mut() {
            r.also = crate::memory_block::Also { rules: counts.rules as u32, commands: counts.commands as u32, locations: counts.locations as u32, facts: counts.facts as u32 };
        }
        (text, record, texts)
    } else {
        (None, None, Vec::new())
    };
    TurnMemory {
        system_part: format!("{text}\n{today}"),
        before_question,
        after_question: None,
        counts,
        bare_notes: facts.iter().filter(live_note).any(|f| !f.embedding.as_ref().is_some_and(|v| !v.is_empty())),
        bare_knowledge: recall.as_deref().is_some_and(|r| r.iter().any(|e| e.kind == "authored" && e.source.is_none() && e.vector.is_empty())),
        given,
        given_texts,
    }
}

/// A question short enough to lean on the one before it is also searched
/// together with that one.
pub(crate) fn follow_up_query(previous: Option<&str>, question: &str) -> Option<String> {
    let (prev, q) = (previous.unwrap_or("").trim(), question.trim());
    if prev.is_empty() || q.is_empty() || q.chars().count() > 200 {
        return None;
    }
    Some(format!("{}\n{q}", prev.chars().take(400).collect::<String>()))
}

/// Two searches as one: each message once, at its better score, best first.
pub(crate) fn merge_hits(a: Vec<crate::memory_index::Hit>, b: Vec<crate::memory_index::Hit>, at_most: usize) -> Vec<crate::memory_index::Hit> {
    let mut all: Vec<crate::memory_index::Hit> = Vec::new();
    for h in a.into_iter().chain(b) {
        match all.iter_mut().find(|x| x.k == h.k && x.seq == h.seq) {
            Some(x) if h.score > x.score => *x = h,
            Some(_) => {}
            None => all.push(h),
        }
    }
    all.sort_by(|x, y| y.score.partial_cmp(&x.score).unwrap_or(std::cmp::Ordering::Equal));
    all.truncate(at_most);
    all
}

/// What memory found for the question, as the block that goes ahead of it,
/// with the record of what was given. No block at all when there is
/// nothing to search: a first message is answered as if memory were not
/// there.
async fn found_beside(
    app: &tauri::AppHandle,
    input: &TurnInput,
    stores_open: bool,
    facts: &[crate::memory::MemoryFact],
    recall: Option<&[TranscriptEmbedding]>,
    today: &str,
) -> (Option<String>, Option<crate::memory_block::GivenRecord>, Vec<String>) {
    use crate::memory_block::{self as mb, About, GivenRecord, SearchReport, WhyNot, Wording};
    let wording = Wording::shipped();
    let record = |search: SearchReport| GivenRecord { v: 1, search, mask_version: crate::secret_mask::MASK_VERSION.to_string(), cut_version: mb::CUT_VERSION.to_string(), ..Default::default() };
    let not_run = |why: WhyNot| (Some(mb::did_not_run(why, today, wording)), Some(record(SearchReport { ran: false, why_not: Some(why), today: today.to_string(), ..Default::default() })), Vec::new());
    if !stores_open {
        return not_run(WhyNot::VaultLocked);
    }
    let reading = crate::memory_reading::memory_reading_status();
    let passages = crate::memory_reading::passages(app);
    let notes: Vec<&crate::memory::MemoryFact> = facts.iter().filter(|f| f.valid_to.is_none() && f.entry_kind == "note" && f.embedding.as_ref().is_some_and(|v| !v.is_empty())).collect();
    let knowledge: Vec<&TranscriptEmbedding> = recall.unwrap_or(&[]).iter().filter(|e| e.kind == "authored" && !e.vector.is_empty()).collect();
    if passages == 0 && reading.total == 0 && notes.is_empty() && knowledge.is_empty() {
        return (None, None, Vec::new());
    }
    if crate::memory_reading::rebuilding(app) {
        return not_run(WhyNot::IndexRebuilding);
    }
    let question = match &input.question_vector {
        Some(v) => v.clone(),
        None if input.embed_here => match crate::inference_memory::embed_for_search(app, &input.question).await {
            Ok(v) => v,
            Err(why) => return not_run(why),
        },
        // The sender embeds the question itself and could not.
        None => return not_run(WhyNot::DidNotStartInTime),
    };
    // The open conversation's own messages are already being sent.
    let open_k = input.conversation.as_deref().filter(|c| !c.is_empty()).and_then(|c| crate::transcript_search::row_key(app, c).ok());
    let on_wire_from = input.on_wire_from.unwrap_or(0);
    // What the person decided about conversations and items: who a
    // conversation is for, what is kept out, what was forgotten.
    let memory_rows = {
        use tauri::Manager;
        app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get().and_then(|m| m.data_key()).map(|k| crate::memory_rows::read_cache(app, &k, crate::memory_items::SCOPE)).unwrap_or_default()
    };
    let uses = crate::memory_use::conv_uses(&memory_rows);
    let forgotten = crate::memory_use::forgotten_sources(&crate::memory_items::items(&memory_rows));
    let who = crate::memory_use::Asking { ai: input.founding_key.as_deref().unwrap_or(""), project: input.project.as_deref().filter(|p| !p.is_empty()), online: input.online };
    let may = |c: &crate::memory_index::Candidate| {
        if forgotten.contains(&(c.meta.started_at, c.seq)) {
            return crate::memory_use::May::Never;
        }
        crate::memory_use::may_use_message(&uses, c.meta.started_at, c.seq, &who)
    };
    let searched = |c: &crate::memory_index::Candidate| crate::memory_index::searched_by_default(c) && !(open_k.as_deref() == Some(c.k) && c.seq >= on_wire_from);
    let keep = |c: &crate::memory_index::Candidate| searched(c) && may(c) == crate::memory_use::May::Use;
    let at_most = crate::memory_index::FOUND_AT_MOST;
    let mut hits = match crate::memory_reading::search(app, &question, at_most, &keep) {
        Ok(f) => f.hits,
        Err(_) => return not_run(WhyNot::IndexRebuilding),
    };
    let follow_up = follow_up_query(input.previous_question.as_deref(), &input.question);
    let mut followed = false;
    if let Some(joined) = &follow_up {
        if let Ok(v) = crate::inference_memory::embed_for_search(app, joined).await {
            if let Ok(f) = crate::memory_reading::search(app, &v, at_most, &keep) {
                hits = merge_hits(hits, f.hits, at_most);
                followed = true;
            }
        }
    }
    // Conversations that would have been found and that this AI may not
    // use: counted, so the person can be told and can allow it.
    let withheld = {
        let held_back = |c: &crate::memory_index::Candidate| searched(c) && may(c) == crate::memory_use::May::Withheld;
        let mut conversations: Vec<String> = crate::memory_reading::search(app, &question, at_most, &held_back).map(|f| f.hits.into_iter().map(|h| h.k).collect()).unwrap_or_default();
        conversations.sort();
        conversations.dedup();
        if uses.is_empty() { 0 } else { conversations.len() as u32 }
    };
    let (exchanges, mut hidden) = crate::memory_reading::exchanges(app, &hits).unwrap_or_default();
    // Saved notes and knowledge given to this AI, found by the rule they
    // were always found by, as dated lines.
    let mut dated: Vec<String> = Vec::new();
    let mut scored: Vec<(&crate::memory::MemoryFact, f32)> = notes.iter().map(|n| (*n, cosine(&question, n.embedding.as_deref().unwrap_or(&[])))).filter(|(_, s)| *s >= NOTE_FLOOR).collect();
    by_score(&mut scored);
    for (n, _) in scored.into_iter().take(NOTES_AT_MOST) {
        let m = crate::secret_mask::hand_on(&n.value);
        hidden += m.hidden;
        dated.push(mb::dated_line(n.updated_at, "a note they saved", &m.text));
    }
    for text in closest(knowledge.iter().copied(), &question) {
        let at = knowledge.iter().find(|e| e.text == text).map(|e| e.created_at).unwrap_or(0);
        let m = crate::secret_mask::hand_on(text);
        hidden += m.hidden;
        dated.push(mb::dated_line(at, "knowledge they gave this AI", &m.text));
    }
    // Names in the question that no conversation mentions. When the
    // keyword index cannot say, nothing is claimed.
    let absent = mb::absent_names(
        &input.question,
        &|name| crate::transcript_search::mentions(app, name).unwrap_or(true),
        &|name| crate::transcript_search::has_close_spelling(app, name).unwrap_or(true),
    );
    let partial = (reading.total > 0 && reading.read < reading.total).then(|| ((reading.read.max(0) * 100) / reading.total) as u8);
    let about = About { today: today.to_string(), absent_names: absent.clone(), partial_percent: partial, withheld, replaced_left_out: false };
    let (count, cap) = mb::budget(input.window_tokens.unwrap_or(4096), input.online);
    // About three characters to a token, to stay inside the room.
    let room = input.room_tokens.map(|t| t as usize * 3);
    let b = mb::block_with(&exchanges, &dated, &about, count, cap, room, wording);
    let read_share = if reading.total > 0 { reading.read as f32 / reading.total as f32 } else { 1.0 };
    let mut rec = record(SearchReport { ran: true, why_not: None, found: b.found, given: b.shown, read_share, absent_names: absent, withheld, follow_up: followed, today: today.to_string(), partial_percent: partial });
    rec.exchanges = b.given;
    rec.lines = b.lines;
    rec.block = mb::fingerprint(&b.text);
    rec.masked = hidden;
    (Some(b.text), Some(rec), b.texts)
}

/// The chat page's door to the builder. While the stores are still opening
/// it says so, and the page asks again; a turn sent with a project folder
/// open is in that project.
#[tauri::command]
pub async fn memory_for_turn(app: tauri::AppHandle, mut input: TurnInput) -> Result<TurnMemory, String> {
    use tauri::Manager;
    app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get()?.data_key()?;
    if input.project.is_none() {
        let bridge = app.state::<crate::agent_bridge::AgentBridgeState>();
        if !bridge.is_tools_session().await {
            input.project = bridge.open_folder().await.map(crate::project_identity::project_identity_for).filter(|p| !p.is_empty());
        }
    }
    Ok(for_turn(&app, input).await)
}

/// The memory part of an agent's system message, kept briefly: an agent
/// calls once per step, and the part changes little within a session. A
/// kept part is given again only to the same AI in the same project under
/// the same rules, so a rule saved mid-session is in the next step.
pub(crate) struct KeptPart {
    kept: std::sync::Mutex<Option<(String, String, std::time::Instant)>>,
    for_secs: u64,
}

impl KeptPart {
    pub(crate) const fn new(for_secs: u64) -> Self {
        Self { kept: std::sync::Mutex::new(None), for_secs }
    }
    pub(crate) fn key(ai_id: &str, project: Option<&str>, rules_revision: &str) -> String {
        format!("{ai_id}|{}|{rules_revision}", project.unwrap_or(""))
    }
    pub(crate) fn get(&self, key: &str) -> Option<String> {
        let kept = self.kept.lock().unwrap_or_else(|e| e.into_inner());
        kept.as_ref().filter(|(k, _, at)| k == key && at.elapsed() < std::time::Duration::from_secs(self.for_secs)).map(|(_, part, _)| part.clone())
    }
    pub(crate) fn put(&self, key: &str, part: &str) {
        *self.kept.lock().unwrap_or_else(|e| e.into_inner()) = Some((key.to_string(), part.to_string(), std::time::Instant::now()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Made-up profiles, each with the block the chat page's own builder
    /// gave for it before this module replaced that builder.
    #[derive(serde::Deserialize)]
    struct Profile {
        name: String,
        facts: Vec<MemoryFact>,
        rows: std::collections::HashMap<String, Vec<MemoryRow>>,
        recall: Vec<TranscriptEmbedding>,
        question: Option<String>,
        ai_id: Option<String>,
        founding_key: Option<String>,
        conversation: Option<String>,
        question_vector: Option<Vec<f32>>,
        block: String,
    }

    fn profiles() -> Vec<Profile> {
        serde_json::from_str(include_str!("memory_prompt_profiles.json")).expect("the profiles parse")
    }

    fn block_of(p: &Profile) -> (String, Counts) {
        let mut rows = p.rows.get("person").cloned().unwrap_or_default();
        if let Some(k) = &p.founding_key {
            rows.extend(p.rows.get(&format!("ai:{k}")).cloned().unwrap_or_default());
        }
        let asked = p.question.as_deref().is_some_and(|q| !q.trim().is_empty());
        block(&Held {
            facts: &p.facts,
            rows: &rows,
            project_rows: &[],
            recall: p.ai_id.as_ref().map(|_| p.recall.as_slice()),
            question_vector: if asked { p.question_vector.as_deref() } else { None },
            conversation: p.conversation.as_deref(),
        })
    }

    #[test]
    fn the_block_equals_the_chat_pages_for_every_made_up_profile() {
        let all = profiles();
        assert!(all.len() >= 10);
        for p in &all {
            assert_eq!(block_of(p).0, p.block, "profile: {}", p.name);
        }
    }

    #[test]
    fn the_counts_are_what_the_block_carried() {
        let all = profiles();
        let p = all.iter().find(|p| p.name.starts_with("rules, commands and locations")).unwrap();
        assert_eq!(block_of(p).1, Counts { rules: 3, commands: 1, locations: 1, facts: 4 });
        let p = all.iter().find(|p| p.name.starts_with("nothing known")).unwrap();
        assert_eq!(block_of(p).1, Counts::default());
    }

    fn row(scope: &str, kind: &str, text: &str, updated_at: i64, pinned: bool) -> MemoryRow {
        serde_json::from_value(serde_json::json!({
            "id": format!("{scope}-{updated_at}"), "scope": scope, "kind": kind, "text": text, "as_of": updated_at,
            "saved_by": "person", "provenance": "authored", "pinned": pinned, "deleted": false, "updated_at": updated_at,
        }))
        .expect("a row")
    }

    fn with_rows(rows: &[MemoryRow], project_rows: &[MemoryRow]) -> (String, Counts) {
        block(&Held { facts: &[], rows, project_rows, recall: None, question_vector: None, conversation: None })
    }

    #[test]
    fn a_projects_rules_follow_the_persons_under_their_own_heading() {
        let own = [row("person", "rule", "Prices are in dollars.", 1, false)];
        let project = [
            row("project:p1", "location", "The recipes are in recipes/.", 2, false),
            row("project:p1", "rule", "Bread only: no pastry in this project.", 3, false),
            row("project:p1", "command", "Print labels: labels --all", 4, false),
            row("project:p1", "fact", "Started in March.", 5, false),
        ];
        let (text, counts) = with_rows(&own, &project);
        assert_eq!(
            text,
            "Rules from them, in their words - follow them in every answer:\n- Prices are in dollars.\n\
             Rules for this project:\n\
             Rules from them, in their words - follow them in every answer:\n- Bread only: no pastry in this project.\n\
             Commands they use here:\n- Print labels: labels --all\n\
             Where things are:\n- The recipes are in recipes/.\n\
             Their name is unknown. Do not assume or allude to it."
        );
        assert_eq!(counts, Counts { rules: 2, commands: 1, locations: 1, facts: 0 });
        // A project with rules of its own and a person with none.
        let (text, _) = with_rows(&[], &project[1..2]);
        assert!(text.starts_with("Rules for this project:\nRules from them, in their words"), "{text}");
        // No project, or a project with no rules: the block is as it was.
        assert_eq!(with_rows(&own, &[]).0, with_rows(&own, &project[3..]).0);
        assert!(!with_rows(&own, &[]).0.contains("Rules for this project"));
    }

    #[test]
    fn rules_over_the_budget_keep_the_pinned_then_the_newest_and_the_persons_before_a_projects() {
        let long = |n: i64| format!("Rule number {n}: {}", "always weigh the flour before the water goes in. ".repeat(6));
        let mut own: Vec<MemoryRow> = (1..=8).map(|n| row("person", "rule", &long(n), n, false)).collect();
        own[0].pinned = true;
        let project = [row("project:p1", "rule", "Bread only.", 99, false)];
        let (text, counts) = with_rows(&own, &project);
        assert!(text.len() <= RULES_BUDGET + "\nTheir name is unknown. Do not assume or allude to it.".len(), "{} bytes", text.len());
        assert!(text.contains("Rule number 1:"), "the pinned rule stays");
        assert!(text.contains("Rule number 8:"), "the newest stays");
        assert!(!text.contains("Rule number 2:"), "the oldest unpinned goes first");
        assert!(!text.contains("Bread only."), "the person's own come before a project's");
        assert!(counts.rules < 9 && counts.rules >= 2);
        // Kept rules are still given in their own order.
        assert!(text.find("Rule number 1:").unwrap() < text.find("Rule number 8:").unwrap());
        // Under the budget nothing is left out.
        let few = &own[..2];
        assert_eq!(with_rows(few, &project).1.rules, 3);
    }

    #[test]
    fn a_password_in_an_earlier_turn_or_a_note_is_hidden_in_the_block() {
        let all = profiles();
        let mut p = all.into_iter().find(|p| p.name.starts_with("earlier turns")).unwrap();
        p.recall[1].text = "The supplier login: password is Fl0ur&Water99, keep it.".into();
        let (text, _) = block_of(&p);
        assert!(text.contains("- The supplier login: password is [hidden: looks like a password or key], keep it."), "{text}");
        assert!(!text.contains("Fl0ur&Water99"));
    }

    #[test]
    fn follow_up_searches_with_the_previous_question() {
        assert_eq!(follow_up_query(Some("What is the free delivery threshold?"), "and when was that?").as_deref(), Some("What is the free delivery threshold?\nand when was that?"));
        assert_eq!(follow_up_query(None, "and when was that?"), None);
        assert_eq!(follow_up_query(Some("  "), "and when was that?"), None);
        assert_eq!(follow_up_query(Some("a question"), &"long ".repeat(50)), None, "a long question stands alone");
        assert_eq!(follow_up_query(Some(&"p".repeat(500)), "why?").unwrap().chars().count(), 400 + 1 + 4);
        let hit = |k: &str, seq: u32, score: f32| crate::memory_index::Hit { k: k.into(), seq, role: 0, score, flags: 0 };
        let merged = merge_hits(vec![hit("a", 1, 0.5), hit("b", 2, 0.4)], vec![hit("b", 2, 0.7), hit("c", 3, 0.45)], 12);
        assert_eq!(merged.iter().map(|h| (h.k.as_str(), h.score)).collect::<Vec<_>>(), vec![("b", 0.7), ("a", 0.5), ("c", 0.45)]);
        assert_eq!(merge_hits(vec![hit("a", 1, 0.5), hit("b", 2, 0.4)], vec![hit("c", 3, 0.45)], 2).len(), 2);
    }

    #[test]
    fn the_date_line_is_the_local_date() {
        assert_eq!(date_line(2026, 3, 9), "Today is 2026-03-09.");
        let (y, m, d) = local_date();
        assert!(y >= 2026 && (1..=12).contains(&m) && (1..=31).contains(&d));
    }

    #[test]
    fn rule_scopes_are_the_persons_then_the_ais() {
        assert_eq!(rule_scopes(None), vec!["person"]);
        assert_eq!(rule_scopes(Some("")), vec!["person"]);
        assert_eq!(rule_scopes(Some("uhCAkF")), vec!["person", "ai:uhCAkF"]);
        assert_eq!(project_scope(Some("p1")).as_deref(), Some("project:p1"));
        assert_eq!(project_scope(Some("")), None);
    }

    #[test]
    fn a_kept_part_is_given_again_only_under_the_same_ai_project_and_rules() {
        let kept = KeptPart::new(120);
        let key = KeptPart::key("ai-1", Some("p1"), "rev-a");
        assert_eq!(kept.get(&key), None);
        kept.put(&key, "the part");
        assert_eq!(kept.get(&key).as_deref(), Some("the part"));
        assert_eq!(kept.get(&KeptPart::key("ai-1", Some("p1"), "rev-b")), None, "a rule edit");
        assert_eq!(kept.get(&KeptPart::key("ai-1", Some("p2"), "rev-a")), None, "another project");
        assert_eq!(kept.get(&KeptPart::key("ai-1", None, "rev-a")), None, "no project");
        assert_eq!(kept.get(&KeptPart::key("ai-2", Some("p1"), "rev-a")), None, "another AI");
        assert_eq!(KeptPart::new(0).get(&key), None);
        let gone = KeptPart::new(0);
        gone.put(&key, "the part");
        assert_eq!(gone.get(&key), None, "kept for no time at all");
    }

    #[test]
    fn a_turn_names_its_path_the_way_the_page_sends_it() {
        let t: TurnInput = serde_json::from_value(serde_json::json!({ "aiId": "ai-1", "foundingKey": "f", "surface": "chat", "question": "hi", "questionVector": [1.0, 0.0] })).unwrap();
        assert_eq!((t.surface, t.question_vector.as_deref(), t.embed_here), (Surface::Chat, Some(&[1.0f32, 0.0][..]), false));
        let t: TurnInput = serde_json::from_value(serde_json::json!({ "surface": "tools-session" })).unwrap();
        assert_eq!((t.surface, t.ai_id, t.question.as_str()), (Surface::ToolsSession, None, ""));
    }
}
