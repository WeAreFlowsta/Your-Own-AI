// Answer quality on this machine's models through the truth matrix (NEXT.md
// §A0, Eric 2026-10-12): eight plain questions with a deterministic rubric,
// graded per model, the registry's family score printed beside each measured
// grade. Slow (a load per model, eight answers each): a long timeout.
import { invoke } from "./_helpers";

describe("answer quality leg", () => {
  it("grades the installed models on plain questions and shows the registry score beside each", async function () {
    this.timeout(2_400_000);
    await browser.waitUntil(async () => !!(await invoke<string | null>("get_current_model").catch(() => null)), { timeout: 180_000, timeoutMsg: "no model loaded" });
    const stamp = `quality-${Date.now().toString(36)}`;
    await browser.execute((st: string) => {
      const w = window as any;
      w.__matrixDone = false;
      w.__matrixLines = [];
      w.__TAURI__.event.listen("matrix-progress", (e: any) => {
        const line = String(e.payload ?? "");
        w.__matrixLines.push(line);
        if (line.includes("MATRIX COMPLETE")) w.__matrixDone = true;
      });
      w.__TAURI__.core.invoke("matrix_run", { stamp: st, legs: ["quality"] }).then((p: string) => { w.__matrixPath = p; w.__matrixDone = true; }).catch((e: unknown) => { w.__matrixError = String(e); w.__matrixDone = true; });
    }, stamp);
    await browser.waitUntil(async () => browser.execute(() => (window as any).__matrixDone === true), { timeout: 2_300_000, interval: 10_000, timeoutMsg: "the matrix never finished" });
    const lines = await browser.execute(() => (window as any).__matrixLines as string[]);
    for (const l of lines) if (l.trim()) console.log(`e2e: matrix | ${l.slice(0, 220)}`);
    const graded = lines.filter((l) => /^quality: .+: \d+\/\d+ points/.test(l));
    expect(graded.length).toBeGreaterThan(0);
    for (const l of graded) expect(/registry overall \d/.test(l)).toBe(true);
  });
});
