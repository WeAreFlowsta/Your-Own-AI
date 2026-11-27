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
    /// The form that ships. Today's date is in the system part of the
    /// prompt, so the block does not repeat it.
    pub(crate) const fn shipped() -> Self {
        Wording { today: false, rule_scope: true, who_and_where: true }
    }
    /// Every addition at once (for measuring them together).
    #[cfg(test)]
    pub(crate) const fn all() -> Self {
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

/// Messages of one conversation as the lines a passage is made of, in
/// the form every judgment reads ("They said: ...", "AI: ...").
pub(crate) fn passage_text(said: &[Said]) -> String {
    said.iter().map(|m| line(m, false).0).collect::<Vec<_>>().join("\n")
}

/// An exchange as text, and as pointers to what the text shows.
pub(crate) fn exchange_text(e: &Exchange, wording: Wording) -> (String, GivenExchange) {
    let mut lines: Vec<String> = Vec::new();
    let ptr = |m: &Said, tail: bool, lines: &mut Vec<String>| {
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

/// What was found, held to what the model can read within the wait before
/// its first word (`wait_chars`; absent = no such limit) and to the room
/// the turn has left. The best result is always given: when even that one
/// exchange is more than the wait allows, the found message goes alone,
/// without the messages around it, and nothing else with it.
pub(crate) fn within_wait(mut found: Vec<Exchange>, room_chars: Option<usize>, wait_chars: Option<usize>, wording: Wording) -> (Vec<Exchange>, Option<usize>) {
    let Some(wait) = wait_chars else { return (found, room_chars) };
    let size = |e: &Exchange| exchange_text(e, wording).0.chars().count();
    let least = |a: usize| Some(room_chars.map_or(a, |r| r.min(a)));
    match found.first() {
        Some(first) if size(first) > wait => {
            found.truncate(1);
            found[0].before = None;
            found[0].after = None;
            let alone = size(&found[0]);
            (found, least(alone.max(wait)))
        }
        _ => (found, least(wait)),
    }
}

/// The block and what it holds.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Block {
    /// The text that goes ahead of the question.
    pub text: String,
    pub given: Vec<GivenExchange>,
    /// How many the search found, before the budget.
    pub found: u32,
    /// How many are shown (exchanges and dated lines).
    pub shown: u32,
    /// The text of each exchange shown, in order (one per `given`), then of
    /// each dated line: what the reply's quotations are checked against.
    pub texts: Vec<String>,
    /// The dated lines that were shown.
    pub lines: Vec<String>,
}

/// The block for a search that ran. `found` is in rank order. Whole
/// exchanges only: they are taken from the first until one would pass the
/// count or the characters allowed; `room_chars` is what the turn has left
/// when that is less than the window's own cap.
pub(crate) fn block(found: &[Exchange], about: &About, at_most: usize, cap_chars: usize, room_chars: Option<usize>, wording: Wording) -> Block {
    block_with(found, &[], about, at_most, cap_chars, room_chars, wording)
}

/// A dated line that is not an exchange: a note the person saved, or
/// knowledge they gave an AI, found for this question.
pub(crate) fn dated_line(at_micros: i64, what: &str, text: &str) -> String {
    format!("[{}, {what}]\nThey said: {}", day(at_micros), first(&collapse(text), PERSON_CHARS).0)
}

/// `block`, with dated lines (`dated_line`) after the exchanges. They are
/// counted with the exchanges and fit by the same rule.
pub(crate) fn block_with(found: &[Exchange], also_dated: &[String], about: &About, at_most: usize, cap_chars: usize, room_chars: Option<usize>, wording: Wording) -> Block {
    let cap = room_chars.map_or(cap_chars, |r| r.min(cap_chars));
    let mut body = String::new();
    let mut given = Vec::new();
    let mut texts: Vec<String> = Vec::new();
    for e in found.iter().take(at_most) {
        let (text, g) = exchange_text(e, wording);
        if body.chars().count() + text.chars().count() > cap {
            break;
        }
        body.push_str(&text);
        body.push_str("\n\n");
        given.push(g);
        texts.push(text);
    }
    let mut lines = 0;
    for l in also_dated {
        if body.chars().count() + l.chars().count() > cap {
            break;
        }
        body.push_str(l);
        body.push_str("\n\n");
        texts.push(l.clone());
        lines += 1;
    }
    let given_len = given.len();
    let n = given_len + lines;
    let mut report = String::new();
    if wording.today && !about.today.is_empty() {
        report.push_str(&about.today);
        report.push(' ');
    }
    report.push_str(RAN);
    if n == 0 && !(found.is_empty() && also_dated.is_empty()) {
        report.push_str(&format!(" It found {} dated exchange(s), but none of them fits in the room this conversation has left, so none is shown.", found.len() + also_dated.len()));
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
    Block { text, given, found: (found.len() + also_dated.len()) as u32, shown: n as u32, lines: texts[given_len..].to_vec(), texts }
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

impl WhyNot {
    /// The cause behind an error from the part of the app that searches.
    pub(crate) fn from_error(e: &str) -> WhyNot {
        let e = e.to_lowercase();
        if e.contains("not downloaded") || e.contains("not installed") {
            WhyNot::ComponentMissing
        } else if e.contains("downloading") {
            WhyNot::ComponentDownloading
        } else if e.contains("not enough free memory") {
            WhyNot::NotEnoughMemory
        } else if e.contains("timed out") || e.contains("timeout") {
            WhyNot::TimedOut
        } else {
            WhyNot::DidNotStartInTime
        }
    }
}

/// What a search reported, as it is recorded with the reply.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct SearchReport {
    pub ran: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why_not: Option<WhyNot>,
    /// Found by the search, and given after the budget.
    pub found: u32,
    pub given: u32,
    /// The share of the history read into the index, 0 to 1.
    pub read_share: f32,
    #[serde(default)]
    pub absent_names: Vec<String>,
    /// Found, but this AI may not use them.
    #[serde(default)]
    pub withheld: u32,
    #[serde(default)]
    pub follow_up: bool,
    /// The date line that was sent.
    #[serde(default)]
    pub today: String,
    /// The percent of the history read, as the block said it, when it
    /// said so.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub partial_percent: Option<u8>,
}

/// What a reply was given by memory: recorded with the reply, as pointers.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct GivenRecord {
    pub v: u8,
    pub search: SearchReport,
    /// In the order given.
    #[serde(default)]
    pub exchanges: Vec<GivenExchange>,
    /// Counts of what rode in the system part.
    #[serde(default)]
    pub also: Also,
    /// How many secrets were hidden in what was given.
    #[serde(default)]
    pub masked: u32,
    #[serde(default)]
    pub mask_version: String,
    #[serde(default)]
    pub cut_version: String,
    /// Dated lines given after the exchanges (a saved note, knowledge given
    /// to the AI), as text: they are not messages a pointer can name.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub lines: Vec<String>,
    /// sha256 of memory's block as it was given: what a later rebuild from
    /// the pointers is compared with.
    #[serde(default)]
    pub block: String,
    /// Found, and left out because memory marks them replaced.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub left_out: Vec<LeftOut>,
    /// The items the note after the question was made from, and the
    /// note's sha256.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub note_items: Vec<String>,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub note: String,
    /// sha256 of the person's turn exactly as sent (set by the sender,
    /// which is the one that knows the final text).
    #[serde(default)]
    pub fingerprint: String,
    /// Fields later versions add ride through untouched.
    #[serde(flatten)]
    pub more: serde_json::Map<String, serde_json::Value>,
}

impl GivenRecord {
    /// Was the reply handed exchanges the search found? Such a reply
    /// repeats them; a later search goes to the exchanges themselves.
    pub(crate) fn gave_exchanges(&self) -> bool {
        self.search.ran && !self.exchanges.is_empty()
    }
}

/// A found message that was left out of what was given, and the replaced
/// item it was left out for.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LeftOut {
    pub hit: Ptr,
    pub score: f32,
    pub item: String,
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq)]
pub struct Also {
    pub rules: u32,
    pub commands: u32,
    pub locations: u32,
    pub facts: u32,
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

/// What a pointer leads to now.
pub(crate) enum Now {
    /// The message, with its secrets hidden, and its conversation.
    Here { by_person: bool, text: String, title: String, ai_name: Option<String>, project: Option<String>, conversation_id: String },
    /// Its conversation was deleted.
    Deleted,
    /// Its conversation is not on this device (yet).
    NotHere,
}

/// One passage of a rebuilt panel.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PanelPassage {
    /// `shown`, `deleted` or `not-here`.
    pub state: &'static str,
    /// The day of the found message (from the pointer, so it is known even
    /// when the message is not).
    pub day: String,
    pub title: String,
    pub ai_name: Option<String>,
    /// The conversation's id on this device and the found message's
    /// sequence: where "Open this in the conversation" goes.
    pub conversation_id: String,
    pub seq: u32,
    /// The conversation's start: its name on every device, which "Do not
    /// use this" is recorded under.
    pub started_at: i64,
    /// The lines as the AI saw them, each `[by the person, text]`.
    pub lines: Vec<(bool, String)>,
    pub quoted: bool,
    pub score: f32,
}

