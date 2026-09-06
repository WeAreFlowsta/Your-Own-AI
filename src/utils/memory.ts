/**
 * Phase A persistent memory — the user-level "remember me" profile.
 *
 * Thin wrapper over the encrypted fact store (Rust `get_memory_facts` /
 * `save_memory_facts`, XSalsa20-Poly1305 under the user data key). The store
 * is a cache rebuildable from the transcripts; extraction + reconciliation
 * (later steps) live in the frontend — this just loads and persists the set.
 */
import { invoke } from "@tauri-apps/api/core";
import {
  embedDocuments,
  embedQuery,
  cosineSimilarity,
  isEmbeddingModelReady,
} from "./embeddings";
import { recallPerAi } from "./transcriptMemory";

export interface Fact {
  id: string;
  subject: string;
  predicate: string;
  value: string;
  confidence: number;
  valid_from: number; // micros, matching transcript timestamps
  valid_to?: number | null;
  /** Provenance → the signed transcript entry this was learned from. */
  source_action_hash?: string | null;
  source_ai_id?: string | null;
  /** "personal" in Phase A; "group:<agreement_hash>" later. */
  owner_scope: string;
  /** Which key encrypts the item — "personal" now; group key later. */
  key_scope: string;
  /** "fact" (structured triple) or "note" (free-text, retrieved — B2). */
  entry_kind: string;
  /** How it came to be: "extracted" | "signed" | "authored" (added by user). */
  provenance: string;
  /** Flowsta identity that signed an authored entry (B3); null until then. */
  author_identity?: string | null;
  /** Embedding vector for retrieval (notes/chunks). Absent for plain facts. */
  embedding?: number[] | null;
  /** Groups the chunk-notes of one long remembered passage so they can be
   *  forgotten together. Absent for everything else. */
  group_id?: string | null;
  created_at: number;
  updated_at: number;
}

/**
 * The encrypted store needs the conductor's data key, which isn't available
 * until the conductor has started. On a fresh launch the Knows tab can call in
 * before that — so treat a "still starting" error as transient and retry,
 * rather than reporting an empty/failed store (which read as "memory lost").
 */
async function invokeWhenReady<T>(
  cmd: string,
  args?: Record<string, unknown>,
): Promise<T> {
  for (let attempt = 0; ; attempt++) {
    try {
      return await invoke<T>(cmd, args);
    } catch (e) {
      const transient = String(e).toLowerCase().includes("starting");
      if (transient && attempt < 8) {
        await new Promise((r) => setTimeout(r, 1000));
        continue;
      }
      throw e;
    }
  }
}

/** Load all profile facts (decrypted in Rust). Empty array if none yet. */
export async function getFacts(): Promise<Fact[]> {
  try {
    return await invokeWhenReady<Fact[]>("get_memory_facts");
  } catch (e) {
    console.warn("[Memory] Failed to load facts:", e);
    return [];
  }
}

/** Persist the full fact set (encrypted in Rust). Fire-and-forget. */
export async function saveFacts(facts: Fact[]): Promise<void> {
  try {
    await invokeWhenReady("save_memory_facts", { facts });
  } catch (e) {
    console.warn("[Memory] Failed to save facts:", e);
  }
}

/** Mint a new personal-scope fact (Phase A). */
export function newFact(input: {
  subject: string;
  predicate: string;
  value: string;
  confidence: number;
  sourceActionHash?: string | null;
  sourceAiId?: string | null;
  /** "extracted" (default) | "signed" | "authored". */
  provenance?: string;
  /** "fact" (default) | "note". */
  entryKind?: string;
}): Fact {
  const now = Date.now() * 1000; // micros, matching transcript timestamps
  return {
    id: crypto.randomUUID(),
    subject: input.subject,
    predicate: input.predicate,
    value: input.value,
    confidence: input.confidence,
    valid_from: now,
    valid_to: null,
    source_action_hash: input.sourceActionHash ?? null,
    source_ai_id: input.sourceAiId ?? null,
    owner_scope: "personal",
    key_scope: "personal",
    entry_kind: input.entryKind ?? "fact",
    provenance: input.provenance ?? "extracted",
    author_identity: null,
    created_at: now,
    updated_at: now,
  };
}

