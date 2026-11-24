//! When an instruction becomes an offer to keep it as a rule.
//!
//! A rule is always the person's choice: it is offered, never made. The
//! offer comes the second time the person asks for the same thing (in any
//! conversation), not the first; an offer they declined is not made
//! again. Whether two instructions are the same is decided by how close
//! they are in meaning, with no model.
//!
//! What was said before, and what was declined, are system rows in
//! memory's scope, so "the second time" counts across conversations and
//! across the person's devices.

use crate::memory_rows::MemoryRow;
use serde::{Deserialize, Serialize};

/// Two instructions at least this close in meaning are the same one.
pub(crate) const SAME_AT: f32 = 0.80;

/// An instruction that was said, or an offer that was declined.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Said {
    pub text: String,
    /// Where: the conversation's start and the message's sequence.
    pub conversation: i64,
    pub seq: u32,
    pub at: i64,
}

fn sys_row(kind: &str, key: &str, s: &Said) -> MemoryRow {
    MemoryRow {
        id: format!("sys-{kind}-{}-{}", s.conversation, s.seq),
        scope: crate::memory_items::SCOPE.to_string(),
        kind: "note".to_string(),
        text: String::new(),
        structured: Some(serde_json::json!({ "sys": kind, key: s })),
        as_of: s.at,
        valid_to: None,
        saved_by: "person".to_string(),
        provenance: "authored".to_string(),
        pinned: false,
        topics: Vec::new(),
        replaces: Vec::new(),
        deleted: false,
        updated_at: s.at,
        action_hash: None,
    }
}

/// An instruction candidate, as the row that is saved.
pub(crate) fn seen_row(s: &Said) -> MemoryRow {
    sys_row("seen_instruction", "said", s)
}

/// A declined offer, as the row that is saved.
pub(crate) fn declined_row(s: &Said) -> MemoryRow {
    sys_row("declined", "said", s)
}

fn read(rows: &[MemoryRow], kind: &str) -> Vec<Said> {
    rows.iter()
        .filter(|r| !r.deleted)
        .filter_map(|r| {
            let s = r.structured.as_ref()?;
            (s.get("sys")?.as_str()? == kind).then(|| serde_json::from_value(s.get("said")?.clone()).ok()).flatten()
        })
        .collect()
}

pub(crate) fn seen(rows: &[MemoryRow]) -> Vec<Said> {
    read(rows, "seen_instruction")
}

pub(crate) fn declined(rows: &[MemoryRow]) -> Vec<Said> {
    read(rows, "declined")
}

/// Does a message read as a standing instruction about how to work? High
/// precision on purpose: short, not a question, with an explicit standing
/// marker.
pub(crate) fn looks_like_instruction(text: &str) -> bool {
    let t = text.trim();
    let n = t.chars().count();
    if !(6..=240).contains(&n) || t.ends_with('?') {
        return false;
    }
    let lower = t.to_lowercase();
    let lower = lower.strip_prefix("please ").unwrap_or(&lower);
    let has_word = |phrase: &str| {
        lower.match_indices(phrase).any(|(i, _)| {
            let before = lower[..i].chars().next_back().is_none_or(|c| !c.is_alphanumeric());
            let after = lower[i + phrase.len()..].chars().next().is_none_or(|c| !c.is_alphanumeric());
            before && after
        })
    };
    let starts_with_word = |phrase: &str| lower.strip_prefix(phrase).is_some_and(|rest| rest.chars().next().is_none_or(|c| !c.is_alphanumeric()));
    ["always", "never", "from now on", "going forward", "in future", "in the future", "every time", "each time"].iter().any(|p| has_word(p))
        || ["don't", "do not", "stop", "avoid", "keep", "use", "only", "no more", "make sure", "remember to", "be more", "be less"].iter().any(|p| starts_with_word(p))
        || ((has_word("instead of") || has_word("rather than")) && ["use", "say", "write", "answer", "reply", "call", "keep"].iter().any(|p| starts_with_word(p)))
}

