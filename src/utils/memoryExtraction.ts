/**
 * Phase A memory extraction — learn durable facts about the user from a
 * conversation turn and reconcile them into the encrypted profile store.
 *
 * Prefers the on-device utility model (Settings → Components); otherwise rides
 * the model the caller chose — a warm local model, or the online model for an
 * online AI (the turn already went to the cloud, so this adds no new exposure).
 * Offline AIs always stay on-device (the caller enforces the wall). The frontend
 * owns this; Rust is a dumb store. See ON_DEVICE_COMPONENTS.md.
 */
import { runUtilityTask, isUtilityModelReady } from "./utilityModel";
import {
  getFacts,
  saveFacts,
  newFact,
  isMemoryPaused,
  isSingleValuedPredicate,
  addPendingTurn,
  removePendingTurn,
  getPendingTurns,
  type Fact,
} from "./memory";
import type { ChatMessage } from "../types";

// NOTE: every part of this prompt + the user-message-only input is load-bearing
// for WEAK local models (tested on Ministral-3-3B, the model that fits a 4 GB
// GPU). Hard-won via runtime evals on 2026-06-18:
//  - Feeding the turn as one "User: …\nAssistant: …" message made the model
//    mis-attribute facts to the assistant ("name not extracted from the
//    assistant's response") → name went to 0/4. Passing ONLY the user's
//    message (the caller does this) fixed it.
//  - "counts even briefly / as a question" + the question-form and casual-name
//    examples were needed — without them "did you know I live in X?" and
//    "my name's X" returned []. The math/greeting examples keep negatives at 0.
//  - Small models still intermittently return [] or malformed JSON on real
//    facts, so extraction retries once on empty (see runExtraction).
//  - The predicate is constrained to a fixed VOCABULARY (EXTRACTION_GRAMMAR's
//    `pred` enum + this prompt). Free-form predicates were a disaster: the
//    model invented `current_lives_in` (so a move didn't supersede `lives_in`)
//    and `prefers_acronyms_or_names_for_projects` (which matched the name check
//    and made the injected name "YOAI"). The enum keeps reconcile + name
//    detection sane and cuts fragmentation. Keep the enum, prompt, grammar, and
//    memory.ts's isNamePredicate/isSingleValuedPredicate in sync.
//  - Third-party facts must NOT hit the user's identity slots. "my partner's
//    name is Alex" was mapped to name_is=Alex → it superseded the user's real
//    name; "my friend Tom lives in Perth" → lives_in=Perth clobbered the user's
//    home. The prompt now reserves name_is/lives_in/born_in/age_is for the USER
//    and routes other people to `has` (e.g. has: "partner named Alex"), plus
//    "only what was actually said" to curb hallucinated padding.
// Re-measure the hit rate on every model/engine bump; don't trust it to "look right".
const EXTRACTION_SYSTEM_PROMPT = `You extract durable facts the USER states ABOUT THEMSELVES, for an AI's long-term memory. A fact is something true about the user's own life: their name, where they live, their job/projects, an explicit preference or trait, a goal they state, or a person/thing they have.

Extract ONLY what the user plainly states about themselves. When unsure, extract nothing.

CRITICAL — a question, a request, a task, or small talk is NOT a fact, so output []:
- Asking you to DO something ("write me a report on X", "write code to…", "explain X") — output [].
- Asking a question ("how does X work?", "what's the weather?") — output [].
- Praise, thanks, or compliments about you or your answer ("thanks", "that's helpful", "great job", "go deeper") — output [].
- Work-in-progress talk about code, builds, bugs, files, or a task at hand ("fix the login bug", "my build is failing", "we should rename X", "i pushed the change") — output []. A durable occupation ("I'm a nurse", "I'm building an app for florists") IS a fact; the day's task list is NOT.
Asking ABOUT a topic does NOT mean they like, dislike, prefer, or are interested in it. (But a fact phrased as a question still counts: "did you know I live in X?" states they live in X.)

likes / dislikes / prefers / interested_in: ONLY from an explicit statement of feeling ("I love jazz", "I can't stand mondays", "I'm really into woodworking") — NEVER inferred from a question, request, or compliment.
works_on: the user's job, role, or projects ("I'm a teacher", "I'm building an app").
name_is / lives_in / born_in / age_is: the USER THEMSELVES only — never another person. Someone else (partner, family, friend, pet, colleague) goes under "has" (e.g. has: "partner named Alex", has: "friend Tom who lives in Perth").

Output ONLY a JSON array. Each item: {"subject": "user", "predicate": "<relation>", "value": "<the fact>", "confidence": <0-1>}. Use ONLY these relations: name_is, lives_in, born_in, age_is, works_on, likes, dislikes, prefers, goal_is, interested_in, has, is. Keep each value short.

message: "I'm Sam, a nurse in Leeds, and I love hiking on weekends."
[{"subject":"user","predicate":"name_is","value":"Sam","confidence":1},{"subject":"user","predicate":"works_on","value":"nurse","confidence":0.9},{"subject":"user","predicate":"lives_in","value":"Leeds","confidence":0.9},{"subject":"user","predicate":"likes","value":"hiking on weekends","confidence":0.8}]
message: "hey my name's Jordan"
[{"subject":"user","predicate":"name_is","value":"Jordan","confidence":1}]
message: "i love jazz"
[{"subject":"user","predicate":"likes","value":"jazz","confidence":0.9}]
message: "my partner's name is Alex"
[{"subject":"user","predicate":"has","value":"partner named Alex","confidence":0.9}]
message: "did you know I'm a teacher?"
[{"subject":"user","predicate":"works_on","value":"teacher","confidence":0.9}]
message: "did you know I live in Brindale?"
[{"subject":"user","predicate":"lives_in","value":"Brindale","confidence":0.9}]
message: "write me a report on the housing market"
[]
message: "i think we should rename the config struct and add a retry to the fetch call"
[]
message: "my build is failing again - can you check the logs?"
[]
message: "how does HTTPS work?"
[]
message: "thanks, that's really helpful!"
[]
message: "go deeper on section 2"
[]
message: "what's 17 times 23?"
[]`;

