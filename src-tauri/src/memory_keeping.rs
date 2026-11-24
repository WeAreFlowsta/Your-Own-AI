//! Keeping without being asked. Each exchange recorded on this device
//! waits here as a pointer. When the computer is idle, a model that passed
//! the built-in check is asked whether the exchange settled something;
//! when it did, the same model writes it as one item. The item is marked
//! as kept by the AI and not confirmed by the person, it can be undone,
//! and tidying looks at it like any other.
//!
//! Nothing here runs during the person's turn, on a model that has not
//! passed, while keeping is turned off, for a private conversation, or
//! for a reply the person stopped. Nothing goes back through older
//! conversations: only exchanges recorded from now on wait here.

use crate::memory_check::{Part, Passage};
use crate::memory_items::{Audience, ItemType};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tauri::{AppHandle, Manager};

const JOB: &str = "keeping what was settled";
/// Exchanges waiting to be looked at: pointers, never text.
const QUEUE_FILE: &str = "memory-keep-queue.json";
/// Each message of an exchange is cut to this, and the whole to `SPAN_CHARS`.
const MESSAGE_CHARS: usize = 1500;
const SPAN_CHARS: usize = 6000;

static WAKE: tokio::sync::Notify = tokio::sync::Notify::const_new();
static RUNNING: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// The item form. The words are measured: items were read with exactly
/// this text.
pub(crate) const ITEM_SYSTEM: &str = "You keep a person's memory of their work with their AI. You are shown ONE dated exchange from their conversations. Decide whether it holds something worth keeping for later, and write it as one item.\nkind \"decision\": they chose something (give the choice and the reason).\nkind \"done\": something was built, fixed, shipped or finished.\nkind \"open\": something was left to do, or is waiting on someone.\nkind \"detail\": a name, number, path, command or setting they may need again.\nkind \"rule\": how they want the AI to behave from now on.\nkind \"about_person\": a lasting fact about the person themselves.\nkind \"nothing\": passing talk, a test message, pasted logs with no outcome, or work in progress with no result yet.\n\"subject\": two to four words naming what it is about. \"statement\": one sentence that stands alone, using the real names and values from the exchange. Never add anything the exchange does not say. Never copy a password, key or secret into the statement; say only that one was given.";

pub(crate) fn item_schema() -> serde_json::Value {
    serde_json::json!({
        "type": "object",
        "properties": {
            "kind": { "type": "string", "enum": ["decision", "done", "open", "detail", "rule", "about_person", "nothing"] },
            "subject": { "type": "string" },
            "statement": { "type": "string" },
        },
        "required": ["kind", "subject", "statement"],
    })
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

/// One exchange that waits: the reply's place, the AI that gave it, and
/// the project the turn was in.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct Waiting {
    /// The conversation, as the records name it.
    pub conversation: String,
    /// The reply's sequence in it.
    pub seq: u32,
    /// The founding key of the AI that replied.
    pub ai: String,
    /// Where that AI's conversations are recorded.
    #[serde(default)]
    pub agent: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project: Option<String>,
}

/// A turn as it is recorded, for the decision to queue it.
pub(crate) struct RecordedTurn<'a> {
    pub role: &'a str,
    pub stopped: bool,
    /// A write of the app's own (a project memory record), not a turn.
    pub own_write: bool,
    pub content: &'a str,
}

/// Does a recorded message put its exchange in the queue? Only a finished
/// reply of an AI does: the person's message waits for its reply, and a
/// reply the person stopped settles nothing.
pub(crate) fn queues(t: &RecordedTurn) -> bool {
    t.role == "assistant" && !t.stopped && !t.own_write && !t.content.trim().is_empty()
}

pub(crate) fn queued(mut queue: Vec<Waiting>, w: Waiting) -> Vec<Waiting> {
    if !queue.iter().any(|q| q.conversation == w.conversation && q.seq == w.seq) {
        queue.push(w);
    }
    queue
}

