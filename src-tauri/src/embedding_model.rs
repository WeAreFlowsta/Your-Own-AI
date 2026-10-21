//! The ONE place that knows which embedding model the app runs and how it
//! wants to be spoken to. Mirrors `EMBEDDING_MODEL` in
//! src/data/recommended-models.ts - change both together.
//!
//! EmbeddingGemma-300M (2026-10-12, replacing bge-small-en-v1.5): chosen by
//! the embedding-model matrix (the embed-matrix tool,
//! results archive/embed-matrix-2026-10-12) - the tools gate 1 wrong of 84
//! held-out messages (bge 9), health and rule gates in six languages from
//! English examples (AUC 0.99 / 0.98; bge 0.76 / 0.69), document recall
//! 12/12 in English, Spanish and German (bge 12 / 8 / 4). 334 MB, 768-dim,
//! mean pooling from the file's own metadata, ~100 ms per query on a CPU.
//! It is trained with task prefixes: queries, documents and "things a
//! person says" each get their own. Every constant below carries to the
//! thresholds that read these scores - the matrix recalibrated them all.

/// The file the memory component downloads (Settings -> Components, and
/// the first download in the welcome flow).
pub const FILE: &str = "embeddinggemma-300M-Q8_0.gguf";
/// Earlier memory components: their indexes are rebuilt, the files reclaimed.
pub const PREVIOUS_FILES: &[&str] = &["bge-small-en-v1.5-f16.gguf"];
/// A search query (recall against stored passages, the router's reference
/// phrases and the message scored against them).
pub const QUERY_PREFIX: &str = "task: search result | query: ";
/// A stored passage: a document chunk, a transcript turn kept for recall,
/// a tool's description.
pub const DOC_PREFIX: &str = "title: none | text: ";
/// Something a person says, scored against example sayings (the tools
/// gate's contrast): classification, not retrieval.
pub const SAID_PREFIX: &str = "task: classification | query: ";
/// `--pooling` for llama-server; None = the file's own metadata decides
/// (EmbeddingGemma declares mean pooling; bge needed `cls` spelled out).
pub const POOLING: Option<&str> = None;
/// Vector length - a stored vector of another length belongs to another
/// model and scores 0, never garbage (see `cosine` guards).
pub const DIMS: usize = 768;

/// The marker that records which embedding model the on-disk indexes
/// (per-AI transcript vectors, library passages) were built with. A
/// different model in the catalog than in the marker = every index is
/// rebuilt once (transcriptMemory's walker + `corpus::reembed_all_passages`).
const INDEX_MARKER_FILE: &str = "embedding-index-model";

fn marker_path(app: &tauri::AppHandle) -> Option<std::path::PathBuf> {
    crate::vault_escrow::reembed_pending_path(app).and_then(|p| p.parent().map(|d| d.join(INDEX_MARKER_FILE)))
}

/// The indexes were built with another model (or an unknown one while
/// indexes exist) - rebuild before trusting a single score. A fresh install
/// with nothing indexed is marked current on the spot.
#[tauri::command]
pub fn embedding_index_stale(app: tauri::AppHandle) -> bool {
    let Some(p) = marker_path(&app) else { return false };
    match std::fs::read_to_string(&p) {
        Ok(s) if s.trim() == FILE => false,
        Ok(_) => true,
        Err(_) => {
            let has_index = crate::transcript_memory::any_index_exists(&app) || crate::corpus::any_passages(&app);
            if !has_index {
                let _ = std::fs::write(&p, FILE);
            }
            has_index
        }
    }
}

/// Called once every index has been rebuilt with the current model.
#[tauri::command]
pub fn embedding_index_mark_current(app: tauri::AppHandle) -> Result<(), String> {
    let p = marker_path(&app).ok_or("no data dir")?;
    std::fs::write(&p, FILE).map_err(|e| e.to_string())
}

/// Expose the restore marker so the model-change path can reuse the
/// transcript walker (`rebuildMemoryIndexIfPending`) unchanged.
#[tauri::command]
pub fn memory_mark_reembed_pending(app: tauri::AppHandle) {
    crate::vault_escrow::set_reembed_pending(&app);
}

#[cfg(test)]
mod tests {
    #[test]
    fn prefixes_are_the_models_own() {
        // EmbeddingGemma's documented task prefixes - a swap changes these with FILE.
        assert!(super::QUERY_PREFIX.starts_with("task: "));
        assert!(super::DOC_PREFIX.starts_with("title: "));
        assert!(super::SAID_PREFIX.starts_with("task: classification"));
        assert!(super::PREVIOUS_FILES.contains(&"bge-small-en-v1.5-f16.gguf"));
    }
}
