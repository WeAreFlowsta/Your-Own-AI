/**
 * Adoption resume: makes "close the app mid-adoption and it continues"
 * true. A pending marker is written before the backend write-loop starts
 * and cleared when it completes; launch finds leftover markers and re-runs
 * them. Safe because the backend dedups on each conversation's original
 * start time (the restore pattern) and refuses overlapping runs of the
 * same archive+AI pair.
 */

const PENDING_KEY = "import-adopt-pending";

export interface PendingAdoption {
  archiveId: string;
  aiId: string;
  aiName: string;
}

function readAll(): PendingAdoption[] {
  try {
    const raw = localStorage.getItem(PENDING_KEY);
    const list = raw ? JSON.parse(raw) : [];
    return Array.isArray(list) ? list : [];
  } catch {
    return [];
  }
}

function writeAll(list: PendingAdoption[]) {
  if (list.length === 0) localStorage.removeItem(PENDING_KEY);
  else localStorage.setItem(PENDING_KEY, JSON.stringify(list));
}

export function markPending(p: PendingAdoption) {
  const list = readAll().filter(
    (x) => !(x.archiveId === p.archiveId && x.aiId === p.aiId),
  );
  list.push(p);
  writeAll(list);
}

export function clearPending(archiveId: string, aiId: string) {
  writeAll(
    readAll().filter((x) => !(x.archiveId === archiveId && x.aiId === aiId)),
  );
}

/** Finish any adoption a previous run left incomplete - call once from the
 *  root layout after startup settles. Serial: chain writes are serialized
 *  per agent anyway, and one loop at a time keeps the machine calm. */
export async function resumeIfPending(): Promise<void> {
  const list = readAll();
  if (list.length === 0) return;
  const { waitForHolochainReady } = await import("./holochainTranscripts");
  if (!(await waitForHolochainReady(30000))) return; // marker stays - next launch retries
  const { invoke } = await import("@tauri-apps/api/core");
  for (const p of list) {
    try {
      await invoke<number>("import_archive_adopt", {
        archiveId: p.archiveId,
        aiId: p.aiId,
        aiName: p.aiName,
      });
      clearPending(p.archiveId, p.aiId);
      const { summarizeAdoptedConversations } = await import(
        "./importSummaries"
      );
      summarizeAdoptedConversations(p.aiId).catch(() => {});
    } catch (e) {
      // "Already being added" means a click beat us - the done event will
      // clear the marker. Anything else: leave the marker for next launch.
      console.warn("[Import] adoption resume:", e);
    }
  }
}
