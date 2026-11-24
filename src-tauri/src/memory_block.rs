//! The block memory puts in the person's turn, ahead of their question:
//! what a search of their conversations found, as whole dated exchanges,
//! with a report of the search and the rule for using it.
//!
//! The texts here are measured: answers were read with exactly these words
//! and this layout, and any change to one of them changes the answers. The
//! core form (`Wording::measured`) is kept reproducible beside the form
//! that ships, so one can always be compared with the other.
//!
//! Everything in this file is text from values: it reads nothing, so the
//! same input always gives the same block.

use serde::{Deserialize, Serialize};

/// The version of the exchange format, recorded with every reply.
pub(crate) const CUT_VERSION: &str = "exchange-1";

/// A person's message is cut to this many characters, an AI's to that.
const PERSON_CHARS: usize = 600;
const AI_CHARS: usize = 1200;
const TITLE_CHARS: usize = 60;

const RAN: &str = "A search of all their conversations was run for this question.";
const RULE: &str = "\nRule for questions about their past work: answer only from a line you can quote from these exchanges, and quote it with its date. If none of them answers the question, say plainly that you searched their conversations and found nothing that answers it, and say what you did find. Never answer about their past from guesswork.";
/// What the rule is for, and what it leaves alone.
const RULE_SCOPE: &str = " The rule is for claims about what they said or did; general knowledge may be used when you say that it is general.";

/// A message of a conversation, with its secrets already hidden.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Said {
    pub by_person: bool,
    pub text: String,
    pub seq: u32,
    /// When it was written, in microseconds.
    pub at: i64,
}

/// A found message with the message before it and the one after.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Exchange {
    /// The conversation's start, in microseconds: what names it everywhere.
    pub conversation: i64,
    pub title: String,
    /// The AI the conversation was with, when the header should say.
    pub ai_name: Option<String>,
    /// The project it was held in, by the name the person knows.
    pub project: Option<String>,
    pub before: Option<Said>,
    pub hit: Said,
    pub after: Option<Said>,
    pub score: f32,
}

/// A place in a conversation, the same on every device and after a
/// restore: the conversation's start, the message's sequence and time, and
/// the characters used.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Ptr {
    pub c: i64,
    pub s: u32,
    pub t: i64,
    pub a: u32,
    pub b: u32,
}

/// One exchange as it was given, by pointers, never a second copy of text.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct GivenExchange {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub before: Option<Ptr>,
    pub hit: Ptr,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub after: Option<Ptr>,
    pub score: f32,
    /// Filled when the reply finishes: did it quote this exchange?
    #[serde(default)]
    pub quoted: bool,
}

/// Which sentences the block carries beyond the measured core.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Wording {
    /// `Today is YYYY-MM-DD.` leads the report.
    pub today: bool,
    /// The sentence saying what the rule is for.
    pub rule_scope: bool,
    /// An exchange's header names the AI and the project.
    pub who_and_where: bool,
}

