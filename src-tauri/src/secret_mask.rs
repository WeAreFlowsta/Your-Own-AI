//! The secret mask: a password, key or recovery phrase in a message is
//! replaced before the text leaves the conversation it was written in (for
//! the search index, for another conversation's prompt, for a memory item,
//! for a log). The record keeps the original; only what is handed on is
//! masked. One function, no model: it runs on every turn.
//!
//! Two steps. Candidates are found by shape: key blocks, tokens with a
//! known prefix, three-part signed tokens, runs of recovery words, a value
//! that follows a word such as "password", and long mixed strings. Then
//! each candidate is confirmed, so that the ordinary long strings of
//! working text (paths, addresses, digests, ids, order numbers) stay as
//! they are. Only the secret is replaced, never the message around it.

use sha2::Digest;
use std::collections::HashSet;
use std::ops::Range;
use std::sync::OnceLock;

/// Recorded with what was masked, so a later change of the rules is known.
pub(crate) const MASK_VERSION: &str = "mask-1";
/// What stands where a secret was.
pub(crate) const HIDDEN: &str = "[hidden: looks like a password or key]";

/// A text with its secrets replaced, and how many were.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Masked {
    pub text: String,
    pub hidden: u32,
}

/// The person's own decisions about particular strings, kept as hashes of
/// the string and never as the string.
#[derive(Default)]
pub(crate) struct MaskCtx<'a> {
    /// "This is not a secret."
    pub not_secret: Option<&'a HashSet<[u8; 32]>>,
    /// "Show it this once": the caller gives the set for one turn only.
    pub allow_once: Option<&'a HashSet<[u8; 32]>>,
    /// "This is a secret": strings the shapes below do not know.
    pub also_secret: &'a [String],
}

/// The hash a decision about a string is kept under.
pub(crate) fn hash_of(secret: &str) -> [u8; 32] {
    sha2::Sha256::digest(secret.trim().as_bytes()).into()
}

/// What kind of secret a span looked like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    KeyBlock,
    PrefixedToken,
    SignedToken,
    RecoveryWords,
    AfterCue,
    AddressPassword,
    LongString,
    SaidByThePerson,
}

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Found {
    pub span: Range<usize>,
    pub kind: Kind,
}

/// Replace every secret in `text`.
pub(crate) fn mask(text: &str, ctx: &MaskCtx) -> Masked {
    let mut found = find(text, ctx.also_secret);
    found.retain(|f| {
        let h = hash_of(&text[f.span.clone()]);
        !ctx.not_secret.is_some_and(|s| s.contains(&h)) && !ctx.allow_once.is_some_and(|s| s.contains(&h))
    });
    if found.is_empty() {
        return Masked { text: text.to_string(), hidden: 0 };
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0;
    for f in &found {
        out.push_str(&text[at..f.span.start]);
        out.push_str(HIDDEN);
        at = f.span.end;
    }
    out.push_str(&text[at..]);
    Masked { text: out, hidden: found.len() as u32 }
}

/// Every secret in `text`, in order, none overlapping.
pub(crate) fn find(text: &str, also_secret: &[String]) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::new();
    key_blocks(text, &mut found);
    recovery_words(text, &mut found);
    after_cues(text, &mut found);
    for (start, word) in words(text) {
        in_word(start, word, &mut found);
    }
    for s in also_secret.iter().map(|s| s.trim()).filter(|s| s.len() >= 4) {
        let mut from = 0;
        while let Some(i) = text[from..].find(s) {
            found.push(Found { span: from + i..from + i + s.len(), kind: Kind::SaidByThePerson });
            from += i + s.len();
        }
    }
    // Longest first at each start, then drop anything inside an earlier span.
    found.sort_by(|a, b| a.span.start.cmp(&b.span.start).then(b.span.end.cmp(&a.span.end)));
    let mut kept: Vec<Found> = Vec::new();
    for f in found {
        match kept.last_mut() {
            Some(last) if f.span.start < last.span.end => {
                if f.span.end > last.span.end {
                    last.span.end = f.span.end;
                }
            }
            _ => kept.push(f),
        }
    }
    kept
}

// ── key blocks ──────────────────────────────────────────────────────────

fn key_blocks(text: &str, found: &mut Vec<Found>) {
    let mut from = 0;
    while let Some(i) = text[from..].find("-----BEGIN ") {
        let start = from + i;
        let head_end = match text[start + 11..].find("-----") {
            Some(j) => start + 11 + j + 5,
            None => break,
        };
        let label = &text[start + 11..head_end - 5];
        if !label.ends_with("PRIVATE KEY") || label.len() > 40 {
            from = head_end;
            continue;
        }
        let end = match text[head_end..].find("-----END ") {
            Some(j) => {
                let tail = head_end + j + 9;
                text[tail..].find("-----").map(|k| tail + k + 5).unwrap_or(text.len())
            }
            // No end line (cut off, or pasted in part): the lines after
            // the first that hold nothing but the key's own characters.
            None => {
                let mut end = head_end;
                for line in text[head_end..].split_inclusive('\n') {
                    let body = line.trim();
                    if !body.is_empty() && !body.bytes().all(|c| c.is_ascii_alphanumeric() || b"+/=".contains(&c)) {
                        break;
                    }
                    end += line.len();
                }
                head_end + text[head_end..end].trim_end().len()
            }
        };
        found.push(Found { span: start..end, kind: Kind::KeyBlock });
        from = end;
    }
}

// ── recovery words ──────────────────────────────────────────────────────

