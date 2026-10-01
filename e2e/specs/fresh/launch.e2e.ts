// Flow 1 (planning/UI_AUTOMATION.md): a fresh profile launches to the
// welcome screen and the hardware check names a model. The smallest proof
// that the window, the webview, the front end and the Rust side all came
// up - the test every release must pass first.
import { resolve } from "node:path";
import { SHOTS } from "../../wdio.conf";

const shot = (name: string) => browser.saveScreenshot(resolve(SHOTS, `${name}.png`));

describe("launch", () => {
  it("a fresh profile lands on the welcome screen", async () => {
    const title = await $('[data-testid="welcome-title"]');
    await title.waitForDisplayed({ timeout: 60_000 });
    await shot("01-welcome");
    expect((await title.getText()).length).toBeGreaterThan(0);
  });

  it("the hardware check recommends a model and offers the download", async () => {
    const rec = await $('[data-testid="welcome-recommended"]');
    // The Rust side measures the machine first ("One moment..") - wait for
    // the real label, which proves an invoke round trip.
    await browser.waitUntil(async () => (await rec.getAttribute("data-pending")) === "0", {
      timeout: 60_000,
      timeoutMsg: "the recommendation never resolved",
    });
    const label = await rec.getText();
    expect(label.length).toBeGreaterThan(0);
    expect(label).not.toContain("One moment");
    const download = await $('[data-testid="welcome-download"]');
    await expect(download).toBeEnabled();
    await shot("02-recommendation");
    // The download itself is never started here: that is a multi-GB fetch
    // and the person's choice, never a test's (feedback: no big downloads
    // without consent).
  });
});
