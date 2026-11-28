// Memory while conversations from another of the person's computers are
// still arriving: Recently says so under the tidying line, with how many
// and from where, and that nothing done here is lost. With nothing
// arriving there is no such line. A debug build answers with the count in
// YOAI_TEST_HISTORY_ARRIVING, so no second device is needed here.
import { waitForChat, shot } from "./_helpers";

const ARRIVING = Number(process.env.YOAI_TEST_HISTORY_ARRIVING ?? "0");

describe("memory while history is arriving", () => {
  it("Recently says it is catching up, or says nothing", async function () {
    this.timeout(300_000);
    await waitForChat();
    await browser.url("tauri://localhost/your-memory/");
    await (await $('[data-testid="memory-tabs"]')).waitForExist({ timeout: 60_000 });
    const tab = await $('[data-testid="memory-tab"][data-tab="recently"]');
    await tab.waitForDisplayed({ timeout: 60_000 });
    await tab.click();
    await (await $('[data-testid="recently-board"]')).waitForDisplayed({ timeout: 60_000 });
    const line = await $('[data-testid="memory-catching-up"]');
    if (ARRIVING > 0) {
      await line.waitForDisplayed({ timeout: 60_000, timeoutMsg: "Recently never said it was catching up" });
      const said = (await line.getText()).replace(/\s+/g, " ").trim();
      console.log(`e2e: catching up reads: ${said}`);
      expect(said).toContain("Catching up with Test computer.");
      expect(said).toContain(ARRIVING === 1 ? "1 conversation is still arriving" : `${ARRIVING} conversations are still arriving`);
      expect(said).toContain("Nothing you did here is lost.");
      await shot("memory-catching-up");
    } else {
      await browser.pause(5000);
      expect(await line.isExisting()).toBe(false);
    }
  });
});
