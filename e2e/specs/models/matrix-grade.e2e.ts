// Model grading through the truth matrix, on the built app: each chat
// model in the folder is put the harder questions and the built-in memory
// check, and the result is written in the report as a figure named by the
// file. A model whose result came with this release is skipped, and the
// report says so. YOAI_E2E_GRADE_ALL=1 grades again whatever came with the
// release: a long run (about half an hour a model, more on a slow machine).
import os from "node:os";
import { invoke, waitForChat } from "./_helpers";

const ALL = !!process.env.YOAI_E2E_GRADE_ALL;
const LIMIT = Number(process.env.YOAI_E2E_TIMEOUT_MS) || (ALL ? 6 * 3_600_000 : 2_400_000);

type Grade = { kind: string; file: string; sha256: string | null; questions: { set: number; right: number; of: number; asked: number }; memory: { set: number; may_keep: boolean; may_close: boolean; may_tidy: boolean } | null };

describe("model grading leg", () => {
  it("grades the models that have no result yet, and writes each result as a figure", async function () {
    this.timeout(LIMIT);
    await waitForChat();
    await browser.waitUntil(async () => !!(await invoke<string | null>("get_current_model").catch(() => null)), { timeout: 180_000, timeoutMsg: "no model loaded" });
    const stamp = `grade-${Date.now().toString(36)}`;
    await browser.execute((st: string, legs: string[]) => {
      const w = window as any;
      w.__matrixDone = false;
      w.__matrixLines = [];
      w.__TAURI__.event.listen("matrix-progress", (e: any) => {
        const line = String(e.payload ?? "");
        w.__matrixLines.push(line);
        if (line.includes("MATRIX COMPLETE")) w.__matrixDone = true;
      });
      w.__TAURI__.core.invoke("matrix_run", { stamp: st, legs }).then((p: string) => { w.__matrixPath = p; w.__matrixDone = true; }).catch((e: unknown) => { w.__matrixError = String(e); w.__matrixDone = true; });
    }, stamp, [ALL ? "grade-all" : "grade"]);
    await browser.waitUntil(async () => browser.execute(() => (window as any).__matrixDone === true), { timeout: LIMIT - 100_000, interval: 10_000, timeoutMsg: "the matrix never finished" });
    const error = await browser.execute(() => (window as any).__matrixError as string | undefined);
    expect(error ?? "").toBe("");
    const lines = await browser.execute(() => (window as any).__matrixLines as string[]);
    for (const l of lines) if (l.trim()) console.log(`e2e: matrix | ${l.slice(0, 260)}`);
    const count = lines.map((l) => /(\d+) chat model\(s\) here; (\d+) already graded in this release; grading (\d+)/.exec(l)).find(Boolean);
    expect(count).toBeTruthy();
    const [here, already, todo] = [Number(count![1]), Number(count![2]), Number(count![3])];
    expect(here).toBeGreaterThan(0);
    expect(already + todo).toBe(here);
    if (ALL) expect(todo).toBe(here);
    const grades = lines.filter((l) => l.startsWith("figure: {") && l.includes('"kind":"model-grade"')).map((l) => JSON.parse(l.slice("figure: ".length)) as Grade);
    // One figure for each model graded, with the count of questions put
    // and, when the check ran to its end, what the model may do in memory.
    expect(grades.length).toBe(todo);
    for (const g of grades) {
      console.log(`e2e: GRADE ${g.file}: ${g.questions.right} of ${g.questions.of}; memory ${g.memory ? `keep ${g.memory.may_keep}, close ${g.memory.may_close}, replace ${g.memory.may_tidy}` : "not finished"}`);
      expect(g.file.endsWith(".gguf")).toBe(true);
      expect(g.questions.of).toBeGreaterThan(0);
      expect(g.questions.asked).toBe(g.questions.of);
      expect(g.questions.right).toBeLessThanOrEqual(g.questions.of);
    }
    // The report is made to be sent by hand: it names neither this
    // computer's account nor its home folder.
    const secrets = [os.homedir(), os.userInfo().username].filter((x) => x.length >= 3).map((x) => x.toLowerCase());
    for (const l of lines) for (const x of secrets) expect(l.toLowerCase().includes(x) ? `"${x}" in: ${l}` : "").toBe("");
  });
});