const MIN_CONFIDENCE = 0.6;
const MAX_EXTRACTION_TOKENS = 512;

// GBNF grammar that forces the local model to emit ONLY a JSON array of fact
// objects (or []). Guarantees schema-valid, parseable output regardless of
// model — retires the malformed-JSON misses weak models occasionally produce.
// Char classes (`["]`, `[^"]`) avoid fragile quote-escaping. The model can
// still choose [] (empty array) when there's nothing to extract, so this
// constrains FORMAT, not CONTENT — the prompt + retry-on-empty still drive
// whether a fact is found. Local llama.cpp only; the online path ignores it.
const EXTRACTION_GRAMMAR = `root ::= "[" ws ( fact ( ws "," ws fact )* ws )? "]"
fact ::= "{" ws qt "subject" qt ws ":" ws qt "user" qt ws "," ws qt "predicate" qt ws ":" ws pred ws "," ws qt "value" qt ws ":" ws str ws "," ws qt "confidence" qt ws ":" ws number ws "}"
pred ::= qt ( "name_is" | "lives_in" | "born_in" | "age_is" | "works_on" | "likes" | "dislikes" | "prefers" | "goal_is" | "interested_in" | "has" | "is" ) qt
qt ::= ["]
str ::= qt [^"]* qt
number ::= [0-9] ( "." [0-9]+ )?
ws ::= [ \t\n\r]*`;

interface FactCandidate {
  subject: string;
  predicate: string;
  value: string;
  confidence: number;
}

/** Tolerantly pull a JSON array of candidates out of the model's output. */
function parseFactCandidates(text: string): FactCandidate[] {
  const start = text.indexOf("[");
  const end = text.lastIndexOf("]");
  if (start === -1 || end === -1 || end <= start) return [];
  try {
    const parsed = JSON.parse(text.slice(start, end + 1));
    if (!Array.isArray(parsed)) return [];
    return parsed.filter(
      (c) =>
        c &&
        typeof c.subject === "string" &&
        typeof c.predicate === "string" &&
        typeof c.value === "string" &&
        typeof c.confidence === "number"
    );
  } catch {
    return [];
  }
}

/** One extraction pass. Prefers the on-device utility model; else rides
 *  `model` (the fallback the caller chose, respecting the offline/online wall).
 *  30s guard so a stuck server can't leak this fire-and-forget call. */