/**
 * Add a fact the user typed in directly: provenance "authored", confidence 1
 * (an explicit assertion). Authored facts are sticky — extraction won't
 * supersede them (see reconcile). Re-adding an existing value just upgrades it
 * to authored; a new value for a single-valued predicate (name, home…)
 * supersedes the current one, mirroring a correction.
 */
export async function addAuthoredFact(input: {
  predicate: string;
  value: string;
}): Promise<Fact[]> {
  const facts = await getFacts();
  const now = Date.now() * 1000;
  const active = (f: Fact) => f.valid_to == null || f.valid_to === undefined;
  const isUser = (f: Fact) => norm(f.subject) === "user";
  const samePred = (f: Fact) => norm(f.predicate) === norm(input.predicate);

  // Already present (same value)? Promote it to an authored, certain fact.
  const existing = facts.find(
    (f) => active(f) && isUser(f) && samePred(f) && norm(f.value) === norm(input.value),
  );
  if (existing) {
    existing.provenance = "authored";
    existing.confidence = 1;
    existing.updated_at = now;
    await saveFacts(facts);
    return facts;
  }

  // Single-valued correction: retire the current value(s) for this predicate.
  if (isSingleValuedPredicate(input.predicate)) {
    for (const f of facts) {
      if (active(f) && isUser(f) && samePred(f)) {
        f.valid_to = now;
        f.updated_at = now;
      }
    }
  }

  facts.push(
    newFact({
      subject: "user",
      predicate: input.predicate,
      value: input.value,
      confidence: 1,
      provenance: "authored",
    }),
  );
  await saveFacts(facts);
  return facts;
}

/**
 * Add a free-text note the user wrote. Unlike a fact, a note is NOT
 * always-injected — it's embedded and pulled into a conversation only when
 * relevant (see loadMemoryBlock). Embedding is best-effort: the note is saved
 * either way, but it can only be recalled once the embedding model is present
 * (re-embed by re-saving). entry_kind "note", provenance "authored".
 */
export async function addAuthoredNote(
  text: string,
  opts?: { groupId?: string },
): Promise<Fact[]> {
  const facts = await getFacts();
  const note = newFact({
    subject: "user",
    predicate: "note",
    value: text,
    confidence: 1,
    provenance: "authored",
    entryKind: "note",
  });
  if (opts?.groupId) note.group_id = opts.groupId;
  try {
    if (await isEmbeddingModelReady()) {
      const [vec] = await embedDocuments([text]);
      if (vec && vec.length) note.embedding = vec;
    }
  } catch (e) {
    console.warn("[Memory] Note embedding failed (saved without it):", e);
  }
  facts.push(note);
  await saveFacts(facts);
  return facts;
}

const norm = (s: string) => s.trim().toLowerCase();

/** Does this predicate name the user's name? An EXACT set, not a substring
 *  match — `/name/` wrongly caught junk like `prefers_..._names_for_projects`
 *  and made the injected name "YOAI". Extraction is constrained to `name_is`
 *  (see the grammar enum); the rest are belt-and-suspenders for legacy data. */
const NAME_PREDICATES = new Set([
  "name_is", "name", "full_name", "first_name", "goes_by", "has_name", "is_named",
]);
export function isNamePredicate(predicate: string): boolean {
  return NAME_PREDICATES.has(norm(predicate));
}

/** Single-valued predicates REPLACE their value on change (you have one name,
 *  one home); everything else accumulates (many likes, many projects). Aligned
 *  with the extraction grammar's predicate enum. */
const SINGLE_VALUED = new Set([
  "lives_in", "located_in", "based_in", "born_in",
  "age_is", "birthday_is", "gender_is", "timezone_is",
]);
export function isSingleValuedPredicate(predicate: string): boolean {
  return isNamePredicate(predicate) || SINGLE_VALUED.has(norm(predicate));
}

/**
 * Format the active profile into the `{{userNameInfo}}` slot of the system
 * prompt (which sits in the prompt's "User Information" block). Name-aware,
 * ranked by confidence then recency, budgeted. With no facts it returns the
 * original name-unknown line, so behaviour is unchanged until memory exists.
 */
