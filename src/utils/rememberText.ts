import { addKnowledge, addDocumentKnowledge, getAiKnowledge } from './transcriptMemory';

/**
 * "Remember this" - save a piece of a conversation into an AI's knowledge so
 * it draws on it in future conversations. One user-facing action, two storage
 * shapes underneath:
 *  - short text (a selection, a short reply) → one loose knowledge entry;
 *  - long text (a whole reply) → a knowledge "document" (chunked, grouped,
 *    removable as a unit on the memory page) so nothing is truncated - the
 *    loose-entry store caps entries at ~600 chars.
 * Returns false when the embedding model isn't ready (caller shows a hint).
 */
export async function rememberText(aiId: string, text: string): Promise<boolean> {
  const t = text.trim();
  if (!aiId || !t) return false;

  if (t.length <= 600) {
    // Skip exact duplicates so double-clicks don't pile up copies.
    try {
      const existing = await getAiKnowledge(aiId);
      if (existing.some((e) => e.text === t)) return true;
    } catch {
      /* best-effort dedup only */
    }
    return addKnowledge(aiId, t);
  }

  const title = `Remembered reply · ${new Date().toLocaleDateString()}`;
  const result = await addDocumentKnowledge(aiId, title, t.length, t);
  return result !== null;
}
