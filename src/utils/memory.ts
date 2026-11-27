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
  isEmbeddingModelReady,
} from "./embeddings";
import { embedBareKnowledge } from "./transcriptMemory";

/** What a turn's memory carried - the rail line under a reply reads it
 *  ("Memory: 3 rules, 12 facts"). */
export interface MemoryCounts {
  rules: number;
  commands: number;
  locations: number;
  facts: number;
}
/** One short line for the rail, or null when nothing was loaded. */
export function memoryLine(s: MemoryCounts | null | undefined): string | null {
  if (!s) return null;
  const parts: string[] = [];
  const n = (k: number, w: string) => (k > 0 ? parts.push(`${k} ${w}${k === 1 ? "" : "s"}`) : 0);
  n(s.rules, "rule");
  n(s.commands, "command");
  n(s.locations, "location");
  n(s.facts, "fact");
  return parts.length ? `Memory: ${parts.join(", ")}` : null;
}

/** The cue rule (the memory design §5.2, until the decision model ships):
 *  does a message read as a standing instruction about how to work? High
 *  precision on purpose - a wrong offer under a message is noise. Short,
 *  not a question, and carries an explicit standing marker. */
export function looksLikeInstruction(text: string): boolean {
  const t = text.trim();
  if (t.length < 6 || t.length > 240 || t.endsWith("?")) return false;
  const lower = t.toLowerCase();
  return (
    /\b(always|never|from now on|going forward|in future|in the future|every time|each time)\b/.test(lower) ||
    /^(please )?(don't|do not|stop|avoid|keep|use|only|no more|make sure|remember to|be more|be less)\b/.test(lower) ||
    /\b(instead of|rather than)\b/.test(lower) && /^(please )?(use|say|write|answer|reply|call|keep)\b/.test(lower)
  );
}

/** Rule offers from extraction (the memory design §5.6): a preference the
 *  extractor finds in a message is saved as a fact as before AND offered as
 *  a rule under that message - never saved as a rule silently. Keyed by the
 *  message's text; the chat bubble asks when it renders or when the offer
 *  lands later (`rule-offer` event). */
const ruleOffers = new Map<string, string>();
const offerKey = (message: string) => message.trim().toLowerCase().replace(/\s+/g, " ");
export function offerRule(message: string, rule: string): void {
  const text = rule.trim();
  if (!text || text.length > 200) return;
  ruleOffers.set(offerKey(message), text);
  try {
    window.dispatchEvent(new CustomEvent("rule-offer", { detail: { message, rule: text } }));
  } catch { /* no window */ }
}
export function pendingRuleOffer(message: string): string | null {
  return ruleOffers.get(offerKey(message)) ?? null;
}
export function clearRuleOffer(message: string): void {
  ruleOffers.delete(offerKey(message));
}

/** The same rule said twice is one rule: lower case, no trailing stop,
 *  single spaces. */
export function normalizeRuleText(text: string): string {
  return text.trim().toLowerCase().replace(/[.!]+$/, "").replace(/\s+/g, " ");
}

/** The always-loaded facts block is capped by BYTES, not lines (the
 *  survey's 2-3 KB figure): about 25 short rows. */
export const MEMORY_FACTS_BUDGET_BYTES = 2048;

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

/** Notes written on another of the person's devices arrive through their
 *  memory rows without a vector (vectors are per device): embed them here
 *  once the embedding model is ready, so recall sees them. Returns how many
 *  were embedded. Never runs twice at once. */
let embeddingBare = false;
export async function embedBareNotes(): Promise<number> {
  if (embeddingBare) return 0;
  embeddingBare = true;
  try {
    const facts = await getFacts();
    const bare = facts.filter((f) => f.entry_kind === "note" && f.valid_to == null && !(f.embedding && f.embedding.length));
    if (bare.length === 0 || !(await isEmbeddingModelReady())) return 0;
    const vectors = await embedDocuments(bare.map((f) => f.value));
    let n = 0;
    bare.forEach((f, i) => {
      const v = vectors[i];
      if (v && v.length) {
        f.embedding = v;
        n++;
      }
    });
    if (n > 0) await saveFacts(facts);
    return n;
  } catch (e) {
    console.warn("[Memory] embedding notes from other devices failed:", e);
    return 0;
  } finally {
    embeddingBare = false;
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

/** What memory adds to one turn (built in the backend, `memory_prompt`,
 *  the one place that turns memory into prompt text for every path). */
export interface TurnMemory {
  /** Fills the `{{userNameInfo}}` slot of the system prompt. */
  systemPart: string;
  beforeQuestion: string | null;
  afterQuestion: string | null;
  counts: MemoryCounts;
  bareNotes: boolean;
  bareKnowledge: boolean;
  /** What the reply is given, recorded with it (the search's report and
   *  pointers to the exchanges). Null when memory gave no block. */
  given: Record<string, unknown> | null;
  /** The passages given, as text (one per exchange of `given`, then any
   *  dated lines). In memory only, for the quote check and the panel. */
  givenTexts: string[];
  /** The person said, in fixed words, to keep something: the exchange
   *  before, or this message. Kept once the turn is recorded. */
  remember?: "before" | "this" | null;
}

export type { WhyNot, ReplyLine } from "./givenLine";

/** What the search block is sized by, known once the model is chosen. */
export interface TurnRoom {
  /** The local model's window, in tokens (absent: the smallest budget). */
  windowTokens?: number | null;
  /** Tokens left after the system prompt, the history and the reply. */
  roomTokens?: number | null;
  online: boolean;
  /** The person's question before this one in the open conversation. */
  previousQuestion?: string | null;
  /** "Share memory with this AI" (an online AI's switch). Absent: shared. */
  memoryShared?: boolean;
}

/**
 * The memory of a turn through the chat page. `query` is what the person
 * asked (absent or empty: nothing is retrieved, as for the warm-up).
 */
export async function memoryForTurn(
  query?: string,
  opts?: {
    aiId?: string;
    /** The AI's founding key: its own memory scope (`ai:<founding>`). */
    foundingKey?: string | null;
    conversationHash?: string | null;
    /** A shared embedding of `query` (the send path embeds the turn once and
     *  hands it to both the router and this retrieval). Absent = embedded
     *  where the memory is built. */
    queryVec?: Promise<number[] | null> | number[] | null;
    /** Search their conversations for this question and size the block to
     *  this room. Absent: only the system part is built (the early call of
     *  a turn, before the model is chosen; the warm-up). */
    room?: TurnRoom;
    /** For the early call, which has no room yet: may this turn be answered
     *  off this computer, and has the person left sharing on for this AI.
     *  With sharing off for an AI that may answer elsewhere, nothing memory
     *  holds is in the system part (their rules still are). */
    sharing?: { online: boolean; memoryShared: boolean };
  },
): Promise<TurnMemory> {
  // Bound on the query embedding so a cold/slow/stuck embed server can
  // never hang the reply (first call after launch may lose the race; fine).
  const vec =
    opts?.queryVec === undefined
      ? null
      : await Promise.race([
          Promise.resolve(opts.queryVec).catch(() => null),
          new Promise<null>((r) => setTimeout(() => r(null), 8000)),
        ]);
  const memory = await invokeWhenReady<TurnMemory>("memory_for_turn", {
    input: {
      aiId: opts?.aiId ?? null,
      foundingKey: opts?.foundingKey ?? null,
      surface: "chat",
      conversation: opts?.conversationHash ?? null,
      question: query ?? "",
      questionVector: vec,
      embedHere: opts?.queryVec === undefined,
      skipSearch: !opts?.room,
      windowTokens: opts?.room?.windowTokens || null,
      roomTokens: opts?.room?.roomTokens ?? null,
      online: opts?.room?.online ?? opts?.sharing?.online ?? false,
      previousQuestion: opts?.room?.previousQuestion ?? null,
      memoryShared: opts?.room?.memoryShared ?? opts?.sharing?.memoryShared ?? null,
      // The open conversation is sent whole with the turn.
      onWireFrom: 0,
    },
  });
  // Notes and knowledge that arrived without a vector: embedded for the
  // next turn; this one goes on.
  if (memory.bareNotes) void embedBareNotes();
  if (memory.bareKnowledge && opts?.aiId) void embedBareKnowledge(opts.aiId);
  return memory;
}

const PAUSED_KEY = "yoai-memory-paused";

/** The switch of releases before memory was rebuilt, as this device left
 *  it: read once by the update, which carries it over as "tidying off". */
export function isMemoryPaused(): boolean {
  try {
    return localStorage.getItem(PAUSED_KEY) === "1";
  } catch {
    return false;
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
  const key = (f: Fact) => `${f.subject}\u0000${f.predicate}`;
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
/** The best match has to score at least this for ANY passage to be given.
 *  This embedding model packs everything into a narrow band - measured on
 *  one library 2026-09-21: an unrelated question 0.62, the right document
 *  0.70 / 0.71 / 0.75 - so below the bar nothing in the library really fits,
 *  and handing over the "least bad" passages only misleads a small model and
 *  sends private text to an online one for nothing. Not a perfect line;
 *  every turn logs its top score so it can be tuned. */
/** EmbeddingGemma (2026-10-12 matrix, 12 questions x 3 languages against 12
 *  passages): right passages 0.21-0.75 (mean 0.57 en / 0.50 es, de), the best
 *  wrong one <= 0.32. 0.30 sits under every right answer measured and above
 *  the wrong ones; verify on a real library through the `documents: best
 *  match` log line (bge's bar was 0.66 on a 0.62-0.75 band). */
export const RECALL_BEST_AT_LEAST = 0.30;
/** A question that names a web page has its material in the page: the AI's
 *  documents join only on a strong match (10-14: "analyse flowsta.com"
 *  pulled in diffusion-model papers at 0.34 and the reply was spent on them). */
export const RECALL_BEST_FOR_PAGE_QUESTION = 0.50;
/** How far below the best match a passage may score and still be given. */
/** EmbeddingGemma spreads scores ~1.7x wider than bge (mean right-vs-wrong
 *  margin 0.37 vs 0.20), so the margin that keeps the same company grows. */
export const RECALL_MARGIN = 0.15;

/** The passages worth giving: none unless the best one clears the bar, then
 *  only those close to it (input in any order). */
export function keepNearBest<T extends { score: number }>(
  hits: T[],
  margin = RECALL_MARGIN,
  bestAtLeast = RECALL_BEST_AT_LEAST,
): T[] {
  if (hits.length === 0) return hits;
  const best = Math.max(...hits.map((h) => h.score));
  if (best < bestAtLeast) return [];
  return hits.filter((h) => h.score >= best - margin);
}

/** A note changed more recently than everything chosen keeps a place. An
 *  older note that discusses a subject at length scores far above the
 *  current note that states today's value in one row, and by scores alone
 *  the older one would be the only voice. Looked for among the first
 *  NEWER_AMONG found, no further than NEWER_WITHIN below the best match,
 *  and at most NEWER_AT_MOST of them, one passage a document. Only into
 *  room the chosen passages left: where a library is large the scores lie
 *  close together and the places are full, and there the scores decide (a
 *  note edited today would otherwise ride along with every question). */
export const NEWER_AMONG = 12;
export const NEWER_WITHIN = 0.25;
export const NEWER_AT_MOST = 2;

/** `chosen` with the newer notes' passages added (`found` is everything the
 *  search returned, best first), up to `room` passages in all. */
export function withNewerNotes<T extends { score: number; as_of: number; doc_id: string }>(
  chosen: T[],
  found: T[],
  room: number,
): T[] {
  if (chosen.length === 0 || room <= 0) return chosen;
  const day = (h: T) => Math.floor(h.as_of / 86_400);
  const newest = Math.max(...chosen.map(day));
  const best = Math.max(...chosen.map((h) => h.score));
  const added: T[] = [];
  for (const h of found.slice(0, NEWER_AMONG)) {
    if (added.length >= Math.min(NEWER_AT_MOST, room - chosen.length)) break;
    if (chosen.includes(h) || h.score < best - NEWER_WITHIN || day(h) <= newest) continue;
    if (added.some((a) => a.doc_id === h.doc_id)) continue;
    added.push(h);
  }
  return added.length === 0 ? chosen : [...chosen, ...added];
}

/** The question's own words count beside its meaning. A search by
 *  meaning alone ranks a long discussion of a subject above the one row
 *  that holds the figure asked for; a passage that also carries the
 *  question's rarer words is the likelier answer. The first WORDS_AMONG
 *  found are put in a new order: the meaning score plus up to WORDS_WEIGHT
 *  for the question's words a passage holds, a word counting for more the
 *  fewer of these passages hold it. A question with no telling words (or in
 *  a script these rules do not split) leaves the order as it was. */
export const WORDS_AMONG = 60;
export const WORDS_WEIGHT = 0.2;

const QUESTION_STOP = new Set(
  "a an the is are was were be do does did how what when where which who why many much of on in at to for from by with and or our we us it its that this have has had can one there their they them as so than then into over under about per not".split(" "),
);

function tellingWords(text: string): Set<string> {
  return new Set((text.toLowerCase().match(/[\p{L}\p{N}][\p{L}\p{N}_.-]*/gu) ?? []).map((w) => w.replace(/[.\-_]+$/, "")).filter((w) => w.length > 1 && !QUESTION_STOP.has(w)));
}

/** `found` (best meaning first) in the order of meaning and words
 *  together; each with the score it was ordered by. */
export function byMeaningAndWords<T extends { score: number; text: string }>(found: T[], question: string): { hit: T; score: number }[] {
  const pool = found.slice(0, WORDS_AMONG);
  const asked = tellingWords(question);
  const held = pool.map((h) => tellingWords(h.text));
  const weight = new Map<string, number>();
  for (const w of asked) {
    const n = held.filter((h) => h.has(w)).length;
    weight.set(w, Math.log(1 + (pool.length - n + 0.5) / (n + 0.5)));
  }
  const match = held.map((h) => [...asked].reduce((sum, w) => sum + (h.has(w) ? weight.get(w) ?? 0 : 0), 0));
  const most = Math.max(0, ...match);
  return pool
    .map((hit, i) => ({ hit, score: hit.score + (most > 0 ? (WORDS_WEIGHT * match[i]) / most : 0) }))
    .sort((a, b) => b.score - a.score);
}

/**
 * The text to ALSO search by when a message leans on the conversation: a
 * follow-up ("and the updated quote?", "what about the second one?") names
 * nothing, so on its own words it matches the wrong document or none. The
 * previous question supplies the subject. Null when there is no previous
 * question, or the message is long enough to stand on its own.
 */
export function followUpQuery(previousQuestion: string | undefined, question: string): string | null {
  const prev = (previousQuestion ?? "").trim();
  const q = question.trim();
  if (!prev || !q || q.length > 200) return null;
  return `${prev.slice(0, 400)}\n${q}`;
}

export async function loadDocumentContext(
  aiId: string,
  queryVec: number[] | null,
  roomTokens: number,
  maxPassages = 8,
  /** A second look, by the message TOGETHER with the previous question
   *  (followUpQuery). Results are merged, best score first. */
  followUpVec: number[] | null = null,
): Promise<string> {
  return (await loadDocumentContextUsed(aiId, queryVec, roomTokens, maxPassages, followUpVec)).text;
}

/** The day a document's file last changed, as YYYY-MM-DD (from seconds); empty when unknown. */
export function documentDay(seconds: number | undefined): string {
  if (!seconds || seconds <= 0) return "";
  const d = new Date(seconds * 1000);
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

/** The same block, WITH which documents' passages went into it - so the
 *  reply's Sources can say what the AI was given. Only documents with a
 *  matched passage are listed (every granted document is also named to the
 *  model as a one-line card; that is not "given a passage"). */
export async function loadDocumentContextUsed(
  aiId: string,
  queryVec: number[] | null,
  roomTokens: number,
  maxPassages = 8,
  followUpVec: number[] | null = null,
  bestAtLeast: number = RECALL_BEST_AT_LEAST,
  /** The question as typed: its words count beside its meaning. */
  question = "",
): Promise<{ text: string; used: import("../types").LibraryDocGiven[] }> {
  const none = { text: "", used: [] };
  if (!aiId || roomTokens <= 0) return none;
  try {
    const { corpusRecall, corpusDocuments } = await import("./corpus");
    const all = await corpusDocuments(aiId);
    if (all.length === 0) return none;
    // A small library is listed by name, so the AI knows what it holds. A
    // large one is not: forty names out of hundreds say nothing true about
    // the rest and take room from the passages. Its count is given, and the
    // cards of the documents whose passages matched.
    const LISTED = 40;
    const large = all.length > LISTED;
    const docs = large ? [] : all;
    const used: import("../types").LibraryDocGiven[] = [];
    let differentDays = false;
    let newest = "";
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
    let block = large
      ? `[From your documents]\nThis AI has ${all.length} documents from your library.\n`
      : `[From your documents]\nThis AI has ${docs.length === 1 ? "one document" : `${docs.length} documents`} from your library:\n${cards}\n`;

    const room = Number.isFinite(roomTokens) ? roomTokens - tokens(cards) : Number.POSITIVE_INFINITY;
    const passages = Number.isFinite(room) ? Math.max(0, Math.min(maxPassages, Math.floor(room / 300))) : maxPassages;
    if (queryVec && passages > 0) {
      // More is asked for than is given: the rest is where a newer note is
      // looked for (withNewerNotes).
      const asked = Math.max(passages, NEWER_AMONG, question ? WORDS_AMONG : 0);
      let hits = await corpusRecall(aiId, queryVec, asked);
      if (followUpVec) {
        // The message alone AND the message in its conversation: a question
        // that stands by itself still wins on its own words; a follow-up is
        // found through what it follows.
        const more = await corpusRecall(aiId, followUpVec, asked);
        const seen = new Set<string>();
        hits = [...hits, ...more]
          .sort((a, b) => b.score - a.score)
          .filter((h) => {
            const k = `${h.doc_id}\u0000${h.text}`;
            if (seen.has(k)) return false;
            seen.add(k);
            return true;
          })
          .slice(0, asked);
      }
      const everyFound = hits;
      const bestMeaning = hits.length ? hits[0].score : 0;
      hits = hits.slice(0, passages);
      // Only what is CLOSE to the best match. The floor alone lets almost
      // anything through (unrelated text scores 0.45-0.6 with this embedding
      // model; one question had 176 passages above it), and recall fills every
      // slot, so a one-passage document arrived padded with passages from an
      // unrelated one. A passage has to be within RECALL_MARGIN of the top.
      const found = hits.length;
      const top = found ? Math.max(...hits.map((h) => h.score)) : 0;
      if (question && bestMeaning >= bestAtLeast) {
        // Meaning and the question's words together choose the passages;
        // the bar on the best match stays on meaning alone.
        const ranked = byMeaningAndWords(everyFound, question).slice(0, passages);
        hits = withNewerNotes(keepNearBest(ranked, RECALL_MARGIN, 0).map((r) => r.hit), everyFound, passages);
      } else {
        hits = withNewerNotes(keepNearBest(hits, RECALL_MARGIN, bestAtLeast), everyFound, passages);
      }
      // Scores only, never a name or a passage - for tuning the two numbers.
      console.log(
        `[Memory] documents: best match ${top.toFixed(2)} (bar ${bestAtLeast}) - ${hits.length} of ${found} passages given`,
      );
      void import("./uiLog").then(({ uiLog }) =>
        uiLog(`documents: best match ${top.toFixed(2)}, bar ${bestAtLeast} - ${hits.length} of ${found} passages given`),
      ).catch(() => {});
      if (hits.length > 0) {
        const byDoc = new Map<string, { name: string; texts: string[]; idx: number[]; best: number; asOf: number; summary?: string }>();
        for (const h of hits) {
          // One name per document, the one its card uses (title, else file name).
          const known = all.find((x) => x.doc_id === h.doc_id);
          const name = known?.meta.title || known?.meta.filename || h.filename;
          const d = byDoc.get(h.doc_id) ?? { name, texts: [], idx: [], best: 0, asOf: 0, summary: known?.meta.summary };
          d.texts.push(h.text);
          d.idx.push(h.idx);
          d.best = Math.max(d.best, h.score);
          d.asOf = Math.max(d.asOf, h.as_of ?? 0);
          byDoc.set(h.doc_id, d);
        }
        for (const [doc_id, d] of byDoc) {
          used.push({ doc_id, name: d.name, passages: d.texts.length, best: Math.round(d.best * 100) / 100, texts: d.texts, idx: d.idx, asOf: d.asOf || undefined });
        }
        block += "\nPassages that match this question:\n";
        // When the days differ, the newest is said in words: a model given
        // only dates has called the older of two the more recent.
        const days = [...byDoc.values()].map((d) => documentDay(d.asOf)).filter(Boolean);
        const newestDay = new Set(days).size > 1 ? [...days].sort().slice(-1)[0] : "";
        const newestDocs = newestDay ? [...byDoc.values()].filter((d) => documentDay(d.asOf) === newestDay) : [];
        if (newestDocs.length === 1) newest = newestDocs[0].name;
        for (const d of byDoc.values()) {
          // A document is a source with a date, like a conversation: the day
          // its file last changed.
          const day = documentDay(d.asOf);
          const what = large && d.summary ? ` (${d.summary.match(/^[^.!?]*[.!?]/)?.[0] ?? d.summary.slice(0, 160)})` : "";
          const mark = newestDocs.length === 1 && d === newestDocs[0] ? ", the most recently changed of these" : "";
          block += `Document "${d.name}"${day ? `, last changed ${day}${mark}` : ""}${what}:\n` + d.texts.map((t) => `- ${t}`).join("\n") + "\n";
        }
        if (new Set([...byDoc.values()].map((d) => documentDay(d.asOf)).filter(Boolean)).size > 1) differentDays = true;
      }
    }
    // The closing instruction depends on what the block holds. With matched
    // passages: use them, name the document. With only the cards (no passage
    // cleared the bar): the cards are an inventory, not material - a small
    // model told "use these" worked a document into an answer about the sky
    // (2026-10-12). The quality matrix leg checks both shapes.
    block += used.length > 0
      ? `\nUse these passages when they answer the question, and say which document they came from.${differentDays ? ` Where two documents disagree, the one changed more recently is the current one${newest ? `: of these, that is "${newest}"` : ""}.` : ""}`
      : large
        ? "\nNo passage of them matched this question. Do not mention the documents unless the question is about them."
        : "\nThese are only the names of the documents this AI holds. Do not mention or use them unless the question is about one of them or about the documents themselves.";
    return { text: block, used };
  } catch (e) {
    console.warn("[Memory] document context skipped:", e);
    return none;
  }
}
