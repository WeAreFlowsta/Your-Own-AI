// The memory slot's measured leg (build-docs MEMORY_REDESIGN.md §7): every
// runnable installed model is graded on the app's own extraction prompt
// and grammar (precision/recall on a fixed set) and on a consolidation
// rubric; the grades land in the slot's store and decide which local model
// does memory work. Slow (a load per model): one test, a long timeout.
import { invoke } from "./_helpers";

describe("memory grades", () => {
  it("grades the installed models and the slot picks by grade", async function () {
    this.timeout(2_400_000);
    await browser.waitUntil(async () => !!(await invoke<string | null>("get_current_model").catch(() => null)), { timeout: 180_000, timeoutMsg: "no model loaded" });
    const stamp = `memory-${Date.now().toString(36)}`;
    // A WebDriver script call cannot wait minutes: fire the matrix without
    // awaiting it, watch its own progress events for the end, then read.
    await browser.execute((st: string) => {
      const w = window as any;
      w.__matrixDone = false;
      w.__matrixLines = [];
      w.__TAURI__.event.listen("matrix-progress", (e: any) => {
        const line = String(e.payload ?? "");
        w.__matrixLines.push(line);
        if (line.includes("MATRIX COMPLETE")) w.__matrixDone = true;
      });
      w.__TAURI__.core.invoke("matrix_run", { stamp: st, legs: ["memory"] }).then((p: string) => { w.__matrixPath = p; w.__matrixDone = true; }).catch((e: unknown) => { w.__matrixError = String(e); w.__matrixDone = true; });
    }, stamp);
    await browser.waitUntil(async () => browser.execute(() => (window as any).__matrixDone === true), { timeout: 2_300_000, interval: 10_000, timeoutMsg: "the memory leg never finished" });
    const lines = await browser.execute(() => (window as any).__matrixLines as string[]);
    for (const l of lines) if (l.trim()) console.log(`e2e: matrix | ${l.slice(0, 200)}`);
    const err = await browser.execute(() => (window as any).__matrixError as string | undefined);
    if (err) console.log(`e2e: matrix error ${err}`);
    const grades = await invoke<Record<string, { precision: number | null; prose: number | null; measured_at: number }>>("memory_grades");
    const rows = Object.entries(grades).map(([m, g]) => [m.replace(/-Q4_K_M\.gguf$/, ""), g.precision, g.prose]);
    console.log(`e2e: grades ${JSON.stringify(rows)}`);
    expect(rows.length).toBeGreaterThan(0);
    const helper = grades["Ministral-3-3B-Instruct-2512-Q4_K_M.gguf"];
    expect(helper?.precision).not.toBeNull();
    const pick = await invoke<{ model: string; via: string; grade: number; reason: string }>("memory_slot_pick", { task: "memory-precision" });
    console.log(`e2e: slot pick ${JSON.stringify(pick)}`);
    expect(["loaded", "helper"]).toContain(pick.via);
  });
});
