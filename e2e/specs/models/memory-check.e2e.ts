// The built-in check, run to its end on the model this machine has
// loaded, on the built app. Long: three runs of the shipped examples.
// Launch with YOAI_MEMORY_CHECK=1 (a development build runs the check only
// then) and YOAI_TEST_AWAY_SECS set high, so the computer counts as idle.
// What is held: the check finishes, the same model then has a stored
// result, and the tidying line's state follows from it (on when the model
// may tidy, "not passed" when it may not). The marks are printed for the
// record; which way the verdict falls is a measure, not an assertion.
import { waitForChat, ask, waitForReplyDone, invoke, shot } from "./_helpers";

type Mark = { passed: boolean; caught: number; ofYes: number; wrongYes: number; ofNo: number; right: number; examples: number };
type Check = { model: string | null; done: number; total: number; result: { mayTidy: boolean; mayKeep: boolean; mayClose: boolean; [k: string]: unknown } | null };
type Tidy = { state: string; model: string | null; mayKeep: boolean };

const ASKED = !!process.env.YOAI_MEMORY_CHECK;

describe("the built-in check", () => {
  it("runs to its end on the loaded model, and the tidying line follows the result", async function () {
    if (!ASKED) this.skip();
    this.timeout(4 * 3_600_000);
    await waitForChat();
    // A reply loads the model the check is about.
    await ask("Name one primary color and nothing else.");
    await waitForReplyDone();
    let last = -1;
    await browser.waitUntil(
      async () => {
        const c = await invoke<Check>("memory_check_status");
        if (c.done !== last) {
          last = c.done;
          console.log(`e2e: check ${c.done} of ${c.total} on ${c.model}`);
        }
        return !!c.result;
      },
      { timeout: 3.5 * 3_600_000, interval: 30_000, timeoutMsg: "the check did not finish" },
    );
    const c = await invoke<Check>("memory_check_status");
    console.log(`e2e: CHECK RESULT on ${c.model}: ${JSON.stringify(c.result)}`);
    for (const [part, m] of Object.entries(c.result ?? {})) {
      const mark = m as Mark | boolean | number | string | null;
      if (mark && typeof mark === "object" && "caught" in mark) console.log(`e2e: ${part}: caught ${mark.caught} of ${mark.ofYes}, wrong ${mark.wrongYes} of ${mark.ofNo}, passed ${mark.passed}`);
    }
    const t = await invoke<Tidy>("memory_tidy_status");
    console.log(`e2e: tidying line state: ${t.state}, may keep ${t.mayKeep}`);
    expect(t.state).toBe(c.result!.mayTidy ? "on" : "not-passed");
    expect(t.mayKeep).toBe(c.result!.mayKeep);
    await browser.url("tauri://localhost/your-memory/");
    await (await $('[data-testid="memory-tabs"]')).waitForExist({ timeout: 60_000 });
    await (await $('[data-testid="memory-tab"][data-tab="recently"]')).click();
    const line = await $('[data-testid="tidying-line"]');
    await line.waitForExist({ timeout: 60_000 });
    expect(await line.getAttribute("data-state")).toBe(t.state);
    await shot("memory-check-line");
  });
});
