//! Does a model on this computer answer from what memory gives it?
//!
//! A made-up history and questions about it, marked here. The questions
//! are the kinds a real history makes hard: a figure that was proposed and
//! not taken, a state changed twice, an answer that needs two exchanges, a
//! question in other words than the answer's, and questions the history
//! comes near and does not answer. The search
//! is not what is measured: each question comes with its results already in
//! order, and the block is built and sized by the same code a real turn
//! uses, for the reading speed this computer showed. What is marked is the
//! reply: did it answer the question from the exchange that holds the
//! answer, did it give an older state that a later exchange replaced, did
//! it say so when the answer was not among what it was shown, or did it
//! answer something nobody asked.
//!
//! No model is named anywhere: any model is marked the same way, on its
//! reply alone.

use crate::memory_block::{Exchange, Said};
use serde::Deserialize;

const SET: &str = include_str!("memory_reply_set.json");

#[derive(Deserialize, Clone, Debug)]
pub(crate) struct Set {
    pub v: u32,
    /// The instructions the replies are asked under: a persona of the
    /// app's kind, with the lines a small model is known to fall back on.
    pub system: String,
    exchanges: Vec<Made>,
    pub questions: Vec<Question>,
}

#[derive(Deserialize, Clone, Debug)]
struct Made {
    id: String,
    date: String,
    title: String,
    before: String,
    hit: String,
    after: String,
}

#[derive(Deserialize, Clone, Debug)]
pub(crate) struct Question {
    pub id: String,
    pub ask: String,
    /// The exchanges as the search would rank them, closest first.
    pub order: Vec<String>,
    /// The exchange that holds the answer; none when the history has none.
    pub answer_in: Vec<String>,
    /// Each group needs one of its strings in the reply (lowercase).
    pub right: Vec<Vec<String>>,
    /// Strings of the older state a later exchange replaced.
    #[serde(default)]
    pub stale: Vec<String>,
    /// Strings of something beside the answer: a figure proposed and not
    /// taken, the other person, the other item.
    #[serde(default)]
    pub wrong: Vec<String>,
    /// For a question the history comes near and cannot answer: words of
    /// the near thing it does say (a price "left where it is"). A reply
    /// that gives them, and none of `wrong`, has said what is there and
    /// invented nothing.
    #[serde(default)]
    pub fine: Vec<String>,
}

pub(crate) fn set() -> Set {
    serde_json::from_str(SET).expect("the built-in set is valid")
}

/// Midnight UTC of a `YYYY-MM-DD` day, in microseconds.
fn day_micros(date: &str) -> i64 {
    let mut p = date.split('-').filter_map(|x| x.parse::<i64>().ok());
    let (Some(y), Some(m), Some(d)) = (p.next(), p.next(), p.next()) else { return 0 };
    // Days from the civil date (the proleptic Gregorian calendar).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y.rem_euclid(400);
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    (era * 146_097 + doe - 719_468) * 86_400_000_000
}

impl Set {
    /// A question's results in rank order, as the search hands them on.
    pub(crate) fn found(&self, q: &Question) -> Vec<Exchange> {
        q.order
            .iter()
            .enumerate()
            .filter_map(|(place, id)| {
                let m = self.exchanges.iter().find(|m| &m.id == id)?;
                let at = day_micros(&m.date) + 9 * 3_600_000_000;
                let said = |by_person: bool, text: &str, seq: u32| Said { by_person, text: text.to_string(), seq, at: at + i64::from(seq) * 90_000_000 };
                Some(Exchange {
                    conversation: at,
                    title: m.title.clone(),
                    ai_name: Some("Juno".into()),
                    project: None,
                    before: Some(said(false, &m.before, 1)),
                    hit: said(true, &m.hit, 2),
                    after: Some(said(false, &m.after, 3)),
                    score: 0.7 - place as f32 * 0.02,
                })
            })
            .collect()
    }
}

impl Set {
    /// Every message of the history, with the exchange it belongs to.
    pub(crate) fn messages(&self) -> Vec<(String, String)> {
        self.exchanges.iter().flat_map(|m| [&m.before, &m.hit, &m.after].map(|t| (m.id.clone(), t.clone()))).collect()
    }
}

