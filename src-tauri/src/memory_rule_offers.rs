//! When a rule is offered.
//!
//! A rule is never made by itself. An instruction about how to work is
//! noted the first time it is given; the offer to keep it as a rule comes
//! the second time, in any conversation. An offer the person answered
//! (kept or declined) is not made again.
//!
//! What was heard and what was answered are system rows in memory's
//! scope, so the count is the same on every device. Sameness is measured,
//! not judged by a model: the two texts' vectors are compared.

use crate::memory_rows::MemoryRow;
use serde::{Deserialize, Serialize};

/// Two instructions are the same instruction at this closeness or more.
/// Measured on twenty made-up pairs: at 0.70 no different pair is offered
/// and eight of ten repeated ones are; no value gives ten and none.
pub(crate) const SAME: f32 = 0.70;
/// This many heard instructions are held; the oldest go first.
const HELD: usize = 200;

/// An instruction that was heard, or an offer that was answered.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub(crate) struct Heard {
    pub text: String,
    /// The text's vector, one signed byte a dimension.
    pub vec: String,
    pub at: i64,
    /// The conversation it was said in.
    #[serde(default)]
    pub conversation: String,
    /// `kept` | `declined`, on an answered offer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
}

pub(crate) fn pack(v: &[f32]) -> String {
    use base64::Engine;
    let top = v.iter().fold(0f32, |m, x| m.max(x.abs())).max(f32::MIN_POSITIVE);
    let bytes: Vec<u8> = v.iter().map(|x| ((x / top) * 127.0).round().clamp(-127.0, 127.0) as i8 as u8).collect();
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub(crate) fn unpack(s: &str) -> Vec<f32> {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.decode(s).unwrap_or_default().into_iter().map(|b| b as i8 as f32).collect()
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na.sqrt() * nb.sqrt()) }
}

fn id_of(kind: &str, text: &str) -> String {
    use sha2::{Digest, Sha256};
    let plain = text.trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ");
    format!("sys-rule-{kind}-{}", hex::encode(&Sha256::digest(plain.as_bytes())[..10]))
}

fn row(kind: &str, sys: &str, h: &Heard) -> MemoryRow {
    MemoryRow {
        id: id_of(kind, &h.text),
        structured: Some(serde_json::json!({ "sys": sys, "heard": h })),
        ..crate::memory_tidy::ledger_row(&crate::memory_tidy::Judged { pair: (String::new(), String::new()), outcome: String::new(), at: h.at, of: None })
    }
}

/// The row for an instruction heard.
pub(crate) fn heard_row(h: &Heard) -> MemoryRow {
    row("heard", "rule_heard", h)
}

/// The row for an offer the person answered.
pub(crate) fn answered_row(h: &Heard) -> MemoryRow {
    row("answered", "rule_answered", h)
}

fn read(rows: &[MemoryRow], sys: &str) -> Vec<Heard> {
    let mut out: Vec<Heard> = rows
        .iter()
        .filter(|r| !r.deleted)
        .filter_map(|r| {
            let s = r.structured.as_ref()?;
            (s.get("sys")?.as_str()? == sys).then(|| serde_json::from_value(s.get("heard")?.clone()).ok()).flatten()
        })
        .collect();
    out.sort_by(|a, b| b.at.cmp(&a.at));
    out
}

pub(crate) fn heard(rows: &[MemoryRow]) -> Vec<Heard> {
    read(rows, "rule_heard")
}

pub(crate) fn answered(rows: &[MemoryRow]) -> Vec<Heard> {
    read(rows, "rule_answered")
}

/// What to do with an instruction just given.
#[derive(Debug, Clone, PartialEq)]
pub(crate) enum Verdict {
    /// The first time: noted, nothing offered.
    First,
    /// Given before: offer to keep it as a rule. Carries the earlier words.
    Offer { earlier: String },
    /// An offer for this was answered already; nothing is offered.
    Answered,
    /// It is a rule already; nothing is offered.
    IsARule,
}

