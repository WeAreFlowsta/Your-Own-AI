/**
 * Memory consolidation - the local "dreaming" pass.
 *
 * The leaders converged on this shape in 2026 (ChatGPT's Dreaming, Claude's
 * daily synthesis): memory works best as a continuously revised SUMMARY of
 * the person, refreshed in the background, not just an append-only fact
 * list. Ours runs entirely on-device, on the same utility model that does
 * extraction, and only when the facts actually changed.
 *
 * What it does, in order:
 *  1. Mechanical dedupe of EXTRACTED facts (same predicate + same value,
 *     case-insensitive) - keeps the earliest, retires the copies. Authored
 *     facts are never touched (stickiness is a guarantee, see B1).
 *  2. If the active fact/note set changed since the last synthesis, asks
 *     the utility model for a short third-person paragraph ("who this
 *     person is"), tense-corrected against today's date.
 *  3. Stores the paragraph as a special entry (entry_kind "synthesis") in
 *     the same encrypted store - so it inherits encryption, backup, and
 *     forget-all behaviour for free. Injection puts it before the facts.
 *
 * Precision lessons apply (the v3 extraction rewrite): the model only ever
 * WRITES PROSE here - it is never allowed to add, merge, or delete facts.
 * Structural changes stay mechanical; a weak model can't corrupt the store.
 */
import { invoke } from "@tauri-apps/api/core";
import { getFacts, saveFacts, type Fact } from "./memory";
import { isMemoryPaused } from "./memory";
import { runUtilityTask } from "./utilityModel";
import { modelFamilies, UTILITY_MODEL } from "../data/recommended-models";

/** Prose quality matters here (unlike extraction, which needs precision):
 *  when a clearly stronger LOCAL model is already loaded and warm, write
 *  the portrait with it instead of Ministral. Never loads anything, never
 *  goes online - the profile is the most sensitive input we hold. */
async function pickSynthesisModel(
  fallbackModel?: string,
): Promise<{ model?: string; preferLoaded: boolean }> {
  try {
    const loaded = await invoke<string | null>("get_current_model");
    if (
      loaded &&
      loaded.endsWith(".gguf") &&
      loaded !== UTILITY_MODEL.filename
    ) {
      for (const fam of modelFamilies) {
        for (const v of fam.variants) {
          if (v.filename === loaded) {
            const b = parseFloat(v.parameterCount);
            if (Number.isFinite(b) && b >= 7) {
              return { model: loaded, preferLoaded: true };
            }
          }
        }
      }
    }
  } catch {
    /* unknown loaded state - the utility path is always safe */
  }
  // The portrait is written on this device or not at all. The caller's
  // fallback is the model that took the turn, which for an online AI is an
  // online model - fine for extracting a fact from a turn that already went
  // online, never for rewriting the whole profile. Without the helper model
  // and without a local model, the rewrite waits (audit 2026-09-05).
  const local =
    fallbackModel && !fallbackModel.startsWith("online:") && !fallbackModel.startsWith("external:")
      ? fallbackModel
      : undefined;
  return { model: local, preferLoaded: false };
}

export const SYNTHESIS_KIND = "synthesis";
const LAST_RUN_KEY = "yoai-memory-consolidated-at";
const MIN_FACTS_FOR_SYNTHESIS = 3;
const MIN_RUN_GAP_MS = 5 * 60 * 1000;

/** Stable fingerprint of the active fact/note set the synthesis describes. */
function factsFingerprint(base: Fact[]): string {
  const parts = base
    .map((f) => `${f.entry_kind}|${f.predicate}|${f.value}`)
    .sort()
    .join("\n");
  // FNV-1a - collision-safe enough for a change detector.
  let h = 0x811c9dc5;
  for (let i = 0; i < parts.length; i++) {
    h ^= parts.charCodeAt(i);
    h = Math.imul(h, 0x01000193);
  }
  return (h >>> 0).toString(16);
}

/** The stored synthesis entry, if any. */
export function findSynthesis(facts: Fact[]): Fact | undefined {
  return facts.find((f) => f.entry_kind === SYNTHESIS_KIND && f.valid_to == null);
}

/** Retire exact duplicates among EXTRACTED facts (authored never touched).
 *  Returns the new set and whether anything changed. */
