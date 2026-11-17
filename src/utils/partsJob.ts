/**
 * The app's parts: what follows the app itself onto a computer and is kept
 * current with it, by itself. Today the memory model and the Projects
 * helper (Your Own AI Build).
 *
 * One job with one set of rules, run at start, when a model download ends,
 * when a project's agent ends, and every few hours:
 *   - nothing is fetched before the person has a model (a new install's
 *     first download is theirs to start);
 *   - a part the person removed is not fetched again until they ask;
 *   - a part in use is replaced at a later check, when it is free;
 *   - a fetch that fails says nothing and is tried again at the next check
 *     (an error row is for a download someone pressed).
 * The parts follow the app's own update decision: there is no second wait
 * for a metered connection or battery here.
 */
import { invoke } from "@tauri-apps/api/core";
import { modelManager } from "./modelManager";
import { EMBEDDING_MODEL } from "../data/recommended-models";

const REMOVED_PREFIX = "part-removed:";

/** The person removed this part (its card's Remove): not fetched uninvited. */
export function notePartRemoved(id: string): void {
  try {
    localStorage.setItem(REMOVED_PREFIX + id, "1");
  } catch { /* the next check may fetch it again */ }
}

/** The person asked for this part (its card's Download). */
export function notePartWanted(id: string): void {
  try {
    localStorage.removeItem(REMOVED_PREFIX + id);
  } catch { /* fine */ }
}

export function partWasRemoved(id: string): boolean {
  try {
    return localStorage.getItem(REMOVED_PREFIX + id) === "1";
  } catch {
    return false;
  }
}

/** What the job does about the memory model. Pure, for the tests. */
export function memoryModelStep(s: {
  hasChatModel: boolean;
  removed: boolean;
  downloaded: boolean;
  downloading: boolean;
}): "wait-for-a-model" | "removed" | "current" | "busy" | "fetch" {
  if (!s.hasChatModel) return "wait-for-a-model";
  if (s.removed) return "removed";
  if (s.downloaded) return "current";
  if (s.downloading) return "busy";
  return "fetch";
}

async function ensureMemoryModel(): Promise<string> {
  const file = EMBEDDING_MODEL.filename;
  const [downloaded, status] = await Promise.all([
    modelManager.isModelDownloaded(file),
    modelManager.downloadStatus(file),
  ]);
  const step = memoryModelStep({
    hasChatModel: true,
    removed: partWasRemoved(EMBEDDING_MODEL.id),
    downloaded,
    downloading: status.downloading,
  });
  if (step !== "fetch") return step;
  await modelManager.downloadModel(EMBEDDING_MODEL.downloadUrl, file, undefined, { quiet: true });
  // It replaces an earlier memory model: stop the server that may hold the
  // old file, reclaim it, and rebuild what was indexed with it (the same
  // steps as the card's Download).
  const stale: string[] = [];
  for (const f of EMBEDDING_MODEL.previousFilenames ?? []) {
    if (await modelManager.isModelDownloaded(f)) stale.push(f);
  }
  if (stale.length) {
    try { await invoke("stop_embedding_server"); } catch { /* not running */ }
    for (const f of stale) {
      try { await modelManager.deleteModel(f); } catch { /* reclaimed at a later update */ }
    }
  }
  try {
    const { rebuildMemoryIndexIfPending, migrateEmbeddingIndexIfNeeded } = await import("./transcriptMemory");
    await rebuildMemoryIndexIfPending();
    await migrateEmbeddingIndexIfNeeded();
  } catch { /* the next start runs them */ }
  return "fetched";
}

async function ensureProjectsHelper(): Promise<string> {
  const outcome = await invoke<string>("ensure_build_agent");
  if (outcome === "installed" || outcome === "updated") {
    const st = await invoke<{ path: string | null }>("build_install_status");
    if (st.path) {
      try {
        localStorage.setItem("build-binary-path", st.path);
      } catch { /* the resolver falls back */ }
    }
  }
  return outcome;
}

let running = false;

/** Run the check once. Safe to call often: one run at a time. */
export async function runPartsJob(reason: string): Promise<void> {
  if (running) return;
  running = true;
  try {
    const models = await modelManager.listModels();
    if (models.length === 0) return; // nothing chosen yet
    const [memory, helper] = await Promise.allSettled([ensureMemoryModel(), ensureProjectsHelper()]);
    const say = (r: PromiseSettledResult<string>) => (r.status === "fulfilled" ? r.value : "put off");
    console.log(`[parts] ${reason}: memory model ${say(memory)}, Projects helper ${say(helper)}`);
  } catch (e) {
    console.warn("[parts] the check did not run:", e);
  } finally {
    running = false;
  }
}
