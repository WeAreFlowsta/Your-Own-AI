// Writes what memory holds on the profile as it stands
// (YOAI_E2E_KEEP_PROFILE=1) to a JSON file, for reading beside a measure.
// Runs only when YOAI_E2E_ITEMS_OUT names the file, kept outside the
// repository.
import { writeFileSync } from "node:fs";
import { waitForChat, invoke } from "../models/_helpers";

const FILE = process.env.YOAI_E2E_ITEMS_OUT ?? "";

describe("what memory holds", () => {
  it("writes every item to the file", async function () {
    if (!FILE) this.skip();
    this.timeout(900_000);
    await waitForChat();
    // A kept profile's records take a while to open: wait until memory answers.
    await browser.waitUntil(async () => invoke<unknown[]>("memory_items").then(() => true, () => false), { timeout: 600_000, interval: 5_000, timeoutMsg: "memory did not answer within ten minutes" });
    const items = await invoke<unknown[]>("memory_items");
    writeFileSync(FILE, JSON.stringify(items, null, 1));
    console.log(`e2e: wrote ${items.length} items`);
    expect(items.length).toBeGreaterThan(0);
  });
});