/// How a rebuilt block compares with what was given.
#[derive(Serialize, Clone, Copy, Debug, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub enum Checked {
    /// The rebuilt block is, to the character, what was given.
    Exact,
    /// A source was deleted or is not here: it cannot all be shown.
    SomeGone,
    /// Everything is here and the text differs from what was given.
    Differs,
    /// The record holds no fingerprint of the block to compare with.
    NoRecord,
}

#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Panel {
    pub checked: Checked,
    pub passages: Vec<PanelPassage>,
    pub deleted: u32,
    pub not_here: u32,
    /// The dated lines that rode along (a saved note, given knowledge).
    pub lines: Vec<String>,
    /// The report and the rule, exactly as given (when it could be rebuilt).
    pub instruction: Option<String>,
    /// Found and left out because memory marks them replaced: each by its
    /// found message.
    pub left_out: Vec<PanelLeftOut>,
}

/// A left-out message as the panel shows it.
#[derive(Serialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PanelLeftOut {
    /// State `left-out`; one line, the found message's.
    #[serde(flatten)]
    pub passage: PanelPassage,
    /// The replaced item it was left out for.
    pub item: String,
    /// The day that item was replaced, and whether it still is (filled by
    /// the reader of the items).
    pub replaced_on: Option<String>,
    pub still_replaced: bool,
}

