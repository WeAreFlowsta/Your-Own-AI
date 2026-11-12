// Background work on a busy computer (e2e/busy-live.sh): another program
// takes about half the processor from the start.
//   1. A kept notes folder is read when the person asks (their own looks,
//      Check now, a documents screen, the app coming back to the front,
//      never wait).
//   2. The folder check that runs by itself (the half-hour timer) waits and
//      says why ("other programs are busy"); the Activity tray stays quiet
//      about it (Settings names it). A second note lands in the folder.
//   3. A reply still answers while the other program runs, and the timer's
//      look does not run.
//   4. The other program stops: the timer's look runs by itself, the second
//      note is in the documents, and nothing is left waiting on the processor.
// Which look ran is read from the app's log ("folder sync <id> (timer)").
// The harness says nobody is at the computer (YOAI_TEST_AWAY_SECS), so the
// processor is the only thing holding the work.
import { spawn, type ChildProcess } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { cpus } from "node:os";
import { resolve } from "node:path";
import { waitForChat, ask, waitForReplyDone, invoke, shot } from "../models/_helpers";

type Reading = { others_cpu_pct: number | null; memory_short: boolean; on_battery: boolean; away_secs: number | null };
type Status = { reading: Reading | null; held: [string, string][] };
const status = () => invoke<Status>("machine_status");
const FOLDER_JOB = "folder indexing";
const BUSY = "other programs are busy";

