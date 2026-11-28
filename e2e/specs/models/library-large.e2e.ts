// A large notes folder, on the built app: how long it takes to read in,
// what it costs in memory and disk while it does, and what a second pass
// over an unchanged folder costs. A measurement, not a gate: it runs only
// when YOAI_E2E_NOTES_DIR names a folder, reads that folder where it is
// (nothing in it is changed), and prints its figures.
import { execSync } from "node:child_process";
import { readdirSync, statSync } from "node:fs";
import { resolve } from "node:path";
import { waitForChat, ask, waitForReplyDone, invoke, invokeLong } from "./_helpers";

const NOTES = process.env.YOAI_E2E_NOTES_DIR ?? "";
const PROFILE = process.env.YOAI_E2E_HOME ?? resolve(process.cwd(), "e2e", "profile");

/** Files under a folder, hidden folders left out: count and bytes. */
function filesUnder(dir: string): { files: number; bytes: number } {
  let files = 0;
  let bytes = 0;
  for (const e of readdirSync(dir, { withFileTypes: true })) {
    if (e.name.startsWith(".")) continue;
    const p = resolve(dir, e.name);
    if (e.isDirectory()) {
      const inner = filesUnder(p);
      files += inner.files;
      bytes += inner.bytes;
    } else if (e.isFile()) {
      files++;
      bytes += statSync(p).size;
    }
  }
  return { files, bytes };
}

/** Resident memory (MB) of the app and its helpers. Each model server is
 *  named by the model it runs, so the three can be told apart. */
function memoryNow(): Record<string, number> {
  const out: Record<string, number> = {};
  try {
    for (const line of execSync("ps -eo rss=,comm=,args=", { encoding: "utf8", maxBuffer: 16 * 1024 * 1024 }).split("\n")) {
      const m = line.trim().match(/^(\d+)\s+(\S+)\s+(.*)$/);
      if (!m) continue;
      let name = m[2];
      if (!/^(your-own-ai|yourowai-|WebKitWebProcess|WebKitNetworkPr)/.test(name)) continue;
      const model = m[3].match(/--model\s+(\S+?)(?:-Q\d\S*)?\.gguf/);
      if (model) name = m[3].includes("--embedding") ? `embedder:${model[1]}` : `model:${model[1]}`;
      out[name] = (out[name] ?? 0) + Math.round(Number(m[1]) / 1024);
    }
  } catch {
    /* no figures on this platform */
  }
  return out;
}

function libraryBytes(): number {
  try {
    return Number(execSync(`find "${PROFILE}" -name "corpus.sqlite*" -printf "%s\\n" | paste -sd+ | bc`, { encoding: "utf8" }).trim() || 0);
  } catch {
    return 0;
  }
}