/// Where a question's answer stands when the history's messages are put
/// in order of `scores` (one a message, highest first): the place of the
/// best message of an exchange that holds the answer, counted from 1.
pub(crate) fn place_of_answer(q: &Question, of: &[String], scores: &[f32]) -> Option<usize> {
    let mut order: Vec<usize> = (0..of.len().min(scores.len())).collect();
    order.sort_by(|a, b| scores[*b].partial_cmp(&scores[*a]).unwrap_or(std::cmp::Ordering::Equal).then(a.cmp(b)));
    order.iter().position(|i| q.answer_in.contains(&of[*i])).map(|p| p + 1)
}

/// A question's turn as a real one carries it, for a model that reads at
/// `wait_chars` within the wait: the block of the closest results (held to
/// the wait, and saying so when fewer are shown than were found), the line
/// that marks the person's message, the question. With it, the ids of the
/// exchanges shown and how many were found.
pub(crate) fn turn(set: &Set, q: &Question, wait_chars: usize) -> (String, Vec<String>, u32) {
    use crate::memory_block as mb;
    let wording = mb::Wording::shipped();
    let found = set.found(q);
    let ids: Vec<(i64, String)> = found.iter().map(|e| e.conversation).zip(q.order.iter().cloned()).collect();
    let (given, room) = mb::within_wait(found, 12, None, Some(wait_chars), wording);
    let mut about = mb::About::default();
    let mut b = mb::block(&given, &about, 12, 42_000, room, wording);
    if b.shown > 0 && b.shown < b.found {
        about.closest_only = true;
        b = mb::block(&given, &about, 12, 42_000, room, wording);
    }
    let mut shown: Vec<String> = b.given.iter().filter_map(|g| ids.iter().find(|(c, _)| *c == g.hit.c).map(|(_, id)| id.clone())).collect();
    // Held to the wait, an exchange may be shown as its found message
    // alone, without the message before or after it. When the answer's
    // own words are then not in the block, the exchange that holds the
    // answer does not count as shown: a model is marked on what it could
    // read, never on what the wait left out.
    let text = b.text.to_lowercase();
    if !q.right.is_empty() && !q.right.iter().all(|group| group.iter().any(|w| text.contains(w.as_str()))) {
        shown.retain(|id| !q.answer_in.contains(id));
    }
    (mb::turn_text(Some(&b.text), &q.ask, None), shown, b.found)
}

/// What a reply did with the question.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Mark {
    /// The answer, from the exchange that holds it.
    Right,
    /// The older state, though the exchange that replaced it was shown.
    Stale,
    /// The answer was not among what it was shown, or is not in the
    /// history at all, and the reply said it was not there.
    SaidNotThere,
    /// Something beside the answer: a figure that was proposed and not
    /// taken, the other person, the other item.
    Wrong,
    /// The answer was shown and the reply said it was not there.
    Missed,
    /// The answer was not there and the reply answered anyway.
    AnsweredAnyway,
    /// A reply to something nobody asked: a greeting, an offer of help.
    OffTheQuestion,
    /// None of the above: neither the answer nor a clear "not there".
    Unclear,
    /// No words came back.
    Nothing,
}

impl Mark {
    pub(crate) fn word(self) -> &'static str {
        match self {
            Mark::Right => "right",
            Mark::Stale => "stale",
            Mark::SaidNotThere => "said it was not there",
            Mark::Wrong => "wrong",
            Mark::Missed => "missed it",
            Mark::AnsweredAnyway => "answered anyway",
            Mark::OffTheQuestion => "off the question",
            Mark::Unclear => "unclear",
            Mark::Nothing => "nothing",
        }
    }
    /// What the question called for.
    pub(crate) fn good(self) -> bool {
        matches!(self, Mark::Right | Mark::SaidNotThere)
    }
}

const NOT_THERE: &[&str] = &[
    "not in the closest", "nothing that answers", "found nothing", "found no ", "did not find", "didn't find", "could not find", "couldn't find", "can't find", "cannot find", "do not have", "don't have", "no record", "no mention",
    "not mentioned", "wasn't mentioned", "was not mentioned", "does not say", "doesn't say", "do not say", "don't say", "do not see", "don't see", "none of the", "none of these", "not among", "isn't in", "is not in", "not in the exchanges", "not in these",
    "no information", "no relevant", "look further", "nothing about", "nothing on ", "nothing in ", "nothing here", "not covered", "no exchange", "no prior", "no direct", "no decision", "no figure", "no price", "no date", "not stated", "not specified", "not given", "not recorded",
    "does not state", "doesn't state", "does not give", "doesn't give", "does not mention", "doesn't mention", "do not mention", "don't mention", "do not show", "don't show", "does not appear", "doesn't appear", "never set", "was never",
];