async function completeOnce(
  messages: ChatMessage[],
  systemPrompt: string,
  model: string
): Promise<string> {
  const userText = messages.map((m) => m.content).join("\n");
  return runUtilityTask(
    systemPrompt,
    userText,
    EXTRACTION_GRAMMAR,
    MAX_EXTRACTION_TOKENS,
    model,
    30000,
  );
}

const FIRST_PERSON = /\b(i|i'm|im|i've|ive|i'd|i'll|my|me|mine|myself)\b/i;

/** Cheap pre-gate: could this message state a personal fact? Skips messages
 *  with no first-person reference (most questions/requests/lookups), so
 *  extraction never spends a model call on them. Conservative — anything with
 *  "I/my/me" passes and the extractor's precision is the backstop. */
export function looksLikeUserFact(message: string): boolean {
  return FIRST_PERSON.test(message);
}

/**
 * Pre-warm the on-device utility model: load it AND prime the prompt cache with
 * the (constant) extraction system prompt, so the first real fact extraction
 * skips the model load (~2s) AND the prompt eval (~11s on a cold CPU). The slow
 * part of a first extraction is then just generation. Best-effort and only when
 * the utility model is installed — it never loads or touches the chat model
 * (which lives on the GPU), so it can't slow a chat query. Call it once, a few
 * seconds after the app settles, NOT during the busy startup.
 */
export async function prewarmExtractionModel(): Promise<void> {
  try {
    if (!(await isUtilityModelReady())) return;
    await runUtilityTask(EXTRACTION_SYSTEM_PROMPT, "hello", EXTRACTION_GRAMMAR, 4, undefined, 60000);
    console.log("[Memory] Utility model pre-warmed");
  } catch {
    /* best-effort — a failed warm-up just means the first real extraction is cold */
  }
}

const norm = (s: string) => s.trim().toLowerCase();

/**
 * Reconcile candidates into the existing fact set (Mem0-style): ADD new,
 * supersede on changed value, NOOP on duplicate. Returns the merged set, or
 * null if nothing changed.
 */
function reconcile(
  existing: Fact[],
  candidates: FactCandidate[],
  sourceActionHash: string | null,
  aiId: string
): Fact[] | null {
  const facts = [...existing];
  let changed = false;
  const now = Date.now() * 1000;

  for (const c of candidates) {
    if (c.confidence < MIN_CONFIDENCE) continue;
    // Currently-valid facts with the same subject + predicate.
    const matches = facts.filter(
      (f) =>
        (f.valid_to == null || f.valid_to === undefined) &&
        norm(f.subject) === norm(c.subject) &&
        norm(f.predicate) === norm(c.predicate)
    );
    const sameValue = matches.find((f) => norm(f.value) === norm(c.value));
    if (sameValue) {
      // NOOP — already known. Keep the higher confidence, freshen the time.
      const bumped = Math.max(sameValue.confidence, c.confidence);
      if (bumped !== sameValue.confidence) {
        sameValue.confidence = bumped;
        sameValue.updated_at = now;
        changed = true;
      }
      continue;
    }
    // Different value. For single-valued predicates (name, home) this is a
    // correction → supersede the old (kept as history). For multi-valued ones
    // (likes, projects, goals) it's an addition → keep both.
    if (isSingleValuedPredicate(c.predicate)) {
      // Authored facts are sticky: the user set this deliberately, so an
      // extracted guess never overrides it. Drop the candidate, keep authored.
      if (matches.some((m) => m.provenance === "authored")) {
        continue;
      }
      for (const m of matches) {
        m.valid_to = now;
        m.updated_at = now;
      }
    }
    facts.push(
      newFact({
        subject: c.subject,
        predicate: c.predicate,
        value: c.value,
        confidence: c.confidence,
        sourceActionHash,
        sourceAiId: aiId,
      })
    );
    changed = true;
  }
  return changed ? facts : null;
}

/**
 * In-flight extractions, tracked at module scope (which survives Qwik route
 * navigation). Lets the Memory page wait for a fact still being learned from
 * the turn the user just sent, instead of reading the store too early and
 * showing stale/empty until a manual reload.
 */
const inFlight = new Set<Promise<void>>();

/** Resolves once every currently-running extraction has settled. */
export function memoryExtractionIdle(): Promise<void> {
  return Promise.allSettled([...inFlight]).then(() => undefined);
}

/** True while any extraction is running (drives the Ask-row "Remembering"
 *  hint - background work the user can otherwise only feel as a pause). */
