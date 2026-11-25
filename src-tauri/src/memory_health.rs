//! Health turns stay on this computer.
//!
//! Every turn is given a verdict when it is recorded: is the person's
//! message about their health? The verdict is a field of the recorded
//! message (inside the sealed text), the search index carries it as a
//! flag, and a marked message that is found for an AI answering off this
//! computer is held back and counted, never sent.
//!
//! A reply carries the verdict of the message it answers. Messages
//! recorded before the verdict existed are marked when the reason their
//! turn was routed by says the health gate kept it on the device.

use std::collections::HashMap;
use std::sync::Mutex;

/// The verdict of the person's last message, by conversation, for the
/// reply that follows it.
static LAST: Mutex<Option<HashMap<String, Option<bool>>>> = Mutex::new(None);

/// Is a recorded message marked as health? Its own field when it has one;
/// else, for an older message, the routing reason's own words.
pub(crate) fn marked(health: Option<bool>, routing_reason: Option<&str>) -> bool {
    match health {
        Some(v) => v,
        None => routing_reason.is_some_and(|r| {
            let r = r.trim_start().to_lowercase();
            r.starts_with("a health question") || r.starts_with("a health image")
        }),
    }
}

/// The verdict a message is recorded with. `checked`: the person's
/// message as the health gate judged it (`None`: the gate could not run).
/// A reply takes the verdict of the person's message before it in the
/// same conversation; any other role has none.
pub(crate) fn carried(last: &mut HashMap<String, Option<bool>>, conversation: &str, role: &str, checked: Option<bool>) -> Option<bool> {
    match role {
        "user" => {
            last.insert(conversation.to_string(), checked);
            checked
        }
        "assistant" => last.get(conversation).copied().flatten(),
        _ => None,
    }
}

/// The verdict for a message being recorded now.
pub(crate) async fn at_record(app: &tauri::AppHandle, conversation: &str, role: &str, content: &str) -> Option<bool> {
    let checked = if role == "user" { crate::router::health_verdict(app, content).await } else { None };
    let mut guard = LAST.lock().unwrap_or_else(|e| e.into_inner());
    let last = guard.get_or_insert_with(HashMap::new);
    // The map holds one small entry per conversation touched since the app started.
    if last.len() > 2000 {
        last.clear();
    }
    carried(last, conversation, role, checked)
}

/// What was found for a turn, with the health messages taken out when the
/// reply is made off this computer: what is left, and how many were held.
pub(crate) fn hold_back(hits: Vec<crate::memory_index::Hit>, online: bool) -> (Vec<crate::memory_index::Hit>, usize) {
    if !online {
        return (hits, 0);
    }
    let before = hits.len();
    let kept: Vec<_> = hits.into_iter().filter(|h| h.flags & crate::memory_index::FLAG_HEALTH == 0).collect();
    let held = before - kept.len();
    (kept, held)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_message_is_marked_by_its_own_field_or_an_older_ones_reason() {
        assert!(marked(Some(true), None));
        assert!(!marked(Some(false), Some("a health question: kept on your device")), "a recorded verdict is the answer");
        assert!(marked(None, Some("a health question: kept on your device")));
        assert!(marked(None, Some("A health image: kept on your device")));
        assert!(!marked(None, Some("a hard question: sent to a stronger model")));
        assert!(!marked(None, None), "an older message with no reason is not held back");
    }

    #[test]
    fn a_reply_carries_the_verdict_of_the_message_it_answers() {
        let mut last = HashMap::new();
        assert_eq!(carried(&mut last, "c1", "assistant", None), None, "no message of the person's before it");
        assert_eq!(carried(&mut last, "c1", "user", Some(true)), Some(true));
        assert_eq!(carried(&mut last, "c1", "assistant", None), Some(true));
        assert_eq!(carried(&mut last, "c2", "assistant", None), None, "another conversation");
        assert_eq!(carried(&mut last, "c1", "user", Some(false)), Some(false));
        assert_eq!(carried(&mut last, "c1", "assistant", None), Some(false), "the next exchange is judged by itself");
        assert_eq!(carried(&mut last, "c1", "user", None), None, "the gate could not run: unknown, not a no");
        assert_eq!(carried(&mut last, "c1", "assistant", None), None);
        assert_eq!(carried(&mut last, "c1", "tool", Some(true)), None);
    }

    #[test]
    fn health_messages_found_for_an_online_ai_are_held_back_and_counted() {
        use crate::memory_index::{Hit, FLAG_HEALTH, FLAG_SECRET, ROLE_AI};
        let hit = |seq: u32, flags: u8| Hit { k: "k".into(), seq, role: ROLE_AI, score: 0.5, flags };
        let found = || vec![hit(1, 0), hit(2, FLAG_HEALTH), hit(3, FLAG_SECRET), hit(4, FLAG_HEALTH | FLAG_SECRET)];
        let (kept, held) = hold_back(found(), true);
        assert_eq!((kept.iter().map(|h| h.seq).collect::<Vec<_>>(), held), (vec![1, 3], 2));
        let (kept, held) = hold_back(found(), false);
        assert_eq!((kept.len(), held), (4, 0), "an AI on this computer is given them");
    }
}
