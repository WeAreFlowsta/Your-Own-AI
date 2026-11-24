//! The quote check, and the line under a reply.
//!
//! When a reply finishes, every quotation in it is looked for in what the
//! AI was given for that turn. No model: the reply's quoted spans are
//! compared with the given passages after folding case, spacing and the
//! kinds of quotation mark, allowing for a slip or two in the copying.
//! The result says which passages were quoted (the line under the reply,
//! the highlight in the panel) and which quotes are in nothing that was
//! given (they are marked).

use crate::memory_block::{GivenRecord, WhyNot};
use serde::Serialize;

/// A quotation shorter than this is a word or a name, not a quote.
const QUOTE_AT_LEAST: usize = 12;

#[derive(Serialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct QuoteCheck {
    /// For each given passage, in order: did the reply quote it?
    pub quoted: Vec<bool>,
    /// Quotations found in nothing that was given, as the reply wrote them.
    pub not_given: Vec<String>,
    /// How many quotations the reply held.
    pub quotes: usize,
}

/// Case, spacing and typographic marks folded away.
fn fold(text: &str) -> Vec<char> {
    let mut out: Vec<char> = Vec::with_capacity(text.len());
    let mut space = true;
    for c in text.chars() {
        let c = match c {
            '\u{2018}' | '\u{2019}' | '\u{201B}' | '`' => '\'',
            '\u{201C}' | '\u{201D}' | '\u{00AB}' | '\u{00BB}' => '"',
            '\u{2013}' | '\u{2014}' | '\u{2212}' => '-',
            '\u{00A0}' => ' ',
            c => c,
        };
        if c.is_whitespace() {
            if !space {
                out.push(' ');
            }
            space = true;
        } else {
            out.extend(c.to_lowercase());
            space = false;
        }
    }
    while out.last() == Some(&' ') {
        out.pop();
    }
    out
}

/// The quotations in a reply: what stands between double quotation marks,
/// straight or curly. A quotation the reply shortened with "..." is its
/// pieces. Only pieces long enough to be a quote are kept.
pub(crate) fn quotations(reply: &str) -> Vec<String> {
    let chars: Vec<char> = reply.chars().collect();
    let mut out = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        let close = match chars[i] {
            '"' => '"',
            '\u{201C}' => '\u{201D}',
            '\u{00AB}' => '\u{00BB}',
            _ => {
                i += 1;
                continue;
            }
        };
        let Some(len) = chars[i + 1..].iter().position(|c| *c == close) else { break };
        let end = i + 1 + len;
        let inside: String = chars[i + 1..end].iter().collect();
        // A quotation stays within its paragraph: a mark with a blank line
        // before its partner is not the start of one.
        if inside.contains("\n\n") {
            i += 1;
            continue;
        }
        for piece in inside.split("...").flat_map(|p| p.split('\u{2026}')) {
            let piece = piece.trim().trim_end_matches(['.', ',', ';', ':', '!', '?']).trim();
            if piece.chars().count() >= QUOTE_AT_LEAST {
                out.push(piece.to_string());
            }
        }
        i = end + 1;
    }
    out
}

/// Is `needle` in `hay`, allowing `slips` single-character edits? (The
/// fewest edits over every place it could start.)
fn found_within(needle: &[char], hay: &[char], slips: usize) -> bool {
    if needle.is_empty() {
        return true;
    }
    if hay.len() + slips < needle.len() {
        return false;
    }
    // Sellers: a row per character of the needle, free to start anywhere.
    // A slip is a letter or a mark; a figure is never one ("$60" is not
    // "$90"), so an edit that touches a digit costs more than any quote
    // is allowed.
    let never = slips + 1;
    let cost = |c: char| if c.is_ascii_digit() { never } else { 1 };
    let mut prev = vec![0usize; hay.len() + 1];
    for (i, n) in needle.iter().enumerate() {
        let mut row = vec![(i + 1).min(never * 2); hay.len() + 1];
        row[0] = prev[0].saturating_add(cost(*n));
        for j in 1..=hay.len() {
            let h = hay[j - 1];
            let swap = if *n == h { 0 } else { cost(*n).max(cost(h)) };
            row[j] = (prev[j].saturating_add(cost(*n))).min(row[j - 1].saturating_add(cost(h))).min(prev[j - 1].saturating_add(swap));
        }
        prev = row;
    }
    prev.iter().min().copied().unwrap_or(usize::MAX) <= slips
}

