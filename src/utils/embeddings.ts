/**
 * Thin wrapper over the Rust `embed_texts` command (a second llama-server in
 * `--embedding` mode). One vector per input text, same order. The embedding
 * server is started lazily on the first call and runs CPU-only so it never
 * contends with the chat model for GPU memory.
 *
 * Used by memory retrieval and the gates. The model is trained with task
 * prefixes (EMBEDDING_PREFIXES): queries, stored passages and "things a
 * person says" each get their own; the helpers below apply them. A swap
 * changes the prefixes with the model, in recommended-models.ts and
 * src-tauri/src/embedding_model.rs together.
 */
import { invoke } from "@tauri-apps/api/core";
import { EMBEDDING_MODEL, EMBEDDING_PREFIXES } from "../data/recommended-models";

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

/** Embed stored documents (notes, transcript turns, tool descriptions) for the index. */
/** How long a prompt waits on its question's embedding before going without
 *  document recall. The embedding server can stop answering when a chat
 *  model has just filled the card and the helpers are being moved; a prompt
 *  must never wait on that with nothing on screen. */
export const EMBED_QUERY_BUDGET_MS = 15_000;

/** embedQuery, bounded: null when the embedding does not come back in time,
 *  so the caller sends without document passages instead of waiting. */
export async function embedQueryBounded(query: string, budgetMs = EMBED_QUERY_BUDGET_MS): Promise<number[] | null> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const late = new Promise<null>((resolve) => { timer = setTimeout(() => resolve(null), budgetMs); });
  try {
    const vec = await Promise.race([embedQuery(query), late]);
    if (vec === null) {
      const { uiLog } = await import("./uiLog");
      uiLog(`[recall] the embedding did not answer within ${Math.round(budgetMs / 1000)} s - sending without document passages`, "warn");
    }
    return vec;
  } finally {
    if (timer) clearTimeout(timer);
  }
}

export async function embedDocuments(texts: string[]): Promise<number[][]> {
  return embedTexts(texts.map((t) => `${EMBEDDING_PREFIXES.doc}${t}`));
}

/** Embed a search query to retrieve against the document vectors. */
export async function embedQuery(query: string): Promise<number[] | null> {
  const [vec] = await embedTexts([`${EMBEDDING_PREFIXES.query}${query}`]);
  return vec ?? null;
}

/** Embed things people SAY, for scoring against example sayings (the tools gate). */
export async function embedSaid(texts: string[]): Promise<number[][]> {
  return embedTexts(texts.map((t) => `${EMBEDDING_PREFIXES.said}${t}`));
}

/** Cosine similarity between two equal-length vectors. */
export function cosineSimilarity(a: number[], b: number[]): number {
  // A vector of another length belongs to another model (an index not yet
  // rebuilt after a model change): no likeness, never garbage.
  if (a.length !== b.length || a.length === 0) return 0;
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