/// Rebuild what a reply was given from its record: each pointer looked up
/// as it reads now, the block put together again by the same rule, and its
/// fingerprint compared with the one recorded.
pub(crate) fn rebuild(record: &GivenRecord, at: &dyn Fn(&Ptr) -> Now, wording: Wording) -> Panel {
    let said = |p: &Option<Ptr>| -> Option<Option<Said>> {
        // None: there was no such neighbor. Some(None): it cannot be read now.
        p.as_ref().map(|p| match at(p) {
            Now::Here { by_person, text, .. } => Some(Said { by_person, text, seq: p.s, at: p.t }),
            _ => None,
        })
    };
    let mut passages = Vec::new();
    let mut exchanges: Vec<Exchange> = Vec::new();
    let (mut deleted, mut not_here) = (0u32, 0u32);
    for g in &record.exchanges {
        let gone = |state: &'static str| PanelPassage { state, day: day(g.hit.t), title: String::new(), ai_name: None, conversation_id: String::new(), seq: g.hit.s, started_at: g.hit.c, lines: Vec::new(), quoted: g.quoted, score: g.score };
        match at(&g.hit) {
            Now::Deleted => {
                deleted += 1;
                passages.push(gone("deleted"));
            }
            Now::NotHere => {
                not_here += 1;
                passages.push(gone("not-here"));
            }
            Now::Here { by_person, text, title, ai_name, project, conversation_id } => {
                let (before, after) = (said(&g.before), said(&g.after));
                let e = Exchange {
                    conversation: g.hit.c,
                    title: title.clone(),
                    ai_name: ai_name.clone(),
                    project,
                    before: before.clone().flatten(),
                    hit: Said { by_person, text, seq: g.hit.s, at: g.hit.t },
                    after: after.clone().flatten(),
                    score: g.score,
                };
                let mut lines = Vec::new();
                if let Some(m) = &e.before {
                    lines.push((m.by_person, line(m, true).0));
                }
                lines.push((e.hit.by_person, line(&e.hit, false).0));
                if let Some(m) = &e.after {
                    lines.push((m.by_person, line(m, false).0));
                }
                // A neighbor that was given and cannot be read now.
                if before == Some(None) || after == Some(None) {
                    not_here += 1;
                }
                let lines = lines.into_iter().map(|(p, t)| (p, t.trim_start_matches(if p { "They said: " } else { "AI: " }).to_string())).collect();
                passages.push(PanelPassage { state: "shown", day: day(g.hit.t), title, ai_name, conversation_id, seq: g.hit.s, started_at: g.hit.c, lines, quoted: g.quoted, score: g.score });
                exchanges.push(e);
            }
        }
    }
    let s = &record.search;
    let about = About { today: s.today.clone(), absent_names: s.absent_names.clone(), partial_percent: s.partial_percent, withheld: s.withheld, replaced_left_out: !record.left_out.is_empty() };
    // Everything that was given is here: put it together again, as given
    // (no budget: what was given already fit).
    let whole = deleted == 0 && not_here == 0;
    let rebuilt = whole.then(|| block_with(&exchanges, &record.lines, &about, usize::MAX, usize::MAX, None, wording));
    let checked = match (&rebuilt, record.block.is_empty()) {
        (None, _) => Checked::SomeGone,
        (Some(_), true) => Checked::NoRecord,
        (Some(b), false) if fingerprint(&b.text) == record.block && s.found == s.given => Checked::Exact,
        // More was found than fit: the report's first sentence said so in
        // words this rebuild does not have; compare what was shown instead.
        (Some(b), false) if s.found != s.given && b.texts.len() as u32 == s.given => Checked::NoRecord,
        (Some(_), false) => Checked::Differs,
    };
    let instruction = rebuilt.as_ref().map(|b| b.text.split("\n\n[").next().unwrap_or("").trim_start_matches("[From memory. ").trim_end_matches(']').to_string());
    let left_out = record
        .left_out
        .iter()
        .map(|g| {
            let gone = PanelPassage { state: "left-out", day: day(g.hit.t), title: String::new(), ai_name: None, conversation_id: String::new(), seq: g.hit.s, started_at: g.hit.c, lines: Vec::new(), quoted: false, score: g.score };
            match at(&g.hit) {
                Now::Here { by_person, text, title, ai_name, conversation_id, .. } => {
                    let shown = line(&Said { by_person, text, seq: g.hit.s, at: g.hit.t }, false).0;
                    let shown = shown.trim_start_matches(if by_person { "They said: " } else { "AI: " }).to_string();
                    PanelPassage { title, ai_name, conversation_id, lines: vec![(by_person, shown)], ..gone }
                }
                _ => gone,
            }
        })
        .zip(&record.left_out)
        .map(|(passage, l)| PanelLeftOut { passage, item: l.item.clone(), replaced_on: None, still_replaced: true })
        .collect();
    Panel { checked, passages, deleted, not_here, lines: record.lines.clone(), instruction, left_out }
}

