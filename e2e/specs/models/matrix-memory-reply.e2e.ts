// Memory through the truth matrix, on the built app. The search: the
// built-in made-up history goes through the search model here and each
// answer must stand among the closest. The replies: each
// chat model in the folder is asked the built-in made-up history's eight
// questions twice, with the results ahead of the question as a real turn
// carries them, and each reply is marked on its words. One figure a model:
// how many replies did what the question called for, how many were off
// the question, how fast the model reads here and how long the slowest
// read took before the first word. Nothing in it is anyone's own history.
import os from "node:os";
import { invoke, waitForChat } from "./_helpers";

const LIMIT = Number(process.env.YOAI_E2E_TIMEOUT_MS) || 2_400_000;

type Figure = {
  kind: string; model: string; set: number; place: string; read_tps: number; shown_least: number; shown_most: number; slowest_read_s: number; mark_s: number;
  asked: number; good: number; right: number; said_not_there: number; stale: number; missed: number; answered_anyway: number; off_the_question: number; unclear: number; nothing: number;
};

describe("replies from memory leg", () => {
  it("asks every model the made-up history's questions and writes one figure a model", async function () {
    this.timeout(LIMIT);
    await waitForChat();
    await browser.waitUntil(async () => !!(await invoke<string | null>("get_current_model").catch(() => null)), { timeout: 180_000, timeoutMsg: "no model loaded" });
    const stamp = `memory-reply-${Date.now().toString(36)}`;
    await browser.execute((st: string) => {
      const w = window as any;
      w.__matrixDone = false;
      w.__matrixLines = [];
      w.__TAURI__.event.listen("matrix-progress", (e: any) => {
        const line = String(e.payload ?? "");
        w.__matrixLines.push(line);
        if (line.includes("MATRIX COMPLETE")) w.__matrixDone = true;
      });
      w.__TAURI__.core.invoke("matrix_run", { stamp: st, legs: ["memory-search", "memory-reply"] }).then((p: string) => { w.__matrixPath = p; w.__matrixDone = true; }).catch((e: unknown) => { w.__matrixError = String(e); w.__matrixDone = true; });
    }, stamp);
    await browser.waitUntil(async () => browser.execute(() => (window as any).__matrixDone === true), { timeout: LIMIT - 100_000, interval: 10_000, timeoutMsg: "the matrix never finished" });
    expect((await browser.execute(() => (window as any).__matrixError as string | undefined)) ?? "").toBe("");
    const lines = await browser.execute(() => (window as any).__matrixLines as string[]);
    for (const l of lines) if (l.trim()) console.log(`e2e: matrix | ${l.slice(0, 330)}`);
    // The search model here puts each answer among the closest.
    const search = lines.filter((l) => l.startsWith("figure: {") && l.includes('"kind":"memory_search"')).map((l) => JSON.parse(l.slice("figure: ".length)) as { answerable: number; first_3: number; first_12: number; messages: number; read_ms: number });
    expect(search.length).toBe(1);
    console.log(`e2e: MEMORY SEARCH first 3 for ${search[0].first_3} of ${search[0].answerable}, first 12 for ${search[0].first_12}; ${search[0].messages} messages read in ${search[0].read_ms} ms`);
    expect(search[0].answerable).toBe(6);
    expect(search[0].first_12).toBe(6);
    expect(search[0].messages).toBe(36);
    const figures = lines.filter((l) => l.startsWith("figure: {") && l.includes('"kind":"memory_reply"')).map((l) => JSON.parse(l.slice("figure: ".length)) as Figure);
    expect(figures.length).toBeGreaterThan(0);
    for (const f of figures) {
      console.log(`e2e: MEMORY REPLY ${f.model}: ${f.good} of ${f.asked}; off the question ${f.off_the_question}; reads ${f.read_tps} tok/s; shown ${f.shown_least} to ${f.shown_most}; slowest read ${f.slowest_read_s} s`);
      expect(f.model.endsWith(".gguf")).toBe(true);
      expect(f.asked).toBe(16);
      expect(f.right + f.said_not_there + f.stale + f.missed + f.answered_anyway + f.off_the_question + f.unclear + f.nothing).toBe(f.asked);
      expect(f.good).toBe(f.right + f.said_not_there);
      expect(f.read_tps).toBeGreaterThan(0);
      // The best result is always given, and never more than were found.
      expect(f.shown_least).toBeGreaterThanOrEqual(1);
      expect(f.shown_most).toBeLessThanOrEqual(12);
    }
    // A reply line for every question that was answered.
    const replyLines = lines.filter((l) => /^memory reply: .*\| shown \d+ of 12 \|/.test(l)).length;
    const asked = figures.reduce((n, f) => n + f.asked, 0);
    expect(replyLines).toBeLessThanOrEqual(asked);
    expect(replyLines).toBeGreaterThanOrEqual(asked - figures.reduce((n, f) => n + f.nothing, 0));
    // The report is made to be sent by hand: it names neither this
    // computer's account nor its home folder.
    const secrets = [os.homedir(), os.userInfo().username].filter((x) => x.length >= 3).map((x) => x.toLowerCase());
    for (const l of lines) for (const x of secrets) expect(l.toLowerCase().includes(x) ? `"${x}" in: ${l}` : "").toBe("");
  });
});