impl Wording {
    /// The form the answers were first measured with.
    #[cfg(test)]
    pub(crate) const fn measured() -> Self {
        Wording { today: false, rule_scope: false, who_and_where: false }
    }
    /// The form that ships.
    pub(crate) const fn shipped() -> Self {
        Wording { today: true, rule_scope: true, who_and_where: true }
    }
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn first(text: &str, n: usize) -> (String, u32, u32) {
    let chars: Vec<char> = text.chars().collect();
    let b = chars.len().min(n);
    (chars[..b].iter().collect(), 0, b as u32)
}

fn last(text: &str, n: usize) -> (String, u32, u32) {
    let chars: Vec<char> = text.chars().collect();
    let a = chars.len().saturating_sub(n);
    (chars[a..].iter().collect(), a as u32, chars.len() as u32)
}

/// One message's line, and the characters of its collapsed text it shows.
/// The message before a found one is shown by its end when the AI wrote
/// it (an answer's conclusion is what the person replied to).
fn line(m: &Said, tail: bool) -> (String, u32, u32) {
    let text = collapse(&m.text);
    let n = if m.by_person { PERSON_CHARS } else { AI_CHARS };
    let (shown, a, b) = if tail && !m.by_person { last(&text, n) } else { first(&text, n) };
    (format!("{}{shown}", if m.by_person { "They said: " } else { "AI: " }), a, b)
}

/// `YYYY-MM-DD`, the UTC day of a time in microseconds.
pub(crate) fn day(micros: i64) -> String {
    let days = micros.div_euclid(86_400_000_000);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// An exchange as text, and as pointers to what the text shows.
pub(crate) fn exchange_text(e: &Exchange, wording: Wording) -> (String, GivenExchange) {
    let mut lines: Vec<String> = Vec::new();
    let mut ptr = |m: &Said, tail: bool, lines: &mut Vec<String>| {
        let (text, a, b) = line(m, tail);
        lines.push(text);
        Ptr { c: e.conversation, s: m.seq, t: m.at, a, b }
    };
    let before = e.before.as_ref().map(|m| ptr(m, true, &mut lines));
    let hit = ptr(&e.hit, false, &mut lines);
    let after = e.after.as_ref().map(|m| ptr(m, false, &mut lines));
    let title: String = e.title.chars().take(TITLE_CHARS).collect();
    let mut header = format!("[{}, conversation \"{title}\"", day(e.hit.at));
    if wording.who_and_where {
        if let Some(ai) = e.ai_name.as_deref().filter(|a| !a.is_empty()) {
            header.push_str(&format!(", with {ai}"));
        }
        if let Some(p) = e.project.as_deref().filter(|p| !p.is_empty()) {
            header.push_str(&format!(", project {p}"));
        }
    }
    header.push(']');
    (format!("{header}\n{}", lines.join("\n")), GivenExchange { before, hit, after, score: e.score, quoted: false })
}

/// How much a block may hold: the number of exchanges and the characters
/// of their text, by the window the reply is made in.
pub(crate) fn budget(window_tokens: u32, online: bool) -> (usize, usize) {
    if online || window_tokens >= 16_000 {
        (12, 42_000)
    } else if window_tokens >= 8_000 {
        (6, 15_000)
    } else {
        (3, 6_000)
    }
}

/// What a search has to say about itself.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct About {
    /// The date line (`Today is YYYY-MM-DD.`), when the wording carries it.
    pub today: String,
    /// Names in the question that no conversation mentions.
    pub absent_names: Vec<String>,
    /// Percent of the history read into the index, when not all of it is.
    pub partial_percent: Option<u8>,
    /// Exchanges found that this AI may not use.
    pub withheld: u32,
    /// Older exchanges were left out because memory marks them replaced.
    pub replaced_left_out: bool,
}

/// The block and what it holds.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Block {
    /// The text that goes ahead of the question.
    pub text: String,
    pub given: Vec<GivenExchange>,
    /// How many the search found, before the budget.
    pub found: u32,
}

/// The block for a search that ran. `found` is in rank order. Whole
/// exchanges only: they are taken from the first until one would pass the
/// count or the characters allowed; `room_chars` is what the turn has left
/// when that is less than the window's own cap.
pub(crate) fn block(found: &[Exchange], about: &About, at_most: usize, cap_chars: usize, room_chars: Option<usize>, wording: Wording) -> Block {
    let cap = room_chars.map_or(cap_chars, |r| r.min(cap_chars));
    let mut body = String::new();
    let mut given = Vec::new();
    for e in found.iter().take(at_most) {
        let (text, g) = exchange_text(e, wording);
        if body.chars().count() + text.chars().count() > cap {
            break;
        }
        body.push_str(&text);
        body.push_str("\n\n");
        given.push(g);
    }
    let n = given.len();
    let mut report = String::new();
    if wording.today && !about.today.is_empty() {
        report.push_str(&about.today);
        report.push(' ');
    }
    report.push_str(RAN);
    if n == 0 && !found.is_empty() {
        report.push_str(&format!(" It found {} dated exchange(s), but none of them fits in the room this conversation has left, so none is shown.", found.len()));
    } else if about.replaced_left_out {
        report.push_str(&format!(" It returned the {n} dated exchange(s) below; older exchanges that memory has marked as replaced were left out."));
    } else {
        report.push_str(&format!(" It returned the {n} dated exchange(s) below."));
    }
    if let Some(p) = about.partial_percent {
        report.push_str(&format!(" Only {p}% of their conversations have been read so far, so the answer may be in the rest; say so."));
    }
    if about.withheld > 0 {
        report.push_str(&format!(" {} more exchange(s) were found in conversations this AI may not use; they are not shown.", about.withheld));
    }
    if !about.absent_names.is_empty() {
        report.push_str(&format!(" No conversation anywhere mentions: {}.", about.absent_names.join(", ")));
    }
    report.push_str(RULE);
    if wording.rule_scope {
        report.push_str(RULE_SCOPE);
    }
    let text = format!("[From memory. {}\n\n{}]", report.trim_end(), body.trim_end());
    // With nothing to show, the report stands alone.
    let text = if n == 0 { format!("[From memory. {}]", report.trim_end()) } else { text };
    Block { text, given, found: found.len() as u32 }
}

