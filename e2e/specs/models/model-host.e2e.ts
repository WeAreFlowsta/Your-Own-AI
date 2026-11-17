// Who hosts an online model and where: each Online Models card that has a
// provider says so in one line ("From <host>, based in <place>." or "Made
// by <maker>. Served by <host>, based in <place>."), from the catalog's
// `provider`, `based_in` and `maker` (utils/modelHost.ts). Reads the
// catalog as any install does; skipped when it cannot be read.
import { waitForChat } from "./_helpers";

describe("an online model says who hosts it", () => {
  it("on its card, in one line", async function () {
    this.timeout(180_000);
    await waitForChat();
    await browser.url("tauri://localhost/online-models/");
    const lines = async () =>
      (await browser.execute(() => [...document.querySelectorAll("[data-testid='model-host']")].map((n) => (n as HTMLElement).innerText.trim()))) as string[];
    try {
      await browser.waitUntil(async () => (await lines()).length > 0, { timeout: 60_000, interval: 1_000 });
    } catch {
      console.log("e2e: no online catalog here (offline, or none served): skipped");
      this.skip();
    }
    const seen = await lines();
    const kinds = [...new Set(seen)];
    console.log(`e2e: ${seen.length} card(s) say who hosts them; ${kinds.length} different line(s): ${JSON.stringify(kinds.slice(0, 8))}`);
    for (const l of seen) expect(l).toMatch(/^(From .+|Made by .+\. Served by .+)\.$/);
    // At least one names where the host is based.
    expect(seen.some((l) => l.includes(", based in "))).toBe(true);
  });
});
