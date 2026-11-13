// "Running on CPU for stability" (e2e/gpu-notice-live.sh seeds the record of
// earlier exits before each launch and sets GPU_NOTICE_PHASE):
//   outside  the app closed twice without a clean exit while no model was
//            loading onto the graphics card (the window's own crash, a
//            force-quit): no notice, the graphics card stays in use.
//   during   it closed twice while a model was loading onto the card: the
//            notice shows, and "Try GPU again" clears it.
import { shot, waitForChat, invoke } from "./_helpers";

const PHASE = process.env.GPU_NOTICE_PHASE ?? "";
type Status = { active: boolean; just_engaged: boolean; cuda_disabled?: boolean };
const notice = () => browser.execute(() => (document.querySelector('[data-testid="gpu-safe-mode-notice"]') as HTMLElement | null)?.innerText?.replace(/\s+/g, " ") ?? null);

// Run by its live script only: in the models suite, where no phase is set,
// it is skipped.
(PHASE ? describe : describe.skip)(`GPU safe mode notice, exits ${PHASE || "(no phase set)"} a model load`, () => {
  it(PHASE === "outside" ? "does not move the AI off the graphics card" : "says it is running on CPU and can be turned back", async function () {
    this.timeout(600_000);
    await waitForChat();
    // The verdict is reached at the first model load; the notice reads it
    // when the page comes up.
    await browser.execute(() => location.reload());
    await browser.pause(2000);
    await waitForChat();
    const st = await invoke<Status>("gpu_safe_mode_status");
    console.log(`e2e: gpu safe mode ${JSON.stringify(st)}; the notice reads ${JSON.stringify(await notice())}`);
    if (PHASE === "outside") {
      expect(st.active).toBe(false);
      expect(await notice()).toBeNull();
      expect(await browser.execute(() => document.body.innerText)).not.toContain("Running on CPU for stability");
    } else if (PHASE === "during") {
      expect(st.active).toBe(true);
      await browser.waitUntil(async () => ((await notice()) ?? "").includes("Running on CPU for stability"), { timeout: 30_000, timeoutMsg: `the notice never showed: ${await notice()}` });
      expect(await notice()).toContain("switched to CPU to keep your system stable");
      await shot("gpu-notice");
      await (await $('[data-testid="gpu-retry"]')).click();
      await browser.waitUntil(async () => ((await notice()) ?? "").includes("Re-enabled - your next reply uses the graphics card."), { timeout: 60_000, timeoutMsg: `Try GPU again did not answer: ${await notice()}` });
      expect((await invoke<Status>("gpu_safe_mode_status")).active).toBe(false);
    } else {
      throw new Error("set GPU_NOTICE_PHASE to outside or during");
    }
  });
});
