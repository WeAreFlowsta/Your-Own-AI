/**
 * The activity tray's small event bus + labels.
 *
 * The tray (components/ActivityTray.tsx) draws every background job in one
 * bottom-right card: model, engine and Projects downloads (all of them ride
 * the `model-download-progress` event), document reading (`corpus-progress`),
 * card writing (documentSummaries), and one-off notes any feature wants to
 * show there - the first model's "is ready" line, a vision download's
 * outcome. Notes arrive as a window event so no feature needs the tray's
 * store.
 */
import { modelFamilies } from "../data/recommended-models";

export const ACTIVITY_EVENT = "yoai-activity";

export interface ActivityNote {
  /** Stable id: a second note with the same id replaces the first. */
  id: string;
  title: string;
  detail?: string;
  state: "done" | "error";
  /** How long the note stays before it clears itself; 0 = until dismissed. */
  ttlMs?: number;
}

/** Show a one-off line in the activity tray. */
export function announceActivity(note: ActivityNote): void {
  try {
    window.dispatchEvent(new CustomEvent<ActivityNote>(ACTIVITY_EVENT, { detail: note }));
  } catch {
    /* not in a window */
  }
}

/** A person's name for a downloaded file: the catalog's model name, or the
 *  component the file is. Falls back to the file name itself. */
export function labelForFile(filename: string): string {
  const f = filename.toLowerCase();
  if (f.startsWith("llama-server-cuda-")) return "CUDA engine";
  if (f.startsWith("your-own-ai-build-")) return "Projects helper";
  if (f.includes("mmproj")) return "Vision add-on";
  if (f.startsWith("bge-")) return "Memory model";
  for (const family of modelFamilies) {
    for (const v of family.variants) {
      if (v.filename === filename) return family.name;
    }
  }
  return filename.replace(/\.gguf$|\.zip$|\.tar\.gz$/i, "");
}

export function formatBytes(n: number): string {
  if (n >= 1024 ** 3) return `${(n / 1024 ** 3).toFixed(1)} GB`;
  if (n >= 1024 ** 2) return `${Math.round(n / 1024 ** 2)} MB`;
  return `${Math.round(n / 1024)} KB`;
}
