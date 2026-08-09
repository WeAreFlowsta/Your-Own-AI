//! Backend memory assembly for the inference server.
//!
//! A faithful port of the frontend `loadMemoryBlock` (`src/utils/memory.ts`) +
//! `recallPerAi` (`src/utils/transcriptMemory.ts`), so AIs served over the HTTP
//! API recall the user the same way the in-app chat does. Reuses the existing
//! Rust primitives — `get_memory_facts`, `get_transcript_embeddings`,
//! `embed_texts` — and never fails: any error degrades to the name-unknown
//! line so a request is never blocked by memory.
//!
//! Keep the thresholds + block wording in sync with the TS originals.

use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};

use crate::commands_holochain::HolochainState;
use crate::memory::MemoryFact;
use crate::transcript_memory::TranscriptEmbedding;

const EMBED_MODEL: &str = "bge-small-en-v1.5-f16.gguf";
/// bge-small wants the query (not docs) wrapped in this retrieval instruction.
const QUERY_INSTRUCTION: &str = "Represent this sentence for searching relevant passages: ";
const RECALL_THRESHOLD: f32 = 0.45;
const RECALL_MARGIN: f32 = 0.12;
const MAX_RECALL: usize = 3;
const MAX_NOTES: usize = 3;
const FACT_BUDGET: usize = 25;
const MAX_PER_AI: usize = 1000;
const MAX_TEXT: usize = 600;

const QUESTION_OPENERS: &[&str] = &[
    "what", "where", "when", "why", "who", "which", "whose", "whom", "how",
];
const QUESTION_AUX: &[&str] = &[
    "is", "are", "was", "were", "do", "does", "did", "can", "could", "will", "would", "should",
    "shall", "have", "has", "had", "am", "may", "might",
];

fn now_micros() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_micros() as i64)
        .unwrap_or(0)
}

fn norm(s: &str) -> String {
    s.trim().to_lowercase()
}

/// EXACT set, not a substring match (mirrors `isNamePredicate`).
fn is_name_predicate(p: &str) -> bool {
    matches!(
        norm(p).as_str(),
        "name_is" | "name" | "full_name" | "first_name" | "goes_by" | "has_name" | "is_named"
    )
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0.0f32, 0.0f32, 0.0f32);
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    if na == 0.0 || nb == 0.0 {
        return 0.0;
    }
    dot / (na.sqrt() * nb.sqrt())
}

/// Is this a "pure" question (a recall query), not a statement worth
/// remembering? Mirrors `isPureQuestion`.
fn is_pure_question(text: &str) -> bool {
    let cleaned: String = text
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphabetic() || c == '\'' { c } else { ' ' })
        .collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    if words.is_empty() {
        return false;
    }
    let first = words[0].trim_end_matches("'s");
    let opens_wh = QUESTION_OPENERS
        .iter()
        .any(|w| first == *w || first == format!("{w}s"));
    if !opens_wh {
        return false;
    }
    text.trim().ends_with('?') || words.get(1).map_or(false, |w| QUESTION_AUX.contains(w))
}

/// Index this turn's user text into the AI's episodic recall store (mirrors
/// `indexTurn`): user-only, skip pure questions, embed raw, append, cap.
/// Best-effort fire-and-forget.
pub async fn index_turn(app: &AppHandle, ai_id: &str, conversation_hash: &str, user_text: &str) {
    let u = user_text.trim();
    if ai_id.is_empty() || conversation_hash.is_empty() || u.is_empty() || is_pure_question(u) {
        return;
    }
    let text: String = u.chars().take(MAX_TEXT).collect();

    // Embed as a raw document (no query instruction), bounded.
    let llm_state = app.state::<crate::llm::LLMState>();
    let vector: Vec<f32> = match tokio::time::timeout(
        Duration::from_secs(8),
        crate::llm::embed_texts(app.clone(), llm_state, vec![text.clone()], EMBED_MODEL.to_string()),
    )
    .await
    {
        Ok(Ok(mut v)) => match v.drain(..).next() {
            Some(x) if !x.is_empty() => x,
            _ => return,
        },
        _ => return,
    };

    let hc_state = app.state::<Arc<HolochainState>>();
    let mut existing =
        crate::transcript_memory::get_transcript_embeddings(app.clone(), ai_id.to_string(), hc_state)
            .unwrap_or_default();
    existing.push(TranscriptEmbedding {
        id: format!("api-{}-{}", now_micros(), existing.len()),
        conversation_hash: conversation_hash.to_string(),
        role: "user".to_string(),
        text,
        vector,
        kind: "episodic".to_string(),
        created_at: now_micros(),
        source: None,
    });
    cap_episodic(&mut existing);
    let hc_state2 = app.state::<Arc<HolochainState>>();
    let _ = crate::transcript_memory::save_transcript_embeddings(
        app.clone(),
        ai_id.to_string(),
        existing,
        hc_state2,
    );
}

