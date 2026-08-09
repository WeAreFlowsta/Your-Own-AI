/**
 * Stage 2 of conversation import: the background distiller.
 *
 * Walks an imported archive's USER messages through the existing fact
 * extractor (memoryExtraction.ts) so the profile learns the user from their
 * history. Module-scope, so it keeps running while the user navigates; a
 * per-message cursor in localStorage survives restarts and resumes on next
 * launch (resumeIfPending, called from the root layout).
 *
 * Deliberately facts-only for now: per-AI episodic summaries and writing
 * imported conversations to the chain wait on the memory-hardening pass
 * (see build-docs CONVERSATION_IMPORT.md). Requires the on-device utility
 * model; if it isn't downloaded yet the cursor stays put and the run
 * resumes automatically on a later launch - same contract as the
 * post-restore re-embed walker.
 */
import { invoke } from "@tauri-apps/api/core";
import { looksLikeUserFact, extractAndStoreFacts } from "./memoryExtraction";
import { isUtilityModelReady } from "./utilityModel";

const CURSOR_KEY = "yoai-import-distill-cursor";
const DONE_KEY = "yoai-import-distilled";
/** Bound per-message text handed to the extractor - imported messages can be
 *  arbitrarily long; the useful personal facts live early in a message. */
const MAX_EXTRACT_CHARS = 4000;
/** Breather between model calls so a long distill never starves the UI or a
 *  live chat's own extraction. */
const PACE_MS = 300;

export interface DistillStatus {
  state: "idle" | "waiting-model" | "running" | "done" | "error";
  archiveId: string | null;
  source: string | null;
  processed: number;
  total: number;
  error: string | null;
}

interface Cursor {
  archiveId: string;
  msgIndex: number;
  total: number;
  source: string;
}

interface ArchiveContent {
  id: string;
  source: string;
  conversations: { messages: { role: string; text: string }[] }[];
}

let running = false;
let status: DistillStatus = {
  state: "idle",
  archiveId: null,
  source: null,
  processed: 0,
  total: 0,
  error: null,
};
const listeners = new Set<(s: DistillStatus) => void>();

export function getDistillStatus(): DistillStatus {
  return { ...status };
}

export function subscribeDistill(cb: (s: DistillStatus) => void): () => void {
  listeners.add(cb);
  cb(getDistillStatus());
  return () => listeners.delete(cb);
}

function notify(partial: Partial<DistillStatus>) {
  status = { ...status, ...partial };
  const snapshot = getDistillStatus();
  listeners.forEach((cb) => {
    try {
      cb(snapshot);
    } catch {
      // A broken listener must not stop the distiller.
    }
  });
}

function readCursor(): Cursor | null {
  try {
    const raw = localStorage.getItem(CURSOR_KEY);
    return raw ? (JSON.parse(raw) as Cursor) : null;
  } catch {
    return null;
  }
}

function writeCursor(c: Cursor | null) {
  try {
    if (c) localStorage.setItem(CURSOR_KEY, JSON.stringify(c));
    else localStorage.removeItem(CURSOR_KEY);
  } catch {
    // localStorage full/unavailable - the run continues, resume just won't.
  }
}

/** Archives whose distillation completed (for the "learned" badge). */
export function distilledArchiveIds(): string[] {
  try {
    const raw = localStorage.getItem(DONE_KEY);
    return raw ? (JSON.parse(raw) as string[]) : [];
  } catch {
    return [];
  }
}

function markDone(archiveId: string) {
  try {
    const ids = new Set(distilledArchiveIds());
    ids.add(archiveId);
    localStorage.setItem(DONE_KEY, JSON.stringify([...ids]));
  } catch {
    // Badge-only state; safe to lose.
  }
}

/** Forget the done/cursor state for a deleted archive. */
export function clearDistillState(archiveId: string) {
  try {
    localStorage.setItem(
      DONE_KEY,
      JSON.stringify(distilledArchiveIds().filter((id) => id !== archiveId)),
    );
    const cursor = readCursor();
    if (cursor?.archiveId === archiveId) writeCursor(null);
  } catch {
    // Best-effort.
  }
}

function sleep(ms: number): Promise<void> {
  return new Promise((r) => setTimeout(r, ms));
}

/** Flatten an archive to its user messages - the distiller's work list.
 *  Deterministic, so the cursor's flat index is stable across restarts. */
function userMessages(archive: ArchiveContent): string[] {
  const out: string[] = [];
  for (const conv of archive.conversations) {
    for (const m of conv.messages) {
      if (m.role === "user" && m.text.trim()) out.push(m.text);
    }
  }
  return out;
}

/** Begin (or re-begin) distilling an archive. One archive at a time; a new
 *  request replaces an unfinished one (the old cursor is superseded). */
export async function startDistill(archiveId: string): Promise<void> {
  if (distilledArchiveIds().includes(archiveId)) return;
  const current = readCursor();
  if (!current || current.archiveId !== archiveId) {
    writeCursor({ archiveId, msgIndex: 0, total: 0, source: "" });
  }
  void ensureLoop();
}

/** Resume an interrupted distill - call once from the root layout. */
export function resumeIfPending(): void {
  if (readCursor()) void ensureLoop();
}

async function ensureLoop(): Promise<void> {
  if (running) return;
  running = true;
  try {
    let cursor = readCursor();
    while (cursor) {
      if (!(await isUtilityModelReady())) {
        // Same contract as the re-embed walker: the cursor stays, a later
        // launch (or the model download finishing) picks it back up.
        notify({
          state: "waiting-model",
          archiveId: cursor.archiveId,
          source: cursor.source || null,
          processed: cursor.msgIndex,
          total: cursor.total,
          error: null,
        });
        return;
      }

      let archive: ArchiveContent;
      try {
        archive = await invoke<ArchiveContent>("import_archive_get", {
          archiveId: cursor.archiveId,
        });
      } catch (e) {
        // Archive deleted or unreadable - drop the cursor, surface, move on.
        writeCursor(null);
        notify({ state: "error", error: String(e) });
        return;
      }

      const messages = userMessages(archive);
      cursor = {
        ...cursor,
        total: messages.length,
        source: archive.source,
      };
      writeCursor(cursor);

      for (let i = cursor.msgIndex; i < messages.length; i++) {
        notify({
          state: "running",
          archiveId: cursor.archiveId,
          source: archive.source,
          processed: i,
          total: messages.length,
          error: null,
        });
        const text = messages[i].slice(0, MAX_EXTRACT_CHARS);
        if (looksLikeUserFact(text)) {
          try {
            await extractAndStoreFacts({
              userText: text,
              // The utility model is required (checked above), so the
              // fallback chat model is never invoked.
              model: "",
              aiId: `import:${archive.source}`,
              sourceActionHash: null,
            });
          } catch (e) {
            console.warn("[Import] extraction failed for one message:", e);
          }
        }
        cursor = { ...cursor, msgIndex: i + 1 };
        writeCursor(cursor);
        await sleep(PACE_MS);
      }

      markDone(cursor.archiveId);
      writeCursor(null);
      notify({
        state: "done",
        archiveId: cursor.archiveId,
        source: archive.source,
        processed: messages.length,
        total: messages.length,
        error: null,
      });
      cursor = readCursor(); // a startDistill during the run queues the next
    }
  } finally {
    running = false;
  }
}