fn queue_path(app: &AppHandle) -> Result<PathBuf, String> {
    Ok(crate::profile::root(app)?.join(QUEUE_FILE))
}

fn read_queue(app: &AppHandle) -> Vec<Waiting> {
    queue_path(app).ok().and_then(|p| std::fs::read(p).ok()).and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or_default()
}

fn write_queue(app: &AppHandle, queue: &[Waiting]) {
    if let (Ok(p), Ok(json)) = (queue_path(app), serde_json::to_vec(queue)) {
        let _ = std::fs::write(p, json);
    }
}

/// A reply was recorded: its exchange waits for idle time.
pub(crate) fn recorded(app: &AppHandle, t: &RecordedTurn, w: Waiting) {
    if !queues(t) {
        return;
    }
    // Off means off: nothing waits while keeping is turned off, so nothing
    // from that time is looked at when it is turned on again.
    let off = app
        .try_state::<std::sync::Arc<crate::commands_holochain::HolochainState>>()
        .and_then(|hc| hc.get().ok().and_then(|m| m.data_key().ok()))
        .is_some_and(|key| crate::memory_tidy::tidy_is_off(&crate::memory_rows::read_cache(app, &key, crate::memory_items::SCOPE)));
    if off {
        return;
    }
    write_queue(app, &queued(read_queue(app), w));
    WAKE.notify_one();
}

/// One message of the conversation, as the exchange is cut from it.
pub(crate) struct Line {
    pub seq: u32,
    pub by_person: bool,
    pub at: i64,
    pub text: String,
}

fn cut(text: &str, n: usize) -> String {
    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    collapsed.chars().take(n).collect()
}

/// The exchange a reply closes, as the measured form shows it: the
/// message before, the person's message and the reply, each cut short.
/// Answers with the text and the person's message (the item points at it).
/// None when the reply does not answer a message of the person.
pub(crate) fn span<'a>(lines: &'a [Line], reply_seq: u32) -> Option<(String, &'a Line)> {
    let at = lines.iter().position(|l| l.seq == reply_seq && !l.by_person)?;
    let person_at = at.checked_sub(1).filter(|i| lines[*i].by_person)?;
    let mut parts: Vec<String> = Vec::new();
    for l in &lines[person_at.saturating_sub(1)..=at] {
        parts.push(format!("{}{}", if l.by_person { "They said: " } else { "AI: " }, cut(&l.text, MESSAGE_CHARS)));
    }
    let text: String = parts.join("\n").chars().take(SPAN_CHARS).collect();
    Some((text, &lines[person_at]))
}

/// The gate's answer: the quote that settles it, or None for "in
/// progress" and for a reply that cannot be read.
pub(crate) fn read_gate(reply: &str) -> Option<String> {
    let (a, b) = (reply.find('{')?, reply.rfind('}')?);
    let v: serde_json::Value = serde_json::from_str(reply.get(a..=b)?).ok()?;
    (v["verdict"].as_str()? == "settled").then(|| v["quote"].as_str().unwrap_or("").trim().to_string())
}

/// What the item form wrote.
#[derive(Debug, PartialEq)]
pub(crate) struct Written {
    pub item_type: ItemType,
    pub subject: String,
    pub statement: String,
}

/// The item the model wrote, or None: kind "nothing", kind "rule" (a rule
/// is only ever offered to the person, never saved by an AI), an empty
/// statement, or a reply that cannot be read.
pub(crate) fn read_item(reply: &str) -> Option<Written> {
    let (a, b) = (reply.find('{')?, reply.rfind('}')?);
    let v: serde_json::Value = serde_json::from_str(reply.get(a..=b)?).ok()?;
    let item_type = match v["kind"].as_str()? {
        "decision" => ItemType::Decision,
        "done" => ItemType::Done,
        "open" => ItemType::Open,
        "detail" => ItemType::Detail,
        "about_person" => ItemType::AboutPerson,
        _ => return None,
    };
    let statement = v["statement"].as_str()?.trim().to_string();
    if statement.is_empty() {
        return None;
    }
    Some(Written { item_type, subject: v["subject"].as_str().unwrap_or("").trim().to_string(), statement })
}