export async function loadMemoryBlock(
  query?: string,
  opts?: {
    aiId?: string;
    conversationHash?: string | null;
    /** A shared embedding of `query` (the send path embeds the turn once and
     *  hands it to both the router and this retrieval). Absent = embed here. */
    queryVec?: Promise<number[] | null> | number[] | null;
    /** Document passages to pull from the library: 8 for an online model,
     *  fewer for a small local window (the reading-room check counts them). */
    docPassages?: number;
  },
): Promise<string> {
  const all = (await getFacts()).filter(
    (f) => f.valid_to == null || f.valid_to === undefined,
  );

  // Always-injected = facts only. Notes are free-text and retrieved on
  // relevance (below); the synthesis paragraph injects separately.
  const factItems = all
    .filter((f) => f.entry_kind !== "note" && f.entry_kind !== "synthesis" && f.entry_kind !== "library")
    .sort((a, b) => b.confidence - a.confidence || b.updated_at - a.updated_at)
    .slice(0, 25);
  const synthesis = all.find((f) => f.entry_kind === "synthesis");

  const name = factItems.find((f) => isNamePredicate(f.predicate));
  const others = factItems.filter((f) => f !== name);

  const nameLine = name
    ? `Their name is ${name.value}.`
    : "Their name is unknown. Do not assume or allude to it.";

  let block = nameLine;
  // The consolidated portrait first (the 2026 lesson: a coherent summary
  // outperforms a bare fact list), the ground-truth facts after it.
  if (synthesis) {
    block += `\nAbout them, from what they've shared over time (use naturally; never recite): ${synthesis.value}`;
  }
  const library = all.find((f) => f.entry_kind === "library");
  if (library) {
    block += `\nFrom the documents they write and keep in their library (use naturally; never recite): ${library.value}`;
  }
  if (others.length > 0) {
    // Include the relation so a bare value isn't ambiguous to the model
    // ("- Melbourne" vs "- lives in: Melbourne").
    const facts = others
      .map((f) => `- ${f.predicate.replace(/_/g, " ")}: ${f.value}`)
      .join("\n");
    block += `\nThings you've learned about them from past conversations (use naturally where relevant; do not recite this list or claim certainty):\n${facts}`;
  }

  // Retrieval: saved notes (user-level) + this AI's relevant past
  // conversations (per-AI), both scored against THIS turn. The query is
  // embedded ONCE and reused for both. Skips entirely (Phase-A behaviour)
  // without a query or the embedding model.
  if (query && query.trim()) {
    try {
      const notes = all.filter(
        (f) => f.entry_kind === "note" && f.embedding && f.embedding.length > 0,
      );
      const wantPerAi = !!opts?.aiId;
      if ((notes.length > 0 || wantPerAi) && (await isEmbeddingModelReady())) {
        // Bound on the query embedding so a cold/slow/stuck embed server can
        // never hang the reply (first call after launch may lose the race; fine).
        const qvec = await Promise.race([
          opts?.queryVec !== undefined
            ? Promise.resolve(opts.queryVec)
            : embedQuery(query),
          new Promise<null>((r) => setTimeout(() => r(null), 8000)),
        ]);
        if (qvec) {
          // Relevant saved notes (shared across all AIs).
          if (notes.length > 0) {
            const relevant = notes
              .map((n) => ({ n, score: cosineSimilarity(qvec, n.embedding as number[]) }))
              .filter((x) => x.score >= NOTE_RELEVANCE_THRESHOLD)
              .sort((a, b) => b.score - a.score)
              .slice(0, MAX_RETRIEVED_NOTES);
            if (relevant.length > 0) {
              const noteLines = relevant.map((x) => `- ${x.n.value}`).join("\n");
              block += `\nRelevant notes they've saved (use naturally if helpful):\n${noteLines}`;
            }
          }
          // This AI's own relevant memory: authored knowledge (packs/lore the
          // user gave it) + past conversation turns (excluding the current one).
          if (wantPerAi) {
            const { episodic, authored } = await recallPerAi(
              opts!.aiId!,
              qvec,
              opts?.conversationHash ?? undefined,
            );
            if (authored.length > 0) {
              const lines = authored.map((t) => `- ${t}`).join("\n");
              block += `\nKnowledge this AI has been given (use if relevant):\n${lines}`;
            }
            // Documents from the library ride next to the question on the
            // wire instead (loadDocumentContext) - sized to the model's window.
            if (episodic.length > 0) {
              const lines = episodic.map((t) => `- ${t}`).join("\n");
              block += `\nFrom earlier conversations with this person (use naturally if relevant):\n${lines}`;
            }
          }
        }
      }
    } catch (e) {
      console.warn("[Memory] Retrieval failed (skipped):", e);
    }
  }

  return block;
}