/// An instruction in the AI's words that the person said yes to: the
/// AI's sentence, when the person's message is a plain yes and the AI's
/// message before it proposed a standing way of working.
pub(crate) fn agreed_instruction(ai_before: &str, person: &str) -> Option<String> {
    let yes: String = person.trim().to_lowercase().chars().filter(|c| c.is_alphanumeric() || c.is_whitespace()).collect();
    let yes = yes.trim();
    let is_yes = ["yes", "yes please", "yes do that", "yes always", "ok", "okay", "ok do that", "do that", "please do", "sounds good", "yes thanks", "yep", "sure"].contains(&yes);
    if !is_yes {
        return None;
    }
    // The last sentence of the AI's message that offers a standing way.
    ai_before
        .split(['.', '!', '?', '\n'])
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .rev()
        .find(|s| {
            let l = s.to_lowercase();
            let offers = ["shall i", "should i", "do you want me to", "would you like me to", "i can", "i will", "i'll"].iter().any(|p| l.starts_with(p) || l.contains(&format!(" {p} ")));
            let standing = ["always", "from now on", "every time", "each time", "going forward", "in future", "in the future", "never"].iter().any(|p| l.contains(p));
            offers && standing
        })
        .map(str::to_string)
}

/// What a message that is an instruction leads to.
#[derive(Debug, PartialEq)]
pub(crate) enum Heard {
    /// The first time: remembered as said, nothing offered.
    FirstTime,
    /// Said before (where it was first said): offer to keep it as a rule.
    Offer { first_said: Said },
    /// The person declined this offer before: nothing.
    Declined,
}

/// Decide for an instruction just said. `like_seen` and `like_declined`:
/// how close it is in meaning to each earlier candidate and each declined
/// offer, in the same order as `seen` and `declined`.
pub(crate) fn heard(seen: &[Said], like_seen: &[f32], declined: &[Said], like_declined: &[f32]) -> Heard {
    if declined.iter().zip(like_declined).any(|(_, s)| *s >= SAME_AT) {
        return Heard::Declined;
    }
    let mut same: Vec<(&Said, f32)> = seen.iter().zip(like_seen.iter().copied()).filter(|(_, s)| *s >= SAME_AT).collect();
    same.sort_by(|a, b| a.0.at.cmp(&b.0.at));
    match same.first() {
        Some((first, _)) => Heard::Offer { first_said: (*first).clone() },
        None => Heard::FirstTime,
    }
}