function dedupeExtracted(facts: Fact[]): { facts: Fact[]; changed: boolean } {
  const seen = new Map<string, Fact>();
  let changed = false;
  const now = Date.now() * 1000;
  for (const f of facts) {
    if (f.valid_to != null || f.entry_kind !== "fact" || f.provenance !== "extracted") {
      continue;
    }
    const key = `${f.predicate}|${f.value.trim().toLowerCase()}`;
    const first = seen.get(key);
    if (!first) {
      seen.set(key, f);
    } else {
      // Keep the earliest record of the fact; retire the echo.
      const [keep, retire] =
        first.created_at <= f.created_at ? [first, f] : [f, first];
      seen.set(key, keep);
      retire.valid_to = now;
      changed = true;
    }
  }
  return { facts, changed };
}

/**
 * Run the consolidation pass if anything changed. Cheap when nothing did
 * (one store read + a hash). Fire-and-forget from its call sites.
 */
export async function maybeConsolidateMemory(fallbackModel?: string): Promise<void> {
  try {
    if (isMemoryPaused()) return;
    const last = Number(localStorage.getItem(LAST_RUN_KEY) || 0);
    if (Date.now() - last < MIN_RUN_GAP_MS) return;

    let all = await getFacts();
    const deduped = dedupeExtracted(all);
    all = deduped.facts;

    const base = all.filter(
      (f) => f.valid_to == null && (f.entry_kind === "fact" || f.entry_kind === "note"),
    );
    const existing = findSynthesis(all);

    // Too little to say anything: retire a stale synthesis, save dedupe.
    if (base.filter((f) => f.entry_kind === "fact").length < MIN_FACTS_FOR_SYNTHESIS) {
      let changed = deduped.changed;
      if (existing) {
        existing.valid_to = Date.now() * 1000;
        changed = true;
      }
      if (changed) await saveFacts(all);
      return;
    }

    const fingerprint = factsFingerprint(base);
    if (existing && existing.group_id === fingerprint) {
      // Synthesis is current; persist any dedupe and stop.
      if (deduped.changed) await saveFacts(all);
      return;
    }
    localStorage.setItem(LAST_RUN_KEY, String(Date.now()));

    const today = new Date().toISOString().slice(0, 10);
    const factLines = base
      .filter((f) => f.entry_kind === "fact")
      .map((f) => `- ${f.predicate.replace(/_/g, " ")}: ${f.value}`)
      .join("\n");
    const noteLines = base
      .filter((f) => f.entry_kind === "note")
      .slice(0, 10)
      .map((f) => `- ${f.value}`)
      .join("\n");

    const system =
      "You maintain a short private profile summary for a person's own AI assistants. " +
      "Write 60-110 words of plain prose in the third person ('They ...'). " +
      "Use ONLY the facts and notes given - never invent, guess, or generalize beyond them. " +
      `Correct tenses against today's date (${today}): a past date means it happened. ` +
      "No lists, no headers, no preamble - output the paragraph only.";
    const user =
      `Facts:\n${factLines}\n` +
      (noteLines ? `Notes:\n${noteLines}\n` : "") +
      "Write the summary paragraph now.";

    const chosen = await pickSynthesisModel(fallbackModel);
    const text = (
      await runUtilityTask(system, user, undefined, 220, chosen.model, 45000, chosen.preferLoaded)
    ).trim();
    // A bad or empty synthesis must never replace a good one.
    if (text.length < 40 || text.length > 1200 || text.startsWith("-")) {
      if (deduped.changed) await saveFacts(all);
      console.warn("[Memory] consolidation produced an unusable summary - kept the old one");
      return;
    }

    const now = Date.now() * 1000;
    if (existing) {
      existing.value = text;
      existing.group_id = fingerprint;
      existing.updated_at = now;
    } else {
      all.push({
        id: `synthesis-${Date.now()}`,
        subject: "user",
        predicate: "profile_synthesis",
        value: text,
        confidence: 1,
        valid_from: now,
        owner_scope: "personal",
        key_scope: "personal",
        entry_kind: SYNTHESIS_KIND,
        provenance: "system",
        group_id: fingerprint,
        created_at: now,
        updated_at: now,
      });
    }
    await saveFacts(all);
    console.log("[Memory] consolidated: synthesis refreshed", { fingerprint });
  } catch (e) {
    console.warn("[Memory] consolidation skipped:", e);
  }
}