/// Drop the oldest EPISODIC entries over the cap. Authored knowledge
/// (documents, lore the user gave the AI) is deliberately exempt: the user
/// placed it there, so chat volume must never evict it. Mirrors the same
/// rule in `transcriptMemory.ts` `indexTurn`.
fn cap_episodic(existing: &mut Vec<TranscriptEmbedding>) {
    let episodic_count = existing.iter().filter(|e| e.kind != "authored").count();
    if episodic_count <= MAX_PER_AI {
        return;
    }
    let mut to_drop = episodic_count - MAX_PER_AI;
    existing.retain(|e| {
        if to_drop > 0 && e.kind != "authored" {
            to_drop -= 1;
            false
        } else {
            true
        }
    });
}

/// Top relevant entries: absolute floor + a relative margin off the best match
/// (mirrors `topMatches`).
fn top_matches(mut scored: Vec<(&TranscriptEmbedding, f32)>) -> Vec<&str> {
    scored.retain(|(_, s)| *s >= RECALL_THRESHOLD);
    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    if scored.is_empty() {
        return vec![];
    }
    let floor = RECALL_THRESHOLD.max(scored[0].1 - RECALL_MARGIN);
    scored
        .into_iter()
        .filter(|(_, s)| *s >= floor)
        .take(MAX_RECALL)
        .map(|(e, _)| e.text.as_str())
        .collect()
}

