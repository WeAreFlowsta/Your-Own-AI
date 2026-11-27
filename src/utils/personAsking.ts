import { invoke } from "@tauri-apps/api/core";

/**
 * The person has just asked something. Background writing that shares the
 * processor with their question (a document's card on the helper model)
 * steps aside at once and stays away for a while, so the question's search
 * and its reply are not kept waiting behind work nobody asked for.
 */
let lastAsked = 0;

/** How long background writing stays away after a question. */
export const ASIDE_FOR_MS = 120_000;

/** Called when a turn starts. Never throws, never waits. */
export function personAsked(): void {
  lastAsked = Date.now();
  void invoke("helper_step_aside").catch(() => {});
}

/** Has a question been asked within the last ASIDE_FOR_MS? */
export function askedLately(now = Date.now()): boolean {
  return lastAsked > 0 && now - lastAsked < ASIDE_FOR_MS;
}

/** Wait until no question has been asked for ASIDE_FOR_MS. */
export async function whileAsking(): Promise<void> {
  while (askedLately()) await new Promise((r) => setTimeout(r, 10_000));
}