/// Make an item's words a rule for its project: the rule row to save. The
/// item itself is left as it is.
pub(crate) fn project_rule(id: &str, project: &str, text: &str, now: i64) -> MemoryRow {
    MemoryRow {
        id: id.to_string(),
        scope: format!("project:{project}"),
        kind: "rule".to_string(),
        text: text.trim().to_string(),
        structured: None,
        as_of: now,
        valid_to: None,
        saved_by: "person".to_string(),
        provenance: "authored".to_string(),
        pinned: true,
        topics: Vec::new(),
        replaces: Vec::new(),
        deleted: false,
        updated_at: now,
        action_hash: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn said(text: &str, conversation: i64, at: i64) -> Said {
        Said { text: text.into(), conversation, seq: 0, at }
    }

    #[test]
    fn first_time_is_not_an_offer() {
        assert!(looks_like_instruction("Always write dates as day month year."));
        assert_eq!(heard(&[], &[], &[], &[]), Heard::FirstTime);
        // Something else was said before: still the first time for this.
        let before = [said("Never suggest nuts.", 1, 10)];
        assert_eq!(heard(&before, &[0.31], &[], &[]), Heard::FirstTime);
        // What reads as an instruction, and what does not.
        for t in ["Please don't use bullet points.", "From now on answer in one sentence", "Use metric units", "Say Hartwell rather than the mill", "never round prices"] {
            assert!(looks_like_instruction(t), "{t}");
        }
        for t in ["Should I always use metric?", "ok", "I always forget the oven timer but that's my problem and a long story about how the bakery started when my aunt left me her mixer and a sack of rye flour that nobody else in the family wanted, which is how I ended up here writing to you at midnight", "Useful, thanks", "The stopwatch is broken"] {
            assert!(!looks_like_instruction(t), "{t}");
        }
    }

    #[test]
    fn second_time_offers() {
        let before = [said("Never suggest nuts.", 1, 10), said("Write dates as day month year.", 2, 20), said("always put the day before the month in dates", 3, 30)];
        // In another conversation, in other words: the same instruction.
        let out = heard(&before, &[0.2, 0.86, 0.91], &[], &[]);
        assert_eq!(out, Heard::Offer { first_said: before[1].clone() }, "the offer names where it was first said");
        // Just under the bar is a different instruction.
        assert_eq!(heard(&before, &[0.2, 0.79, 0.5], &[], &[]), Heard::FirstTime);
        // The candidates travel as rows and count from any device.
        let rows: Vec<MemoryRow> = before.iter().map(seen_row).collect();
        assert_eq!(seen(&rows).len(), 3);
        assert!(crate::memory_items::items(&rows).is_empty());
        assert!(rows.iter().all(|r| r.text.is_empty()), "a system row is not shown as words anywhere");
    }

    #[test]
    fn declined_stays_declined() {
        let before = [said("Write dates as day month year.", 2, 20)];
        let no = [said("Write dates as day month year.", 5, 40)];
        // A third time, and a fourth in other words: no offer.
        assert_eq!(heard(&before, &[0.99], &no, &[0.99]), Heard::Declined);
        assert_eq!(heard(&before, &[0.84], &no, &[0.83]), Heard::Declined);
        // Another instruction said twice is still offered.
        assert!(matches!(heard(&before, &[0.9], &no, &[0.2]), Heard::Offer { .. }));
        let rows = vec![declined_row(&no[0]), seen_row(&before[0])];
        assert_eq!((declined(&rows), seen(&rows)), (no.to_vec(), before.to_vec()));
    }

    #[test]
    fn an_instruction_in_the_ais_words_with_a_yes_counts() {
        let ai = "I wrote it as 3 March. Shall I always write dates as day month year from now on?";
        assert_eq!(agreed_instruction(ai, "yes").as_deref(), Some("Shall I always write dates as day month year from now on"));
        assert_eq!(agreed_instruction(ai, "Yes, please!").as_deref(), Some("Shall I always write dates as day month year from now on"));
        assert_eq!(agreed_instruction("Done. I'll use metric units every time.", "sounds good").as_deref(), Some("I'll use metric units every time"));
        // A yes to something that is not a standing way of working.
        assert_eq!(agreed_instruction("Shall I send the order now?", "yes"), None);
        // Not a plain yes.
        assert_eq!(agreed_instruction(ai, "yes but only for invoices"), None);
        assert_eq!(agreed_instruction(ai, "no"), None);
    }

    #[test]
    fn an_item_made_a_project_rule_keeps_the_item() {
        use crate::memory_block::Ptr;
        use crate::memory_items::{keep, meta_of, Audience, ItemState, ItemType, Keep};
        let (item, _) = keep(&[], Keep { id: "it-1".into(), text: "Deploy with: firebase deploy --only hosting", item_type: ItemType::Detail, ptr: Ptr { c: 1, s: 0, t: 1, a: 0, b: 5 }, as_of: 1, by: "person", audience: Some(Audience::Project { project: "id:p1".into() }), kind: "exchange", why: "pressed", quote: None, subject: None, now: 5 }).ok().unwrap();
        let rule = project_rule("rule-1", "id:p1", &item.text, 9);
        assert_eq!((rule.scope.as_str(), rule.kind.as_str(), rule.text.as_str(), rule.saved_by.as_str()), ("project:id:p1", "rule", "Deploy with: firebase deploy --only hosting", "person"));
        // The item is as it was, in memory's own scope.
        assert_eq!((item.scope.as_str(), meta_of(&item).unwrap().state), ("memory", ItemState::Current));
        // And the rule is given with every message in that project: it is one of the project's rule rows.
        let (text, counts) = crate::memory_prompt::block(&crate::memory_prompt::Held { facts: &[], rows: &[], project_rows: &[rule], recall: None, question_vector: None, conversation: None });
        assert!(text.contains("Rules for this project:") && text.contains("- Deploy with: firebase deploy --only hosting"));
        assert_eq!(counts.rules, 1);
    }
}