fn plain(text: &str) -> String {
    text.trim().trim_end_matches(['.', '!']).to_lowercase().split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The decision, from what was heard and answered before. `from` is the
/// message's own place (its conversation and time), so a message read a
/// second time (a reload, another device) is not its own earlier asking.
pub(crate) fn decide(text: &str, vec: &[f32], from: (&str, i64), heard: &[Heard], answered: &[Heard], rules: &[String]) -> Verdict {
    let me = plain(text);
    if rules.iter().any(|r| plain(r) == me) {
        return Verdict::IsARule;
    }
    let same = |h: &Heard| plain(&h.text) == me || cosine(vec, &unpack(&h.vec)) >= SAME;
    if answered.iter().any(same) {
        return Verdict::Answered;
    }
    let itself = |h: &Heard| h.conversation == from.0 && h.at == from.1;
    match heard.iter().filter(|h| !itself(h)).find(|h| same(h)) {
        Some(h) => Verdict::Offer { earlier: h.text.clone() },
        None => Verdict::First,
    }
}

/// How close the nearest instruction heard before is, the message itself left out.
pub(crate) fn closest(vec: &[f32], from: (&str, i64), heard: &[Heard]) -> Option<f32> {
    heard.iter().filter(|h| !(h.conversation == from.0 && h.at == from.1)).map(|h| cosine(vec, &unpack(&h.vec))).fold(None, |m: Option<f32>, c| Some(m.map_or(c, |m| m.max(c))))
}

/// The heard rows that no longer fit, as rows marked deleted.
fn over(rows: &[MemoryRow], now: i64) -> Vec<MemoryRow> {
    let mut held: Vec<&MemoryRow> = rows.iter().filter(|r| !r.deleted && r.structured.as_ref().and_then(|s| s.get("sys")).and_then(|s| s.as_str()) == Some("rule_heard")).collect();
    held.sort_by(|a, b| b.as_of.cmp(&a.as_of));
    held.into_iter().skip(HELD).map(|r| MemoryRow { deleted: true, updated_at: now, ..r.clone() }).collect()
}

fn now_ms() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as i64).unwrap_or(0)
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "camelCase")]
pub struct HeardInput {
    /// The instruction, in the words it was given.
    pub text: String,
    #[serde(default)]
    pub conversation: Option<String>,
    /// When the message was sent (ms).
    #[serde(default)]
    pub at: Option<i64>,
    /// The person's rules and the rules in view (this AI's, this project's).
    #[serde(default)]
    pub rules: Vec<String>,
}

#[derive(Serialize, Debug, Default)]
#[serde(rename_all = "camelCase")]
pub struct HeardAnswer {
    /// Offer to keep it as a rule.
    pub offer: bool,
    /// The earlier words, when it was given before.
    pub earlier: Option<String>,
    /// `first` | `offer` | `answered` | `is_a_rule` | `not_measured` | `a_control`.
    pub why: &'static str,
    /// How close the nearest instruction heard before is (1 = the same).
    pub closest: Option<f32>,
}