/// What a pass did, or why it did nothing.
#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    pub ran: bool,
    pub why_not: Option<String>,
    pub model: Option<String>,
    /// Exchanges put to the gate.
    pub looked_at: u32,
    pub settled: u32,
    pub kept: u32,
    /// Settled, but the item form wrote nothing to keep.
    pub nothing: u32,
    pub left_in_queue: usize,
    /// Each exchange looked at, for a test to read.
    pub exchanges: Vec<serde_json::Value>,
}

fn refuse(mut report: Report, why: &str) -> Report {
    report.why_not = Some(why.to_string());
    report
}

/// One pass over the queue. `force` (dev builds' tests only) runs it now,
/// on whatever model is loaded.
pub(crate) async fn pass(app: &AppHandle, force: bool) -> Report {
    let _one = RUNNING.lock().await;
    let queue = read_queue(app);
    let report = Report { left_in_queue: queue.len(), ..Default::default() };
    if queue.is_empty() {
        return refuse(report, "no new exchange waits");
    }
    match run(app, force, queue, report.clone()).await {
        Ok(r) => r,
        Err(e) => refuse(report, &e),
    }
}

async fn run(app: &AppHandle, force: bool, queue: Vec<Waiting>, mut report: Report) -> Result<Report, String> {
    let key = app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get()?.data_key()?;
    let scope = crate::memory_items::SCOPE;
    let rows = crate::memory_rows::read_cache(app, &key, scope);
    if crate::memory_tidy::tidy_is_off(&rows) {
        // Off means off: what waited is not looked at later either.
        write_queue(app, &[]);
        report.left_in_queue = 0;
        return Ok(refuse(report, "keeping is turned off"));
    }
    let llm = app.state::<crate::llm::LLMState>();
    let Some(model) = llm.current_model.lock().await.clone() else { return Ok(refuse(report, "no model is loaded")) };
    report.model = Some(model.clone());
    if !force && !crate::memory_check::stored(app, &model).is_some_and(|c| c.may_keep) {
        return Ok(refuse(report, "the loaded model has not passed the check"));
    }
    if !force && !crate::machine::may_run(crate::machine::Work::Heavy, JOB) {
        return Ok(refuse(report, crate::machine::held_reason(JOB).unwrap_or("the computer is busy")));
    }
    let done = look(app, force, &key, &llm, queue, report).await;
    crate::machine::done(JOB);
    done
}

