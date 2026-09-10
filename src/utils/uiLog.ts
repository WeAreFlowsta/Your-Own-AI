/**
 * A line into the app log from the webview. Use it where what the person
 * SAW matters next to what the backend did (a conversation that did not
 * open, a dead click): diagnostics reports carry the app log, and the
 * browser console never leaves the machine. Never throws.
 */
import { invoke } from "@tauri-apps/api/core";

export function uiLog(line: string, level: "info" | "warn" | "error" = "info"): void {
  try {
    void invoke("ui_log", { level, line }).catch(() => {});
  } catch {
    /* not in the app */
  }
}