/// An instruction was given: note it, and say whether this is the second
/// time. Nothing is offered when the text cannot be measured.
#[tauri::command]
pub async fn memory_rule_heard(app: tauri::AppHandle, input: HeardInput) -> Result<HeardAnswer, String> {
    use tauri::Manager;
    let text = input.text.trim().to_string();
    if text.is_empty() || text.len() > 400 {
        return Ok(HeardAnswer { why: "not_measured", ..Default::default() });
    }
    // Words the app acts on by a fixed rule ("that's done", "forget
    // that", "keep this conversation out of memory") are not instructions
    // for how to answer.
    if crate::memory_items::said_control(&text).is_some() || crate::memory_items::said_done(&text) {
        return Ok(HeardAnswer { why: "a_control", ..Default::default() });
    }
    let key = app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get()?.data_key()?;
    // The turn does not wait for this, so it can wait for the part that
    // measures text: just after launch that part may still be starting.
    let mut vec = None;
    for attempt in 0..4 {
        if attempt > 0 {
            tokio::time::sleep(std::time::Duration::from_secs(6)).await;
        }
        vec = crate::inference_memory::embed_question(&app, &text).await;
        if vec.is_some() {
            break;
        }
    }
    let Some(vec) = vec else {
        log::info!("[rules] an instruction could not be measured; nothing noted");
        return Ok(HeardAnswer { why: "not_measured", ..Default::default() });
    };
    let rows = crate::memory_rows::read_cache(&app, &key, crate::memory_items::SCOPE);
    let conversation = input.conversation.unwrap_or_default();
    let at = input.at.unwrap_or_else(now_ms);
    let verdict = decide(&text, &vec, (&conversation, at), &heard(&rows), &answered(&rows), &input.rules);
    let near = closest(&vec, (&conversation, at), &heard(&rows));
    if matches!(verdict, Verdict::First | Verdict::Offer { .. }) {
        let h = Heard { text: text.clone(), vec: pack(&vec), at, conversation, answer: None };
        crate::memory_rows::save_and_cache(&app, heard_row(&h)).await?;
        for gone in over(&crate::memory_rows::read_cache(&app, &key, crate::memory_items::SCOPE), now_ms()) {
            let _ = crate::memory_rows::save_and_cache(&app, gone).await;
        }
    }
    log::info!("[rules] an instruction heard: {}, nearest before {}", match &verdict { Verdict::First => "first time", Verdict::Offer { .. } => "given before, offered", Verdict::Answered => "an offer was answered before", Verdict::IsARule => "a rule already" }, near.map_or("none".to_string(), |n| format!("{n:.3}")));
    Ok(match verdict {
        Verdict::First => HeardAnswer { why: "first", closest: near, ..Default::default() },
        Verdict::Offer { earlier } => HeardAnswer { offer: true, earlier: Some(earlier), why: "offer", closest: near },
        Verdict::Answered => HeardAnswer { why: "answered", closest: near, ..Default::default() },
        Verdict::IsARule => HeardAnswer { why: "is_a_rule", closest: near, ..Default::default() },
    })
}

/// The person answered an offer (`kept` or `declined`): it is not made again.
#[tauri::command]
pub async fn memory_rule_offer_answered(app: tauri::AppHandle, text: String, answer: String) -> Result<(), String> {
    if answer != "kept" && answer != "declined" {
        return Err("unknown answer".into());
    }
    let text = text.trim().to_string();
    let vec = crate::inference_memory::embed_question(&app, &text).await.unwrap_or_default();
    let h = Heard { text, vec: pack(&vec), at: now_ms(), conversation: String::new(), answer: Some(answer) };
    crate::memory_rows::save_and_cache(&app, answered_row(&h)).await.map(|_| ())
}

/// The offers the person declined, newest first, for the Rules board.
#[tauri::command]
pub fn memory_rule_offers_declined(app: tauri::AppHandle) -> Result<Vec<String>, String> {
    use tauri::Manager;
    let key = app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get()?.data_key()?;
    let rows = crate::memory_rows::read_cache(&app, &key, crate::memory_items::SCOPE);
    Ok(answered(&rows).into_iter().filter(|h| h.answer.as_deref() == Some("declined")).map(|h| h.text).collect())
}