/// One thing memory holds to be true now, for the note after a question.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Held {
    /// The sentence: what a replacement judgment wrote as true now, or the
    /// item's own statement.
    pub sentence: String,
    /// When it became so (ms).
    pub as_of: i64,
    /// The person confirmed it.
    pub confirmed: bool,
}

/// A state note holds at most this many items, each at least this close
/// in meaning to the question, and at most this many characters.
pub(crate) const NOTE_ITEMS: usize = 3;
pub(crate) const NOTE_FLOOR: f32 = 0.55;
const NOTE_CHARS: usize = 600;
const NOT_CONFIRMED: &str = " (an AI's note, not confirmed by them)";

/// The items a note is made from: the closest to the question over the
/// floor, at most three, newest first. `near`: each candidate's index and
/// its likeness to the question.
pub(crate) fn note_items(held: &[Held], near: &[(usize, f32)]) -> Vec<Held> {
    note_order(held, near).into_iter().map(|i| held[i].clone()).collect()
}

/// `note_items`, as places in `held`.
pub(crate) fn note_order(held: &[Held], near: &[(usize, f32)]) -> Vec<usize> {
    let mut close: Vec<(usize, f32)> = near.iter().copied().filter(|(i, s)| *s >= NOTE_FLOOR && *i < held.len()).collect();
    close.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    // The same sentence held twice takes one place: the confirmed one,
    // else the newer.
    let same = |a: usize, b: usize| collapse(&held[a].sentence).eq_ignore_ascii_case(&collapse(&held[b].sentence));
    let mut once: Vec<(usize, f32)> = Vec::new();
    for (i, s) in close {
        match once.iter_mut().find(|(o, _)| same(*o, i)) {
            Some(o) => {
                let (old, new) = (&held[o.0], &held[i]);
                if (new.confirmed, new.as_of) > (old.confirmed, old.as_of) {
                    o.0 = i;
                }
            }
            None => once.push((i, s)),
        }
    }
    let mut close = once;
    close.truncate(NOTE_ITEMS);
    let mut picked: Vec<usize> = close.into_iter().map(|(i, _)| i).collect();
    picked.sort_by(|a, b| held[*b].as_of.cmp(&held[*a].as_of));
    picked
}

