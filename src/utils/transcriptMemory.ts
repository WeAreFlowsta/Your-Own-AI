/**
 * Per-AI episodic memory — each AI recalls its OWN past conversations.
 *
 * Distinct from the shared user profile/notes (memory.ts): those are about the
 * user and injected into every AI. This indexes each conversation turn into a
 * per-AI vector store and, on a new turn, retrieves the most relevant turns
 * from that AI's OTHER conversations to inject — so an AI remembers across
 * sessions. Reuses the same embedding path (embeddings.ts) and the conductor
 * data key (encrypted at rest, Rust side). The vectors are a rebuildable cache
 * over the canonical transcripts.
 */
import { invoke } from "@tauri-apps/api/core";
import {
  embedDocuments,
  cosineSimilarity,
  isEmbeddingModelReady,
} from "./embeddings";

export interface TranscriptEmbedding {
  id: string;
  conversation_hash: string;
  role: string;
  text: string;
  vector: number[];
  /** "episodic" (learned from chat) | "authored" (knowledge the user gave). */
  kind: string;
  created_at: number;
  /** Set on authored chunks ingested from a document, so the UI can list and
   *  remove a document as a unit. Absent for facts/notes/lore/episodic. */
  source?: KnowledgeSource;
}

/** A document a set of authored chunks came from. */
export interface KnowledgeSource {
  doc_id: string;
  filename: string;
  size_bytes: number;
}

/** One ingested document, grouped from its chunks for the UI. */
export interface KnowledgeDocument {
  docId: string;
  filename: string;
  sizeBytes: number;
  chunkCount: number;
  addedAt: number;
}

/** Cap EPISODIC entries per AI (drop oldest) so the per-AI blob can't grow
 *  unbounded. Authored knowledge (documents, lore the user gave the AI) is
 *  deliberately exempt: the user placed it there, so chat volume must never
 *  evict it. */
const MAX_PER_AI = 1000;
/** Cap stored/injected text length, to bound both the store and the prompt. */
const MAX_TEXT = 600;
/** Absolute cosine floor (calibrated for bge-small): catches a typo'd/fuzzy
 *  primary match (~0.47) while rejecting unrelated turns (≤~0.41). */
const RECALL_THRESHOLD = 0.45;
/** Relative margin: also drop any match much weaker than the BEST one. First-
 *  person turns ("my dog…", "I'm allergic…") cluster ~0.47–0.49, so without
 *  this a strong hit (0.69) drags in an unrelated second (0.47). Keeping only
 *  scores within this margin of the top removes that noise. (Pre-flight tested.) */
const RECALL_MARGIN = 0.12;
/** Max recalled turns injected per message. */
const MAX_RECALL = 3;

async function getEmb(aiId: string): Promise<TranscriptEmbedding[]> {
  try {
    return await invoke<TranscriptEmbedding[]>("get_transcript_embeddings", { aiId });
  } catch (e) {
    console.warn("[Memory] get transcript embeddings failed:", e);
    return [];
  }
}

async function saveEmb(aiId: string, items: TranscriptEmbedding[]): Promise<void> {
  try {
    await invoke("save_transcript_embeddings", { aiId, items });
  } catch (e) {
    console.warn("[Memory] save transcript embeddings failed:", e);
  }
}

/** Question words that mark a message as info-seeking rather than self-disclosing. */
const QUESTION_OPENERS = [
  "what", "where", "when", "why", "who", "which", "whose", "whom", "how",
];
/** Auxiliary/be verbs that, right after a wh-word, signal a question even with
 *  NO "?": "where WERE you born", "what DID i say", "how DO you work". */
const QUESTION_AUX = [
  "is", "are", "was", "were", "do", "does", "did", "can", "could", "will",
  "would", "should", "shall", "have", "has", "had", "am", "may", "might",
];

/**
 * Is this a "pure" question (a recall query / info request), not a statement?
 * Opens with a wh-/how word AND any of: ends with "?"; the wh-word is a
 * contraction ("what's"/"how's"/"whats" = "wh + is", inherently a question — so
 * "whats todays headlines" counts even with no "?"); OR is immediately followed
 * by an auxiliary verb ("where were you born").
 * Deliberately keeps statement-questions like "did you know I live in Berlin?"
 * (opens with an auxiliary, carries a fact) and "what I need is coffee"
 * (a bare wh-word not contracted, not followed by an auxiliary). Those are
 * worth remembering.
 */