/// A reply to a greeting nobody gave: short, opens like one, and says
/// nothing of what was asked.
fn greets(reply: &str) -> bool {
    let r = reply.trim().to_lowercase();
    let opens = ["hello", "hi ", "hi,", "hi.", "hey", "good morning", "good afternoon", "good evening"].iter().any(|g| r.starts_with(g));
    let offers = ["what can i help", "how can i help", "what do you need", "ready to look at what you need", "what would you like", "how may i help"].iter().any(|o| r.contains(o));
    r.chars().count() < 260 && (offers || (opens && (r.contains("doing fine") || r.contains("functioning"))))
}

/// The mark for a reply. `shown` holds the ids of the exchanges the block
/// carried for this question.
pub(crate) fn mark(q: &Question, shown: &[String], reply: &str) -> Mark {
    // A model may write the apostrophe either way; the lists hold one.
    let r = reply.to_lowercase().replace('\u{2019}', "'");
    if r.trim().is_empty() {
        return Mark::Nothing;
    }
    let has_right = !q.right.is_empty() && q.right.iter().all(|group| group.iter().any(|s| r.contains(s.as_str())));
    let has_stale = q.stale.iter().any(|s| r.contains(s.as_str()));
    let has_wrong = q.wrong.iter().any(|s| r.contains(s.as_str()));
    let not_there = NOT_THERE.iter().any(|p| r.contains(p));
    let answer_shown = !q.answer_in.is_empty() && q.answer_in.iter().all(|a| shown.contains(a));
    if answer_shown {
        if has_right {
            // Naming the older state beside the newer one is still right.
            return Mark::Right;
        }
        if has_stale {
            return Mark::Stale;
        }
        if has_wrong {
            return Mark::Wrong;
        }
        if greets(reply) {
            return Mark::OffTheQuestion;
        }
        return if not_there { Mark::Missed } else { Mark::Unclear };
    }
    // The answer was not shown (held back for the wait), or there is none.
    if greets(reply) {
        return Mark::OffTheQuestion;
    }
    if not_there || (q.answer_in.is_empty() && !has_wrong && q.fine.iter().any(|s| r.contains(s.as_str()))) {
        return Mark::SaidNotThere;
    }
    // Anything else is an answer where none could be given from what was
    // shown (the older state, shown without the exchange that replaced
    // it, is counted here too).
    let _ = (has_right, has_stale, has_wrong);
    Mark::AnsweredAnyway
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_block as mb;

    fn q(id: &str) -> Question {
        set().questions.into_iter().find(|q| q.id == id).unwrap()
    }
    fn all(q: &Question) -> Vec<String> {
        q.order.clone()
    }

    #[test]
    fn the_set_is_whole() {
        let s = set();
        assert_eq!(s.v, 2);
        assert_eq!(s.exchanges.len(), 18);
        assert_eq!(s.questions.len(), 23);
        let mut days: Vec<&str> = s.exchanges.iter().map(|e| e.date.as_str()).collect();
        days.sort();
        days.dedup();
        assert_eq!(days.len(), 18, "an exchange is told from another by its day");
        for question in &s.questions {
            assert_eq!(question.order.len(), 12, "{}", question.id);
            let mut ids = question.order.clone();
            ids.sort();
            ids.dedup();
            assert_eq!(ids.len(), 12, "{}: an exchange is ranked twice", question.id);
            assert_eq!(s.found(question).len(), 12, "{}: an unknown exchange", question.id);
            for a in &question.answer_in {
                assert!(question.order.contains(a));
            }
            assert_eq!(question.answer_in.is_empty(), question.right.is_empty(), "{}", question.id);
            for w in question.right.iter().flatten().chain(&question.stale).chain(&question.wrong) {
                assert_eq!(w, &w.to_lowercase());
            }
            // The answer's own words are in the exchanges that hold it
            // (their days count: a question may ask when).
            let text: String = s.exchanges.iter().filter(|e| question.answer_in.contains(&e.id)).map(|e| format!("{} {} {} {} ", e.date, e.before, e.hit, e.after)).collect::<String>().to_lowercase();
            assert!(question.right.iter().all(|g| g.iter().any(|w| text.contains(w.as_str()))), "{}", question.id);
        }
        assert_eq!(s.questions.iter().filter(|q| q.answer_in.is_empty()).count(), 4, "questions the history cannot answer");
        assert!(s.questions.iter().filter(|q| !q.stale.is_empty()).count() >= 6, "questions with an older state");
        assert!(s.questions.iter().filter(|q| q.answer_in.len() > 1).count() >= 1, "a question that needs two exchanges");
    }

    #[test]
    fn a_whole_block_is_long_and_no_message_is_cut() {
        let s = set();
        for question in &s.questions {
            let b = mb::block(&s.found(question), &mb::About::default(), 12, 42_000, None, mb::Wording::shipped());
            assert_eq!(b.shown, 12, "{}", question.id);
            let n = b.text.chars().count();
            assert!((7_000..12_000).contains(&n), "{}: {n}", question.id);
        }
        // Whole messages: an answer is never lost to the block's own cut.
        for e in &s.exchanges {
            assert!(e.hit.chars().count() <= 600 && e.before.chars().count() <= 1_200 && e.after.chars().count() <= 1_200, "{}", e.id);
        }
    }

    #[test]
    fn a_turn_is_held_to_the_wait_and_marks_the_message() {
        let s = set();
        let reader = q("card-reader");
        // A fast reader is shown everything found.
        let (text, shown, found) = turn(&s, &reader, 60_000);
        assert_eq!((shown.len(), found), (12, 12));
        assert_eq!(shown, reader.order);
        assert!(text.ends_with(&format!("{}\n{}", mb::THEIR_MESSAGE, reader.ask)));
        assert!(!text.contains("Only the closest results"));
        // A slow one is shown the closest only, told so, and here the
        // exchange with the answer (ninth) is not among them.
        let (text, shown, found) = turn(&s, &reader, 1_500);
        assert_eq!(found, 12);
        assert!(!shown.is_empty() && shown.len() < 9, "{}", shown.len());
        assert_eq!(shown[..], reader.order[..shown.len()]);
        assert!(text.contains("Only the closest results of the search are shown here"));
        assert!(!shown.contains(&"card-reader".to_string()));
        // The best result is given whatever the speed.
        assert_eq!(turn(&s, &reader, 1).1, vec!["tuneup".to_string()]);
        // An answer that stands in the message after the found one: shown
        // whole it counts, and shown as the found message alone it does not.
        let members = q("tuneup-members");
        assert!(turn(&s, &members, 60_000).1.contains(&"tuneup-members".to_string()));
        let (text, shown, _) = turn(&s, &members, 1_200);
        assert!(text.contains("members go back to the old price") && !text.contains("$70"), "the found message alone");
        assert!(!shown.contains(&"tuneup-members".to_string()));
    }

    #[test]
    fn the_answers_place_among_the_messages() {
        let s = set();
        let of: Vec<String> = s.messages().into_iter().map(|(id, _)| id).collect();
        assert_eq!(of.len(), 54);
        let tune = q("tuneup-price");
        // The three messages of "tuneup" come first in the set.
        let mut scores = vec![0.1f32; 54];
        scores[20] = 0.9;
        scores[1] = 0.8;
        assert_eq!(place_of_answer(&tune, &of, &scores), Some(2));
        scores[2] = 0.95;
        assert_eq!(place_of_answer(&tune, &of, &scores), Some(1));
        assert_eq!(place_of_answer(&q("accountant"), &of, &scores), None);
    }

    #[test]
    fn dates_are_days() {
        assert_eq!(day_micros("1970-01-01"), 0);
        assert_eq!(day_micros("2026-03-03"), 1_772_496_000_000_000);
    }

    #[test]
    fn a_reply_is_marked_on_what_it_says() {
        let tune = q("tuneup-price");
        assert_eq!(mark(&tune, &all(&tune), "On 2026-02-09 you said: \"call it 85 from march 1st\"."), Mark::Right);
        // The figure that was proposed and not taken.
        assert_eq!(mark(&tune, &all(&tune), "The tune-up went to $90."), Mark::Wrong);
        assert_eq!(mark(&tune, &all(&tune), "$85 for walk-ins (I had suggested $90)."), Mark::Right);
        assert_eq!(mark(&tune, &all(&tune), "Hello. I am doing fine, thank you. What can I help you with today?"), Mark::OffTheQuestion);
        assert_eq!(mark(&tune, &all(&tune), "The answer is not in the closest results. You can ask me to look further."), Mark::Missed);
        assert_eq!(mark(&tune, &all(&tune), "It was raised a little in the spring."), Mark::Unclear);
        assert_eq!(mark(&tune, &all(&tune), "  "), Mark::Nothing);
        // The exchange with the answer was held back for the wait.
        let few: Vec<String> = vec!["truing-stand".into()];
        assert_eq!(mark(&tune, &few, "The answer is not in the closest results; I found the truing stand order. Ask me to look further."), Mark::SaidNotThere);
        assert_eq!(mark(&tune, &few, "You settled on $95."), Mark::AnsweredAnyway);
    }

    #[test]
    fn the_older_state_is_stale_only_when_the_newer_one_was_shown() {
        let tubes = q("tubes-26");
        assert_eq!(mark(&tubes, &all(&tubes), "Calder Parts, since 2026-04-21."), Mark::Stale);
        assert_eq!(mark(&tubes, &all(&tubes), "Northgate Wholesale for now; Calder are out until late July."), Mark::Right);
        let pickup = q("pickup-radius");
        assert_eq!(mark(&pickup, &all(&pickup), "Free pickup is within 5 km."), Mark::Stale);
        assert_eq!(mark(&pickup, &all(&pickup), "Within 12 km from June (it was 5 km before)."), Mark::Right);
        let members = q("tuneup-members");
        assert_eq!(mark(&members, &all(&members), "Members pay $85 like everyone."), Mark::Stale);
        assert_eq!(mark(&members, &all(&members), "Members are back on the old $70; walk-ins pay $85."), Mark::Right);
        // Shown the older exchange alone, the older state is all it could say.
        assert_eq!(mark(&tubes, &["tubes-old".to_string()], "Brightwell."), Mark::AnsweredAnyway);
    }

    #[test]
    fn an_answer_that_needs_two_exchanges() {
        let desk = q("late-desk");
        assert_eq!(desk.answer_in.len(), 2);
        assert_eq!(mark(&desk, &all(&desk), "Petra: the late evening is Wednesday now, and she has the desk every Wednesday."), Mark::Right);
        assert_eq!(mark(&desk, &all(&desk), "Imogen covers the front desk."), Mark::Wrong);
        // Shown only one of the two, it cannot be answered.
        assert_eq!(mark(&desk, &["front-desk".to_string()], "The closest results do not say which evening is the late one."), Mark::SaidNotThere);
        let both = q("late-evening");
        assert_eq!(mark(&both, &all(&both), "Wednesday, until 7:30pm."), Mark::Right);
        assert_eq!(mark(&both, &all(&both), "Wednesday."), Mark::Unclear);
        assert_eq!(mark(&both, &all(&both), "Thursday until 8pm."), Mark::Stale);
    }

    #[test]
    fn a_question_the_history_comes_near_and_cannot_answer() {
        let overhaul = q("overhaul-price");
        assert_eq!(mark(&overhaul, &all(&overhaul), "No figure for the overhaul is in these exchanges: on 2026-02-09 you said to leave it where it is."), Mark::SaidNotThere);
        assert_eq!(mark(&overhaul, &all(&overhaul), "The full overhaul is $140."), Mark::AnsweredAnyway);
        // What the history does say, with nothing invented.
        assert_eq!(mark(&overhaul, &all(&overhaul), "On 2026-02-09 you said: \"leave the full overhaul where it is\"."), Mark::SaidNotThere);
        assert_eq!(mark(&overhaul, &all(&overhaul), "It stays as it is, so $85 like the tune-up."), Mark::AnsweredAnyway);
        let class = q("september-class");
        assert_eq!(mark(&class, &all(&class), "Nothing in this conversation history answers that. I found the July and August classes."), Mark::SaidNotThere);
        let marlow = q("second-workshop");
        assert_eq!(mark(&marlow, &all(&marlow), "No prior decision about a second workshop was mentioned in these exchanges."), Mark::SaidNotThere);
        assert_eq!(mark(&marlow, &all(&marlow), "No relevant conversation about opening a second workshop was found in these messages."), Mark::SaidNotThere);
        assert_eq!(mark(&marlow, &all(&marlow), "The conversation doesn\u{2019}t mention a second workshop."), Mark::SaidNotThere);
        assert_eq!(mark(&marlow, &all(&marlow), "Hi. I'm doing fine, thanks for asking. What can I help you with today?"), Mark::OffTheQuestion);
        assert!(Mark::SaidNotThere.good() && Mark::Right.good() && !Mark::AnsweredAnyway.good() && !Mark::Wrong.good());
    }
}
