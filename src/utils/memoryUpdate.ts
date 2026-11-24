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

export function updateLineDismissed(): boolean {
  try {
    return localStorage.getItem(DISMISSED_KEY) === "1";
  } catch {
    return false;
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