fn word_list() -> &'static HashSet<&'static str> {
    static LIST: OnceLock<HashSet<&'static str>> = OnceLock::new();
    LIST.get_or_init(|| include_str!("secret_mask_words.txt").lines().map(str::trim).filter(|w| !w.is_empty()).collect())
}

/// The shortest run of recovery words that is a phrase.
const PHRASE_WORDS: usize = 12;

fn recovery_words(text: &str, found: &mut Vec<Found>) {
    let list = word_list();
    let mut run: Vec<Range<usize>> = Vec::new();
    let close = |run: &mut Vec<Range<usize>>, found: &mut Vec<Found>| {
        if run.len() >= PHRASE_WORDS {
            found.push(Found { span: run[0].start..run[run.len() - 1].end, kind: Kind::RecoveryWords });
        }
        run.clear();
    };
    for (start, word) in words(text) {
        let core = word.trim_matches(|c: char| !c.is_ascii_alphanumeric());
        // A numbered list ("1. abandon 2) ability"): the numbers ride along.
        if !core.is_empty() && core.chars().all(|c| c.is_ascii_digit()) && core.len() <= 2 {
            continue;
        }
        let lower = core.to_ascii_lowercase();
        if core.chars().all(|c| c.is_ascii_alphabetic()) && list.contains(lower.as_str()) {
            let at = start + word.find(core).unwrap_or(0);
            run.push(at..at + core.len());
        } else {
            close(&mut run, found);
        }
    }
    close(&mut run, found);
}

// ── a value after a cue ─────────────────────────────────────────────────

/// Words that say a secret follows. `true`: a person chooses this kind, so
/// plain words joined by dashes can be one.
const CUES: [(&str, bool); 22] = [
    ("password", true),
    ("passwd", true),
    ("passphrase", true),
    ("pass phrase", true),
    ("passcode", true),
    ("pwd", true),
    ("recovery phrase", true),
    ("seed phrase", true),
    ("recovery key", false),
    ("secret key", false),
    ("secret_key", false),
    ("private key", false),
    ("private_key", false),
    ("access key", false),
    ("access_key", false),
    ("api key", false),
    ("api_key", false),
    ("apikey", false),
    ("api-key", false),
    ("secret", false),
    ("token", false),
    ("bearer", false),
];

fn after_cues(text: &str, found: &mut Vec<Found>) {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    for (cue, chosen) in CUES {
        let mut from = 0;
        while let Some(i) = lower[from..].find(cue) {
            let at = from + i;
            from = at + cue.len();
            // The cue starts a word or sits inside a name (DB_PASSWORD).
            if at > 0 && bytes[at - 1].is_ascii_alphabetic() && !lower[..at].ends_with('_') {
                continue;
            }
            let Some(value) = value_after(text, at + cue.len(), cue == "bearer") else { continue };
            let v = &text[value.clone()];
            if is_secret_value(v, chosen) {
                found.push(Found { span: value, kind: Kind::AfterCue });
            }
        }
    }
}

/// The value a cue points at: after the rest of the cue's own name, a
/// separator (`:`, `=`, "is", or for "bearer" a space), then one run of
/// text. Up to four plain words may stand between ("password for the
/// router is ...").
fn value_after(text: &str, mut at: usize, space_is_enough: bool) -> Option<Range<usize>> {
    let b = text.as_bytes();
    // The rest of the name: "passwords", "token_value", "_KEY".
    while at < b.len() && (b[at].is_ascii_alphanumeric() || b[at] == b'_') {
        at += 1;
    }
    let skip = |mut i: usize, set: &[u8]| {
        while i < b.len() && set.contains(&b[i]) {
            i += 1;
        }
        i
    };
    at = skip(at, b"\"'`]) \t");
    let mut separated = false;
    let after_name = at;
    for _ in 0..5 {
        if at < b.len() && (b[at] == b':' || b[at] == b'=') {
            at = skip(at, b":=>");
            separated = true;
            break;
        }
        let word_end = at + text[at..].find(|c: char| !c.is_ascii_alphabetic()).unwrap_or(text.len() - at);
        let word = &text[at..word_end];
        if word.is_empty() {
            break;
        }
        at = skip(word_end, b" \t");
        if word.eq_ignore_ascii_case("is") || word.eq_ignore_ascii_case("was") {
            separated = true;
            break;
        }
    }
    if !separated {
        if !space_is_enough {
            return None;
        }
        at = after_name;
    }
    at = skip(at, b" \t");
    // A quoted value may hold spaces; anything else is one run.
    if at < b.len() && (b[at] == b'"' || b[at] == b'\'' || b[at] == b'`') {
        let quote = b[at];
        let end = text[at + 1..].find(|c: char| c as u32 == u32::from(quote) || c == '\n').map(|j| at + 1 + j)?;
        return (b[end] == quote && end > at + 1).then_some(at + 1..end);
    }
    let end = at + text[at..].find(char::is_whitespace).unwrap_or(text.len() - at);
    let run = &text[at..end];
    let trimmed = run.trim_end_matches(|c: char| ".,;:!?)\"'`".contains(c));
    (!trimmed.is_empty()).then_some(at..at + trimmed.len())
}