/** Cosine floor for a note to count as relevant. Calibrated for bge-small-en-v1.5
 *  against a real note (2026-06-21): unrelated queries score ≤0.431, a clean
 *  match 0.662, but a typo'd ("trianing") or vaguely-phrased ("any tips for me?")
 *  match lands 0.475–0.493 — just above the unrelated ceiling. 0.45 sits in that
 *  gap: it keeps fuzzy/typo'd real queries while still rejecting unrelated ones.
 *  Bias is toward recall (a stray note is injected as "use if helpful" and
 *  ignored; a miss reads as broken). Revisit if the model changes. */
const NOTE_RELEVANCE_THRESHOLD = 0.45;
/** Cap retrieved notes per turn, to bound prompt growth. */
const MAX_RETRIEVED_NOTES = 3;

const PAUSED_KEY = "yoai-memory-paused";

/** Is remembering paused? (extraction skips while paused; injection still uses
 *  what's already there). */
export function isMemoryPaused(): boolean {
  try {
    return localStorage.getItem(PAUSED_KEY) === "1";
  } catch {
    return false;
  }
}

export function setMemoryPaused(paused: boolean): void {
  try {
    localStorage.setItem(PAUSED_KEY, paused ? "1" : "0");
  } catch {
    /* ignore */
  }
}

/** Forget a single fact (hard delete from the store). */
/**
 * A forget can remove the fact that superseded an older one (bulk-forgetting
 * an import, or trashing a wrong "update"), leaving that subject+predicate
 * with a hidden predecessor and no live fact - the user's original memory
 * silently gone from view. Restore the newest such predecessor per pair:
 * forgetting the replacement brings the original back. Notes are exempt
 * (many live at once by design; they are never superseded).
 */
function restoreOrphanedPredecessors(facts: Fact[]): boolean {
  const key = (f: Fact) => `${f.subject} ${f.predicate}`;
  const live = new Set(facts.filter((f) => f.valid_to == null).map(key));
  const newestOrphan = new Map<string, Fact>();
  for (const f of facts) {
    if (f.valid_to == null || f.predicate === "note" || live.has(key(f))) {
      continue;
    }
    const cur = newestOrphan.get(key(f));
    if (!cur || (f.valid_to ?? 0) > (cur.valid_to ?? 0)) {
      newestOrphan.set(key(f), f);
    }
  }
  for (const f of newestOrphan.values()) {
    f.valid_to = null;
    f.updated_at = Date.now() * 1000;
  }
  return newestOrphan.size > 0;
}

/** One-time launch repair for stores that already contain orphaned
 *  predecessors (created before restore-on-forget existed). */
export async function repairMemoryOrphans(): Promise<void> {
  const facts = await getFacts();
  if (restoreOrphanedPredecessors(facts)) {
    await saveFacts(facts);
  }
}

export async function forgetFact(id: string): Promise<Fact[]> {
  const facts = (await getFacts()).filter((f) => f.id !== id);
  restoreOrphanedPredecessors(facts);
  await saveFacts(facts);
  return facts;
}

/** Forget every note in a group (the chunks of one long remembered passage). */
export async function forgetGroup(groupId: string): Promise<Fact[]> {
  const facts = (await getFacts()).filter((f) => f.group_id !== groupId);
  await saveFacts(facts);
  return facts;
}

/** Edit a fact's value (re-stamps updated_at). */
export async function editFactValue(id: string, value: string): Promise<Fact[]> {
  const facts = await getFacts();
  const f = facts.find((x) => x.id === id);
  if (f) {
    f.value = value;
    f.updated_at = Date.now() * 1000;
  }
  await saveFacts(facts);
  return facts;
}

/** Forget everything (and any facts learned from a specific AI, used by purge). */
export async function forgetAll(sourceAiId?: string): Promise<void> {
  if (!sourceAiId) {
    await saveFacts([]);
    return;
  }
  const facts = (await getFacts()).filter((f) => f.source_ai_id !== sourceAiId);
  restoreOrphanedPredecessors(facts);
  await saveFacts(facts);
}

// ── Pending-turn queue ────────────────────────────────────────────────────
// Extraction is fire-and-forget (a second local generation after the reply).
// Closing the app within a couple seconds cuts it off and that turn is lost.
// So we persist each turn before extracting and clear it when extraction
// finishes; anything left on next launch is drained (see drainPendingExtractions).