/// How many slips a quotation of this length may hold: none in a short
/// one, one per forty characters after that, three at most.
fn slips_for(len: usize) -> usize {
    if len < 20 { 0 } else { (len / 40 + 1).min(3) }
}

/// Check a reply's quotations against the passages it was given (each
/// passage as the text the model saw).
pub(crate) fn check(reply: &str, given: &[String]) -> QuoteCheck {
    let passages: Vec<Vec<char>> = given.iter().map(|g| fold(g)).collect();
    let mut quoted = vec![false; given.len()];
    let mut not_given = Vec::new();
    let quotes = quotations(reply);
    for q in &quotes {
        let needle = fold(q);
        let slips = slips_for(needle.len());
        let mut anywhere = false;
        for (i, p) in passages.iter().enumerate() {
            if found_within(&needle, p, slips) {
                quoted[i] = true;
                anywhere = true;
            }
        }
        if !anywhere {
            not_given.push(q.clone());
        }
    }
    QuoteCheck { quoted, not_given, quotes: quotes.len() }
}

/// A quotation shorter than this (letters and figures, folded) keeps
/// nothing: a few words are not a decision or a detail.
const KEEP_AT_LEAST: usize = 20;

/// What a reply's quotations keep: for each exchange it was given and
/// quoted word for word (no slip at all), the longest such quotation and
/// the place of the found message. The words must be the conversation's
/// own: a quotation of the passage's heading (its date and title) keeps
/// nothing.
pub(crate) fn quoted_exactly(reply: &str, given: &GivenRecord, texts: &[String]) -> Vec<(crate::memory_block::Ptr, String)> {
    let quotes = quotations(reply);
    let mut out = Vec::new();
    for (e, text) in given.exchanges.iter().zip(texts) {
        let said = fold(text.split_once('\n').map_or(text.as_str(), |(_, lines)| lines));
        let best = quotes
            .iter()
            .filter(|q| {
                let needle = fold(q);
                needle.len() >= KEEP_AT_LEAST && found_within(&needle, &said, 0)
            })
            .max_by_key(|q| q.chars().count());
        if let Some(q) = best {
            out.push((e.hit, q.clone()));
        }
    }
    out
}

/// The page's door to the check, when a reply finishes.
#[tauri::command]
pub fn memory_check_quotes(reply: String, given: Vec<String>) -> QuoteCheck {
    check(&reply, &given)
}

/// A finished reply, checked: its quotations against what it was given,
/// the record with each quoted passage marked, and the line it gets.
#[derive(Serialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub struct Checked {
    pub check: QuoteCheck,
    pub given: Option<GivenRecord>,
    pub line: Option<ReplyLine>,
}

/// The page's door when a reply finishes. `texts` are the passages as the
/// model saw them: one per exchange of the record, then the dated lines.
#[tauri::command]
pub fn memory_reply_checked(reply: String, given: Option<GivenRecord>, texts: Vec<String>) -> Checked {
    let check = check(&reply, &texts);
    let line = line_for(given.as_ref(), &check, &reply);
    let given = given.map(|mut g| {
        for (e, q) in g.exchanges.iter_mut().zip(&check.quoted) {
            e.quoted = *q;
        }
        g
    });
    Checked { check, given, line }
}

/// The line a recorded reply gets when its conversation is opened again:
/// from the record alone, whose passages carry whether they were quoted.
pub(crate) fn line_from_record(given: Option<&GivenRecord>, reply: &str) -> Option<ReplyLine> {
    let g = given?;
    let check = QuoteCheck { quoted: g.exchanges.iter().map(|e| e.quoted).collect(), not_given: Vec::new(), quotes: 0 };
    line_for(Some(g), &check, reply)
}

/// Does the reply say that a search was made? ("I searched your
/// conversations and found nothing that answers it.")
pub(crate) fn reports_a_search(reply: &str) -> bool {
    let r: String = fold(reply).into_iter().collect();
    ["i searched", "i looked through", "searched your conversations", "searched their conversations", "found nothing", "no conversation mentions", "did not find anything", "didn't find anything", "could not find anything", "couldn't find anything"].iter().any(|p| r.contains(p))
}