/// The state note, placed right after the question: what memory holds to
/// be true now, with the date of the newest. `mark_unconfirmed` says of an
/// AI's own note that the person has not confirmed it. Never dropped for
/// room: it is short by rule (later sentences give way, then the one that
/// is left is cut).
pub(crate) fn state_note(items: &[Held], mark_unconfirmed: bool) -> Option<String> {
    let newest = items.iter().map(|i| i.as_of).max()?;
    let sentence = |i: &Held| {
        let mut s = collapse(&i.sentence);
        if !s.ends_with(['.', '!', '?']) {
            s.push('.');
        }
        if mark_unconfirmed && !i.confirmed {
            s.push_str(NOT_CONFIRMED);
        }
        s
    };
    let mut said: Vec<String> = Vec::new();
    for i in items {
        let s = sentence(i);
        if !said.is_empty() && said.iter().map(|x| x.chars().count() + 1).sum::<usize>() + s.chars().count() > NOTE_CHARS {
            break;
        }
        said.push(s);
    }
    let mut body = said.join(" ");
    if body.chars().count() > NOTE_CHARS {
        body = body.chars().take(NOTE_CHARS).collect();
    }
    Some(format!("[From memory, the state record for this, current as of {}: {body} Anything older that disagrees is history.]", day(newest * 1000)))
}