describe("a large notes folder", () => {
  let folderId = "";
  let aiId = "";

  it("is read in, and the cost is printed", async function () {
    if (!NOTES) this.skip();
    this.timeout(4 * 3_600_000);
    await waitForChat();
    await ask("Name one primary colour and nothing else.");
    await waitForReplyDone();
    aiId = (await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const store = await t.store.load(await t.core.invoke("profile_store_path", { name: "ai-data.json" }));
      return (((await store.get("custom-ais")) || []) as { id?: string }[])[0]?.id ?? "";
    })) as string;
    expect(aiId).toBeTruthy();
    const there = filesUnder(NOTES);
    console.log(`e2e: the folder holds ${there.files} files, ${(there.bytes / 1_048_576).toFixed(1)} MB; memory before: ${JSON.stringify(memoryNow())}; library on disk before: ${libraryBytes()} bytes`);

    const started = Date.now();
    folderId = await invoke<string>("corpus_folder_add", { path: NOTES, aiId, kind: null });
    // The page asks for the folder to be read once it is added: asked here
    // the same way, without waiting on the answer (it comes when the whole
    // folder has been read).
    await browser.execute((id: string) => {
      const w = window as any;
      w.__e2eFolderRead = { done: false };
      w.__TAURI__.core.invoke("corpus_folder_sync", { folderId: id }).then((v: unknown) => { w.__e2eFolderRead = { done: true, value: v }; }).catch((e: unknown) => { w.__e2eFolderRead = { done: true, error: String(e) }; });
    }, folderId);
    // Progress is the count of documents
    // whose passages are ready; the peak of each process is kept.
    const peak: Record<string, number> = {};
    let ready = 0;
    let listed = 0;
    let passages = 0;
    let lastChange = Date.now();
    let lastSaid = 0;
    await browser.waitUntil(
      async () => {
        const docs = await invoke<{ chunk_count: number }[]>("corpus_documents", { aiId }).catch(() => [] as { chunk_count: number }[]);
        const nowReady = docs.filter((d) => d.chunk_count > 0).length;
        const nowPassages = docs.reduce((n, d) => n + d.chunk_count, 0);
        if (nowReady !== ready || docs.length !== listed || nowPassages !== passages) lastChange = Date.now();
        ready = nowReady;
        listed = docs.length;
        passages = nowPassages;
        for (const [name, mb] of Object.entries(memoryNow())) peak[name] = Math.max(peak[name] ?? 0, mb);
        if (Date.now() - lastSaid > (Number(process.env.YOAI_E2E_NOTES_SECS) ? 60_000 : 120_000)) {
          lastSaid = Date.now();
          console.log(`e2e: ${Math.round((Date.now() - started) / 1000)} s: ${ready} of ${listed} documents ready, ${passages} passages; memory ${JSON.stringify(memoryNow())}`);
        }
        // Done when the read has answered; or, failing an answer, when
        // nothing has changed for ten minutes.
        const read = (await browser.execute(() => (window as any).__e2eFolderRead)) as { done: boolean; value?: unknown; error?: string };
        if (read?.done) {
          lastChange = Date.now();
          console.log(`e2e: the read answered: ${JSON.stringify(read.error ?? read.value).slice(0, 500)}`);
          return true;
        }
        // YOAI_E2E_NOTES_SECS: a short look (the read is left as it stands).
        const short = Number(process.env.YOAI_E2E_NOTES_SECS) || 0;
        if (short > 0 && Date.now() - started > short * 1000) return true;
        return listed > 0 && Date.now() - lastChange > 600_000;
      },
      { timeout: 4 * 3_600_000 - 600_000, interval: 15_000, timeoutMsg: "the folder was still being read after four hours" },
    );
    const took = Math.round((lastChange - started) / 1000);
    console.log(`e2e: READ IN: ${ready} of ${listed} documents ready (${there.files} files in the folder), ${passages} passages, in ${took} s (${(took / Math.max(1, ready)).toFixed(1)} s a document, ${(passages / Math.max(1, took)).toFixed(1)} passages a second)`);
    console.log(`e2e: PEAK MEMORY (MB): ${JSON.stringify(peak)}; library on disk after: ${(libraryBytes() / 1_048_576).toFixed(1)} MB`);
    expect(ready).toBeGreaterThan(0);
  });

  it("a second pass over the unchanged folder is quick", async function () {
    if (!NOTES || !folderId || process.env.YOAI_E2E_NOTES_SECS) this.skip();
    this.timeout(3_600_000);
    const started = Date.now();
    let report: unknown = null;
    for (let i = 0; i < 40 && report === null; i++) {
      try {
        report = await invokeLong<unknown>("corpus_folder_sync", { folderId }, 3_000_000);
      } catch (e) {
        if (!String(e).includes("busy")) throw e;
        await new Promise((r) => setTimeout(r, 15_000));
      }
    }
    console.log(`e2e: SECOND PASS: ${Math.round((Date.now() - started) / 1000)} s; report ${JSON.stringify(report).slice(0, 400)}`);
  });

  it("a question is answered with the library in place, and the time is printed", async function () {
    if (!NOTES || !folderId || process.env.YOAI_E2E_NOTES_SECS) this.skip();
    this.timeout(1_200_000);
    await waitForChat();
    const started = Date.now();
    await ask("From my notes: what is the name of the first note you can find about deployment? One short sentence.");
    const r = await waitForReplyDone(900_000);
    console.log(`e2e: A QUESTION WITH THE LIBRARY: ${Math.round((Date.now() - started) / 1000)} s to the end of the reply; memory ${JSON.stringify(memoryNow())}; reply ${JSON.stringify(r.text.slice(0, 200))}`);
  });
});
