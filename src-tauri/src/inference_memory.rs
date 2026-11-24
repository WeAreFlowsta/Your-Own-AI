//! The local port's part in an AI's index of earlier turns: a turn that
//! arrives over the port is indexed as a turn in the app is, and a question
//! is embedded for retrieval. What memory adds to a request is built in
//! `memory_prompt`, for every path.

use std::sync::Arc;
use std::time::Duration;
use tauri::{AppHandle, Manager};

use crate::commands_holochain::HolochainState;
use crate::transcript_memory::TranscriptEmbedding;

use crate::embedding_model::{FILE as EMBED_MODEL, QUERY_PREFIX as QUERY_INSTRUCTION};
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

/// The question embedded for retrieval, bounded so a cold or stuck embed
/// server can't hang the reply (the first call after launch may lose the
/// race; that's fine).
pub async fn embed_question(app: &AppHandle, query: &str) -> Option<Vec<f32>> {
    let llm_state = app.state::<crate::llm::LLMState>();
    let qtext = format!("{QUERY_INSTRUCTION}{query}");
    match tokio::time::timeout(
        Duration::from_secs(8),
        crate::llm::embed_texts(app.clone(), llm_state, vec![qtext], EMBED_MODEL.to_string()),
    )
    .await
    {
        Ok(Ok(mut v)) => v.drain(..).next().filter(|x| !x.is_empty()),
        _ => None,
    }
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
