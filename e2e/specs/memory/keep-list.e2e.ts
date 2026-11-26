// Keeps a list of things in memory, as Remember under a message does, on
// the profile as it stands (YOAI_E2E_KEEP_PROFILE=1). For building a
// measuring profile where settled things were kept. Runs only when
// YOAI_E2E_KEEP_LIST names a JSON file, kept outside the repository:
//   [{ c, s, t, text, itemType }]  c: the conversation's start, s: the
//   message's sequence, t: its time (both in microseconds).
import { readFileSync } from "node:fs";
import { waitForChat, invoke } from "../models/_helpers";

type Entry = { c: number; s: number; t: number; text: string; itemType: string };
const FILE = process.env.YOAI_E2E_KEEP_LIST ?? "";

describe("things a person kept", () => {
  it("keeps each entry of the list as an item", async function () {
    if (!FILE) this.skip();
    this.timeout(1_800_000);
    const list = JSON.parse(readFileSync(FILE, "utf8")) as Entry[];
    await waitForChat();
    // A kept profile's records take a while to open: wait until memory answers.
    await browser.waitUntil(async () => invoke<unknown[]>("memory_items").then(() => true, () => false), { timeout: 600_000, interval: 5_000, timeoutMsg: "memory did not answer within ten minutes" });
    let kept = 0;
    let already = 0;
    for (const e of list) {
      const answer = await invoke<{ already: boolean }>("memory_keep", {
        input: { text: e.text, itemType: e.itemType, ptr: { c: e.c, s: e.s, t: e.t, a: 0, b: 0 }, asOf: Math.floor(e.t / 1000), kind: "exchange", why: "pressed" },
      });
      if (answer.already) already++;
      else kept++;
    }
    const items = await invoke<unknown[]>("memory_items");
    console.log(`e2e: KEPT ${kept} of ${list.length} (${already} were already kept); memory now holds ${items.length} items`);
    expect(kept + already).toBe(list.length);
  });
});