/// Why a search did not run.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WhyNot {
    ComponentMissing,
    ComponentDownloading,
    NotEnoughMemory,
    DidNotStartInTime,
    TimedOut,
    VaultLocked,
    IndexRebuilding,
    SwitchedOffForThisAi,
}

impl WhyNot {
    /// The cause, as the model is told it.
    fn said(self) -> &'static str {
        match self {
            WhyNot::ComponentMissing => "the part of the app that searches is not installed",
            WhyNot::ComponentDownloading => "the part of the app that searches is still being downloaded",
            WhyNot::NotEnoughMemory => "the computer did not have enough free memory to search",
            WhyNot::DidNotStartInTime => "the search did not start in time",
            WhyNot::TimedOut => "the search took too long and was stopped",
            WhyNot::VaultLocked => "their Vault is locked, so their conversations cannot be opened",
            WhyNot::IndexRebuilding => "the search index is being rebuilt",
            WhyNot::SwitchedOffForThisAi => "they have switched memory off for this AI",
        }
    }
}

/// The block for a search that did not run: one sentence saying so and
/// why, so "did not look" is never read as "looked and found nothing".
pub(crate) fn did_not_run(why: WhyNot, today: &str, wording: Wording) -> String {
    let lead = if wording.today && !today.is_empty() { format!("{today} ") } else { String::new() };
    format!(
        "[From memory. {lead}No search of their conversations was run for this question: {}. If the question is about their past work, say that you could not look this time, and do not answer about their past from guesswork.]",
        why.said()
    )
}

/// The person's turn as it is sent: the block, the question exactly as
/// typed, and the state note, in that order (the order is measured).
pub(crate) fn turn_text(before_question: Option<&str>, question: &str, after_question: Option<&str>) -> String {
    let mut out = String::new();
    if let Some(b) = before_question.filter(|b| !b.is_empty()) {
        out.push_str(b);
        out.push_str("\n\n");
    }
    out.push_str(question);
    if let Some(a) = after_question.filter(|a| !a.is_empty()) {
        out.push_str("\n\n");
        out.push_str(a);
    }
    out
}

/// The fingerprint of a turn: of the exact text sent.
pub(crate) fn fingerprint(turn_text: &str) -> String {
    use sha2::Digest;
    hex::encode(sha2::Sha256::digest(turn_text.as_bytes()))
}

// ── names the question mentions ─────────────────────────────────────────

const STOP: &str = "the a an of to in on for and or is are was were did do does we our us i you it that this what which why how when where with about from at by be been have has had not still any ever already into out up down as if so than then there their they them can could should would will just get got one all also but";

/// Names in a question: runs of capitalized words, not counting one that
/// starts the question (a sentence's first word is capitalized anyway).
pub(crate) fn names_in(question: &str) -> Vec<String> {
    let chars: Vec<char> = question.chars().collect();
    let is_word = |c: char| c.is_ascii_alphanumeric() || c == '_';
    let mut names: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        // A name starts at a word boundary, on a capital, not at the start.
        let at_boundary = i > 0 && !is_word(chars[i - 1]);
        if !(at_boundary && chars[i].is_ascii_uppercase()) {
            i += 1;
            continue;
        }
        let mut end = i;
        loop {
            // One capitalized word of two characters or more.
            let mut j = end;
            if j >= chars.len() || !chars[j].is_ascii_uppercase() {
                break;
            }
            j += 1;
            let rest_from = j;
            while j < chars.len() && chars[j].is_ascii_alphanumeric() {
                j += 1;
            }
            if j == rest_from {
                break;
            }
            end = j;
            // Another one after a single space joins the same name.
            if end + 1 < chars.len() && chars[end] == ' ' && chars[end + 1].is_ascii_uppercase() {
                let mut k = end + 2;
                while k < chars.len() && chars[k].is_ascii_alphanumeric() {
                    k += 1;
                }
                if k > end + 2 {
                    end += 1;
                    continue;
                }
            }
            break;
        }
        if end > i {
            let name: String = chars[i..end].iter().collect();
            if !names.contains(&name) {
                names.push(name);
            }
            i = end;
        } else {
            i += 1;
        }
    }
    let stop: Vec<&str> = STOP.split(' ').collect();
    names.into_iter().filter(|n| !stop.contains(&n.to_lowercase().as_str())).collect()
}