/// Found exchanges that memory marks as replaced are left out of what is
/// given: the exchange an item now replaced came from. Answers with what
/// stays and what was left out, both in rank order.
pub(crate) fn leave_out_replaced(found: Vec<Exchange>, replaced_sources: &[(i64, u32)]) -> (Vec<Exchange>, Vec<Exchange>) {
    found.into_iter().partition(|e| !replaced_sources.contains(&(e.conversation, e.hit.seq)))
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
    fn what_is_given_is_held_to_the_wait_and_the_best_result_always_goes() {
        let w = Wording::shipped();
        let size = |e: &Exchange| exchange_text(e, w).0.chars().count();
        let whole = size(&mill());
        // No wait known: nothing changes.
        let (found, room) = within_wait(vec![mill(), mill()], Some(900), None, w);
        assert_eq!((found.len(), room), (2, Some(900)));
        // A wait that holds two: the room is the wait, or the turn's own room when that is less.
        let (found, room) = within_wait(vec![mill(), mill(), mill()], None, Some(whole * 2 + 10), w);
        assert_eq!(found.len(), 3);
        let b = block(&found, &About::default(), 12, 42_000, room, w);
        assert_eq!(b.shown, 2);
        assert_eq!(within_wait(vec![mill()], Some(50), Some(whole + 1), w).1, Some(50));
        // A wait shorter than the best exchange: the found message goes
        // alone, without the ones around it, and it is shown.
        let (found, room) = within_wait(vec![mill(), mill()], None, Some(whole - 1), w);
        assert_eq!(found.len(), 1);
        assert!(found[0].before.is_none() && found[0].after.is_none());
        let b = block(&found, &About::default(), 12, 42_000, room, w);
        assert_eq!(b.shown, 1);
        assert!(b.text.contains("Hartwell Mills") && !b.text.contains("Dunmore"));
        // Shorter than the found message itself: it still goes.
        let (found, room) = within_wait(vec![mill()], None, Some(10), w);
        assert_eq!(block(&found, &About::default(), 12, 42_000, room, w).shown, 1);
        // Nothing found: nothing to give.
        assert!(within_wait(Vec::new(), None, Some(10), w).0.is_empty());
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
        let b = block(&[mill()], &about, 12, 42_000, None, Wording::all());
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
    fn a_saved_note_rides_after_the_exchanges_as_a_dated_line() {
        let note = dated_line(T, "a note they saved", "The oven   runs 10 degrees hot.");
        assert_eq!(note, "[2026-01-20, a note they saved]\nThey said: The oven runs 10 degrees hot.");
        let b = block_with(&[mill()], &[note.clone()], &About::default(), 12, 42_000, None, Wording::measured());
        assert!(b.text.contains("It returned the 2 dated exchange(s) below."));
        assert!(b.text.ends_with(&format!("AI: Noted: Hartwell Mills from now on.\n\n{note}]")), "{}", b.text);
        assert_eq!((b.given.len(), b.found, b.shown), (1, 2, 2));
        assert_eq!(b.texts.len(), 2);
        assert!(b.texts[0].starts_with("[2026-01-20, conversation") && b.texts[1] == note);
        // A note alone, with no exchange found.
        let b = block_with(&[], &[note.clone()], &About::default(), 12, 42_000, None, Wording::measured());
        assert!(b.text.contains("It returned the 1 dated exchange(s) below.") && b.text.ends_with(&format!("{note}]")));
        // Nothing found at all: the report alone.
        let b = block_with(&[], &[], &About::default(), 12, 42_000, None, Wording::measured());
        assert!(b.text.contains("It returned the 0 dated exchange(s) below.") && b.text.ends_with("from guesswork.]"));
    }

    #[test]
    fn did_not_run_says_why() {
        for why in [WhyNot::ComponentMissing, WhyNot::ComponentDownloading, WhyNot::NotEnoughMemory, WhyNot::DidNotStartInTime, WhyNot::TimedOut, WhyNot::VaultLocked, WhyNot::IndexRebuilding, WhyNot::SwitchedOffForThisAi] {
            let text = did_not_run(why, "Today is 2026-11-24.", Wording::all());
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
    fn a_search_error_is_one_of_the_causes() {
        assert_eq!(WhyNot::from_error("Embedding model not downloaded: embed.gguf"), WhyNot::ComponentMissing);
        assert_eq!(WhyNot::from_error("Not enough free memory for the memory model right now"), WhyNot::NotEnoughMemory);
        assert_eq!(WhyNot::from_error("Embedding server did not become ready in time"), WhyNot::DidNotStartInTime);
        assert_eq!(WhyNot::from_error("Embedding request failed: operation timed out"), WhyNot::TimedOut);
        assert_eq!(WhyNot::from_error("something else"), WhyNot::DidNotStartInTime);
    }

    #[test]
    fn a_record_reads_back_and_carries_what_a_later_version_adds() {
        let b = block(&[mill()], &About::default(), 12, 42_000, None, Wording::shipped());
        let rec = GivenRecord { v: 1, search: SearchReport { ran: true, found: 1, given: 1, read_share: 1.0, ..Default::default() }, exchanges: b.given.clone(), cut_version: CUT_VERSION.into(), ..Default::default() };
        let mut json = serde_json::to_value(&rec).unwrap();
        assert!(json["search"].get("why_not").is_none());
        json["docs"] = serde_json::json!([{ "doc_id": "d1", "idx": 2 }]);
        json["fingerprint"] = serde_json::json!("abc");
        let back: GivenRecord = serde_json::from_value(json.clone()).unwrap();
        assert_eq!(back.exchanges, b.given);
        assert_eq!(back.fingerprint, "abc");
        assert_eq!(serde_json::to_value(&back).unwrap()["docs"], json["docs"], "a field this version does not know is kept");
        let not: GivenRecord = serde_json::from_value(serde_json::json!({ "v": 1, "search": { "ran": false, "why_not": "vault_locked", "found": 0, "given": 0, "read_share": 0.0 } })).unwrap();
        assert_eq!(not.search.why_not, Some(WhyNot::VaultLocked));
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
    fn note_text_matches_the_measured_form() {
        let march_3 = 1_772_496_000_000i64; // 2026-03-03 00:00 UTC, ms
        let one = [Held { sentence: "Free delivery starts at $60.".into(), as_of: march_3, confirmed: true }];
        assert_eq!(
            state_note(&one, true).unwrap(),
            "[From memory, the state record for this, current as of 2026-03-03: Free delivery starts at $60. Anything older that disagrees is history.]"
        );
        assert_eq!(state_note(&one, false), state_note(&one, true), "a confirmed item reads the same either way");
        assert_eq!(state_note(&[], true), None);
        // Several: newest first as given, the date of the newest, a missing full stop added.
        let two = [Held { sentence: "Free delivery starts at $60".into(), as_of: march_3, confirmed: true }, Held { sentence: "Orders under $60 keep the $6 fee.".into(), as_of: march_3 - 86_400_000, confirmed: true }];
        assert_eq!(state_note(&two, true).unwrap(), "[From memory, the state record for this, current as of 2026-03-03: Free delivery starts at $60. Orders under $60 keep the $6 fee. Anything older that disagrees is history.]");
        // After the question, never before it.
        let turn = turn_text(None, "What is the free delivery threshold?", state_note(&one, true).as_deref());
        assert!(turn.starts_with("What is the free delivery threshold?\n\n[From memory, the state record for this"));
        // Short by rule.
        let long: Vec<Held> = (0..3).map(|i| Held { sentence: format!("{i} {}", "word ".repeat(80)), as_of: march_3, confirmed: true }).collect();
        let note = state_note(&long, true).unwrap();
        assert!(note.chars().count() < 600 + 130, "{}", note.chars().count());
        assert!(note.contains("0 word") && !note.contains("2 word"));
    }

    #[test]
    fn an_unconfirmed_note_is_marked() {
        let march_3 = 1_772_496_000_000i64;
        let mixed = [Held { sentence: "Closed on Saturdays from March.".into(), as_of: march_3, confirmed: false }, Held { sentence: "Open 7:30 on weekdays.".into(), as_of: march_3 - 5, confirmed: true }];
        assert_eq!(
            state_note(&mixed, true).unwrap(),
            "[From memory, the state record for this, current as of 2026-03-03: Closed on Saturdays from March. (an AI's note, not confirmed by them) Open 7:30 on weekdays. Anything older that disagrees is history.]"
        );
        assert!(!state_note(&mixed, false).unwrap().contains("not confirmed"), "the measured form has no mark");
    }

    #[test]
    fn a_note_is_made_from_the_closest_items_newest_first() {
        let held: Vec<Held> = (0..6).map(|i| Held { sentence: format!("Item {i}."), as_of: 1000 + i as i64, confirmed: true }).collect();
        let near = [(0, 0.9), (1, 0.54), (2, 0.6), (3, 0.7), (4, 0.56), (5, 0.2), (9, 0.99)];
        let picked: Vec<String> = note_items(&held, &near).into_iter().map(|h| h.sentence).collect();
        assert_eq!(picked, vec!["Item 3.", "Item 2.", "Item 0."], "the three closest over the floor, newest first");
        assert!(note_items(&held, &[(0, 0.5), (1, 0.3)]).is_empty(), "nothing close enough: no note");
        // The same sentence held three times takes one place, and the confirmed one speaks.
        let h = |s: &str, as_of: i64, confirmed: bool| Held { sentence: s.into(), as_of, confirmed };
        let twice = [h("Free delivery starts at $60.", 5, false), h("free delivery starts at  $60.", 3, true), h("Free delivery starts at $60.", 9, false), h("Orders under $60 keep the $6 fee.", 1, false), h("The van runs on Tuesdays.", 2, false)];
        let picked = note_items(&twice, &[(0, 0.9), (1, 0.88), (2, 0.87), (3, 0.7), (4, 0.6)]);
        assert_eq!(picked.len(), 3, "{picked:?}");
        assert_eq!(picked.iter().filter(|p| p.sentence.to_lowercase().starts_with("free delivery")).count(), 1);
        assert!(picked.iter().any(|p| p.confirmed && p.as_of == 3));
    }

    #[test]
    fn replaced_sources_are_left_out_and_reported() {
        let old = Exchange { conversation: 111, title: "Delivery, January".into(), hit: person(0, "let's make delivery free over $40."), before: None, after: None, ..mill() };
        let new = Exchange { conversation: 222, title: "Delivery, March".into(), hit: person(0, "raise the free delivery threshold to $60 from Monday."), before: None, after: None, ..mill() };
        let (stay, left_out) = leave_out_replaced(vec![new.clone(), old.clone(), mill()], &[(111, 0)]);
        assert_eq!(stay.iter().map(|e| e.conversation).collect::<Vec<_>>(), vec![222, mill().conversation]);
        assert_eq!(left_out, vec![old]);
        // The report says older, replaced exchanges were left out (the measured sentence).
        let b = block(&stay, &About { replaced_left_out: !left_out.is_empty(), ..Default::default() }, 12, 42_000, None, Wording::measured());
        assert!(b.text.contains("It returned the 2 dated exchange(s) below; older exchanges that memory has marked as replaced were left out."));
        assert!(!b.text.contains("$40"));
        // Nothing replaced among what was found: nothing is said of it.
        let (stay, left_out) = leave_out_replaced(vec![new], &[(999, 4)]);
        assert_eq!((stay.len(), left_out.len()), (1, 0));
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

    fn recorded(found: &[Exchange], lines: &[String]) -> GivenRecord {
        let b = block_with(found, lines, &About::default(), 12, 42_000, None, Wording::shipped());
        GivenRecord { v: 1, search: SearchReport { ran: true, found: b.found, given: b.shown, read_share: 1.0, ..Default::default() }, exchanges: b.given, lines: b.lines, block: fingerprint(&b.text), cut_version: CUT_VERSION.into(), ..Default::default() }
    }

    /// The made-up conversation the pointers of `mill()` lead into.
    fn mill_now(p: &Ptr, changed: Option<(u32, &str)>) -> Now {
        let e = mill();
        let m = [e.before.unwrap(), e.hit, e.after.unwrap()].into_iter().find(|m| m.seq == p.s).unwrap();
        let text = match changed {
            Some((seq, text)) if seq == p.s => text.to_string(),
            _ => m.text,
        };
        Now::Here { by_person: m.by_person, text, title: "Flour supplier".into(), ai_name: Some("Juno".into()), project: Some("Bakery site".into()), conversation_id: "uhCkk-made-up".into() }
    }

    #[test]
    fn a_rebuilt_panel_is_exactly_what_was_given() {
        let note = dated_line(T, "a note they saved", "The oven runs 10 degrees hot.");
        let rec = recorded(&[mill()], &[note.clone()]);
        let panel = rebuild(&rec, &|p| mill_now(p, None), Wording::shipped());
        assert_eq!(panel.checked, Checked::Exact);
        assert_eq!((panel.deleted, panel.not_here), (0, 0));
        let p = &panel.passages[0];
        assert_eq!((p.state, p.day.as_str(), p.title.as_str(), p.ai_name.as_deref(), p.seq), ("shown", "2026-01-20", "Flour supplier", Some("Juno"), 2));
        assert_eq!(p.lines, vec![(false, "Dunmore Grain is the cheaper one, but their delivery is weekly.".to_string()), (true, "Then we go with Hartwell Mills. They deliver twice a week.".to_string()), (false, "Noted: Hartwell Mills from now on.".to_string())]);
        assert_eq!(panel.lines, vec![note]);
        let instruction = panel.instruction.unwrap();
        assert!(instruction.starts_with("A search of all their conversations was run for this question. It returned the 2 dated exchange(s) below.") && instruction.ends_with("general."), "{instruction}");
        // The passage the reply quoted keeps its mark.
        let mut quoted = rec.clone();
        quoted.exchanges[0].quoted = true;
        assert!(rebuild(&quoted, &|p| mill_now(p, None), Wording::shipped()).passages[0].quoted);
    }

    #[test]
    fn a_deleted_source_cannot_be_shown_and_a_changed_one_is_said_to_differ() {
        let rec = recorded(&[mill()], &[]);
        let panel = rebuild(&rec, &|_| Now::Deleted, Wording::shipped());
        assert_eq!((panel.checked, panel.deleted, panel.passages[0].state), (Checked::SomeGone, 1, "deleted"));
        assert_eq!(panel.passages[0].day, "2026-01-20", "the date is known from the pointer");
        assert!(panel.passages[0].lines.is_empty() && panel.instruction.is_none());
        let panel = rebuild(&rec, &|_| Now::NotHere, Wording::shipped());
        assert_eq!((panel.checked, panel.not_here, panel.passages[0].state), (Checked::SomeGone, 1, "not-here"));
        // The found message is here and a neighbor is not.
        let panel = rebuild(&rec, &|p| if p.s == 3 { Now::NotHere } else { mill_now(p, None) }, Wording::shipped());
        assert_eq!((panel.checked, panel.passages[0].state, panel.passages[0].lines.len()), (Checked::SomeGone, "shown", 2));
        // Read again since, and one word no longer matches.
        let panel = rebuild(&rec, &|p| mill_now(p, Some((2, "Then we go with Hartwell Mills. They deliver three times a week."))), Wording::shipped());
        assert_eq!(panel.checked, Checked::Differs);
        assert_eq!(panel.passages[0].lines[1].1, "Then we go with Hartwell Mills. They deliver three times a week.", "it is shown as it reads now");
        // A record from before the block's fingerprint was kept.
        let mut old = rec.clone();
        old.block = String::new();
        assert_eq!(rebuild(&old, &|p| mill_now(p, None), Wording::shipped()).checked, Checked::NoRecord);
        assert_eq!(serde_json::to_value(Checked::SomeGone).unwrap(), "some-gone");
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
