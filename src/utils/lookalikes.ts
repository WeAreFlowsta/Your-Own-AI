/**
 * AIs that look alike. Two AIs are shown as one only when the person said
 * they are one (config_sync.rs). When two look like the same AI, the app
 * says so once, with how much each holds, and the person chooses; either
 * answer can be changed later. This is the page's side: read what there is
 * to ask, say it, and send the answer.
 */
import { invoke } from "@tauri-apps/api/core";

export type Lookalike = {
  a: string;
  b: string;
  name: string;
  /** "starter": asked with a card. "own": a quiet line on Your AIs only. */
  kind: "starter" | "own";
  a_conversations: number;
  b_conversations: number;
  /** null: made on this computer. A string: the device it came from ("" when unnamed). */
  a_from: string | null;
  b_from: string | null;
};
export type Merged = { leader: string; other: string; other_from: string | null; other_conversations: number };
export type Pairing = { lookalikes: Lookalike[]; merged: Merged[]; origins: Record<string, string | null> };

export const EMPTY_PAIRING: Pairing = { lookalikes: [], merged: [], origins: {} };

export async function readPairing(): Promise<Pairing> {
  try {
    return await invoke<Pairing>("ai_pairing");
  } catch {
    return EMPTY_PAIRING;
  }
}

/** "Merge them": one AI from now on, on every device. */
export const mergePair = (a: string, b: string) => invoke("ai_pair_merge", { a, b });
/** "Keep both" and "Separate them again": two AIs, not asked about again. */
export const keepPairApart = (a: string, b: string) => invoke("ai_pair_keep_apart", { a, b });

/** Where an AI is from, as the sentence says it: "this computer", a device's name, or "your account". */
export function placeOf(from: string | null): string {
  if (from === null) return "this computer";
  return from.trim() ? from.trim() : "your account";
}

/** The short tag beside a name: "this computer", "from Eric's laptop", "from your account". */
export function tagOf(from: string | null): string {
  return from === null ? "this computer" : `from ${placeOf(from)}`;
}

export function conversationsText(n: number): string {
  if (n <= 0) return "no conversations yet";
  return n === 1 ? "1 conversation" : `${n.toLocaleString()} conversations`;
}

/** The question, with this computer's AI first when one of the two is from here. */
export function pairQuestion(p: Lookalike): string {
  const sides = [
    { from: p.a_from, n: p.a_conversations },
    { from: p.b_from, n: p.b_conversations },
  ].sort((x, y) => (x.from === null ? 0 : 1) - (y.from === null ? 0 : 1));
  // Both from the same place (two on this computer): say the place once.
  if (placeOf(sides[0].from) === placeOf(sides[1].from)) {
    const where = sides[0].from === null ? "on this computer" : `from ${placeOf(sides[0].from)}`;
    return `You have two AIs named ${p.name} ${where}, one with ${conversationsText(sides[0].n)} and one with ${conversationsText(sides[1].n)}. Are they the same AI?`;
  }
  const [first, second] = sides.map((s) => `from ${placeOf(s.from)} (${conversationsText(s.n)})`);
  return `You have two AIs named ${p.name}. One ${first}, one ${second}. Are they the same AI?`;
}

/** An AI's address on the person's network, as the backend keys a pair. */
export const foundingOf = (ai: { foundingKey?: string; agentPubKey?: string }) => ai.foundingKey || ai.agentPubKey || "";
