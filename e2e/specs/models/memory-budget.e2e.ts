// Phase 6 gate (the design notes §13.6): the layer-2 budget
// holds live. 510 episodes in a throwaway project scope, one dream: at
// most 500 stay live, the oldest ten go to history (valid_to, never
// deleted), and a second dream changes nothing. Slow (510 writes): a long
// harness limit.
import { invoke } from "./_helpers";

describe("memory budget", () => {
  it("keeps a scope at 500 live layer-2 rows after a dream, and a second dream changes nothing", async function () {
    this.timeout(2_400_000);
    await browser.waitUntil(async () => {
      try { await invoke("list_memory_rows", { agentKey: null, scope: "person" }); return true; } catch { return false; }
    }, { timeout: 180_000, timeoutMsg: "the records never answered" });
    const scope = `project:budget-${Date.now().toString(36)}`;
    const base = Date.now() - 600_000;
    const t0 = Date.now();
    for (let i = 0; i < 510; i++) {
      await invoke("save_memory_row", { agentKey: null, row: {
        id: `b-${i}`, scope, kind: "episode", text: `Session ${i}: did thing number ${i} and decided item ${i}`, structured: null,
        as_of: base + i * 1000, valid_to: null, saved_by: "person", provenance: "distilled:test", pinned: false, topics: ["t"], replaces: [], deleted: false, updated_at: base + i * 1000,
      } });
    }
    console.log(`e2e: 510 rows written in ${Math.round((Date.now() - t0) / 1000)} s`);
    const before = await invoke<any[]>("list_memory_rows", { agentKey: null, scope });
    expect(before.length).toBe(510);
    const n1 = await invoke<number>("dream_now", { scope, agentKey: null });
    console.log(`e2e: first dream wrote ${n1} revision(s)`);
    const after = await invoke<any[]>("list_memory_rows", { agentKey: null, scope });
    const live = after.filter((r) => !r.valid_to);
    const retired = after.filter((r) => !!r.valid_to);
    console.log(`e2e: live ${live.length}, retired ${retired.length}`);
    expect(live.length).toBeLessThanOrEqual(500);
    expect(retired.length).toBeGreaterThanOrEqual(10);
    expect(retired.every((r) => r.provenance.startsWith("dream:"))).toBe(true);
    expect(retired.map((r) => r.id).sort()).toEqual(["b-0", "b-1", "b-2", "b-3", "b-4", "b-5", "b-6", "b-7", "b-8", "b-9"].sort());
    expect(after.some((r) => r.deleted)).toBe(false);
    const n2 = await invoke<number>("dream_now", { scope, agentKey: null });
    console.log(`e2e: second dream wrote ${n2} revision(s)`);
    expect(n2).toBe(0);
  });
});
