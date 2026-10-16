// The routing battery, signed in with online models: the same 204 questions
// in both modes at all three dial positions, now judged on the online side
// too - fresh questions go online, the dial moves everyday questions, long
// turns go online with their reason. Decisions only: nothing is answered,
// nothing is billed.
import { invoke, runMatrixLeg, routingVerdict } from "../models/_helpers";
import { cheapPicks, haveIdentity, setRoutingPrefs, signInAsTestIdentity } from "./_online";

describe("routing battery (signed in, online models)", () => {
  it("routes every question to the side it should, in both modes", async function () {
    if (!haveIdentity()) this.skip();
    this.timeout(1_900_000);
    await browser.waitUntil(async () => !!(await invoke<string | null>("get_current_model").catch(() => null)), { timeout: 180_000, timeoutMsg: "no model loaded" });
    const session = await signInAsTestIdentity();
    expect(session.tier).toBe("pro");
    await setRoutingPrefs({ ...cheapPicks(), onlineEntitled: "yes" });
    const { lines, error } = await runMatrixLeg("routing");
    for (const l of lines) if (l.trim()) console.log(`e2e: battery | ${l.slice(0, 220)}`);
    expect(error ?? null).toBeNull();
    // The environment line must say online was exercised, or the run proves nothing new.
    const env = lines.find((l) => l.includes("online catalog:")) ?? "";
    expect(env).toMatch(/online catalog: \d+ models/);
    expect(env).toContain("entitled: yes");
    const v = routingVerdict(lines);
    console.log(`e2e: ${v.decided} decisions, ${v.msPer?.toFixed(0)} ms each; ${v.failures.length} failure line(s)`);
    expect(v.decided).toBeGreaterThan(1000);
    expect(v.failures).toEqual([]);
    expect(v.msPer ?? 0).toBeLessThan(1_000);
  });
});