/// How many single-character edits turn one word into another, up to
/// `limit` (anything further answers `limit + 1`).
pub(crate) fn edit_distance(a: &str, b: &str, limit: usize) -> usize {
    let (a, b): (Vec<char>, Vec<char>) = (a.chars().collect(), b.chars().collect());
    if a.len().abs_diff(b.len()) > limit {
        return limit + 1;
    }
    let mut prev: Vec<usize> = (0..=b.len()).collect();
    for i in 1..=a.len() {
        let mut row = vec![i; b.len() + 1];
        for j in 1..=b.len() {
            row[j] = (prev[j] + 1).min(row[j - 1] + 1).min(prev[j - 1] + usize::from(a[i - 1] != b[j - 1]));
        }
        if row.iter().min().copied().unwrap_or(0) > limit {
            return limit + 1;
        }
        prev = row;
    }
    prev[b.len()].min(limit + 1)
}

/// Which of the question's names no conversation mentions. `mentioned`
/// says whether a lower-cased name appears in what this AI can use;
/// `close` whether a word of nearly the same spelling does (a slip of the
/// keyboard is not an absent name).
pub(crate) fn absent_names(question: &str, mentioned: &dyn Fn(&str) -> bool, close: &dyn Fn(&str) -> bool) -> Vec<String> {
    names_in(question).into_iter().filter(|n| { let l = n.to_lowercase(); !mentioned(&l) && !close(&l) }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const T: i64 = 1_768_896_000_000_000; // 2026-01-20 08:00:00 UTC

    fn person(seq: u32, text: &str) -> Said {
        Said { by_person: true, text: text.to_string(), seq, at: T + i64::from(seq) * 90_000_000 }
    }

    fn ai(seq: u32, text: &str) -> Said {
        Said { by_person: false, ..person(seq, text) }
    }

    fn mill() -> Exchange {
        Exchange {
            conversation: T - 60_000_000,
            title: "Flour supplier".into(),
            ai_name: Some("Juno".into()),
            project: Some("Bakery site".into()),
            before: Some(ai(1, "Dunmore Grain is the cheaper one,\n  but their delivery is weekly.")),
            hit: person(2, "Then we go with  Hartwell Mills.\nThey deliver twice a week."),
            after: Some(ai(3, "Noted: Hartwell Mills from now on.")),
            score: 0.71,
        }
    }

    #[test]
    fn the_day_is_the_utc_day() {
        assert_eq!(day(T), "2026-01-20");
        assert_eq!(day(0), "1970-01-01");
        assert_eq!(day(T + 16 * 3_600_000_000 - 1), "2026-01-20");
        assert_eq!(day(T + 16 * 3_600_000_000), "2026-01-21");
        assert_eq!(day(1_709_164_800_000_000), "2024-02-29");
    }

    #[test]
    fn block_text_matches_the_measured_form() {
        let second = Exchange { title: "Delivery".into(), before: None, hit: person(0, "Free delivery starts at $60 from March 9."), after: Some(ai(1, "Set: $60 from Monday March 9.")), score: 0.6, ..mill() };
        let b = block(&[mill(), second], &About::default(), 12, 42_000, None, Wording::measured());
        assert_eq!(
            b.text,
            "[From memory. A search of all their conversations was run for this question. It returned the 2 dated exchange(s) below.\n\
             Rule for questions about their past work: answer only from a line you can quote from these exchanges, and quote it with its date. If none of them answers the question, say plainly that you searched their conversations and found nothing that answers it, and say what you did find. Never answer about their past from guesswork.\n\n\
             [2026-01-20, conversation \"Flour supplier\"]\n\
             AI: Dunmore Grain is the cheaper one, but their delivery is weekly.\n\
             They said: Then we go with Hartwell Mills. They deliver twice a week.\n\
             AI: Noted: Hartwell Mills from now on.\n\n\
             [2026-01-20, conversation \"Delivery\"]\n\
             They said: Free delivery starts at $60 from March 9.\n\
             AI: Set: $60 from Monday March 9.]"
        );
        assert_eq!(turn_text(Some(&b.text), "Which mill do we use?", None), format!("{}\n\nWhich mill do we use?", b.text));
        // A name no conversation mentions is said before the rule.
        let about = About { absent_names: vec!["Bellamy".into(), "Crest Bank".into()], ..Default::default() };
        let b = block(&[mill()], &about, 12, 42_000, None, Wording::measured());
        assert!(b.text.starts_with("[From memory. A search of all their conversations was run for this question. It returned the 1 dated exchange(s) below. No conversation anywhere mentions: Bellamy, Crest Bank.\nRule for questions about their past work:"), "{}", b.text);
        // Replaced material left out: the measured first sentence.
        let b = block(&[mill()], &About { replaced_left_out: true, ..Default::default() }, 12, 42_000, None, Wording::measured());
        assert!(b.text.contains("It returned the 1 dated exchange(s) below; older exchanges that memory has marked as replaced were left out.\nRule"), "{}", b.text);
    }

    #[test]
    fn the_shipped_form_adds_only_what_is_named() {
        let about = About { today: "Today is 2026-11-24.".into(), partial_percent: Some(62), withheld: 3, ..Default::default() };
        let b = block(&[mill()], &about, 12, 42_000, None, Wording::shipped());
        assert!(b.text.starts_with(
            "[From memory. Today is 2026-11-24. A search of all their conversations was run for this question. It returned the 1 dated exchange(s) below. \
             Only 62% of their conversations have been read so far, so the answer may be in the rest; say so. \
             3 more exchange(s) were found in conversations this AI may not use; they are not shown.\nRule for questions about their past work:"
        ), "{}", b.text);
        assert!(b.text.contains("Never answer about their past from guesswork. The rule is for claims about what they said or did; general knowledge may be used when you say that it is general.\n\n[2026-01-20, conversation \"Flour supplier\", with Juno, project Bakery site]\n"), "{}", b.text);
        // Taking the additions away gives the measured text back.
        let plain = block(&[mill()], &About::default(), 12, 42_000, None, Wording::measured());
        let stripped = b.text.replace("Today is 2026-11-24. ", "").replace(" Only 62% of their conversations have been read so far, so the answer may be in the rest; say so.", "").replace(" 3 more exchange(s) were found in conversations this AI may not use; they are not shown.", "").replace(RULE_SCOPE, "").replace(", with Juno, project Bakery site", "");
        assert_eq!(stripped, plain.text);
    }

    #[test]
    fn exchange_cuts() {
        let long_person: String = (0..700).map(|i| char::from(b'a' + (i % 26) as u8)).collect();
        let long_ai: String = (0..1500).map(|i| char::from(b'A' + (i % 26) as u8)).collect();
        let e = Exchange { before: Some(ai(4, &long_ai)), hit: person(5, &long_person), after: Some(ai(6, &long_ai)), ..mill() };
        let (text, given) = exchange_text(&e, Wording::measured());
        let lines: Vec<&str> = text.lines().collect();
        // The AI's message before: its LAST 1,200 characters.
        assert_eq!(lines[1], format!("AI: {}", &long_ai[300..]));
        assert_eq!(given.before, Some(Ptr { c: e.conversation, s: 4, t: e.before.as_ref().unwrap().at, a: 300, b: 1500 }));
        // The found message, the person's: its first 600.
        assert_eq!(lines[2], format!("They said: {}", &long_person[..600]));
        assert_eq!(given.hit, Ptr { c: e.conversation, s: 5, t: e.hit.at, a: 0, b: 600 });
        // The AI's message after: its first 1,200.
        assert_eq!(lines[3], format!("AI: {}", &long_ai[..1200]));
        assert_eq!(given.after.unwrap().b, 1200);
        // A found AI message is cut to 1,200 from its start; a person's message before it to 600 from its start.
        let e = Exchange { before: Some(person(4, &long_person)), hit: ai(5, &long_ai), after: Some(person(6, &long_person)), ..mill() };
        let (text, given) = exchange_text(&e, Wording::measured());
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines[1], format!("They said: {}", &long_person[..600]));
        assert_eq!(lines[2], format!("AI: {}", &long_ai[..1200]));
        assert_eq!(lines[3], format!("They said: {}", &long_person[..600]));
        assert_eq!((given.before.unwrap().a, given.hit.b), (0, 1200));
        // A title is cut to 60 characters; the first and the last message of a conversation have one neighbor.
        let e = Exchange { title: "t".repeat(80), before: None, after: None, ..mill() };
        let (text, given) = exchange_text(&e, Wording::measured());
        assert_eq!(text, format!("[2026-01-20, conversation \"{}\"]\nThey said: Then we go with Hartwell Mills. They deliver twice a week.", "t".repeat(60)));
        assert_eq!((given.before, given.after), (None, None));
        // Characters, never bytes.
        let e = Exchange { hit: person(0, &"é".repeat(700)), before: None, after: None, ..mill() };
        assert_eq!(exchange_text(&e, Wording::measured()).1.hit.b, 600);
    }

    #[test]
    fn whole_exchanges_only_in_a_small_room() {
        let found: Vec<Exchange> = (0..12).map(|i| Exchange { hit: person(i, &format!("{} {}", i, "word ".repeat(100))), before: None, after: None, ..mill() }).collect();
        let one = exchange_text(&found[0], Wording::measured()).0.chars().count();
        assert_eq!(budget(16_384, false), (12, 42_000));
        assert_eq!(budget(8_192, false), (6, 15_000));
        assert_eq!(budget(4_096, false), (3, 6_000));
        assert_eq!(budget(4_096, true), (12, 42_000), "an online model has the room");
        // The count allowed by the window.
        let (n, cap) = budget(4_096, false);
        assert_eq!(block(&found, &About::default(), n, cap, None, Wording::measured()).given.len(), 3);
        // The room left in the turn, when smaller: whole exchanges that fit, never a cut one.
        let b = block(&found, &About::default(), 12, 42_000, Some(one * 2 + one / 2), Wording::measured());
        assert_eq!(b.given.len(), 2);
        assert!(b.text.contains("It returned the 2 dated exchange(s) below."));
        assert!(b.text.ends_with(&format!("{}]", "word ".repeat(100).trim_end())), "the last exchange shown is whole");
        assert_eq!(b.found, 12);
        // No room for even one: the report says how many were found and that none fits.
        let b = block(&found, &About::default(), 12, 42_000, Some(10), Wording::measured());
        assert!(b.given.is_empty());
        assert!(b.text.contains("It found 12 dated exchange(s), but none of them fits in the room this conversation has left, so none is shown."), "{}", b.text);
        assert!(b.text.ends_with("Never answer about their past from guesswork.]"));
    }

    #[test]
    fn did_not_run_says_why() {
        for why in [WhyNot::ComponentMissing, WhyNot::ComponentDownloading, WhyNot::NotEnoughMemory, WhyNot::DidNotStartInTime, WhyNot::TimedOut, WhyNot::VaultLocked, WhyNot::IndexRebuilding, WhyNot::SwitchedOffForThisAi] {
            let text = did_not_run(why, "Today is 2026-11-24.", Wording::shipped());
            assert!(text.starts_with("[From memory. Today is 2026-11-24. No search of their conversations was run for this question: "), "{text}");
            assert!(text.contains(why.said()) && text.ends_with("do not answer about their past from guesswork.]"));
            // Never the words of a search that ran.
            assert!(!text.contains("was run for this question. It returned") && !text.contains("found nothing"));
        }
        assert_eq!(serde_json::to_string(&WhyNot::DidNotStartInTime).unwrap(), "\"did_not_start_in_time\"");
        let all: std::collections::HashSet<&str> = [WhyNot::ComponentMissing, WhyNot::ComponentDownloading, WhyNot::NotEnoughMemory, WhyNot::DidNotStartInTime, WhyNot::TimedOut, WhyNot::VaultLocked, WhyNot::IndexRebuilding, WhyNot::SwitchedOffForThisAi].iter().map(|w| w.said()).collect();
        assert_eq!(all.len(), 8, "each cause has its own sentence");
    }

    #[test]
    fn order_is_block_then_question_then_note() {
        let b = block(&[mill()], &About::default(), 12, 42_000, None, Wording::shipped());
        let note = "[From memory, the state record for this, current as of 2026-03-09: Free delivery starts at $60. Anything older that disagrees is history.]";
        let question = "What is the free delivery threshold?";
        let turn = turn_text(Some(&b.text), question, Some(note));
        assert_eq!(turn, format!("{}\n\n{question}\n\n{note}", b.text));
        let (at_block, at_question, at_note) = (turn.find("[From memory. ").unwrap(), turn.find(question).unwrap(), turn.find("[From memory, the state record").unwrap());
        assert!(at_block == 0 && at_block + b.text.len() < at_question && at_question + question.len() < at_note);
        // With no memory at all the turn is the question as typed.
        assert_eq!(turn_text(None, question, None), question);
        assert_eq!(turn_text(Some(""), question, Some("")), question);
        assert_eq!(turn_text(None, question, Some(note)), format!("{question}\n\n{note}"));
    }

    #[test]
    fn given_holds_pointers_not_text() {
        let b = block(&[mill()], &About::default(), 12, 42_000, None, Wording::shipped());
        let stored = serde_json::to_string(&b.given).unwrap();
        for word in ["Hartwell", "Dunmore", "Flour supplier", "Juno", "Bakery"] {
            assert!(!stored.contains(word), "{word} is in the record: {stored}");
        }
        let back: Vec<GivenExchange> = serde_json::from_str(&stored).unwrap();
        assert_eq!(back, b.given);
        assert_eq!(back[0].hit.c, T - 60_000_000);
        assert_eq!((back[0].hit.s, back[0].before.unwrap().s, back[0].after.unwrap().s), (2, 1, 3));
        // An exchange with no neighbors stores none.
        let alone = exchange_text(&Exchange { before: None, after: None, ..mill() }, Wording::shipped()).1;
        assert!(!serde_json::to_string(&alone).unwrap().contains("before"));
    }

    #[test]
    fn fingerprint_is_of_the_exact_wire_text() {
        let turn = turn_text(Some("[From memory. x]"), "Which mill?", None);
        assert_eq!(fingerprint(&turn), fingerprint("[From memory. x]\n\nWhich mill?"));
        assert_ne!(fingerprint(&turn), fingerprint("[From memory. x]\n\nWhich mill? "));
        assert_eq!(fingerprint("abc"), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    }

    #[test]
    fn names_are_capitalized_runs_not_at_the_start() {
        assert_eq!(names_in("What did we decide about Hartwell Mills and the Crest Bank loan?"), vec!["Hartwell Mills", "Crest Bank"]);
        assert_eq!(names_in("Hartwell Mills or Dunmore?"), vec!["Mills", "Dunmore"], "the pattern skips only the very first word");
        assert_eq!(names_in("did I ever tell you about juno"), Vec::<String>::new());
        assert_eq!(names_in("which is it, The or That? Ask Bellamy2, then Bellamy2 again."), vec!["Ask Bellamy2", "Bellamy2"]);
        assert_eq!(names_in("what about X and the Q4 report"), vec!["Q4"], "one letter is not a name");
        assert_eq!(names_in("is it in When or Where"), Vec::<String>::new(), "stop words are never names");
    }

    #[test]
    fn name_absent_and_name_close_spelling() {
        let history = "we chose hartwell mills for the flour; bellamy does the books";
        let mentioned = |n: &str| history.contains(n);
        let words: Vec<&str> = history.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
        let close = |n: &str| !n.contains(' ') && words.iter().any(|w| edit_distance(n, w, 2) <= 2 && w.len() >= 4);
        assert_eq!(absent_names("did we pick Hartwell Mills or Dunmore?", &mentioned, &close), vec!["Dunmore"]);
        // A slip of the keyboard is not an absent name.
        assert_eq!(absent_names("what does Belamy charge?", &mentioned, &close), Vec::<String>::new());
        assert_eq!(absent_names("what does Bellomy charge?", &mentioned, &close), Vec::<String>::new());
        assert_eq!(absent_names("what does Whitlock charge?", &mentioned, &close), vec!["Whitlock"]);
        assert_eq!(edit_distance("bellamy", "belamy", 2), 1);
        assert_eq!(edit_distance("bellamy", "bellomi", 2), 2);
        assert_eq!(edit_distance("bellamy", "whitlock", 2), 3);
        assert_eq!(edit_distance("", "abc", 2), 3);
        assert_eq!(edit_distance("same", "same", 2), 0);
    }
}
