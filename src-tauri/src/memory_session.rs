//! Memory in a project or tools session.
//!
//! A session's model calls come from the session program, which keeps its
//! own history and calls once per step. What memory found is added here,
//! at the port, to the person's prompt of the turn in hand: on every step
//! of that turn, and on no later turn. It never enters the program's
//! history, so nothing has to be taken out again.
//!
//! The program marks the person's prompt: its text begins `<user_query>`.
//! Its reminders, wake-ups and summaries are also sent in the person's
//! role, so "the last message of the person" is not a question.

use serde_json::Value;
use std::sync::Mutex;

const OPEN: &str = "<user_query>\n";
const CLOSE: &str = "</user_query>";

fn text_of(content: &Value) -> String {
    match content {
        Value::String(s) => s.clone(),
        Value::Array(parts) => parts.iter().filter(|p| p["type"] == "text").filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n"),
        _ => String::new(),
    }
}

/// Where the person's prompt of this turn stands: the last message in
/// their role that the program marked as their words.
pub(crate) fn prompt_at(msgs: &[Value]) -> Option<usize> {
    msgs.iter().rposition(|m| m["role"] == "user" && text_of(&m["content"]).starts_with(OPEN))
}

/// The words inside the program's marks.
pub(crate) fn inner(text: &str) -> String {
    let t = text.strip_prefix(OPEN).unwrap_or(text);
    let t = t.rfind(CLOSE).map_or(t, |at| &t[..at]);
    t.trim().to_string()
}

/// The question to search for. The page hands the program more than the
/// person typed (guidance for tools, attached text) and says which part
/// is theirs; that part is used when this prompt ends with it.
pub(crate) fn question_of(prompt: &str, typed: Option<&str>) -> String {
    let whole = inner(prompt);
    match typed.map(str::trim).filter(|t| !t.is_empty()) {
        Some(t) if whole.ends_with(t) => t.to_string(),
        _ => whole,
    }
}

/// What names a turn: the program's session and its count of prompts
/// when it sends them, else the prompt's own words.
pub(crate) fn turn_key(session: Option<&str>, index: Option<&str>, prompt: &str) -> String {
    use sha2::{Digest, Sha256};
    match (session, index) {
        (Some(s), Some(i)) if !s.is_empty() && !i.is_empty() => format!("{s}#{i}"),
        _ => format!("words#{}", hex::encode(&Sha256::digest(prompt.as_bytes())[..12])),
    }
}

/// A call for a helper of the session (a plan, a summary) is not a turn
/// of the person's.
pub(crate) fn is_helper_call(model: &str) -> bool {
    let m = model.trim();
    m.ends_with(":plan") || m.ends_with(":summary")
}

/// What memory gave one turn of a session.
#[derive(serde::Serialize, Clone, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Turn {
    #[serde(skip)]
    pub key: String,
    /// The person's question, as it was searched for.
    pub question: String,
    #[serde(skip)]
    pub before: Option<String>,
    #[serde(skip)]
    pub after: Option<String>,
    pub given: Option<crate::memory_block::GivenRecord>,
    pub given_texts: Vec<String>,
    pub remember: Option<crate::memory_items::SaidRemember>,
    /// How many steps of the turn carried it.
    pub steps: u32,
}

/// What the person typed in the prompt last handed to the session program.
static TYPED: Mutex<Option<String>> = Mutex::new(None);

pub(crate) fn note_typed(question: Option<String>) {
    *TYPED.lock().unwrap_or_else(|e| e.into_inner()) = question.filter(|q| !q.trim().is_empty());
}

