/**
 * Measured generation speed per offline model, on THIS machine.
 *
 * The model card's speed claim must be this computer's number, never a
 * bench box's. Every offline turn long enough to measure feeds a per-file
 * moving average (localStorage - a per-machine convenience, not a record);
 * the Models page shows it as "~N tok/s measured". Resets with the browser
 * profile; a re-download or a different engine simply re-measures.
 */

const KEY = "modelSpeeds";
/** Turns shorter than this are dominated by startup/latency - not a speed. */
const MIN_TOKENS = 32;
/** Moving-average weight of the newest sample. */
const ALPHA = 0.3;

interface SpeedEntry {
  tps: number;
  samples: number;
  updatedAt: number;
}

function readAll(): Record<string, SpeedEntry> {
  try {
    return JSON.parse(localStorage.getItem(KEY) || "{}") || {};
  } catch {
    return {};
  }
}

/** Record one finished turn: tokens generated and the measured tokens/sec. */
export function recordModelSpeed(filename: string, completionTokens: number, tps: number): void {
  if (!filename || !(tps > 0) || completionTokens < MIN_TOKENS) return;
  try {
    const all = readAll();
    const prev = all[filename];
    const next: SpeedEntry = prev
      ? { tps: prev.tps + ALPHA * (tps - prev.tps), samples: prev.samples + 1, updatedAt: Date.now() }
      : { tps, samples: 1, updatedAt: Date.now() };
    all[filename] = next;
    localStorage.setItem(KEY, JSON.stringify(all));
  } catch {
    /* convenience metadata */
  }
}

/** Measured tokens/sec for a model file on this machine, if any. */
export function getModelSpeed(filename: string): number | null {
  const e = readAll()[filename];
  return e ? e.tps : null;
}

/** All measured speeds, filename -> tokens/sec. */
export function getModelSpeeds(): Record<string, number> {
  const out: Record<string, number> = {};
  for (const [k, v] of Object.entries(readAll())) out[k] = v.tps;
  return out;
}
