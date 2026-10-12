// The report/code mode classifier and the semantic gates, measured on this
// machine through the truth matrix (NEXT.md §A0, Eric 2026-10-12): the matrix
// runs the app's own prompt, grammar and pre-gate against a held-out set on
// the installed models, and the routing leg's gate lines score the freshness
// and health gates against the battery's labeled buckets. Slow (a load per
// model; the battery embeds every query): long timeouts.
import { invoke } from "./_helpers";

async function runLegs(legs: string[], stamp: string): Promise<string[]> {
  await browser.execute((st: string, lg: string[]) => {
    const w = window as any;
    w.__matrixDone = false;
    w.__matrixLines = [];
    w.__TAURI__.event.listen("matrix-progress", (e: any) => {
      const line = String(e.payload ?? "");
      w.__matrixLines.push(line);
      if (line.includes("MATRIX COMPLETE")) w.__matrixDone = true;
    });
    w.__TAURI__.core.invoke("matrix_run", { stamp: st, legs: lg }).then((p: string) => { w.__matrixPath = p; w.__matrixDone = true; }).catch((e: unknown) => { w.__matrixError = String(e); w.__matrixDone = true; });
  }, stamp, legs);
  await browser.waitUntil(async () => browser.execute(() => (window as any).__matrixDone === true), { timeout: 2_300_000, interval: 10_000, timeoutMsg: "the matrix never finished" });
  const lines = await browser.execute(() => (window as any).__matrixLines as string[]);
  for (const l of lines) if (l.trim()) console.log(`e2e: matrix | ${l.slice(0, 220)}`);
  const err = await browser.execute(() => (window as any).__matrixError as string | undefined);
  if (err) console.log(`e2e: matrix error ${err}`);
  return lines;
}

describe("classifier and gate legs", () => {
  it("grades the report/code mode classifier on the installed models", async function () {
    this.timeout(2_400_000);
    await browser.waitUntil(async () => !!(await invoke<string | null>("get_current_model").catch(() => null)), { timeout: 180_000, timeoutMsg: "no model loaded" });
    const lines = await runLegs(["modes"], `modes-${Date.now().toString(36)}`);
    const graded = lines.filter((l) => /^modes: .+: \d+\/\d+ right/.test(l));
    expect(graded.length).toBeGreaterThan(0);
    // The helper is always among the candidates when installed; its line must parse.
    const helper = graded.find((l) => l.includes("Ministral"));
    if (helper) expect(/(\d+)\/(\d+) right/.exec(helper)).not.toBeNull();
  });

  it("scores the freshness and health gates on the battery", async function () {
    this.timeout(2_400_000);
    const lines = await runLegs(["routing"], `gates-${Date.now().toString(36)}`);
    const gate = lines.find((l) => l.startsWith("gates ("));
    expect(gate).toBeDefined();
    const m = /fresh caught (\d+)\/(\d+).*health caught (\d+)\/(\d+)/.exec(gate ?? "");
    expect(m).not.toBeNull();
    if (m && gate!.includes("semantic stage on")) {
      // With the memory component present the gates are a promise: four in five at least.
      expect(Number(m[1]) * 5).toBeGreaterThanOrEqual(Number(m[2]) * 4);
      expect(Number(m[3]) * 5).toBeGreaterThanOrEqual(Number(m[4]) * 4);
    }
  });
});