export function memoryExtractionBusy(): boolean {
  return inFlight.size > 0;
}

function announceBusy(): void {
  try {
    window.dispatchEvent(
      new CustomEvent('memoryExtractionChanged', { detail: { busy: inFlight.size > 0 } }),
    );
  } catch {
    /* non-browser context */
  }
}

/**
 * Extract durable user facts from the user's message and reconcile them into
 * the store. Fire-and-forget; swallows its own errors. Runs on a LOCAL model
 * (the caller passes one even for online chats — see useChat).
 */
/** A signed action hash, known now (string), absent (null), or arriving from
 *  the parallel transcript write (promise — see useChat). */
type SourceHash = string | null | Promise<string | null>;

/** Resolve the provenance hash without ever hanging extraction: a pending
 *  transcript write falls back to null after a short wait (unlinked fact). */
function resolveSourceHash(h: SourceHash): Promise<string | null> {
  if (h === null || typeof h === "string") return Promise.resolve(h);
  return Promise.race([
    h,
    new Promise<null>((res) => setTimeout(() => res(null), 8000)),
  ]).catch(() => null);
}

export function extractAndStoreFacts(input: {
  userText: string;
  model: string; // a LOCAL model filename
  aiId: string;
  sourceActionHash: SourceHash;
}): Promise<void> {
  // Persist the turn so it survives an app close mid-extraction; clear it once
  // extraction settles (whether or not it found facts).
  const pendingId = addPendingTurn({ userText: input.userText, aiId: input.aiId });
  const p = runExtraction(input).finally(() => {
    inFlight.delete(p);
    removePendingTurn(pendingId);
    announceBusy();
    // Consolidation rides every extraction: hash-gated, so a turn that
    // taught nothing costs one store read.
    import("./memoryConsolidation").then((m) => void m.maybeConsolidateMemory(input.model));
  });
  inFlight.add(p);
  announceBusy();
  return p;
}

/**
 * On launch, re-run extraction for any turns whose extraction didn't finish
 * before the app closed. Runs on the given loaded local model; reconcile makes
 * replays idempotent (a turn already learned just NOOPs). Called once the chat
 * route has a local model ready.
 */
export async function drainPendingExtractions(model: string): Promise<void> {
  const pending = getPendingTurns();
  if (pending.length === 0) return;
  console.log(
    `[Memory] Draining ${pending.length} unfinished extraction(s) from a previous session…`,
  );
  for (const t of pending) {
    try {
      await runExtraction({
        userText: t.userText,
        model,
        aiId: t.aiId,
        sourceActionHash: null, // the original signed hash is gone post-restart
      });
    } catch (e) {
      console.warn("[Memory] Pending drain failed:", e);
    }
    removePendingTurn(t.id);
  }
}

async function runExtraction(input: {
  userText: string;
  model: string;
  aiId: string;
  sourceActionHash: SourceHash;
}): Promise<void> {
  try {
    if (isMemoryPaused()) return;
    // Pass ONLY the user's message — including the assistant's reply made weak
    // local models mis-attribute the user's facts to the assistant (see the
    // prompt note). The reply is the AI's answer anyway, not fact context.
    const messages: ChatMessage[] = [{ role: "user", content: input.userText }];

    // Small local models intermittently return [] or malformed JSON on a real
    // fact (esp. terse/casual phrasings), so sample twice before giving up.
    let candidates = parseFactCandidates(
      await completeOnce(messages, EXTRACTION_SYSTEM_PROMPT, input.model),
    );
    if (candidates.length === 0) {
      candidates = parseFactCandidates(
        await completeOnce(messages, EXTRACTION_SYSTEM_PROMPT, input.model),
      );
    }
    if (candidates.length === 0) return;

    // Generation is done (the slow part) — the parallel transcript write has
    // usually resolved the hash by now, so this adds ~no latency.
    const sourceActionHash = await resolveSourceHash(input.sourceActionHash);
    const existing = await getFacts();
    const merged = reconcile(existing, candidates, sourceActionHash, input.aiId);
    if (merged) {
      await saveFacts(merged);
      console.log(
        `[Memory] Learned from turn; profile now ${merged.length} fact(s)`
      );
    }
  } catch (e) {
    console.warn("[Memory] Extraction failed:", e);
  }
}