/// Build the memory block injected into the persona's `{{userNameInfo}}` slot.
/// Returns at minimum the name line, so it's a drop-in replacement for that
/// slot. `query` is this turn's user text; `ai_id` the resolved AI's store id;
/// `exclude_conv` the current conversation hash to skip (None over the API).
pub async fn build_memory_block(
    app: &AppHandle,
    query: &str,
    ai_id: &str,
    exclude_conv: Option<&str>,
) -> String {
    let hc_state = app.state::<Arc<HolochainState>>();
    let facts: Vec<MemoryFact> =
        crate::memory::get_memory_facts(app.clone(), hc_state).unwrap_or_default();

    // Always-injected = active, non-note facts, ranked by confidence then
    // recency, budgeted.
    let mut items: Vec<&MemoryFact> = facts
        .iter()
        .filter(|f| f.valid_to.is_none() && f.entry_kind != "note")
        .collect();
    items.sort_by(|a, b| {
        b.confidence
            .partial_cmp(&a.confidence)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(b.updated_at.cmp(&a.updated_at))
    });
    items.truncate(FACT_BUDGET);

    let name = items.iter().find(|f| is_name_predicate(&f.predicate)).copied();
    let mut block = match &name {
        Some(f) => format!("Their name is {}.", f.value),
        None => "Their name is unknown. Do not assume or allude to it.".to_string(),
    };

    let name_id = name.map(|f| f.id.as_str());
    let others: Vec<&MemoryFact> = items
        .iter()
        .copied()
        .filter(|f| Some(f.id.as_str()) != name_id)
        .collect();
    if !others.is_empty() {
        let lines = others
            .iter()
            .map(|f| format!("- {}: {}", f.predicate.replace('_', " "), f.value))
            .collect::<Vec<_>>()
            .join("\n");
        block.push_str(&format!(
            "\nThings you've learned about them from past conversations (use naturally where relevant; do not recite this list or claim certainty):\n{lines}"
        ));
    }

    if query.trim().is_empty() {
        return block;
    }

    // Retrieval: relevant notes (user-level) + this AI's relevant memory.
    let notes: Vec<&MemoryFact> = facts
        .iter()
        .filter(|f| {
            f.valid_to.is_none()
                && f.entry_kind == "note"
                && f.embedding.as_ref().map_or(false, |v| !v.is_empty())
        })
        .collect();
    let want_per_ai = !ai_id.is_empty();
    if notes.is_empty() && !want_per_ai {
        return block;
    }

    // Embed the query ONCE, bounded so a cold/stuck embed server can't hang the
    // reply (the first call after launch may lose the race; that's fine).
    let llm_state = app.state::<crate::llm::LLMState>();
    let qtext = format!("{QUERY_INSTRUCTION}{query}");
    let qvec: Option<Vec<f32>> = match tokio::time::timeout(
        Duration::from_secs(8),
        crate::llm::embed_texts(app.clone(), llm_state, vec![qtext], EMBED_MODEL.to_string()),
    )
    .await
    {
        Ok(Ok(mut v)) => v.drain(..).next(),
        _ => None,
    };
    let Some(qvec) = qvec else {
        return block;
    };

    // Relevant saved notes (shared across all AIs): threshold + top-N, no margin.
    if !notes.is_empty() {
        let mut scored: Vec<(&MemoryFact, f32)> = notes
            .iter()
            .map(|n| (*n, cosine(&qvec, n.embedding.as_ref().unwrap())))
            .filter(|(_, s)| *s >= RECALL_THRESHOLD)
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        let picked: Vec<&str> = scored
            .into_iter()
            .take(MAX_NOTES)
            .map(|(n, _)| n.value.as_str())
            .collect();
        if !picked.is_empty() {
            let lines = picked.iter().map(|v| format!("- {v}")).collect::<Vec<_>>().join("\n");
            block.push_str(&format!(
                "\nRelevant notes they've saved (use naturally if helpful):\n{lines}"
            ));
        }
    }

    // This AI's own relevant memory: authored knowledge + past conversation
    // turns (excluding the current one).
    if want_per_ai {
        let hc_state2 = app.state::<Arc<HolochainState>>();
        let emb: Vec<TranscriptEmbedding> =
            crate::transcript_memory::get_transcript_embeddings(app.clone(), ai_id.to_string(), hc_state2)
                .unwrap_or_default();

        let episodic = top_matches(
            emb.iter()
                .filter(|e| {
                    e.kind != "authored"
                        && exclude_conv.map_or(true, |c| e.conversation_hash != c)
                })
                .map(|e| (e, cosine(&qvec, &e.vector)))
                .collect(),
        );
        let authored = top_matches(
            emb.iter()
                .filter(|e| e.kind == "authored")
                .map(|e| (e, cosine(&qvec, &e.vector)))
                .collect(),
        );

        if !authored.is_empty() {
            let lines = authored.iter().map(|t| format!("- {t}")).collect::<Vec<_>>().join("\n");
            block.push_str(&format!(
                "\nKnowledge this AI has been given (use if relevant):\n{lines}"
            ));
        }
        if !episodic.is_empty() {
            let lines = episodic.iter().map(|t| format!("- {t}")).collect::<Vec<_>>().join("\n");
            block.push_str(&format!(
                "\nFrom earlier conversations with this person (use naturally if relevant):\n{lines}"
            ));
        }
    }

    block
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(kind: &str, id: usize) -> TranscriptEmbedding {
        TranscriptEmbedding {
            id: id.to_string(),
            conversation_hash: "c".into(),
            role: "user".into(),
            text: "t".into(),
            vector: vec![],
            kind: kind.into(),
            created_at: id as i64,
            source: None,
        }
    }

    #[test]
    fn cap_drops_oldest_episodic_only() {
        // 2 authored at the FRONT (oldest) + MAX_PER_AI + 3 episodic.
        let mut all: Vec<TranscriptEmbedding> = vec![entry("authored", 0), entry("authored", 1)];
        all.extend((2..MAX_PER_AI + 5).map(|i| entry("episodic", i)));
        cap_episodic(&mut all);
        let authored = all.iter().filter(|e| e.kind == "authored").count();
        let episodic = all.iter().filter(|e| e.kind != "authored").count();
        assert_eq!(authored, 2, "authored knowledge must never be evicted");
        assert_eq!(episodic, MAX_PER_AI);
        // The dropped entries are the OLDEST episodic ones (ids 2, 3, 4).
        assert!(!all.iter().any(|e| e.id == "2" || e.id == "3" || e.id == "4"));
        assert!(all.iter().any(|e| e.id == (MAX_PER_AI + 4).to_string()));
    }

    #[test]
    fn cap_noop_under_limit() {
        let mut all: Vec<TranscriptEmbedding> =
            (0..10).map(|i| entry(if i % 2 == 0 { "episodic" } else { "authored" }, i)).collect();
        cap_episodic(&mut all);
        assert_eq!(all.len(), 10);
    }
}