async fn look(app: &AppHandle, force: bool, key: &[u8; 32], llm: &crate::llm::LLMState, queue: Vec<Waiting>, mut report: Report) -> Result<Report, String> {
    report.ran = true;
    let scope = crate::memory_items::SCOPE;
    let private = crate::memory_use::conv_uses(&crate::memory_rows::read_cache(app, key, scope));
    let mut cached = crate::transcript_search::cached_conversations(app)?;
    // A conversation begun since this device's text was last brought level
    // is read in first (one read per AI).
    let mut read: Vec<&str> = Vec::new();
    for w in &queue {
        let k = crate::transcript_search::row_key(app, &w.conversation)?;
        if !w.agent.is_empty() && !read.contains(&w.agent.as_str()) && !cached.iter().any(|c| c.k == k) {
            read.push(&w.agent);
            let _ = crate::transcript_search::read_agent(app, &w.agent).await;
        }
    }
    if !read.is_empty() {
        cached = crate::transcript_search::cached_conversations(app)?;
    }
    let mut waiting: Vec<Waiting> = Vec::new();
    for (at, w) in queue.iter().enumerate() {
        // The person is back: the rest waits.
        if !force && !crate::machine::would_run(crate::machine::Work::Heavy) {
            waiting.extend(queue[at..].iter().cloned());
            break;
        }
        let k = crate::transcript_search::row_key(app, &w.conversation)?;
        // Not in this device's text yet: next time.
        let Some(conv) = cached.iter().find(|c| c.k == k) else {
            waiting.push(w.clone());
            continue;
        };
        if crate::memory_use::is_private(&private, conv.started_at) {
            continue;
        }
        let lines: Vec<Line> = crate::transcript_search::cached_entries(app, &k, 0)?
            .into_iter()
            .filter(|e| e.role == "user" || e.role == "assistant")
            .map(|e| Line { seq: e.seq, by_person: e.role == "user", at: e.at, text: crate::secret_mask::hand_on(&e.content).text })
            .collect();
        let Some((text, person)) = span(&lines, w.seq) else { continue };
        let passage = Passage { date: crate::memory_block::day(person.at), text };
        let user = crate::memory_check::gate_user(&passage);
        let ask = |system: &'static str, schema: serde_json::Value, temperature: f64, max_tokens: u32| crate::llm::fixed_form_once(llm, system, &user, schema, temperature, max_tokens, None);
        let gate = match ask(crate::memory_check::GATE_SYSTEM, crate::memory_check::schema(Part::Gate), 0.2, 600).await {
            Ok(r) => r,
            Err(e) => {
                // The engine is gone or busy: this one and the rest wait.
                log::info!("[memory keeping] an exchange was not looked at: {e}");
                waiting.extend(queue[at..].iter().cloned());
                break;
            }
        };
        report.looked_at += 1;
        let Some(quote) = read_gate(&gate) else {
            report.exchanges.push(serde_json::json!({ "conversation": conv.started_at, "seq": person.seq, "verdict": "in_progress" }));
            continue;
        };
        report.settled += 1;
        let written = match ask(ITEM_SYSTEM, item_schema(), 0.7, 400).await {
            Ok(r) => read_item(&r),
            Err(e) => {
                log::info!("[memory keeping] an item was not written: {e}");
                waiting.extend(queue[at..].iter().cloned());
                break;
            }
        };
        let Some(written) = written else {
            report.nothing += 1;
            report.exchanges.push(serde_json::json!({ "conversation": conv.started_at, "seq": person.seq, "verdict": "settled", "kept": false }));
            continue;
        };
        let by = format!("ai:{}", w.ai);
        let audience = w.project.clone().map(|project| Audience::Project { project });
        let ptr = crate::memory_block::Ptr { c: conv.started_at, s: person.seq, t: person.at, a: 0, b: 0 };
        let kept = keep_as_ai(app, key, &written.statement, written.item_type, Some(ptr), person.at / 1000, &by, audience, "exchange", "settled", Some(quote).filter(|q| !q.is_empty()), Some(&written.subject)).await?;
        if kept.is_some() {
            report.kept += 1;
        }
        report.exchanges.push(serde_json::json!({ "conversation": conv.started_at, "seq": person.seq, "verdict": "settled", "kept": kept.is_some(), "statement": written.statement }));
    }
    // What was queued while this pass ran stays.
    let now_queue = read_queue(app);
    let arrived: Vec<Waiting> = now_queue.into_iter().filter(|q| !queue.iter().any(|w| w == q)).collect();
    waiting.extend(arrived);
    report.left_in_queue = waiting.len();
    write_queue(app, &waiting);
    Ok(report)
}

