/**
 * The update from the older memory: one pass in the background that
 * carries over what the person saved, sets aside what an AI had guessed,
 * and reports where each thing went. Nothing waits for it.
 */
import { invoke } from "@tauri-apps/api/core";
import { isMemoryPaused } from "./memory";

export interface UpdateReport {
  keptByYou: number;
  setAside: number;
  name: boolean;
  projectLines: number;
  projects: number;
  portraitRetired: boolean;
  written: number;
  tidyingStartsOff: boolean;
  doneAt: number;
}

export const UPDATE_EVENT = "yoai-memory-update";
const DISMISSED_KEY = "yoai-memory-update-line-dismissed";

export async function updateReport(): Promise<UpdateReport | null> {
  return invoke<UpdateReport | null>("memory_update_report").catch(() => null);
}

/** Was there anything from before the update? A new profile has nothing,
 *  and is told nothing. */
export function hadOlderMemory(r: UpdateReport | null): boolean {
  return !!r && (r.keptByYou + r.setAside + r.projectLines > 0 || r.name || r.portraitRetired);
}

/** How long the line stays when nobody presses anything. */
export const UPDATE_LINE_DAYS = 7;
const FIRST_SHOWN_KEY = `${DISMISSED_KEY}-first-shown`;

/** The line is over: dismissed, "See what changed" was opened, or it has
 *  been showing for a week. `now` is given by tests. */
export function updateLineDismissed(now: number = Date.now()): boolean {
  try {
    if (localStorage.getItem(DISMISSED_KEY) === "1") return true;
    const first = Number(localStorage.getItem(FIRST_SHOWN_KEY) ?? "");
    return first > 0 && now - first > UPDATE_LINE_DAYS * 86_400_000;
  } catch {
    return false;
  }
}

/** The line is on screen: the week it may stay starts the first time. */
export function updateLineShown(now: number = Date.now()): void {
  try {
    if (!localStorage.getItem(FIRST_SHOWN_KEY)) localStorage.setItem(FIRST_SHOWN_KEY, String(now));
  } catch {
    /* it stays until dismissed */
  }
}

export function dismissUpdateLine(): void {
  try {
    localStorage.setItem(DISMISSED_KEY, "1");
  } catch {
    /* it shows again next time */
  }
  window.dispatchEvent(new CustomEvent(UPDATE_EVENT));
}

let running = false;

/** Run the pass until it has finished once on this device. The records
 *  open a little after the app does, so a pass that cannot start yet is
 *  tried again; a profile with no AI yet is tried again at the next start. */
export async function runUpdateInBackground(): Promise<void> {
  if (running) return;
  running = true;
  try {
    if (await updateReport()) return;
    for (let attempt = 0; attempt < 12; attempt++) {
      await new Promise((r) => setTimeout(r, attempt === 0 ? 20_000 : 60_000));
      try {
        await invoke<UpdateReport>("memory_update_run", { learningWasPaused: isMemoryPaused() });
        window.dispatchEvent(new CustomEvent(UPDATE_EVENT));
        return;
      } catch (e) {
        console.debug("[memory] the update's pass will be tried again:", e);
      }
    }
  } finally {
    running = false;
  }
}