pub(crate) fn typed() -> Option<String> {
    TYPED.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// The turns in hand: the one the session program named by its own
/// session and count, and the one named only by its words (a call that
/// carries no name). One session runs at a time; a new turn replaces the
/// one of its kind, so a call with no name never takes the place of the
/// turn the person's reply is waiting to be shown with.
static TURN: Mutex<(Option<Turn>, Option<Turn>)> = Mutex::new((None, None));

fn by_words(key: &str) -> bool {
    key.starts_with("words#")
}

/// Which turn the page is shown: the session's own when there is one.
pub(crate) fn shown(named: Option<&Turn>, words: Option<&Turn>) -> Option<Turn> {
    named.or(words).cloned()
}

/// The kept turn, when it is this one; its count of steps goes up.
pub(crate) fn kept(key: &str) -> Option<Turn> {
    let mut held = TURN.lock().unwrap_or_else(|e| e.into_inner());
    let slot = if by_words(key) { &mut held.1 } else { &mut held.0 };
    let turn = slot.as_mut().filter(|t| t.key == key)?;
    turn.steps += 1;
    Some(turn.clone())
}

pub(crate) fn keep(turn: Turn) {
    let mut held = TURN.lock().unwrap_or_else(|e| e.into_inner());
    if by_words(&turn.key) {
        held.1 = Some(turn);
    } else {
        // A new named turn: an older turn kept by its words is of a session before it.
        *held = (Some(turn), None);
    }
}

/// Put what memory found around the person's prompt at `at`. The caller's
/// own copy of the messages is untouched; every other message stays as
/// it was sent.
pub(crate) fn wrap(msgs: &mut [Value], at: usize, before: Option<&str>, after: Option<&str>) {
    if before.is_none() && after.is_none() {
        return;
    }
    let Some(m) = msgs.get_mut(at) else { return };
    match &mut m["content"] {
        Value::String(text) => *text = crate::memory_block::turn_text(before, text, after),
        Value::Array(parts) => {
            if let Some(part) = parts.iter_mut().find(|p| p["type"] == "text") {
                let text = part["text"].as_str().unwrap_or_default().to_string();
                part["text"] = Value::String(crate::memory_block::turn_text(before, &text, after));
            }
        }
        _ => {}
    }
}

/// What memory gave the session's last turn: the page records it with the
/// reply, shows the line under it and opens the panel from it.
#[tauri::command]
pub fn memory_session_given() -> Option<Turn> {
    let held = TURN.lock().unwrap_or_else(|e| e.into_inner());
    shown(held.0.as_ref(), held.1.as_ref())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn a_call_with_no_name_does_not_take_the_place_of_the_sessions_own_turn() {
        let turn = |key: &str, question: &str| Turn { key: key.into(), question: question.into(), before: None, after: None, given: None, given_texts: Vec::new(), remember: None, steps: 1 };
        let (named, words) = (turn("session-1#2", "What is the delivery threshold?"), turn("words#abc", "another call"));
        assert_eq!(shown(Some(&named), Some(&words)).map(|t| t.question), Some("What is the delivery threshold?".to_string()));
        assert_eq!(shown(None, Some(&words)).map(|t| t.question), Some("another call".to_string()), "a program that names no turns is still served");
        assert!(shown(None, None).is_none());
        assert!(by_words("words#abc") && !by_words("session-1#2"));
    }

    fn person(text: &str) -> Value {
        json!({ "role": "user", "content": format!("<user_query>\n{text}\n</user_query>") })
    }

    #[test]
    fn the_persons_prompt_is_the_marked_message_not_the_last_in_their_role() {
        let msgs = vec![
            json!({ "role": "system", "content": "s" }),
            json!({ "role": "user", "content": "<user_info>\nOS: linux\n</user_info>" }),
            person("What did we decide about the oven?"),
            json!({ "role": "assistant", "content": "", "tool_calls": [{ "id": "1" }] }),
            json!({ "role": "tool", "content": "listing" }),
            json!({ "role": "user", "content": "<system_reminder>\nKeep going.\n</system_reminder>" }),
        ];
        assert_eq!(prompt_at(&msgs), Some(2));
        assert_eq!(prompt_at(&msgs[..2]), None);
    }

    #[test]
    fn a_tool_result_is_not_a_question() {
        let msgs = vec![json!({ "role": "tool", "content": "<user_query>\nnot theirs\n</user_query>" })];
        assert_eq!(prompt_at(&msgs), None);
    }

    #[test]
    fn the_question_is_what_the_person_typed() {
        let prompt = "<user_query>\n[How to use the tools: ...]\n\nWhat did we decide about the oven?\n</user_query>";
        assert_eq!(question_of(prompt, Some("What did we decide about the oven?")), "What did we decide about the oven?");
        // The page's note is from another prompt: the whole marked text is used.
        assert_eq!(question_of("<user_query>\nAnd the mixer?\n</user_query>", Some("What did we decide about the oven?")), "And the mixer?");
        assert_eq!(question_of("<user_query>\nAnd the mixer?\n</user_query>", None), "And the mixer?");
    }

    #[test]
    fn a_block_is_added_to_the_turn_in_hand_and_to_no_other_message() {
        let mut msgs = vec![person("What flour did we order last month?"), json!({ "role": "assistant", "content": "Rye." }), person("And the oven?")];
        let before_wrap = msgs.clone();
        let at = prompt_at(&msgs).unwrap();
        wrap(&mut msgs, at, Some("[From memory. ...]"), Some("[From memory, the state record ...]"));
        assert_eq!(msgs[0], before_wrap[0], "an earlier prompt carries no block");
        assert_eq!(msgs[1], before_wrap[1]);
        let text = msgs[2]["content"].as_str().unwrap();
        assert!(text.starts_with("[From memory. ...]\n\n<user_query>\nAnd the oven?"));
        assert!(text.ends_with("[From memory, the state record ...]"));
    }

    #[test]
    fn a_turn_is_searched_once_and_given_on_each_of_its_steps() {
        let key = turn_key(Some("session-a"), Some("4"), "p");
        assert!(kept(&key).is_none());
        keep(Turn { key: key.clone(), question: "q".into(), before: Some("b".into()), ..Default::default() });
        assert_eq!(kept(&key).unwrap().steps, 1);
        assert_eq!(kept(&key).unwrap().steps, 2);
        // The next prompt of the session is another turn.
        assert!(kept(&turn_key(Some("session-a"), Some("5"), "p")).is_none());
    }

    #[test]
    fn a_turn_is_named_by_its_words_when_the_program_sends_no_count() {
        assert_eq!(turn_key(None, None, "a"), turn_key(Some(""), None, "a"));
        assert_ne!(turn_key(None, None, "a"), turn_key(None, None, "b"));
    }

    #[test]
    fn a_plan_or_summary_call_is_not_a_turn() {
        assert!(is_helper_call("Atlas:plan"));
        assert!(is_helper_call("Atlas:summary"));
        assert!(!is_helper_call("Atlas:agent"));
    }
}
