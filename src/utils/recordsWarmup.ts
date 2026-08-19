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

/** Warmup-aware records read for surfaces whose reads silently return []
 *  while the conductor starts (their utils catch and swallow the error).
 *
 *  Polls holochain_ready first - "the conductor says it's starting" is a
 *  fact, not a guess - then reads; an empty result inside the grace
 *  window keeps polling. Resolves with the first trustworthy result:
 *  non-empty, or empty once warming is genuinely over. `onWarming`
 *  drives the caller's "records are warming up" line; `alive` lets an
 *  unmounting component stop the loop. Deadline-capped so a conductor
 *  that never comes up can't poll forever.
 */
export async function readThroughWarmup<T>(
  read: () => Promise<T[]>,
  onWarming: (warming: boolean) => void,
  alive: () => boolean = () => true,
): Promise<T[]> {
  const { invoke } = await import("@tauri-apps/api/core");
  const deadline = Date.now() + 5 * 60_000;
  let last: T[] = [];
  while (alive() && Date.now() < deadline) {
    // Outside Tauri (plain-browser dev) the probe throws - treat as ready.
    const ready = await invoke<boolean>("holochain_ready").catch(() => true);
    if (ready) {
      last = await read();
      if (last.length > 0) {
        noteRecordsSeen();
        onWarming(false);
        return last;
      }
      if (!emptyMayBeWarmup()) {
        onWarming(false);
        return last;
      }
    }
    onWarming(true);
    await new Promise((r) => setTimeout(r, WARMUP_POLL_MS));
  }
  onWarming(false);
  return last;
}
