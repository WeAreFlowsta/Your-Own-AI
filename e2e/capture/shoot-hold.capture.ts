// Not a take: keeps a kept shoot profile's app open on Your AIs (so it backs up and syncs with the identity's other
// devices) for YOAI_HOLD_MIN minutes (default 20), logging the AIs it has every minute.
import { appUrl } from "../wdio.conf";

describe("capture", () => {
  it("shoot hold", async () => {
    await browser.url(appUrl("/your-ais/"));
    const minutes = Number(process.env.YOAI_HOLD_MIN ?? 20);
    for (let m = 0; m < minutes; m++) {
      const names = await browser.execute(() => [...document.querySelectorAll('button[title^="Edit AI"]')].map((b) => b.getAttribute("title")));
      console.log(`shoot-hold: minute ${m}: ${JSON.stringify(names)}`);
      await browser.pause(60_000);
    }
  });
});