/// Keep something as an AI: not confirmed by the person, in the change
/// log with its undo, and looked at by tidying. Answers with the item's
/// id, or None when the same thing from the same place was kept before
/// (and perhaps undone): it is not kept twice.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn keep_as_ai(
    app: &AppHandle,
    key: &[u8; 32],
    text: &str,
    item_type: ItemType,
    ptr: Option<crate::memory_block::Ptr>,
    as_of_ms: i64,
    by: &str,
    audience: Option<Audience>,
    kind: &str,
    why: &str,
    quote: Option<String>,
    subject_words: Option<&str>,
) -> Result<Option<String>, String> {
    let scope = crate::memory_items::SCOPE;
    let have = crate::memory_items::items(&crate::memory_rows::read_cache(app, key, scope));
    // What is kept is kept without its secrets.
    let text = crate::secret_mask::hand_on(text).text;
    let now = now_ms();
    let id = {
        use rand::Rng as _;
        format!("it-{:016x}{:016x}", rand::thread_rng().gen::<u64>(), rand::thread_rng().gen::<u64>())
    };
    // The same words kept from the same place before, or by a tool with
    // no place: once is enough, and an undone keep is not made again.
    let again = have.iter().any(|i| i.row.text.trim() == text.trim() && i.meta.source.ptr.map(|p| (p.c, p.s)) == ptr.map(|p| (p.c, p.s)));
    if again {
        return Ok(None);
    }
    let place = ptr.unwrap_or(crate::memory_block::Ptr { c: 0, s: 0, t: as_of_ms * 1000, a: 0, b: 0 });
    let k = crate::memory_items::Keep { id: id.clone(), text: &text, item_type, ptr: place, as_of: as_of_ms, by, audience, kind, why, quote, subject: None, now };
    let Ok((mut row, change)) = crate::memory_items::keep(&have, k) else { return Ok(None) };
    // The words the writer named the matter with: subjects are made from
    // where an item came from, and may take a name from these.
    if let Some(mut meta) = crate::memory_items::meta_of(&row) {
        if ptr.is_none() {
            meta.source.ptr = None;
        }
        if let Some(words) = subject_words.map(str::trim).filter(|w| !w.is_empty()) {
            meta.more.insert("subject_words".into(), serde_json::Value::String(words.chars().take(60).collect()));
        }
        if let Some(serde_json::Value::Object(o)) = row.structured.as_mut() {
            o.insert("item".into(), serde_json::to_value(&meta).map_err(|e| e.to_string())?);
        }
    }
    crate::memory_rows::save_and_cache(app, row).await?;
    crate::memory_items::log_change(app, key, change)?;
    crate::memory_tidying::kept(app, &id);
    Ok(Some(id))
}

/// The AI's own tools in a session (`remember`, `remember_for_project`):
/// what it asks to keep becomes an item, marked as kept by that AI and
/// not confirmed, with an undo in the list of changes. With a project, the
/// item is for turns in that project.
pub(crate) async fn keep_by_tool(app: &AppHandle, agent_key: &str, text: &str, project: Option<&str>) -> Result<String, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("nothing to remember - the text was empty".into());
    }
    let key = app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get()?.data_key()?;
    if crate::memory_tidy::tidy_is_off(&crate::memory_rows::read_cache(app, &key, crate::memory_items::SCOPE)) {
        return Ok("Not kept: the person has turned off keeping by AIs. They can keep it themselves with Remember.".into());
    }
    let by = format!("ai:{}", crate::commands_holochain::founding_key_for(app, agent_key));
    let audience = project.filter(|p| !p.is_empty()).map(|p| Audience::Project { project: p.to_string() });
    let for_project = audience.is_some();
    let kept = keep_as_ai(app, &key, text, ItemType::Detail, None, now_ms(), &by, audience, "tool", "tool", None, None).await?;
    Ok(match (kept, for_project) {
        (None, _) => "That is already kept.".to_string(),
        (Some(_), true) => "Kept for this project. The person sees it in Memory and can undo it.".to_string(),
        (Some(_), false) => "Kept. The person sees it in Memory and can undo it.".to_string(),
    })
}