/// Is the value after a cue a secret? Not when it is short, a plain word,
/// a placeholder, or a piece of code that fetches the real value.
fn is_secret_value(v: &str, chosen_by_a_person: bool) -> bool {
    if v.chars().count() < 6 || v.len() > 256 {
        return false;
    }
    if v.chars().all(|c| c.is_alphabetic()) {
        return false;
    }
    // Code, placeholders, references to where the value lives.
    // A value may hold any symbol; code shows itself by how it starts (a
    // placeholder, a variable) or by a call.
    if v.starts_with(['<', '$', '{', '[', '(', '%', '|', '\\', '&', '*']) || v.contains('(') || v.contains("://") || v.contains("...") || v.contains("…") {
        return false;
    }
    let letters_only = |s: &str| s.chars().all(|c| c.is_alphabetic());
    let stripped: String = v.chars().filter(|c| !"*xX•·-_.".contains(*c)).collect();
    if stripped.is_empty() {
        return false;
    }
    let has_digit = v.chars().any(|c| c.is_ascii_digit());
    // Names: words joined by dots, underscores, dashes or slashes.
    let pieces: Vec<&str> = v.split(['.', '_', '-', '/', ':', ',', '=']).filter(|p| !p.is_empty()).collect();
    let all_words = pieces.iter().all(|p| letters_only(p));
    if all_words && !has_digit {
        // "correct-horse-battery" after "password" is a password a person
        // chose; "user-provided" after "token" is a description.
        return chosen_by_a_person && v.contains('-') && pieces.len() >= 3 && !v.contains(['.', '/', ':', ',', '=']);
    }
    if v.starts_with('/') || v.starts_with("./") || v.starts_with("~/") {
        return false;
    }
    // A version, a date, a time, a count: digits and separators only.
    if v.chars().all(|c| c.is_ascii_digit() || ".-/:,".contains(c)) {
        return false;
    }
    true
}

// ── inside one word: addresses, known prefixes, signed tokens, long strings ──

/// The words of a text with their byte offsets.
fn words(text: &str) -> impl Iterator<Item = (usize, &str)> {
    let mut at = 0;
    text.split(char::is_whitespace).filter_map(move |w| {
        let start = at;
        at += w.len() + 1;
        (!w.is_empty()).then_some((start, w))
    })
}

/// Prefixes of token formats, with the least length of what follows.
const PREFIXES: [(&str, usize); 30] = [
    ("sk-ant-", 20),
    ("sk-proj-", 20),
    ("sk-or-", 20),
    ("sk-", 20),
    ("sk_live_", 16),
    ("sk_test_", 16),
    ("rk_live_", 16),
    ("rk_test_", 16),
    ("whsec_", 16),
    ("ghp_", 30),
    ("gho_", 30),
    ("ghu_", 30),
    ("ghs_", 30),
    ("ghr_", 30),
    ("github_pat_", 30),
    ("glpat-", 16),
    ("xoxb-", 16),
    ("xoxp-", 16),
    ("xoxa-", 16),
    ("xoxs-", 16),
    ("xapp-", 16),
    ("hf_", 30),
    ("npm_", 30),
    ("pypi-", 30),
    ("dop_v1_", 30),
    ("shpat_", 24),
    ("AIza", 30),
    ("AKIA", 16),
    ("ASIA", 16),
    ("SG.", 30),
];

fn is_token_char(c: u8) -> bool {
    c.is_ascii_alphanumeric() || b"_-+/.~".contains(&c)
}