const PENDING_KEY = "yoai-memory-pending";

export interface PendingTurn {
  id: string;
  userText: string;
  aiId: string;
}

export function getPendingTurns(): PendingTurn[] {
  try {
    const raw = localStorage.getItem(PENDING_KEY);
    const parsed = raw ? JSON.parse(raw) : [];
    return Array.isArray(parsed) ? parsed : [];
  } catch {
    return [];
  }
}

function writePendingTurns(turns: PendingTurn[]): void {
  try {
    // Cap the queue so a never-draining device can't grow it unbounded.
    localStorage.setItem(PENDING_KEY, JSON.stringify(turns.slice(-30)));
  } catch {
    /* ignore */
  }
}

/** Record a turn whose extraction is about to run. Returns its id, to clear. */
export function addPendingTurn(turn: { userText: string; aiId: string }): string {
  const id = crypto.randomUUID();
  writePendingTurns([...getPendingTurns(), { id, userText: turn.userText, aiId: turn.aiId }]);
  return id;
}

export function removePendingTurn(id: string): void {
  writePendingTurns(getPendingTurns().filter((t) => t.id !== id));
}

/**
 * What the send path puts in the user turn, next to the question (where a
 * small model reads it best), never in the system prompt:
 *  1. a card for every document this AI has been granted, so a question
 *     about the collection ("common threads in my documents") sees all of
 *     them by name, and
 *  2. the passages that match this question.
 * `roomTokens` is the room the caller measured in the model's window
 * (Infinity online): cards take up to a third of it, at the richest level
 * that fits (full card, first sentence, name only); passages get the rest
 * at ~300 tokens each, up to `maxPassages`.
 */
export async function loadDocumentContext(
  aiId: string,
  queryVec: number[] | null,
  roomTokens: number,
  maxPassages = 8,
): Promise<string> {
  if (!aiId || roomTokens <= 0) return "";
  try {
    const { corpusRecall, corpusDocuments } = await import("./corpus");
    const docs = (await corpusDocuments(aiId)).slice(0, 40);
    if (docs.length === 0) return "";
    const tokens = (t: string) => Math.ceil(t.length / 4);
    const label = (d: (typeof docs)[number]) =>
      `"${d.meta.title || d.meta.filename}"${d.meta.mine ? " (written by you)" : ""}`;
    const firstSentence = (t: string) => {
      const m = t.match(/^[^.!?]*[.!?]/);
      return m ? m[0] : t.slice(0, 160);
    };
    const levels = [
      docs.map((d) => `- ${label(d)}${d.meta.summary ? `: ${d.meta.summary}` : ""}`),
      docs.map((d) => `- ${label(d)}${d.meta.summary ? `: ${firstSentence(d.meta.summary)}` : ""}`),
      docs.map((d) => `- ${label(d)}`),
    ];
    const cardBudget = Number.isFinite(roomTokens) ? Math.floor(roomTokens / 3) : Number.POSITIVE_INFINITY;
    let cards = levels[levels.length - 1].join("\n");
    for (const lines of levels) {
      const text = lines.join("\n");
      if (tokens(text) <= cardBudget) {
        cards = text;
        break;
      }
    }
    let block = `[From your documents]\nThis AI has ${docs.length === 1 ? "one document" : `${docs.length} documents`} from your library:\n${cards}\n`;

    const room = Number.isFinite(roomTokens) ? roomTokens - tokens(cards) : Number.POSITIVE_INFINITY;
    const passages = Number.isFinite(room) ? Math.max(0, Math.min(maxPassages, Math.floor(room / 300))) : maxPassages;
    if (queryVec && passages > 0) {
      const hits = await corpusRecall(aiId, queryVec, passages);
      if (hits.length > 0) {
        const byDoc = new Map<string, { name: string; texts: string[] }>();
        for (const h of hits) {
          const d = byDoc.get(h.doc_id) ?? { name: h.filename, texts: [] };
          d.texts.push(h.text);
          byDoc.set(h.doc_id, d);
        }
        block += "\nPassages that match this question:\n";
        for (const d of byDoc.values()) {
          block += `Document "${d.name}":\n` + d.texts.map((t) => `- ${t}`).join("\n") + "\n";
        }
      }
    }
    block += "\nUse these when they answer the question, and say which document they came from.";
    return block;
  } catch (e) {
    console.warn("[Memory] document context skipped:", e);
    return "";
  }
}