const HOME = process.env.YOAI_E2E_HOME || resolve(process.cwd(), "e2e", "profile");
const APP_LOG = resolve(HOME, ".local/share/com.solar.yourowai/logs/Your Own AI.log");
/** The app log's lines (colour codes removed). */
function logLines(): string[] {
  try {
    return readFileSync(APP_LOG, "utf8").replace(/\x1b\[[0-9;]*m/g, "").split("\n");
  } catch {
    return [];
  }
}
const TIMER_SYNC = /\[corpus\] folder sync \S+ \(timer\): .*/;

// The other program: busy loops at the lowest priority, outside the app's
// process tree, on half the processor's threads.
const HOG_MARK = "yoai-busy-hog";
let hogs: ChildProcess[] = [];
function startHog(n: number) {
  for (let i = 0; i < n; i++) hogs.push(spawn("nice", ["-n", "19", "sh", "-c", `: ${HOG_MARK}; while :; do :; done`], { stdio: "ignore" }));
}
function stopHog() {
  for (const h of hogs) {
    try { h.kill("SIGKILL"); } catch { /* already gone */ }
  }
  hogs = [];
}
process.on("exit", stopHog);

async function docsMention(aiId: string, word: string): Promise<boolean> {
  const docs = await invoke<{ meta: unknown; chunk_count: number }[]>("corpus_documents", { aiId });
  return docs.some((d) => JSON.stringify(d.meta).toLowerCase().includes(word) && d.chunk_count > 0);
}

describe("background work on a busy computer", () => {
  after(stopHog);

  it("waits for the other program, still answers, and runs once it stops", async function () {
    this.timeout(1_800_000);
    const t0 = Date.now();
    const at = () => `${Math.round((Date.now() - t0) / 1000)} s`;
    const threads = cpus().length;
    startHog(Math.max(2, Math.ceil(threads / 2)));
    console.log(`e2e: the other program started on ${hogs.length} of ${threads} threads`);

    await waitForChat();
    const aiId = await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const path = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(path);
      const ais = (await store.get("custom-ais")) || [];
      return ais[0]?.id || null;
    });
    expect(aiId).toBeTruthy();

    // 1. The person's own look reads the first note, busy computer or not.
    const dir = resolve(HOME, "busy-notes");
    mkdirSync(dir, { recursive: true });
    writeFileSync(resolve(dir, "lighthouse.md"), "# Lighthouse notes\n\nThe lighthouse keeper's dog is called Biscuit.\n");
    const folderId = await invoke<string>("corpus_folder_add", { path: dir, aiId, kind: null });
    let report: unknown = null;
    for (let i = 0; i < 12 && report === null; i++) {
      try {
        report = await invoke<unknown>("corpus_folder_sync", { folderId });
      } catch (e) {
        if (!String(e).includes("busy") && !String(e).includes("timed out")) throw e;
        await new Promise((r) => setTimeout(r, 5000));
      }
    }
    await browser.waitUntil(() => docsMention(aiId, "lighthouse"), { timeout: 180_000, timeoutMsg: "the person's own look did not read the first note" });
    console.log(`e2e: ${at()}: the person's own look read the first note`);

    // 2. The timer's look waits for the computer, and the tray says nothing.
    let st: Status | null = null;
    await browser.waitUntil(async () => { st = await status(); return st.held.some(([j, w]) => j === FOLDER_JOB && w === BUSY); }, {
      timeout: 600_000, interval: 10_000, timeoutMsg: () => `the folder check never waited for the busy processor; last status ${JSON.stringify(st)}`,
    });
    console.log(`e2e: ${at()}: held ${JSON.stringify(st!.held)}, reading ${JSON.stringify(st!.reading)}`);
    expect(st!.reading?.others_cpu_pct ?? 0).toBeGreaterThanOrEqual(30);
    expect(logLines().some((l) => TIMER_SYNC.test(l))).toBe(false);
    const tray = await browser.execute(() => (document.querySelector('[data-testid="activity-tray"]') as HTMLElement | null)?.innerText ?? "");
    console.log(`e2e: ${at()}: the Activity tray reads ${JSON.stringify(tray.replace(/\s+/g, " ").slice(0, 200))}`);
    // Nothing is said in the tray for a short wait (its "still waiting"
    // note is for work held a whole day): what is waiting shows in Settings.
    expect(tray).not.toContain("Background work is still waiting");
    writeFileSync(resolve(dir, "ferry.md"), "# Ferry notes\n\nThe ferry's bell is named Marigold.\n");

    // 3. A reply still answers, and the timer's look stays held.
    const asked = Date.now();
    await ask("Name one ocean and nothing else.");
    const r = await waitForReplyDone(300_000);
    // The message's text starts with its header (the AI's name, its buttons):
    // the answer is the last line.
    const answer = r.text.trim().split("\n").map((l) => l.trim()).filter(Boolean).pop() ?? "";
    console.log(`e2e: ${at()}: a reply answered in ${Math.round((Date.now() - asked) / 1000)} s while the other program ran: ${answer.slice(0, 80)}`);
    expect(answer).toMatch(/pacific|atlantic|indian|arctic|southern/i);
    await shot("busy-reply");
    // Long enough for the timer's next look (every 5 minutes while held).
    await browser.waitUntil(async () => (await status()).held.some(([j]) => j === FOLDER_JOB) && Date.now() - asked > 330_000, {
      timeout: 600_000, interval: 15_000, timeoutMsg: "the folder check stopped waiting while the other program still ran",
    });
    expect(logLines().some((l) => TIMER_SYNC.test(l))).toBe(false);
    console.log(`e2e: ${at()}: the timer's look still waits; ${logLines().filter((l) => / waits: /.test(l)).length} wait line(s) in the log`);

    // 4. The other program stops; the timer's look runs by itself.
    stopHog();
    console.log(`e2e: ${at()}: the other program stopped`);
    let ran = "";
    await browser.waitUntil(async () => { ran = logLines().find((l) => TIMER_SYNC.test(l)) ?? ""; return !!ran; }, {
      timeout: 900_000, interval: 15_000, timeoutMsg: "the timer's folder check did not run by itself after the other program stopped",
    });
    console.log(`e2e: ${at()}: the timer's look ran by itself: ${ran.match(TIMER_SYNC)![0]}`);
    await browser.waitUntil(() => docsMention(aiId, "ferry"), { timeout: 120_000, timeoutMsg: "the second note is not in the documents after the timer's look" });
    st = await status();
    console.log(`e2e: ${at()}: the second note is in the documents; held ${JSON.stringify(st.held)}, reading ${JSON.stringify(st.reading)}`);
    expect(st.held.some(([j]) => j === FOLDER_JOB)).toBe(false);
    expect(st.reading?.others_cpu_pct ?? 100).toBeLessThan(30);
    expect(st.held.filter(([, w]) => w === BUSY)).toEqual([]);
    await invoke("corpus_folder_remove", { folderId, removeDocuments: true }).catch(() => {});
  });
});
