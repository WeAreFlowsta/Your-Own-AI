//! Embedding a question for memory, from the backend's own paths (the
//! local port, an agent's step). What memory adds to a request is built in
//! `memory_prompt`, for every path.

use std::time::Duration;
use tauri::{AppHandle, Manager};

use crate::embedding_model::{FILE as EMBED_MODEL, QUERY_PREFIX as QUERY_INSTRUCTION};

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

/// Is the file of the part that searches in the models folder? A folder
/// that cannot be read counts as here: the cause is then another one.
pub(crate) fn search_part_is_here(app: &AppHandle) -> bool {
    crate::llm::get_models_dir(app).map(|d| d.join(EMBED_MODEL).exists()).unwrap_or(true)
}

/// The question embedded for memory's search, with the cause when it could
/// not be: "did not look" is told apart from "looked and found nothing".
pub async fn embed_for_search(app: &AppHandle, query: &str) -> Result<Vec<f32>, crate::memory_block::WhyNot> {
    use crate::memory_block::WhyNot;
    let llm_state = app.state::<crate::llm::LLMState>();
    let qtext = format!("{QUERY_INSTRUCTION}{query}");
    match tokio::time::timeout(Duration::from_secs(15), crate::llm::embed_texts(app.clone(), llm_state, vec![qtext], EMBED_MODEL.to_string())).await {
        Err(_) => Err(WhyNot::TimedOut),
        Ok(Err(e)) => Err(WhyNot::from_error(&e)),
        Ok(Ok(mut v)) => v.drain(..).next().filter(|x| !x.is_empty()).ok_or(WhyNot::DidNotStartInTime),
    }
}