/// The job: after each recorded reply and every ten minutes, look at what
/// waits, when the computer is idle.
pub(crate) fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(120)).await;
        loop {
            let report = pass(&app, false).await;
            if report.ran {
                log::info!("[memory keeping] {} exchange(s) looked at, {} settled, {} kept, {} left", report.looked_at, report.settled, report.kept, report.left_in_queue);
            }
            tokio::select! {
                _ = WAKE.notified() => tokio::time::sleep(std::time::Duration::from_secs(60)).await,
                _ = tokio::time::sleep(std::time::Duration::from_secs(600)) => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(seq: u32, by_person: bool, text: &str) -> Line {
        Line { seq, by_person, at: 1_768_896_000_000_000 + i64::from(seq), text: text.to_string() }
    }

    fn turn<'a>(role: &'a str, stopped: bool, content: &'a str) -> RecordedTurn<'a> {
        RecordedTurn { role, stopped, own_write: false, content }
    }

    #[test]
    fn only_a_finished_reply_puts_its_exchange_in_the_queue() {
        assert!(queues(&turn("assistant", false, "Set: free delivery from $60.")));
        assert!(!queues(&turn("user", false, "Make delivery free over $60.")), "the person's message waits for its reply");
        assert!(!queues(&turn("assistant", true, "Set: free deli")), "a stopped reply makes no item");
        assert!(!queues(&turn("assistant", false, "  ")));
        assert!(!queues(&RecordedTurn { role: "assistant", stopped: false, own_write: true, content: "notes" }));
    }

    #[test]
    fn a_session_exchange_is_queued_with_its_project_and_once() {
        let w = Waiting { conversation: "c1".into(), seq: 5, ai: "founding-a".into(), agent: "agent-a".into(), project: Some("proj-1".into()) };
        let q = queued(queued(Vec::new(), w.clone()), w.clone());
        assert_eq!(q, vec![w]);
        let json = serde_json::to_string(&q).unwrap();
        assert!(json.contains("proj-1") && !json.contains("text"), "pointers only");
    }

    #[test]
    fn the_exchange_is_the_message_before_the_persons_message_and_the_reply() {
        let lines = vec![line(0, true, "Hello"), line(1, false, "Hi. What do you need?"), line(2, true, "Make   delivery free\nover $60."), line(3, false, "Set: free delivery from $60.")];
        let (text, person) = span(&lines, 3).unwrap();
        assert_eq!(text, "AI: Hi. What do you need?\nThey said: Make delivery free over $60.\nAI: Set: free delivery from $60.");
        assert_eq!(person.seq, 2);
        // The first exchange of a conversation has no message before it.
        assert_eq!(span(&lines, 1).unwrap().0, "They said: Hello\nAI: Hi. What do you need?");
        // A reply that follows a reply answers no message of the person.
        let two = vec![line(0, false, "a"), line(1, false, "b")];
        assert!(span(&two, 1).is_none());
        assert!(span(&lines, 2).is_none(), "the person's own message is not a reply");
        let long = vec![line(0, true, &"x".repeat(4000)), line(1, false, "ok")];
        assert_eq!(span(&long, 1).unwrap().0.chars().count(), "They said: ".len() + MESSAGE_CHARS + "\nAI: ok".len());
    }

    #[test]
    fn the_gate_passes_only_what_was_settled() {
        assert_eq!(read_gate(" {\"verdict\":\"settled\",\"quote\":\"free delivery from $60\"}"), Some("free delivery from $60".to_string()));
        assert_eq!(read_gate("{\"verdict\":\"in_progress\",\"quote\":\"\"}"), None);
        assert_eq!(read_gate("settled"), None);
    }

    #[test]
    fn kind_nothing_is_dropped_and_a_rule_is_never_saved_as_an_item() {
        let item = |kind: &str| read_item(&format!("{{\"kind\":\"{kind}\",\"subject\":\"Delivery\",\"statement\":\"Delivery is free over $60.\"}}"));
        assert_eq!(item("decision"), Some(Written { item_type: ItemType::Decision, subject: "Delivery".into(), statement: "Delivery is free over $60.".into() }));
        assert_eq!(item("about_person").map(|w| w.item_type), Some(ItemType::AboutPerson));
        assert_eq!(item("nothing"), None);
        assert_eq!(item("rule"), None);
        assert_eq!(read_item("{\"kind\":\"detail\",\"subject\":\"x\",\"statement\":\"  \"}"), None);
    }
}