/// "Offer it again": a declined offer is taken back.
#[tauri::command]
pub async fn memory_rule_offer_forget(app: tauri::AppHandle, text: String) -> Result<(), String> {
    use tauri::Manager;
    let key = app.state::<std::sync::Arc<crate::commands_holochain::HolochainState>>().get()?.data_key()?;
    let id = id_of("answered", &text);
    let rows = crate::memory_rows::read_cache(&app, &key, crate::memory_items::SCOPE);
    if let Some(r) = rows.iter().find(|r| r.id == id && !r.deleted) {
        crate::memory_rows::save_and_cache(&app, MemoryRow { deleted: true, updated_at: now_ms(), ..r.clone() }).await?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(a: f32, b: f32, c: f32) -> Vec<f32> {
        vec![a, b, c]
    }
    fn h(text: &str, vec: &[f32], at: i64, conversation: &str) -> Heard {
        Heard { text: text.into(), vec: pack(vec), at, conversation: conversation.into(), answer: None }
    }

    #[test]
    fn first_time_is_not_an_offer() {
        assert_eq!(decide("Always write dates as 3 March 2026.", &v(1.0, 0.0, 0.0), ("c1", 10), &[], &[], &[]), Verdict::First);
    }

    #[test]
    fn second_time_offers() {
        let before = vec![h("Always write dates as 3 March 2026.", &v(1.0, 0.0, 0.0), 10, "c1")];
        // Other words, the same instruction: close in meaning.
        let got = decide("Please write the date like 3 March 2026 from now on.", &v(0.95, 0.2, 0.0), ("c2", 99), &before, &[], &[]);
        assert_eq!(got, Verdict::Offer { earlier: "Always write dates as 3 March 2026.".into() });
        // Another instruction is a first time of its own.
        assert_eq!(decide("Never use tables.", &v(0.0, 1.0, 0.0), ("c2", 120), &before, &[], &[]), Verdict::First);
    }

    #[test]
    fn a_message_read_again_is_not_its_own_second_time() {
        let before = vec![h("Always write dates as 3 March 2026.", &v(1.0, 0.0, 0.0), 10, "c1")];
        assert_eq!(decide("Always write dates as 3 March 2026.", &v(1.0, 0.0, 0.0), ("c1", 10), &before, &[], &[]), Verdict::First);
        // The same words later in the same conversation are a second time.
        assert!(matches!(decide("Always write dates as 3 March 2026.", &v(1.0, 0.0, 0.0), ("c1", 50), &before, &[], &[]), Verdict::Offer { .. }));
    }

    #[test]
    fn declined_stays_declined() {
        let before = vec![h("Always write dates as 3 March 2026.", &v(1.0, 0.0, 0.0), 10, "c1"), h("Write dates as 3 March 2026.", &v(1.0, 0.0, 0.0), 20, "c2")];
        let mut no = h("Write dates as 3 March 2026.", &v(1.0, 0.0, 0.0), 21, "");
        no.answer = Some("declined".into());
        assert_eq!(decide("Dates as 3 March 2026, always.", &v(0.97, 0.1, 0.0), ("c3", 30), &before, &[no], &[]), Verdict::Answered);
    }

    #[test]
    fn what_is_a_rule_already_is_not_offered() {
        let before = vec![h("Never use tables.", &v(0.0, 1.0, 0.0), 10, "c1")];
        assert_eq!(decide("never use tables", &v(0.0, 1.0, 0.0), ("c2", 20), &before, &[], &["Never use tables.".into()]), Verdict::IsARule);
    }

    #[test]
    fn a_vector_survives_being_packed() {
        let a = vec![0.12f32, -0.5, 0.33, 0.0, 0.81];
        let b = unpack(&pack(&a));
        assert!(cosine(&a, &b) > 0.999, "{}", cosine(&a, &b));
        assert_eq!(cosine(&a, &[]), 0.0);
    }

    #[test]
    fn heard_and_answered_are_read_from_the_rows() {
        let one = h("Never use tables.", &v(0.0, 1.0, 0.0), 10, "c1");
        let mut two = h("Use short replies.", &v(1.0, 0.0, 0.0), 20, "");
        two.answer = Some("kept".into());
        let rows = vec![heard_row(&one), answered_row(&two)];
        assert_eq!(heard(&rows), vec![one.clone()]);
        assert_eq!(answered(&rows), vec![two]);
        assert_eq!(heard_row(&one).id, heard_row(&h("never use  tables.", &v(0.0, 1.0, 0.0), 30, "c9")).id, "the same words are one row");
        assert_ne!(heard_row(&one).id, answered_row(&one).id);
    }

    #[test]
    fn only_so_many_heard_instructions_are_held() {
        let rows: Vec<MemoryRow> = (0..HELD as i64 + 3).map(|i| heard_row(&h(&format!("Always do thing {i}."), &v(1.0, 0.0, 0.0), i, "c"))).collect();
        let gone = over(&rows, 999);
        assert_eq!(gone.len(), 3);
        assert!(gone.iter().all(|r| r.deleted && r.as_of < 3));
    }
}
