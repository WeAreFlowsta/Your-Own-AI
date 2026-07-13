/**
 * Thin wrapper over the Rust `embed_texts` command (a second llama-server in
 * `--embedding` mode). One vector per input text, same order. The embedding
 * server is started lazily on the first call and runs CPU-only so it never
 * contends with the chat model for GPU memory.
 *
 * Used by memory retrieval. bge-small-en-v1.5 wants the QUERY (not the stored
 * documents) wrapped in a retrieval instruction; the helpers below apply it.
 * If the embedding model is swapped, revisit these prefixes + the Rust pooling
 * flag together.
 */
import { invoke } from "@tauri-apps/api/core";
import { EMBEDDING_MODEL } from "../data/recommended-models";

/** Is the embedding model downloaded? (recall degrades gracefully without it.) */
export async function isEmbeddingModelReady(): Promise<boolean> {
  try {
    return await invoke<boolean>("is_model_downloaded", {
      filename: EMBEDDING_MODEL.filename,
    });
  } catch {
    return false;
  }
}

/** Embed raw texts → one vector each. Empty input → empty output. */
export async function embedTexts(texts: string[]): Promise<number[][]> {
  if (texts.length === 0) return [];
  return await invoke<number[][]>("embed_texts", {
    texts,
    model: EMBEDDING_MODEL.filename,
  });
}

/** bge-small wants the query (not docs) prefixed with this retrieval instruction. */
const BGE_QUERY_INSTRUCTION =
  "Represent this sentence for searching relevant passages: ";

/** Embed stored documents (notes, transcript chunks) for the index. */
export async function embedDocuments(texts: string[]): Promise<number[][]> {
  return embedTexts(texts); // bge: documents are embedded raw
}

/** Embed a search query to retrieve against the document vectors. */
export async function embedQuery(query: string): Promise<number[] | null> {
  const [vec] = await embedTexts([`${BGE_QUERY_INSTRUCTION}${query}`]);
  return vec ?? null;
}

/** Cosine similarity between two equal-length vectors. */
export function cosineSimilarity(a: number[], b: number[]): number {
  let dot = 0;
  let na = 0;
  let nb = 0;
  for (let i = 0; i < a.length; i++) {
    dot += a[i] * b[i];
    na += a[i] * a[i];
    nb += b[i] * b[i];
  }
  const denom = Math.sqrt(na) * Math.sqrt(nb);
  return denom === 0 ? 0 : dot / denom;
}
