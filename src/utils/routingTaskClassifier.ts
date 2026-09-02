/**
 * Router V2 Phase 2-3 — routing TASK + DIFFICULTY classifier.
 *
 * Classifies a query by what KIND of model it wants (code/math/reasoning/general)
 * AND how hard it is (easy/hard), in ONE gated generative call on the utility
 * model. Task drives the offline model pick (code→coder); difficulty drives
 * online ESCALATION (hard → a stronger online model) — but only for an
 * online+offline Auto AI under freshness-leaning eagerness, so privacy/balanced
 * users never escalate on difficulty and pay nothing.
 *
 * Distinct from the report/code *mode* classifier (that's answer FORMAT — "write
 * me code"; this is query DOMAIN — "why is my code crashing" is still CODE).
 *
 * Gated so it stays free for users it can't help:
 *   1. SPECIALIST gate (Rust `routing_specialist_tasks`) for the task signal.
 *   2. DIFFICULTY only wanted for online+offline auto under freshness eagerness.
 *   3. KEYWORD pre-gate skips plainly-general messages.
 * If none apply → no model call at all.
 *
 * Task-aware routing signals for the Auto modes.
 */
import { invoke } from "@tauri-apps/api/core";
import { runUtilityTask, isUtilityModelReady } from "./utilityModel";

export type RoutingTask = "code" | "math" | "reasoning" | "general";
export type RoutingDifficulty = "easy" | "hard" | "unknown";
export interface RoutingSignals {
  task: RoutingTask;
  difficulty: RoutingDifficulty;
}

const GRAMMAR =
  'root ::= ("CODE" | "MATH" | "REASONING" | "GENERAL") " " ("EASY" | "HARD")';

import { CODE_KW, MATH_KW, REASONING_KW } from "./routingKeywords";

/** Coarse, free pre-gate: does the message look code/math/reasoning-ish at all? */
function looksRoutable(message: string): boolean {
  const m = ` ${message.toLowerCase()} `;
  return (
    CODE_KW.some((k) => m.includes(k)) ||
    MATH_KW.some((k) => m.includes(k)) ||
    REASONING_KW.some((k) => m.includes(k))
  );
}

const BASE = `Classify the user's message for model routing. Output exactly TWO words: the TASK then the DIFFICULTY.
TASK = CODE | MATH | REASONING | GENERAL.
  CODE = programming — writing, debugging, explaining, reviewing, or refactoring code; algorithms; APIs; shell/config.
  MATH = mathematics — calculations, equations, proofs, numeric/quantitative problems.
  REASONING = hard multi-step logic, planning, deep analysis, system design, puzzles.
  GENERAL = everything else — casual chat, facts, writing, advice, simple questions.
DIFFICULTY = HARD if it needs a powerful model (complex, multi-step, deep, expert-level); EASY otherwise (simple, short, routine, factual).

Examples:
write a python function to parse a csv => CODE EASY
implement a lock-free concurrent hashmap in rust => CODE HARD
why is my code throwing a null pointer exception => CODE EASY
what is the integral of x squared => MATH EASY
prove there are infinitely many primes => MATH HARD
plan a detailed multi-step migration strategy => REASONING HARD
design a fault-tolerant distributed system => REASONING HARD
what's the capital of France => GENERAL EASY
write me a short poem about the sea => GENERAL EASY
how's your day going => GENERAL EASY`;

/** Classify task + difficulty, or null (keyword-gated / unparseable). Never throws. */
export async function classifyRoutingSignals(
  message: string,
  model: string | undefined,
): Promise<RoutingSignals | null> {
  if (!looksRoutable(message)) return null;
  try {
    // 12 tokens: two grammar words, and "REASONING" alone is several tokens.
    const out = await runUtilityTask(BASE, message, GRAMMAR, 12, model, 6000);
    const [t, d] = out.trim().toUpperCase().split(/\s+/);
    const task: RoutingTask | null =
      t === "CODE" ? "code"
        : t === "MATH" ? "math"
          : t === "REASONING" ? "reasoning"
            : t === "GENERAL" ? "general"
              : null;
    if (!task) return null;
    return { task, difficulty: d === "HARD" ? "hard" : "easy" };
  } catch {
    return null;
  }
}

// The specialist set is stable within a session (changes only when models are
// added/removed). Cache the one Rust call; reload picks up new specialists.
let specialistCache: Promise<string[]> | null = null;
function getSpecialistTasks(): Promise<string[]> {
  if (!specialistCache) {
    specialistCache = invoke<string[]>("routing_specialist_tasks").catch(() => []);
  }
  return specialistCache;
}
/** Call after a model download/removal so the gate re-evaluates. */
export function invalidateRoutingSpecialistCache(): void {
  specialistCache = null;
}
if (typeof window !== "undefined") {
  // modelManager announces downloads and removals; the gate re-evaluates.
  window.addEventListener("localModelsChanged", invalidateRoutingSpecialistCache);
}

/**
 * Resolve the signals to hand `route_model`. Classifies at most once, ONLY
 * on the helper model, and only when the verdict can change the outcome
 * (a task specialist is installed, or difficulty is wanted).
 *
 * Without the helper model there is no call at all: the classifier used to
 * fall back to a blocking grammar call on the chat server before every
 * routed turn (a measured multi-second stall on a cold server). Then the
 * task is the free keyword hint and difficulty is `unknown`; the router
 * treats unknown as "not hard" and, in frontier-first, "go online".
 */
export async function resolveRoutingSignals(
  message: string,
  model: string | undefined,
  baseTask: RoutingTask,
  wantDifficulty: boolean,
): Promise<RoutingSignals> {
  if (!(await isUtilityModelReady())) {
    return { task: baseTask, difficulty: "unknown" };
  }
  const specialists = await getSpecialistTasks();
  const wantTask = specialists.length > 0;
  if (!wantTask && !wantDifficulty) {
    return { task: baseTask, difficulty: "unknown" };
  }
  const s = await classifyRoutingSignals(message, model);
  const task =
    wantTask && s?.task && specialists.includes(s.task) ? s.task : baseTask;
  const difficulty = wantDifficulty && s?.difficulty ? s.difficulty : "unknown";
  return { task, difficulty };
}
