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
    const path = await invoke<string>("matrix_run", { stamp, legs: ["memory"] });
    console.log(`e2e: matrix report ${path}`);
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
