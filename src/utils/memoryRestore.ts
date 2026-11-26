/**
 * What a restore from the Vault backup brought back to memory: a note the
 * app keeps until the person has seen it once.
 */
import { invoke } from "@tauri-apps/api/core";

export interface MemoryBack {
  /** When the restore ran (ms). */
  at: number;
  /** When the backup was made, as the backup says it (may be empty). */
  backupOf: string;
  items: number;
  rules: number;
}

export const RESTORE_NOTE_EVENT = "yoai-memory-restore-note";

export async function restoreNote(): Promise<MemoryBack | null> {
  return invoke<MemoryBack | null>("memory_restore_note").catch(() => null);
}

export async function restoreNoteSeen(): Promise<void> {
  await invoke("memory_restore_note_seen").catch(() => undefined);
  window.dispatchEvent(new CustomEvent(RESTORE_NOTE_EVENT));
}

/** "March 8", or nothing when the backup carries no date. */
export function backupDay(iso: string): string {
  const t = Date.parse(iso);
  return Number.isNaN(t) ? "" : new Date(t).toLocaleDateString(undefined, { month: "long", day: "numeric" });
}

const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

/** "120 kept items and 4 rules", "4 rules", or nothing. */
export function backWords(items: number, rules: number): string {
  const parts = [items > 0 ? count(items, "kept item", "kept items") : "", rules > 0 ? count(rules, "rule", "rules") : ""].filter(Boolean);
  return parts.join(" and ");
}

/** The first sentence under the heading. */
export function restoredFrom(note: MemoryBack): string {
  const day = backupDay(note.backupOf);
  const what = backWords(note.items, note.rules);
  const from = day ? `Restored from the backup of ${day}` : "Restored from your Vault backup";
  return what ? `${from}: ${what}.` : `${from}.`;
}

/** What is not in the backup, with its day when known. */
export function afterBackup(note: MemoryBack): string {
  const day = backupDay(note.backupOf);
  return `Anything from after ${day || "that backup"} is not in it. If another of your devices has it, it arrives when that device connects.`;
}