/// The line under a reply. One of these, or none.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ReplyLine {
    /// The reply quoted what it was given.
    Given { passages: u32 },
    /// The reply reports a search and quoted nothing.
    QuotedNone { passages: u32 },
    /// Not everything found could be given.
    DidNotFit { given: u32, found: u32 },
    /// Only part of the history is read so far.
    ReadSoFar { found: u32, percent: u8 },
    /// A name in the question is in no conversation.
    NoMention { names: Vec<String> },
    /// The AI answered without a search.
    DidNotLook { why: WhyNot },
    /// Conversations this AI may not use mention it.
    Withheld { conversations: u32 },
}

/// Which line a reply gets, from what it was given and what it quoted.
/// A reply that was given passages and quoted none, and says nothing of a
/// search, gets no line (an ordinary answer is not about memory); what it
/// was given is still one step away, in the reply's details.
pub(crate) fn line_for(given: Option<&GivenRecord>, check: &QuoteCheck, reply: &str) -> Option<ReplyLine> {
    let g = given?;
    let s = &g.search;
    if !s.ran {
        return s.why_not.map(|why| ReplyLine::DidNotLook { why });
    }
    if s.withheld > 0 {
        return Some(ReplyLine::Withheld { conversations: s.withheld });
    }
    if !s.absent_names.is_empty() {
        return Some(ReplyLine::NoMention { names: s.absent_names.clone() });
    }
    if s.read_share < 0.995 {
        return Some(ReplyLine::ReadSoFar { found: s.found, percent: (s.read_share * 100.0).floor().clamp(0.0, 99.0) as u8 });
    }
    if s.given < s.found {
        return Some(ReplyLine::DidNotFit { given: s.given, found: s.found });
    }
    if check.quoted.iter().any(|q| *q) {
        return Some(ReplyLine::Given { passages: s.given });
    }
    if s.given > 0 && reports_a_search(reply) {
        return Some(ReplyLine::QuotedNone { passages: s.given });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::memory_block::SearchReport;

    fn given() -> Vec<String> {
        vec![
            "[2026-01-20, conversation \"Flour supplier\"]\nAI: Dunmore Grain is the cheaper one, but their delivery is weekly.\nThey said: Then we go with Hartwell Mills. They deliver twice a week.\nAI: Noted: Hartwell Mills from now on.".to_string(),
            "[2026-03-02, conversation \"Delivery\"]\nThey said: Free delivery starts at $60 from Monday March 9.\nAI: Set: $60 from Monday March 9.".to_string(),
        ]
    }

    fn record(search: SearchReport) -> GivenRecord {
        GivenRecord { v: 1, search, ..Default::default() }
    }

    fn ran(found: u32, given: u32) -> SearchReport {
        SearchReport { ran: true, found, given, read_share: 1.0, ..Default::default() }
    }

    #[test]
    fn quote_found() {
        let reply = "You chose Hartwell Mills. On 2026-01-20 you said: \"Then we go with Hartwell Mills. They deliver twice a week.\"";
        let c = check(reply, &given());
        assert_eq!(c, QuoteCheck { quoted: vec![true, false], not_given: vec![], quotes: 1 });
        assert_eq!(line_for(Some(&record(ran(2, 2))), &c, reply), Some(ReplyLine::Given { passages: 2 }));
    }

    #[test]
    fn quote_with_changed_spacing_found() {
        // Curly marks, another case, a line break, a dropped full stop, one slipped letter.
        let reply = "On March 2 you wrote \u{201C}free delivery  starts at\n$60 from monday march 9\u{201D}, and earlier: \"then we go with Hartwel Mills\".";
        let c = check(reply, &given());
        assert_eq!(c.quoted, vec![true, true]);
        assert!(c.not_given.is_empty(), "{:?}", c.not_given);
        // A quotation shortened in the middle is its two pieces.
        let c = check("\"Dunmore Grain is the cheaper one ... their delivery is weekly\"", &given());
        assert_eq!((c.quotes, c.quoted.clone(), c.not_given.len()), (2, vec![true, false], 0));
    }

    #[test]
    fn invented_quote_marked() {
        let reply = "You said \"we will switch to Brightloaf Mills in April\" on 2026-02-11.";
        let c = check(reply, &given());
        assert_eq!(c.quoted, vec![false, false]);
        assert_eq!(c.not_given, vec!["we will switch to Brightloaf Mills in April"]);
        // A real quote and an invented one in the same reply: each is told apart.
        let c = check("\"Noted: Hartwell Mills from now on.\" and also \"the contract runs for three years\"", &given());
        assert_eq!(c.quoted, vec![true, false]);
        assert_eq!(c.not_given, vec!["the contract runs for three years"]);
        // A short quotation is a name, not a quote; a changed number is not a slip.
        assert_eq!(check("the \"Delivery\" chat", &given()).quotes, 0);
        assert_eq!(check("\"Free delivery starts at $90\"", &given()).not_given.len(), 1);
    }

    #[test]
    fn no_quotes_means_no_line_for_a_local_ai() {
        let poem = "Rain on the bakery roof,\nflour dust in the morning light.";
        let c = check(poem, &given());
        assert_eq!(c, QuoteCheck { quoted: vec![false, false], not_given: vec![], quotes: 0 });
        assert_eq!(line_for(Some(&record(ran(12, 12))), &c, poem), None);
        // No record at all (a reply from before one was kept): no line.
        assert_eq!(line_for(None, &c, poem), None);
        // No block was given (an empty history): no line.
        assert_eq!(line_for(Some(&record(ran(0, 0))), &QuoteCheck::default(), poem), None);
    }

    #[test]
    fn a_reply_that_reports_a_search_and_quotes_nothing_says_so() {
        let reply = "I searched your conversations and found nothing that answers it. What I did find is about flour suppliers.";
        let c = check(reply, &given());
        assert_eq!(line_for(Some(&record(ran(9, 9))), &c, reply), Some(ReplyLine::QuotedNone { passages: 9 }));
        assert!(reports_a_search("I looked through your earlier conversations."));
        assert!(!reports_a_search("Here is a poem about rain."));
    }

    #[test]
    fn each_other_state_has_its_line() {
        let none = QuoteCheck::default();
        let not_run = record(SearchReport { ran: false, why_not: Some(WhyNot::NotEnoughMemory), ..Default::default() });
        assert_eq!(line_for(Some(&not_run), &none, "x"), Some(ReplyLine::DidNotLook { why: WhyNot::NotEnoughMemory }));
        assert_eq!(line_for(Some(&record(SearchReport { withheld: 3, ..ran(4, 4) })), &none, "x"), Some(ReplyLine::Withheld { conversations: 3 }));
        assert_eq!(line_for(Some(&record(SearchReport { absent_names: vec!["Brightloaf".into()], ..ran(4, 4) })), &none, "x"), Some(ReplyLine::NoMention { names: vec!["Brightloaf".into()] }));
        assert_eq!(line_for(Some(&record(SearchReport { read_share: 0.62, ..ran(4, 4) })), &none, "x"), Some(ReplyLine::ReadSoFar { found: 4, percent: 62 }));
        assert_eq!(line_for(Some(&record(ran(12, 2))), &none, "x"), Some(ReplyLine::DidNotFit { given: 2, found: 12 }));
        assert_eq!(serde_json::to_value(ReplyLine::DidNotFit { given: 2, found: 12 }).unwrap(), serde_json::json!({ "kind": "did-not-fit", "given": 2, "found": 12 }));
        assert_eq!(serde_json::to_value(ReplyLine::DidNotLook { why: WhyNot::VaultLocked }).unwrap(), serde_json::json!({ "kind": "did-not-look", "why": "vault_locked" }));
    }

    #[test]
    fn a_checked_reply_marks_the_quoted_passages_in_its_record() {
        use crate::memory_block::{GivenExchange, Ptr};
        let ptr = |s: u32| Ptr { c: 1, s, t: 2, a: 0, b: 10 };
        let mut rec = record(ran(2, 2));
        rec.exchanges = vec![GivenExchange { before: None, hit: ptr(2), after: None, score: 0.7, quoted: false }, GivenExchange { before: None, hit: ptr(0), after: None, score: 0.6, quoted: false }];
        let done = memory_reply_checked("As you said: \"Set: $60 from Monday March 9.\"".into(), Some(rec), given());
        let g = done.given.unwrap();
        assert_eq!((g.exchanges[0].quoted, g.exchanges[1].quoted), (false, true));
        assert_eq!(done.line, Some(ReplyLine::Given { passages: 2 }));
        // More texts than exchanges (a saved note rode along): the record keeps its own length.
        let mut rec = record(ran(3, 3));
        rec.exchanges = vec![GivenExchange { before: None, hit: ptr(2), after: None, score: 0.7, quoted: false }];
        let done = memory_reply_checked("\"Free delivery starts at $60 from Monday March 9.\"".into(), Some(rec), given());
        assert_eq!(done.given.unwrap().exchanges.len(), 1);
        assert_eq!(done.check.quoted, vec![false, true]);
        assert!(memory_reply_checked("no record".into(), None, vec![]).line.is_none());
        // Opened again later: the same line, from the record alone.
        let done = memory_reply_checked("As you said: \"Set: $60 from Monday March 9.\"".into(), Some({ let mut r = record(ran(2, 2)); r.exchanges = vec![GivenExchange { before: None, hit: ptr(2), after: None, score: 0.7, quoted: false }, GivenExchange { before: None, hit: ptr(0), after: None, score: 0.6, quoted: false }]; r }), given());
        assert_eq!(line_from_record(done.given.as_ref(), "As you said..."), done.line);
        assert_eq!(line_from_record(None, "x"), None);
    }

    #[test]
    fn a_near_match_far_from_the_text_is_not_found() {
        assert!(found_within(&fold("hartwell mills"), &fold("we go with Hartwell Mills now"), 0));
        assert!(found_within(&fold("hartwel mills"), &fold("we go with Hartwell Mills now"), 1));
        assert!(!found_within(&fold("hartwel mils"), &fold("we go with Hartwell Mills now"), 1));
        assert!(!found_within(&fold("a much longer needle than the hay"), &fold("short hay"), 2));
        assert_eq!((slips_for(15), slips_for(25), slips_for(60), slips_for(400)), (0, 1, 2, 3));
    }

    #[test]
    fn only_a_word_for_word_quotation_of_the_conversation_keeps_its_exchange() {
        use crate::memory_block::{GivenExchange, Ptr};
        let ptr = |s: u32| Ptr { c: 1, s, t: 2, a: 0, b: 10 };
        let mut rec = record(ran(2, 2));
        rec.exchanges = vec![GivenExchange { before: None, hit: ptr(2), after: None, score: 0.7, quoted: false }, GivenExchange { before: None, hit: ptr(0), after: None, score: 0.6, quoted: false }];
        let texts = given();
        // Word for word, from the second passage: kept, by its found message.
        let kept = quoted_exactly("You set it in March: \"Set: $60 from Monday March 9.\"", &rec, &texts);
        assert_eq!(kept, vec![(ptr(0), "Set: $60 from Monday March 9".to_string())]);
        // One letter off is still a quotation for the line under the reply, and keeps nothing.
        assert!(check("As you said: \"Set: $60 from Monday Marsh 9.\"", &texts).quoted[1]);
        assert!(quoted_exactly("As you said: \"Set: $60 from Monday Marsh 9.\"", &rec, &texts).is_empty());
        // A few words keep nothing; neither does a reply with no quotation.
        assert!(quoted_exactly("You said \"from Monday\" I think.", &rec, &texts).is_empty());
        assert!(quoted_exactly("The threshold is $60.", &rec, &texts).is_empty());
        // Words that are only in a passage's heading are not the conversation's.
        let headed = vec!["[2026-03-03, conversation \"Delivery threshold for the van\"]\nThey said: raise it".to_string()];
        let mut one = record(ran(1, 1));
        one.exchanges = vec![GivenExchange { before: None, hit: ptr(4), after: None, score: 0.7, quoted: false }];
        assert!(quoted_exactly("In \"Delivery threshold for the van\" you raised it.", &one, &headed).is_empty());
    }
}
