// The routing battery, signed out: 204 questions in both modes at all three
// dial positions, judged on the promises that hold without online models -
// offline mode never goes online, health questions stay on the device, a
// follow-up keeps its side. (Signed in, with online models: specs/online.)
import { invoke, runMatrixLeg, routingVerdict } from "./_helpers";

describe("routing battery (signed out)", () => {
  it("keeps every routing promise that holds without online models", async function () {
    this.timeout(1_900_000);
    await browser.waitUntil(async () => !!(await invoke<string | null>("get_current_model").catch(() => null)), { timeout: 180_000, timeoutMsg: "no model loaded" });
    const { lines, error } = await runMatrixLeg("routing");
    for (const l of lines) if (l.trim()) console.log(`e2e: battery | ${l.slice(0, 220)}`);
    expect(error ?? null).toBeNull();
    const v = routingVerdict(lines);
    console.log(`e2e: ${v.decided} decisions, ${v.msPer?.toFixed(0)} ms each; ${v.failures.length} failure line(s)`);
    expect(v.decided).toBeGreaterThan(1000);
    expect(v.failures).toEqual([]);
    // Routing is on every turn's path: it must stay well under a second.
    expect(v.msPer ?? 0).toBeLessThan(1_000);
  });
});