fn in_word(start: usize, word: &str, found: &mut Vec<Found>) {
    // An address: its password, and nothing else of it.
    if let Some(scheme) = word.find("://") {
        let rest = scheme + 3;
        let authority_end = rest + word[rest..].find(['/', '?', '#']).unwrap_or(word.len() - rest);
        if let Some(at) = word[rest..authority_end].rfind('@') {
            if let Some(colon) = word[rest..rest + at].find(':') {
                let pass = rest + colon + 1..rest + at;
                if pass.len() >= 3 && !word[pass.clone()].chars().all(|c| "*xX".contains(c)) {
                    found.push(Found { span: start + pass.start..start + pass.end, kind: Kind::AddressPassword });
                }
            }
        }
        // Named values in the query (`?token=...`) are found by the cues.
        return;
    }
    // Pieces of the word that could each be a token: `KEY=value`, `"value",`.
    let b = word.as_bytes();
    let mut i = 0;
    while i < b.len() {
        if !is_token_char(b[i]) {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < b.len() && is_token_char(b[j]) {
            j += 1;
        }
        // Padding at the end of an encoded value belongs to it; `=` anywhere
        // else joins a name to its value.
        let mut padded = j;
        while padded < b.len() && b[padded] == b'=' && padded - j < 2 {
            padded += 1;
        }
        if padded == b.len() || !is_token_char(b[padded]) {
            j = padded;
        }
        // A full stop after a token ends the sentence, not the token.
        let piece = word[i..j].trim_end_matches('.');
        if let Some(kind) = token_kind(piece) {
            found.push(Found { span: start + i..start + i + piece.len(), kind });
        }
        i = j;
    }
}

fn token_kind(piece: &str) -> Option<Kind> {
    for (prefix, least) in PREFIXES {
        if let Some(rest) = piece.strip_prefix(prefix) {
            // "sk-" also starts ordinary names (a package, a slug): there
            // the rest must not read as words. The other prefixes are
            // particular enough to stand by themselves.
            let particular = prefix != "sk-";
            if rest.len() >= least && rest.bytes().all(|c| c.is_ascii_alphanumeric() || b"_-.".contains(&c)) && (particular || !is_wordy(rest)) {
                return Some(Kind::PrefixedToken);
            }
        }
    }
    // Three parts joined by dots, the first the start of an encoded header.
    if piece.starts_with("eyJ") {
        let parts: Vec<&str> = piece.split('.').collect();
        if parts.len() == 3 && parts.iter().all(|p| p.len() >= 8 && p.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')) {
            return Some(Kind::SignedToken);
        }
    }
    is_long_secret(piece).then_some(Kind::LongString)
}

/// A long string with no word before it that says what it is: a secret
/// only when it is long, mixed, and none of the ordinary shapes.
fn is_long_secret(s: &str) -> bool {
    let n = s.len();
    if !(20..=128).contains(&n) {
        return false;
    }
    let b = s.as_bytes();
    if !b.iter().all(|c| c.is_ascii_alphanumeric() || b"_-+/=".contains(c)) {
        return false;
    }
    let lower = b.iter().filter(|c| c.is_ascii_lowercase()).count();
    let upper = b.iter().filter(|c| c.is_ascii_uppercase()).count();
    let digits = b.iter().filter(|c| c.is_ascii_digit()).count();
    let classes = usize::from(lower > 0) + usize::from(upper > 0) + usize::from(digits > 0);
    // Letters of one case with digits can be a key too, when the digits
    // are many; one class alone never is.
    if classes < 2 || digits == 0 || lower + upper < 8 || (classes == 2 && digits < 4) {
        return false;
    }
    // A digest, a commit id, a color, an id made of hex groups.
    if b.iter().all(|c| c.is_ascii_hexdigit() || *c == b'-') {
        return false;
    }
    // An order, license or part number: short groups joined by dashes.
    if s.contains('-') && s.split('-').all(|g| g.len() <= 6) {
        return false;
    }
    // A path, or an integrity value of a package.
    if s.starts_with('/') || s.matches('/').count() >= 3 || ["sha1-", "sha256-", "sha384-", "sha512-", "md5-"].iter().any(|p| s.starts_with(p)) {
        return false;
    }
    // An address in the person's own network (an agent, an entry, an
    // action): public by its nature.
    if s.len() == 53 && s.starts_with("uhC") {
        return false;
    }
    if is_wordy(s) {
        return false;
    }
    entropy(b) >= 3.5
}

/// Bits per character, from how often each character appears.
fn entropy(b: &[u8]) -> f64 {
    let mut counts = [0usize; 256];
    for c in b {
        counts[*c as usize] += 1;
    }
    let n = b.len() as f64;
    counts.iter().filter(|c| **c > 0).map(|c| { let p = *c as f64 / n; -p * p.log2() }).sum()
}

/// Is the string made mostly of words (a name in code, a file name, a
/// slug)? Its letters are cut into runs of one case (a capital may lead a
/// lower-case run); a run reads as a word when it is four letters or more
/// with vowels spread through it.
fn is_wordy(s: &str) -> bool {
    let b = s.as_bytes();
    let (mut in_words, mut i) = (0usize, 0usize);
    while i < b.len() {
        if !b[i].is_ascii_alphabetic() {
            i += 1;
            continue;
        }
        let mut j = i + 1;
        if b[i].is_ascii_uppercase() && j < b.len() && b[j].is_ascii_uppercase() {
            while j < b.len() && b[j].is_ascii_uppercase() {
                j += 1;
            }
        } else {
            while j < b.len() && b[j].is_ascii_lowercase() {
                j += 1;
            }
        }
        let run = &b[i..j];
        if run.len() >= 4 && reads_as_a_word(run) {
            in_words += run.len();
        }
        i = j;
    }
    in_words * 2 >= b.len()
}

fn reads_as_a_word(run: &[u8]) -> bool {
    let is_vowel = |c: u8| b"aeiouy".contains(&c.to_ascii_lowercase());
    let vowels = run.iter().filter(|c| is_vowel(**c)).count();
    if vowels * 5 < run.len() || vowels * 10 > run.len() * 7 {
        return false;
    }
    let mut consonants = 0;
    for c in run {
        consonants = if is_vowel(*c) { 0 } else { consonants + 1 };
        if consonants >= 4 {
            return false;
        }
    }
    true
}

/// Text on its way out of the conversation it was written in. Every exit
/// calls this, so the person's decisions about particular strings are
/// read in one place.
pub(crate) fn hand_on(text: &str) -> Masked {
    mask(text, &MaskCtx::default())
}

/// The page's door to the mask, for text it builds itself.
#[tauri::command]
pub fn mask_text(text: String) -> MaskedText {
    let m = hand_on(&text);
    MaskedText { text: m.text, hidden: m.hidden, version: MASK_VERSION }
}

#[derive(serde::Serialize)]
pub struct MaskedText {
    pub text: String,
    pub hidden: u32,
    pub version: &'static str,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn masked(text: &str) -> Masked {
        mask(text, &MaskCtx::default())
    }

    /// Made-up random strings, the same on every run.
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            self.0 >> 33
        }
        fn pick<'a>(&mut self, from: &'a [&'a str]) -> &'a str {
            from[(self.next() % from.len() as u64) as usize]
        }
        fn string(&mut self, alphabet: &[u8], n: usize) -> String {
            (0..n).map(|_| alphabet[(self.next() % alphabet.len() as u64) as usize] as char).collect()
        }
        fn base62(&mut self, n: usize) -> String {
            // Made so that it always mixes cases and digits, as real keys of this length nearly always do.
            loop {
                let s = self.string(b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789", n);
                if s.bytes().any(|c| c.is_ascii_digit()) && s.bytes().any(|c| c.is_ascii_lowercase()) && s.bytes().any(|c| c.is_ascii_uppercase()) {
                    return s;
                }
            }
        }
    }

    #[test]
    fn every_typed_format_is_masked() {
        let mut r = Rng(7);
        let body = |r: &mut Rng, n: usize| r.base62(n);
        let cases = [
            format!("-----BEGIN OPENSSH PRIVATE KEY-----\n{}\n{}\n-----END OPENSSH PRIVATE KEY-----", body(&mut r, 64), body(&mut r, 40)),
            format!("-----BEGIN RSA PRIVATE KEY-----\n{}\n-----END RSA PRIVATE KEY-----", body(&mut r, 64)),
            format!("-----BEGIN PRIVATE KEY-----\n{}", body(&mut r, 64)),
            format!("sk-ant-api03-{}", body(&mut r, 40)),
            format!("sk-proj-{}", body(&mut r, 48)),
            format!("sk-{}", body(&mut r, 48)),
            format!("sk_live_{}", body(&mut r, 24)),
            format!("ghp_{}", body(&mut r, 36)),
            format!("github_pat_{}_{}", body(&mut r, 22), body(&mut r, 59)),
            format!("glpat-{}", body(&mut r, 20)),
            format!("xoxb-{}-{}-{}", r.string(b"0123456789", 12), r.string(b"0123456789", 12), body(&mut r, 24)),
            format!("hf_{}", body(&mut r, 34)),
            format!("npm_{}", body(&mut r, 36)),
            format!("AIza{}", body(&mut r, 35)),
            format!("AKIA{}", r.string(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567", 16)),
            format!("eyJhbGciOiJIUzI1NiJ9.{}.{}", body(&mut r, 40), body(&mut r, 43)),
        ];
        for secret in &cases {
            let text = format!("Here it is, use it for the deploy: {secret}\nTell me if it works.");
            let m = masked(&text);
            assert_eq!(m.hidden, 1, "{secret}");
            assert_eq!(m.text, format!("Here it is, use it for the deploy: {HIDDEN}\nTell me if it works."), "{secret}");
        }
    }

    #[test]
    fn the_secrets_planted_in_the_made_up_history_are_masked() {
        let history: serde_json::Value = serde_json::from_str(include_str!("../../e2e/fixtures/memory/history.json")).unwrap();
        let planted: serde_json::Value = serde_json::from_str(include_str!("../../e2e/fixtures/memory/secrets.json")).unwrap();
        let secrets = planted["secrets"].as_array().unwrap();
        assert!(secrets.len() >= 2);
        for s in secrets {
            let (secret, conv, seq) = (s["text"].as_str().unwrap(), s["in"].as_str().unwrap(), s["seq"].as_u64().unwrap() as usize);
            let c = history["conversations"].as_array().unwrap().iter().find(|c| c["id"] == conv).expect("the conversation");
            let text = c["messages"][seq]["text"].as_str().unwrap();
            assert!(text.contains(secret), "{conv} seq {seq} holds the secret");
            let m = masked(text);
            assert!(!m.text.contains(secret) && m.hidden >= 1, "not masked in: {text}");
            // The words around it are still there.
            assert!(m.text.len() > HIDDEN.len() + 10, "{}", m.text);
        }
        // And nothing else in the made-up history is touched.
        let mut touched = 0;
        let mut messages = 0;
        for c in history["conversations"].as_array().unwrap() {
            for m in c["messages"].as_array().unwrap() {
                messages += 1;
                touched += usize::from(masked(m["text"].as_str().unwrap()).hidden > 0);
            }
        }
        assert_eq!((touched, messages >= 60), (secrets.len(), true), "only the planted secrets are masked");
    }

    #[test]
    fn recovery_words_are_masked_as_one_phrase() {
        let twelve = "legal winner thank year wave sausage worth useful legal winner thank yellow";
        let m = masked(&format!("My recovery words are {twelve} - keep them safe."));
        assert_eq!(m.text, format!("My recovery words are {HIDDEN} - keep them safe."));
        // Twenty-four, numbered, one to a line.
        let words: Vec<&str> = "abandon ability able about above absent absorb abstract absurd abuse access accident account accuse achieve acid acoustic acquire across act action actor actress actual".split(' ').collect();
        let numbered = words.iter().enumerate().map(|(i, w)| format!("{}. {w}", i + 1)).collect::<Vec<_>>().join("\n");
        let m = masked(&format!("Write these down:\n{numbered}\nDone."));
        assert_eq!(m.hidden, 1);
        assert!(!m.text.contains("abstract") && m.text.ends_with("\nDone."), "{}", m.text);
        // Eleven such words in a row are a sentence, not a phrase.
        let eleven = "legal winner thank year wave sausage worth useful legal winner thank";
        assert_eq!(masked(eleven).hidden, 0);
        // Ordinary writing never runs twelve list words together.
        let prose = "The quick brown fox jumps over the lazy dog and then the author makes another small change to the plan before the baker arrives";
        assert_eq!(masked(prose).hidden, 0);
    }

    #[test]
    fn cue_and_value() {
        for (text, want) in [
            ("the wifi password is Tr0ub4dor&3", format!("the wifi password is {HIDDEN}")),
            ("Password: hunter2!x", format!("Password: {HIDDEN}")),
            ("Username: bakery_orders\nPassword: k7{mq42R.wzT", format!("Username: bakery_orders\nPassword: {HIDDEN}")),
            ("pwd = \"Rye[2026]&more\"", format!("pwd = \"{HIDDEN}\"")),
            ("my password for the router is blue-kettle-forty-two", format!("my password for the router is {HIDDEN}")),
            ("DB_PASSWORD=p4ssw0rd-for-db", format!("DB_PASSWORD={HIDDEN}")),
            ("export API_KEY=\"k9Xp2mQ7vL4tR8wZ\"", format!("export API_KEY=\"{HIDDEN}\"")),
            ("the api key is 9f8a7b6c5d4e3f2a1b0c", format!("the api key is {HIDDEN}")),
            ("Authorization: Bearer k9Xp2mQ7vL4tR8wZa1", format!("Authorization: Bearer {HIDDEN}")),
            ("passphrase: \"seven green bottles 77\"", format!("passphrase: \"{HIDDEN}\"")),
            ("curl https://api.example.com/v1/items?token=k9Xp2mQ7vL4t&page=2", format!("curl https://api.example.com/v1/items?token={HIDDEN}")),
            ("postgres://baker:s3cretLoaf@db.example.com:5432/orders", format!("postgres://baker:{HIDDEN}@db.example.com:5432/orders")),
        ] {
            assert_eq!(masked(text).text, want, "{text}");
        }
    }

    #[test]
    fn talk_about_secrets_is_not_a_secret() {
        for text in [
            "the password is required for every login",
            "Your token is invalid or has expired.",
            "password: ********",
            "password: <your password here>",
            "let token = get_token(user.id)?;",
            "const apiKey = process.env.API_KEY;",
            "api_key: ${{ secrets.API_KEY }}",
            "password = input(\"Password: \")",
            "token: String,",
            "the secret is to chill the butter first",
            "token: user-provided",
            "set PASSWORD in the environment, then run the script",
            "The token count is 4096 and the window is 8192.",
            "password_min_length = 12",
            "the api key is in ~/.config/app/key.txt",
            "Reset your password at https://example.com/reset?step=2",
            "token = \"\"",
            "secret key: see the vault",
            "bearer of bad news again",
        ] {
            assert_eq!(masked(text).hidden, 0, "{text}");
        }
    }

    #[test]
    fn order_number_is_not_a_secret() {
        for text in [
            "Order ORD-2026-0045-8812 shipped on Monday.",
            "License key shape: ABCD-EFGH-1234-5678-WXYZ",
            "The invoice is INV20261124000123 for the flour.",
            "Part number 4X7-22B-9981-AAC",
            "id 3f2b8c1e-9d4a-4b7c-8e1f-2a6b5c4d3e2f in the log",
            "color #a1b2c3 and #FFAA00CC",
        ] {
            assert_eq!(masked(text).hidden, 0, "{text}");
        }
    }

    #[test]
    fn commit_id_in_code_is_not_a_secret() {
        for text in [
            "fixed in 0b977e9c4a1d2e3f4a5b6c7d8e9f0a1b2c3d4e5f",
            "sha256:9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
            "\"integrity\": \"sha512-Kq5sNclPz7QV2+lfQIuc6R7oRu0=\"",
            "see src-tauri/src/inference_server.rs:1485 and e2e/specs/models/memory-rules.e2e.ts",
            "pip install sk-learn-contrib-extensions-toolkit and scikit-image",
            "https://docs.example.com/document/d/1AbCdEfGhIjKlMnOpQrStUvWxYz0123456789abcd/edit",
            "call getUserAccountSettingsById2 then handleConversationImportAdopt3",
            "screenshot_2026-11-24_at_10.11.12_conversation_list.png",
            "the model is Qwen3.5-4B-Opus-Distilled-Q4_K_M.gguf on the card",
            "uhCAkJunoFoundingKeyForTheBakeryAssistant1 is its name here",
            "node_modules/@tauri-apps/plugin-store/dist-js/index.js",
        ] {
            assert_eq!(masked(text).hidden, 0, "{text}");
        }
    }

    #[test]
    fn an_address_in_the_persons_network_is_not_a_secret() {
        let address = format!("uhCAk{}", Rng(3).base62(48));
        assert_eq!(address.len(), 53);
        assert_eq!(masked(&format!("agent {address} signed it")).hidden, 0);
        // The same characters without that shape are a long mixed string.
        assert_eq!(masked(&format!("use {}", &address[5..])).hidden, 1);
    }

    #[test]
    fn only_the_secret_is_replaced() {
        let text = "Two things. First, the oven runs hot. Second, the supplier login: password is Fl0ur&Water99 (do not share). Then call Hartwell Mills.";
        let m = masked(text);
        assert_eq!(m.text, format!("Two things. First, the oven runs hot. Second, the supplier login: password is {HIDDEN} (do not share). Then call Hartwell Mills."));
        assert_eq!(m.hidden, 1);
        // Two secrets, both gone, the words between them kept.
        let m = masked("old password: Fl0ur&Water99 new password: Rye&Starter2026");
        assert_eq!(m.text, format!("old password: {HIDDEN} new password: {HIDDEN}"));
        assert_eq!(m.hidden, 2);
        // Nothing to hide: the text comes back as it was.
        let plain = "Rye needs an overnight starter.";
        assert_eq!(masked(plain), Masked { text: plain.to_string(), hidden: 0 });
        // Text that is not ASCII is cut only at character boundaries.
        let m = masked("Zoë's café: mot de passe password: Crème&Brûlée77 ✓ voilà");
        assert_eq!(m.text, format!("Zoë's café: mot de passe password: {HIDDEN} ✓ voilà"));
    }

    #[test]
    fn not_secret_override_holds() {
        let text = "the wifi password is Tr0ub4dor&3 and the api key is 9f8a7b6c5d4e3f2a1b0c";
        let not: HashSet<[u8; 32]> = [hash_of("Tr0ub4dor&3")].into();
        let m = mask(text, &MaskCtx { not_secret: Some(&not), ..Default::default() });
        assert_eq!(m.text, format!("the wifi password is Tr0ub4dor&3 and the api key is {HIDDEN}"));
        assert_eq!(m.hidden, 1);
        // The decision is kept as a hash: the set holds no readable string.
        assert!(!not.iter().any(|h| h.windows(4).any(|w| w == b"Tr0u")));
    }

    #[test]
    fn allow_once_is_once() {
        let text = "the wifi password is Tr0ub4dor&3";
        let once: HashSet<[u8; 32]> = [hash_of("Tr0ub4dor&3")].into();
        // The turn it is allowed for.
        assert_eq!(mask(text, &MaskCtx { allow_once: Some(&once), ..Default::default() }).hidden, 0);
        // Every other turn: the caller gives no set, and it is hidden again.
        assert_eq!(masked(text).hidden, 1);
    }

    #[test]
    fn a_string_the_person_calls_a_secret_is_masked() {
        let text = "the gate code is magpie and the alarm word is magpie too";
        assert_eq!(masked(text).hidden, 0);
        let m = mask(text, &MaskCtx { also_secret: &["magpie".to_string()], ..Default::default() });
        assert_eq!(m.text, format!("the gate code is {HIDDEN} and the alarm word is {HIDDEN} too"));
    }

    const SENTENCES: [&str; 8] = [
        "Here is the one for the delivery portal:",
        "I changed it this morning, it is now",
        "Can you check this is the right format?",
        "For the supplier site use",
        "Note for later, the new one:",
        "This is what the bank sent me:",
        "Put this in the config please:",
        "ok so",
    ];

    /// 200 made-up messages that each hold one secret, of every shape.
    fn secret_messages() -> Vec<(String, String)> {
        let mut r = Rng(20261124);
        let symbols = ["&", "!", "#", "@", "+", "*", "?"];
        let names = ["Flour", "Kettle", "Rye", "Crumb", "Oven", "Baker", "Loaf", "Starter"];
        let list: Vec<&str> = include_str!("secret_mask_words.txt").lines().filter(|w| !w.is_empty()).collect();
        (0..200)
            .map(|i| {
                let lead = r.pick(&SENTENCES).to_string();
                let (before, secret) = match i % 10 {
                    0 => ("password is ".to_string(), format!("{}{}{}{}", r.pick(&names), r.pick(&symbols), r.pick(&names), 10 + r.next() % 90)),
                    1 => ("API_KEY=".to_string(), r.base62(32)),
                    2 => (String::new(), format!("sk-ant-api03-{}", r.base62(48))),
                    3 => (String::new(), format!("ghp_{}", r.base62(36))),
                    4 => (String::new(), format!("eyJhbGciOiJIUzI1NiJ9.{}.{}", r.base62(36), r.base62(43))),
                    5 => (String::new(), (0..if i % 20 == 5 { 12 } else { 24 }).map(|_| r.pick(&list)).collect::<Vec<_>>().join(" ")),
                    6 => (String::new(), format!("-----BEGIN PRIVATE KEY-----\n{}\n{}\n-----END PRIVATE KEY-----", r.base62(64), r.base62(64))),
                    7 => ("the token: ".to_string(), r.string(b"abcdefghijklmnopqrstuvwxyz0123456789", 24)),
                    8 => (String::new(), r.base62(40)),
                    _ => ("passphrase: ".to_string(), format!("{}-{}-{}-{}", r.pick(&list), r.pick(&list), r.pick(&list), r.pick(&list))),
                };
                (format!("{lead} {before}{secret}\nThanks."), secret)
            })
            .collect()
    }

    #[test]
    fn two_hundred_made_up_secrets_are_all_masked() {
        let all = secret_messages();
        assert_eq!(all.len(), 200);
        let missed: Vec<&String> = all.iter().filter(|(text, secret)| masked(text).text.contains(secret.as_str())).map(|(t, _)| t).collect();
        assert!(missed.is_empty(), "{} of 200 not masked, the first: {:?}", missed.len(), missed.first());
        for (text, _) in &all {
            let m = masked(text);
            assert!(m.text.ends_with("\nThanks."), "the words after the secret are kept: {}", m.text);
        }
    }

    /// 2,000 made-up ordinary messages: code, logs, order numbers, ids, paths.
    fn ordinary_messages() -> Vec<String> {
        let mut r = Rng(42);
        let hex = b"0123456789abcdef";
        let words = ["conversation", "memory", "bakery", "delivery", "invoice", "supplier", "settings", "account", "import", "handler", "request", "window", "project", "session"];
        let exts = ["rs", "ts", "tsx", "json", "md", "png", "log", "toml"];
        (0..2000)
            .map(|i| {
                let w = |r: &mut Rng| r.pick(&words).to_string();
                let cap = |s: String| s[..1].to_uppercase() + &s[1..];
                match i % 20 {
                    0 => format!("commit {} fixes the {} list", r.string(hex, 40), w(&mut r)),
                    1 => format!("Order ORD-{}-{} for the {} arrived", 2020 + r.next() % 7, r.string(b"0123456789", 6), w(&mut r)),
                    2 => format!("see src/{}/{}_{}.{}:{}", w(&mut r), w(&mut r), w(&mut r), r.pick(&exts), r.next() % 3000),
                    3 => format!("https://example.com/{}/{}?page={}&sort={}", w(&mut r), r.base62(22), r.next() % 40, w(&mut r)),
                    4 => format!("request id {}-{}-{}-{}-{} took {} ms", r.string(hex, 8), r.string(hex, 4), r.string(hex, 4), r.string(hex, 4), r.string(hex, 12), r.next() % 900),
                    5 => format!("fn {}{}{}By{}(id: u64) -> Result<{}, String>", w(&mut r), cap(w(&mut r)), cap(w(&mut r)), cap(w(&mut r)), cap(w(&mut r))),
                    6 => format!("2026-11-{:02}T{:02}:{:02}:{:02}Z [info] {} loaded in {} ms", 1 + r.next() % 28, r.next() % 24, r.next() % 60, r.next() % 60, w(&mut r), r.next() % 5000),
                    7 => format!("sha256:{}", r.string(hex, 64)),
                    8 => format!("\"{}\": \"^{}.{}.{}\",", w(&mut r), r.next() % 20, r.next() % 30, r.next() % 30),
                    9 => format!("The {} password is required before the {} opens.", w(&mut r), w(&mut r)),
                    10 => format!("let {}_token = {}.{}_token()?;", w(&mut r), w(&mut r), w(&mut r)),
                    11 => format!("{}{}{}{}{}.{}", cap(w(&mut r)), cap(w(&mut r)), cap(w(&mut r)), 2020 + r.next() % 7, cap(w(&mut r)), r.pick(&exts)),
                    12 => format!("License ABCD-{}-{}-{}", r.string(b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789", 4), r.string(b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789", 4), r.string(b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789", 4)),
                    13 => format!("{} {} bags of flour at ${}.{:02} each, total ${}", r.next() % 90, w(&mut r), r.next() % 40, r.next() % 100, r.next() % 5000),
                    14 => format!("/home/baker/{}/{}/{}-{}-{}.{}", w(&mut r), w(&mut r), w(&mut r), 2020 + r.next() % 7, w(&mut r), r.pick(&exts)),
                    15 => format!("color: #{}; background: #{};", r.string(hex, 6), r.string(hex, 8)),
                    16 => format!("uhCAk{}{}{}{} joined the {} cell", cap(w(&mut r)), cap(w(&mut r)), cap(w(&mut r)), r.next() % 10, w(&mut r)),
                    17 => format!("Call {} on +61 4{} {} {} about the {}.", cap(w(&mut r)), r.string(b"0123456789", 2), r.string(b"0123456789", 3), r.string(b"0123456789", 3), w(&mut r)),
                    18 => format!("{}-{}-{}-v{}.{}.{}-x86_64-unknown-linux-gnu.tar.gz", w(&mut r), w(&mut r), w(&mut r), r.next() % 9, r.next() % 20, r.next() % 20),
                    _ => format!("Can you move the {} {} to Thursday and tell the {} it changed?", w(&mut r), w(&mut r), w(&mut r)),
                }
            })
            .collect()
    }

    #[test]
    fn at_most_one_in_a_hundred_ordinary_messages_is_touched() {
        let all = ordinary_messages();
        assert_eq!(all.len(), 2000);
        let touched: Vec<&String> = all.iter().filter(|t| masked(t).hidden > 0).collect();
        assert!(touched.len() <= 20, "{} of 2,000 masked; some: {:?}", touched.len(), &touched[..touched.len().min(8)]);
    }

    /// Counts only, over a history file given by path (never in the
    /// repository): how many messages hold something masked, and of what
    /// kind. `MASK_HISTORY=<file> cargo test --lib -- --ignored mask_counts`
    #[test]
    #[ignore]
    fn mask_counts_over_a_history_file() {
        let Ok(path) = std::env::var("MASK_HISTORY") else { return };
        let v: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        let (mut messages, mut flagged) = (0usize, 0usize);
        let mut kinds: std::collections::BTreeMap<String, usize> = Default::default();
        for c in v["conversations"].as_array().unwrap() {
            for m in c["messages"].as_array().unwrap() {
                messages += 1;
                let found = find(m["text"].as_str().unwrap_or(""), &[]);
                if !found.is_empty() {
                    flagged += 1;
                }
                for f in found {
                    let text = m["text"].as_str().unwrap_or("");
                    // Long strings by shape (how they start, how long), never the string.
                    let shape = if f.kind == Kind::LongString {
                        let t = &text[f.span.clone()];
                        format!("LongString {}{}", if t.starts_with("uhC") { "uhC.. " } else { "" }, t.len() / 10 * 10)
                    } else {
                        format!("{:?}", f.kind)
                    };
                    *kinds.entry(shape).or_default() += 1;
                }
            }
        }
        println!("MASK {flagged} of {messages} messages flagged ({:.2}%); by kind: {kinds:?}", flagged as f64 * 100.0 / messages as f64);
        // Known places, given as `[{ name, texts }]`: is something masked in each?
        if let Ok(known) = std::env::var("MASK_KNOWN") {
            let cases: serde_json::Value = serde_json::from_slice(&std::fs::read(&known).unwrap()).unwrap();
            for c in cases.as_array().unwrap() {
                let texts = c["texts"].as_array().unwrap();
                let hits = texts.iter().filter(|t| !find(t.as_str().unwrap_or(""), &[]).is_empty()).count();
                println!("MASK known {}: {} of {} messages around it hold something masked", c["name"], hits, texts.len());
            }
        }
    }
}
