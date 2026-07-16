import {
  addKnowledge,
  addDocumentKnowledge,
  findAuthoredByText,
  authoredTextIndex,
  removeKnowledgeDocument,
  deleteAiMemory,
  chunkDocumentText,
} from './transcriptMemory';
import { addAuthoredNote, forgetFact, forgetGroup, getFacts } from './memory';

/**
 * "Remember this" - save a piece of a conversation into memory so an AI draws
 * on it in future conversations. Where it goes is a per-surface setting
 * (Settings → Memory):
 *  - per-ai (default): that AI's knowledge store - the privacy boundary; a
 *    save made talking to one AI is never surfaced to another;
 *  - global: the shared notes store on Your Memory - every AI can retrieve it.
 * Within each destination, short text becomes one entry/note; long text is
 * chunked (per-AI: a knowledge document; global: a group of notes sharing a
 * group_id) so retrieval stays passage-sized and nothing is truncated.
 *
 * Every save returns a handle the button keeps, so Remember is a TOGGLE:
 * `forgetRemembered` reverses exactly what was saved, and `findRemembered`
 * lets a button reflect already-saved state across reloads.
 */

/** The two chat surfaces with their own destination setting. The memory-page
 *  transcript button saves whole entries, so it follows 'reply'. */
export type RememberSurface = 'selection' | 'reply';

export type RememberScope = 'per-ai' | 'global';

export type RememberHandle =
  | { kind: 'entry'; id: string } // per-AI loose knowledge entry
  | { kind: 'doc'; id: string } // per-AI knowledge document (docId)
  | { kind: 'note'; id: string } // global note (fact id)
  | { kind: 'note-group'; id: string }; // global chunked notes (group_id)

const SCOPE_KEYS: Record<RememberSurface, string> = {
  selection: 'yoai-remember-scope-selection',
  reply: 'yoai-remember-scope-reply',
};

export function getRememberScope(surface: RememberSurface): RememberScope {
  try {
    return localStorage.getItem(SCOPE_KEYS[surface]) === 'global' ? 'global' : 'per-ai';
  } catch {
    return 'per-ai';
  }
}

export function setRememberScope(surface: RememberSurface, scope: RememberScope): void {
  try {
    localStorage.setItem(SCOPE_KEYS[surface], scope);
  } catch {
    /* ignore */
  }
}

/** Long-text threshold: matches the knowledge store's per-entry text cap. */
const SHORT_MAX = 600;

export async function rememberText(
  aiId: string,
  text: string,
  surface: RememberSurface,
): Promise<RememberHandle | null> {
  const t = text.trim();
  if (!t) return null;

  // Exact repeats (double-clicks, re-saves) resolve to the existing save.
  const existing = await findRemembered(aiId, t, surface);
  if (existing) return existing;

  if (getRememberScope(surface) === 'global') {
    invalidateRememberedCache();
    if (t.length <= SHORT_MAX) {
      await addAuthoredNote(t);
      const note = (await getFacts()).find(
        (f) => f.entry_kind === 'note' && f.value === t && f.valid_to == null,
      );
      return note ? { kind: 'note', id: note.id } : null;
    }
    // Long global save: one note per chunk under a shared group_id, so
    // retrieval injects passages (not the whole reply) and undo removes the
    // group as a unit.
    const groupId = crypto.randomUUID();
    for (const chunk of chunkDocumentText(t)) {
      await addAuthoredNote(chunk, { groupId });
    }
    return { kind: 'note-group', id: groupId };
  }

  if (!aiId) return null;
  invalidateRememberedCache();
  if (t.length <= SHORT_MAX) {
    const id = await addKnowledge(aiId, t);
    return id ? { kind: 'entry', id } : null;
  }
  const title = `Remembered reply · ${new Date().toLocaleDateString()}`;
  const result = await addDocumentKnowledge(aiId, title, t.length, t);
  return result ? { kind: 'doc', id: result.docId } : null;
}

/** Reverse a save: remove exactly what `rememberText` stored. */
export async function forgetRemembered(aiId: string, handle: RememberHandle): Promise<void> {
  invalidateRememberedCache();
  switch (handle.kind) {
    case 'entry':
      await deleteAiMemory(aiId, handle.id);
      break;
    case 'doc':
      await removeKnowledgeDocument(aiId, handle.id);
      break;
    case 'note':
      await forgetFact(handle.id);
      break;
    case 'note-group':
      await forgetGroup(handle.id);
      break;
  }
}

/** Is this exact text already remembered (in the surface's current
 *  destination)? Returns its handle so the button renders "Remembered" and
 *  can forget it - state survives reloads. */
export async function findRemembered(
  aiId: string,
  text: string,
  surface: RememberSurface,
): Promise<RememberHandle | null> {
  const t = text.trim();
  if (!t) return null;

  if (getRememberScope(surface) === 'global') {
    const facts = (await getFacts()).filter(
      (f) => f.entry_kind === 'note' && f.valid_to == null,
    );
    if (t.length <= SHORT_MAX) {
      const note = facts.find((f) => f.value === t);
      if (!note) return null;
      return note.group_id
        ? { kind: 'note-group', id: note.group_id }
        : { kind: 'note', id: note.id };
    }
    const first = chunkDocumentText(t)[0];
    const chunk = first ? facts.find((f) => f.group_id && f.value === first) : undefined;
    return chunk?.group_id ? { kind: 'note-group', id: chunk.group_id } : null;
  }

  if (!aiId) return null;
  return findAuthoredByText(aiId, t);
}

// ── Cached saved-state lookups for the buttons ──────────────────────
// A chat page mounts one Remember button per assistant message; checking each
// against the store directly would decrypt + deserialize the whole per-AI
// vector store once PER MESSAGE. Instead one index per store is built on
// first use and invalidated on our own saves/forgets. Display-only: the
// save path always dedupes against a FRESH read (findRemembered above), so a
// stale index can never cause a double-save.

const GLOBAL_KEY = '*global-notes*';
const indexCache = new Map<string, Promise<Map<string, RememberHandle>>>();

function invalidateRememberedCache(): void {
  indexCache.clear();
}

async function globalNotesIndex(): Promise<Map<string, RememberHandle>> {
  const map = new Map<string, RememberHandle>();
  const facts = await getFacts();
  for (const f of facts) {
    if (f.entry_kind !== 'note' || f.valid_to != null) continue;
    map.set(
      f.value,
      f.group_id ? { kind: 'note-group', id: f.group_id } : { kind: 'note', id: f.id },
    );
  }
  return map;
}

/** `findRemembered`, but served from a per-store index (built once, reused by
 *  every button on the page). Use for rendering saved state, not for saving. */
export async function findRememberedCached(
  aiId: string,
  text: string,
  surface: RememberSurface,
): Promise<RememberHandle | null> {
  const t = text.trim();
  if (!t) return null;
  const global = getRememberScope(surface) === 'global';
  if (!global && !aiId) return null;

  const cacheKey = global ? GLOBAL_KEY : aiId;
  let promise = indexCache.get(cacheKey);
  if (!promise) {
    promise = global ? globalNotesIndex() : authoredTextIndex(aiId);
    indexCache.set(cacheKey, promise);
    promise.catch(() => indexCache.delete(cacheKey));
  }
  const index = await promise;
  let key = t;
  if (t.length > SHORT_MAX) {
    const first = chunkDocumentText(t)[0] ?? '';
    // Per-AI chunk entries are stored capped at the store's text limit;
    // global chunk-notes keep the full chunk text.
    key = global ? first : first.slice(0, SHORT_MAX);
  }
  return index.get(key) ?? null;
}
