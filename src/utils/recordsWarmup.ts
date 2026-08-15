/**
 * Records warmup - the honest answer to "is an EMPTY conversation list
 * believable yet?"
 *
 * After launch, the conductor enables an AI's cell and its app websocket
 * connects within seconds - but the cell serves its records only once it
 * has finished loading them from disk, which can take a minute or more on
 * a slow or busy machine. During that window a zome read SUCCEEDS with an
 * empty list. Rendering that as "0 tamper-proof conversations" or "Nothing
 * yet" against a history the user knows exists reads as data loss - the one
 * message a records product must never send by accident.
 *
 * So: an empty result inside the grace window is "still warming" - keep
 * polling and show a warmup state; a NON-empty result ends the warmup for
 * good; only past the window (or after a real result) is zero honest.
 *
 * Module-level: the window is measured from the first import, i.e. app
 * launch (single-window desktop app), and "warmed" is remembered for the
 * session so later navigations never re-enter the warmup state.
 */

const GRACE_MS = 90_000;
const startedAt = Date.now();
let seenRecords = false;

/** Call when any conversation read returned a non-empty result. */
export function noteRecordsSeen(): void {
  seenRecords = true;
}

/** True while an empty result should be treated as "not yet", not "none". */
export function emptyMayBeWarmup(): boolean {
  return !seenRecords && Date.now() - startedAt < GRACE_MS;
}

/** Poll delay while warming (ms). */
export const WARMUP_POLL_MS = 3_000;