function isPureQuestion(text: string): boolean {
  const t = text.trim();
  const words = t
    .toLowerCase()
    .replace(/[^a-z\s']/g, " ")
    .split(/\s+/)
    .filter(Boolean);
  if (words.length === 0) return false;
  const w0 = words[0];
  // Match the bare wh-word, allowing a trailing "is" contraction ("what's",
  // "whats", "how's", "hows").
  const bare = QUESTION_OPENERS.find(
    (w) => w0 === w || w0 === `${w}s` || w0 === `${w}'s`,
  );
  if (bare) {
    const contracted = w0 !== bare; // matched via "+s" / "+'s" (i.e. "what is")
    return t.endsWith("?") || contracted || QUESTION_AUX.includes(words[1] ?? "");
  }
  // Yes/no questions opening with an auxiliary ("is it raining", "do you like
  // jazz", "are you sure"). Treat as pure UNLESS the message carries a
  // first-person reference, which may hide a fact worth keeping ("did you know
  // I live in Berlin", "am I right that …").
  if (QUESTION_AUX.includes(w0)) {
    const FIRST_PERSON = new Set([
      "i", "i'm", "im", "i've", "ive", "my", "me", "myself", "mine",
    ]);
    return !words.some((w) => FIRST_PERSON.has(w));
  }
  return false;
}

/** Verbs that open a request/command TO the assistant ("write me a report",
 *  "summarize this", "explain X") — not memories about the user. */
const REQUEST_VERBS = new Set([
  "write", "summarize", "summarise", "explain", "describe", "draft", "compose",
  "create", "generate", "make", "build", "give", "list", "show", "tell",
  "translate", "rewrite", "rephrase", "fix", "debug", "refactor", "implement",
  "calculate", "compute", "convert", "compare", "analyze", "analyse", "find",
  "define", "outline", "expand", "continue", "elaborate", "suggest", "recommend",
  "draw", "design", "review", "format", "edit", "improve", "respond", "reply",
]);
/** Greeting / acknowledgment / politeness filler — nothing to recall. */
const FILLER = new Set([
  "hi", "hey", "hello", "yo", "sup", "greetings", "thanks", "thank", "thanx",
  "thx", "ty", "cheers", "great", "ok", "okay", "k", "cool", "nice", "sure",
  "yep", "yeah", "yes", "no", "nope", "good", "perfect", "awesome", "wow", "lol",
  "haha", "right", "fine", "alright", "please", "pls", "morning", "afternoon",
  "evening", "neat",
]);

/**
 * Low-value for episodic recall: a request/command to the assistant ("write me
 * a report on it"), or pure greeting/acknowledgment ("thanks", "ok great").
 * Substantive statements are kept (they don't open with a request verb), and any
 * fact buried in a command is still captured by the global fact extractor — so
 * skipping here only drops noise, never a durable memory about the user.
 */
function isCommandOrFiller(text: string): boolean {
  const words = text
    .toLowerCase()
    .replace(/[^a-z\s']/g, " ")
    .split(/\s+/)
    .filter(Boolean);
  if (words.length === 0) return true;
  if (words.every((w) => FILLER.has(w))) return true; // "thanks", "ok cool", "great"
  // Acknowledgments opening with thanks ("thank you", "thanks, that helps").
  if (["thank", "thanks", "thanx", "thx", "ty", "cheers"].includes(words[0])) return true;
  // Skip leading politeness/filler ("great, please …"), then test the first real
  // word for a request verb or a "go deeper/on/further" continuation.
  let i = 0;
  while (i < words.length && FILLER.has(words[i])) i++;
  if (i >= words.length) return true;
  if (REQUEST_VERBS.has(words[i])) return true;
  if (words[i] === "go" && ["deeper", "on", "further"].includes(words[i + 1] ?? "")) return true;
  return false;
}

/**
 * Index a conversation turn into this AI's recall store. We index the USER's
 * message only — that's the durable signal ("what the user told this AI").
 * Assistant replies are verbose and persona-flavoured, so recalling them back
 * just adds noise. Best-effort, fire-and-forget — no embedding model, no
 * indexing (recall stays empty until one is present). `assistantText` is
 * accepted but unused for now (kept so the call site is stable if we ever index
 * replies separately).
 */
export async function indexTurn(input: {
  aiId: string;
  conversationHash: string;
  userText: string;
  assistantText?: string;
}): Promise<void> {
  const { aiId, conversationHash } = input;
  if (!aiId || !conversationHash) return;
  if (!(await isEmbeddingModelReady())) return;

  const u = input.userText?.trim();
  if (!u) return;
  // Skip recall queries (pure questions) and low-value turns (commands to the
  // assistant, greetings/acknowledgments) — they're not memories about the user.
  if (isPureQuestion(u) || isCommandOrFiller(u)) {
    console.log("[Memory] Skipped indexing (question / command / filler)");
    return;
  }
  const entries = [{ role: "user", text: u.slice(0, MAX_TEXT) }];

  let vecs: number[][];
  try {
    vecs = await embedDocuments(entries.map((e) => e.text));
  } catch (e) {
    console.warn("[Memory] indexTurn embed failed:", e);
    return;
  }

  const now = Date.now() * 1000;
  const existing = await getEmb(aiId);
  entries.forEach((e, i) => {
    const v = vecs[i];
    if (v && v.length) {
      existing.push({
        id: crypto.randomUUID(),
        conversation_hash: conversationHash,
        role: e.role,
        text: e.text,
        vector: v,
        kind: "episodic",
        created_at: now,
      });
    }
  });

  // Cap episodic only - authored knowledge is never evicted by chat volume.
  const authored = existing.filter((e) => e.kind === "authored");
  const episodic = existing.filter((e) => e.kind !== "authored").slice(-MAX_PER_AI);
  const capped = [...authored, ...episodic];
  await saveEmb(aiId, capped);
  console.log(
    `[Memory] Indexed turn for AI ${aiId.slice(0, 8)} (${episodic.length} memories, ${authored.length} knowledge entries)`,
  );
}

/** Top relevant entries: absolute floor + a relative margin off the best match
 *  (drops weak seconds). Shared by episodic + authored recall. */
function topMatches(
  entries: TranscriptEmbedding[],
  queryVec: number[],
): TranscriptEmbedding[] {
  const scored = entries
    .map((e) => ({ e, score: cosineSimilarity(queryVec, e.vector) }))
    .filter((x) => x.score >= RECALL_THRESHOLD)
    .sort((a, b) => b.score - a.score);
  if (scored.length === 0) return [];
  const floor = Math.max(RECALL_THRESHOLD, scored[0].score - RECALL_MARGIN);
  return scored
    .filter((x) => x.score >= floor)
    .slice(0, MAX_RECALL)
    .map((x) => x.e);
}

/**
 * Recall relevant entries for this AI, split by kind: `episodic` (past
 * conversation turns, excluding the current one) and `authored` (knowledge the
 * user gave this AI — packs/lore). Each kind gets its own top-K so authored
 * knowledge isn't crowded out by chatter. Takes a pre-computed query vector.
 */
export async function recallPerAi(
  aiId: string,
  queryVec: number[] | null,
  excludeConversationHash?: string,
): Promise<{ episodic: string[]; authored: string[] }> {
  const empty = { episodic: [], authored: [] };
  if (!aiId || !queryVec) return empty;
  const all = await getEmb(aiId);
  if (all.length === 0) return empty;

  const episodicPool = all.filter(
    (e) =>
      e.kind !== "authored" &&
      (!excludeConversationHash || e.conversation_hash !== excludeConversationHash),
  );
  const authoredPool = all.filter((e) => e.kind === "authored");

  return {
    episodic: topMatches(episodicPool, queryVec).map((e) => e.text),
    authored: topMatches(authoredPool, queryVec).map((e) => e.text),
  };
}

/** This AI's remembered conversation turns (most recent first) — Remembers tab. */
export async function getAiMemories(aiId: string): Promise<TranscriptEmbedding[]> {
  const all = await getEmb(aiId);
  return all.filter((e) => e.kind !== "authored").reverse();
}

/** Knowledge the user has given this AI (most recent first) — the pack. */
export async function getAiKnowledge(aiId: string): Promise<TranscriptEmbedding[]> {
  const all = await getEmb(aiId);
  // Loose authored lore only - document chunks (which carry a `source`) are
  // surfaced grouped-by-document via listKnowledgeDocuments, not as individual
  // lines here.
  return all.filter((e) => e.kind === "authored" && !e.source).reverse();
}

/** Add a piece of authored knowledge to this AI (embedded for retrieval).
 *  Returns the new entry's id, or null if the embedding model isn't
 *  available (can't index it). */
export async function addKnowledge(aiId: string, text: string): Promise<string | null> {
  const t = text.trim();
  if (!aiId || !t) return null;
  if (!(await isEmbeddingModelReady())) return null;
  let vec: number[] | undefined;
  try {
    [vec] = await embedDocuments([t]);
  } catch (e) {
    console.warn("[Memory] addKnowledge embed failed:", e);
    return null;
  }
  if (!vec || !vec.length) return null;
  const all = await getEmb(aiId);
  const id = crypto.randomUUID();
  all.push({
    id,
    conversation_hash: "authored",
    role: "authored",
    text: t.slice(0, MAX_TEXT),
    vector: vec,
    kind: "authored",
    created_at: Date.now() * 1000,
  });
  await saveEmb(aiId, all);
  return id;
}

/** Index this AI's authored knowledge by text for cheap saved-state lookups:
 *  loose entries keyed by their text, documents keyed by their FIRST chunk's
 *  text (chunking is deterministic, so a full reply re-keys to the same
 *  chunk). One store read builds the whole map - per-message "is this already
 *  remembered?" checks then cost nothing. */
export async function authoredTextIndex(
  aiId: string,
): Promise<Map<string, { kind: "entry" | "doc"; id: string }>> {
  const map = new Map<string, { kind: "entry" | "doc"; id: string }>();
  if (!aiId) return map;
  const all = await getEmb(aiId);
  const seenDocs = new Set<string>();
  for (const e of all) {
    if (e.kind !== "authored") continue;
    if (!e.source) {
      map.set(e.text, { kind: "entry", id: e.id });
    } else if (!seenDocs.has(e.source.doc_id)) {
      seenDocs.add(e.source.doc_id);
      map.set(e.text, { kind: "doc", id: e.source.doc_id });
    }
  }
  return map;
}

/** Find already-saved authored knowledge by its exact text: a loose entry
 *  (short remembers) or a document (long remembers, matched on the first
 *  chunk — chunking is deterministic). Lets "Remember" buttons dedupe and
 *  reflect saved state across reloads. */
export async function findAuthoredByText(
  aiId: string,
  text: string,
): Promise<{ kind: "entry" | "doc"; id: string } | null> {
  const t = text.trim();
  if (!aiId || !t) return null;
  const all = await getEmb(aiId);
  if (t.length <= MAX_TEXT) {
    const entry = all.find((e) => e.kind === "authored" && !e.source && e.text === t);
    if (entry) return { kind: "entry", id: entry.id };
  }
  const first = chunkDocumentText(t)[0]?.slice(0, MAX_TEXT);
  if (first) {
    const chunk = all.find((e) => e.kind === "authored" && e.source && e.text === first);
    if (chunk?.source) return { kind: "doc", id: chunk.source.doc_id };
  }
  return null;
}

/** Chunk a document's text for retrieval: pack whole paragraphs/sentences into
 *  ~MAX_TEXT-char pieces so each embedded chunk is a coherent passage, with a
 *  sentence of overlap so a fact split across a boundary is still findable. */
export function chunkDocumentText(text: string): string[] {
  const clean = text.replace(/\r\n/g, "\n").replace(/[ \t]+/g, " ").trim();
  if (!clean) return [];
  // Split on blank lines first (paragraphs), then oversize paragraphs by sentence.
  const units: string[] = [];
  for (const para of clean.split(/\n\s*\n/)) {
    const p = para.trim();
    if (!p) continue;
    if (p.length <= MAX_TEXT) {
      units.push(p);
    } else {
      let buf = "";
      for (const sentence of p.split(/(?<=[.!?])\s+/)) {
        if (buf && (buf.length + sentence.length + 1) > MAX_TEXT) {
          units.push(buf.trim());
          buf = "";
        }
        // A single sentence longer than a chunk: hard-split it.
        if (sentence.length > MAX_TEXT) {
          for (let i = 0; i < sentence.length; i += MAX_TEXT) {
            units.push(sentence.slice(i, i + MAX_TEXT).trim());
          }
        } else {
          buf += (buf ? " " : "") + sentence;
        }
      }
      if (buf.trim()) units.push(buf.trim());
    }
  }
  // Merge tiny trailing fragments into the previous chunk for coherence.
  const chunks: string[] = [];
  for (const u of units) {
    if (chunks.length && (chunks[chunks.length - 1].length + u.length + 1) <= MAX_TEXT) {
      chunks[chunks.length - 1] += " " + u;
    } else {
      chunks.push(u);
    }
  }
  return chunks;
}

/** Ingest a document as this AI's knowledge: chunk, embed, and store every
 *  chunk tagged with a shared source so it can be listed/removed as one.
 *  Returns the docId + chunk count, or null if the embedding model isn't ready
 *  (the caller should tell the user to finish downloading it). */
export async function addDocumentKnowledge(
  aiId: string,
  filename: string,
  sizeBytes: number,
  fullText: string,
): Promise<{ docId: string; chunkCount: number } | null> {
  if (!aiId) return null;
  const chunks = chunkDocumentText(fullText);
  if (chunks.length === 0) return null;
  if (!(await isEmbeddingModelReady())) return null;

  let vecs: number[][];
  try {
    vecs = await embedInBatches(chunks);
  } catch (e) {
    console.warn("[Memory] addDocumentKnowledge embed failed:", e);
    return null;
  }
  if (vecs.length !== chunks.length) return null;

  const docId = crypto.randomUUID();
  const source: KnowledgeSource = { doc_id: docId, filename, size_bytes: sizeBytes };
  const now = Date.now() * 1000;
  const all = await getEmb(aiId);
  chunks.forEach((text, i) => {
    if (!vecs[i] || !vecs[i].length) return;
    all.push({
      id: crypto.randomUUID(),
      conversation_hash: "authored",
      role: "authored",
      text: text.slice(0, MAX_TEXT),
      vector: vecs[i],
      kind: "authored",
      created_at: now,
      source,
    });
  });
  await saveEmb(aiId, all);
  return { docId, chunkCount: chunks.length };
}

/** The documents this AI has been given, grouped from their chunks. */
export async function listKnowledgeDocuments(aiId: string): Promise<KnowledgeDocument[]> {
  const all = await getEmb(aiId);
  const byDoc = new Map<string, KnowledgeDocument>();
  for (const e of all) {
    if (e.kind !== "authored" || !e.source) continue;
    const existing = byDoc.get(e.source.doc_id);
    if (existing) {
      existing.chunkCount += 1;
    } else {
      byDoc.set(e.source.doc_id, {
        docId: e.source.doc_id,
        filename: e.source.filename,
        sizeBytes: e.source.size_bytes,
        chunkCount: 1,
        addedAt: e.created_at,
      });
    }
  }
  return [...byDoc.values()].sort((a, b) => b.addedAt - a.addedAt);
}

/** Remove one ingested document (all its chunks) from this AI's knowledge. */
export async function removeKnowledgeDocument(aiId: string, docId: string): Promise<void> {
  const all = await getEmb(aiId);
  const kept = all.filter((e) => e.source?.doc_id !== docId);
  await saveEmb(aiId, kept);
}

// ── Post-restore re-embed walker ─────────────────────────────────────

/** Batch size per embed_texts call - bounds request size on slow CPUs. */
const REEMBED_BATCH = 24;

async function embedInBatches(texts: string[]): Promise<number[][]> {
  const out: number[][] = [];
  for (let i = 0; i < texts.length; i += REEMBED_BATCH) {
    out.push(...(await embedDocuments(texts.slice(i, i + REEMBED_BATCH))));
  }
  return out;
}

/**
 * Rebuild the per-AI memory index after a device restore.
 *
 * A Vault restore replays conversations under fresh action hashes and wipes
 * the old embedding caches (they were keyed to the old material), and restored
 * authored knowledge arrives with empty vectors. This walks every AI once:
 * episodic recall is rebuilt from the local transcripts (the canonical
 * record), vectorless authored entries are re-embedded in place, and the
 * Rust-side marker is cleared. No-op unless the marker is set; if the
 * embedding model isn't downloaded yet the marker stays for a later pass
 * (next launch, or right after the model download in Settings → Components).
 */
export async function rebuildMemoryIndexIfPending(): Promise<void> {
  try {
    if (!(await invoke<boolean>("memory_reembed_pending"))) return;
  } catch {
    return;
  }
  if (!(await isEmbeddingModelReady())) {
    console.log("[Memory] Re-embed pending, but no embedding model yet - deferred");
    return;
  }

  const { getLocalCustomAis } = await import("./localAiStorage");
  const { getConversations, getTranscript } = await import("./holochainTranscripts");
  const ais = await getLocalCustomAis();
  let failed = false;

  for (const ai of ais) {
    const agentKey = ai.agentPubKey ?? ai.id;
    try {
      const existing = await getEmb(ai.id);
      const authored = existing.filter((e) => e.kind === "authored");

      // Episodic entries rebuild from the transcripts - same filters as
      // indexTurn, so the rebuilt store matches what live indexing builds.
      const turns: { text: string; conversationHash: string; createdAt: number }[] = [];
      for (const conv of await getConversations(agentKey)) {
        for (const entry of await getTranscript(conv.agent_key ?? agentKey, conv.hash)) {
          if (entry.role !== "user") continue;
          const t = entry.content?.trim();
          if (!t || isPureQuestion(t) || isCommandOrFiller(t)) continue;
          turns.push({
            text: t.slice(0, MAX_TEXT),
            conversationHash: conv.hash,
            createdAt: entry.timestamp,
          });
        }
      }
      turns.sort((a, b) => a.createdAt - b.createdAt);
      const kept = turns.slice(-MAX_PER_AI);

      const staleAuthored = authored.filter((e) => !e.vector.length);
      const toEmbed = [...staleAuthored.map((e) => e.text), ...kept.map((t) => t.text)];
      if (toEmbed.length === 0) continue;
      const vecs = await embedInBatches(toEmbed);

      let v = 0;
      for (const e of staleAuthored) {
        e.vector = vecs[v++] ?? [];
      }
      const episodic: TranscriptEmbedding[] = [];
      for (const t of kept) {
        const vec = vecs[v++];
        if (!vec || !vec.length) continue;
        episodic.push({
          id: crypto.randomUUID(),
          conversation_hash: t.conversationHash,
          role: "user",
          text: t.text,
          vector: vec,
          kind: "episodic",
          created_at: t.createdAt,
        });
      }
      // A transcript read that came back empty (conductor hiccup) must not
      // wipe episodic entries indexed since the restore - keep them instead.
      const finalEpisodic = kept.length
        ? episodic
        : existing.filter((e) => e.kind !== "authored");
      await saveEmb(ai.id, [...authored, ...finalEpisodic]);
      console.log(
        `[Memory] Rebuilt index for AI ${ai.id.slice(0, 8)}: ${finalEpisodic.length} turns, ${authored.length} knowledge entries`,
      );
    } catch (e) {
      console.warn(`[Memory] Re-embed failed for AI ${ai.id.slice(0, 8)}:`, e);
      failed = true;
    }
  }

  if (!failed) {
    try {
      await invoke("memory_reembed_done");
      console.log("[Memory] Post-restore re-embed complete");
    } catch (e) {
      console.warn("[Memory] Could not clear re-embed marker:", e);
    }
  }
}

/** Forget a single remembered turn by id. */
export async function deleteAiMemory(aiId: string, id: string): Promise<void> {
  const all = await getEmb(aiId);
  await saveEmb(aiId, all.filter((e) => e.id !== id));
}

/** Forget everything this AI remembers from conversations (user control). */
export async function clearAiMemory(aiId: string): Promise<void> {
  await saveEmb(aiId, []);
}
