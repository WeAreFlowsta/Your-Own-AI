// Something the person confirmed on this device, and a replacement of it
// that tidying made on another of their devices before it had heard: the
// confirmed item stands, and Recently asks about it once. The other
// device's part is its ledger entry, written here as that device would
// have synced it, so no second device and no model is needed.
import { waitForChat, invoke, shot } from "./_helpers";

type Seen = { id: string; text: string; item: { state: string; confirmed?: { at: number; how: string } | null } };
type Question = { id: string; older: { id: string }; newer: { id: string }; now: string; kept: boolean };

const OLDER = "Kept-version test: the site is on the small plan.";
const NEWER = "Kept-version test: the site is on the medium plan.";

async function keep(text: string, c: number): Promise<string> {
  const kept = await invoke<{ item: Seen }>("memory_keep", {
    input: { text, itemType: "decision", ptr: { c, s: 1, t: c, a: 0, b: 0 }, asOf: Math.round(c / 1000), kind: "exchange", why: "pressed" },
  });
  return kept.item.id;
}

describe("a confirmation that outranked tidying on another device", () => {
  it("is asked about once in Recently, and any answer closes it", async function () {
    this.timeout(900_000);
    await waitForChat();
    await browser.waitUntil(async () => invoke<unknown[]>("memory_items").then(() => true, () => false), { timeout: 600_000, interval: 5_000, timeoutMsg: "memory did not answer within ten minutes" });
    const older = await keep(OLDER, 1_700_000_100_000_000);
    const newer = await keep(NEWER, 1_700_000_900_000_000);
    expect((await invoke<Question[]>("memory_questions")).length).toBe(0);

    // The other device's entry: it replaced the older item a moment ago.
    const at = Date.now();
    const pair = older <= newer ? [older, newer] : [newer, older];
    await invoke("save_memory_row", {
      agentKey: null,
      row: { id: `sys-judged-${pair[0]}-${pair[1]}-${at}`, scope: "memory", kind: "note", text: "", structured: { sys: "undo", judged: { pair, outcome: "replaced", at } }, as_of: at, valid_to: null, saved_by: "person", provenance: "authored", pinned: false, topics: [], replaces: [], deleted: false, updated_at: at },
    });
    const waiting = await invoke<Question[]>("memory_questions");
    console.log(`e2e: questions waiting: ${JSON.stringify(waiting.map((q) => ({ older: q.older.id === older, newer: q.newer.id === newer, kept: q.kept })))}`);
    expect(waiting.length).toBe(1);
    expect(waiting[0].kept).toBe(true);
    expect(waiting[0].older.id).toBe(older);
    expect(waiting[0].newer.id).toBe(newer);

    await browser.url("tauri://localhost/your-memory/");
    await (await $('[data-testid="memory-tabs"]')).waitForExist({ timeout: 60_000 });
    const tab = await $('[data-testid="memory-tab"][data-tab="recently"]');
    await tab.waitForDisplayed({ timeout: 60_000 });
    await tab.click();
    const line = await $('[data-testid="memory-question"]');
    await line.waitForDisplayed({ timeout: 60_000, timeoutMsg: "Recently did not ask about the version that was kept" });
    const said = (await line.getText()).replace(/\s+/g, " ").trim();
    console.log(`e2e: the question: ${said}`);
    expect(said).toContain("Your version was kept.");
    expect(said).toContain("the small plan");
    expect(said).toContain("the medium plan");
    expect(said).toContain("Are both still true?");
    await shot("memory-kept-version");

    await (await $('[data-testid="question-both"]')).click();
    await browser.waitUntil(async () => (await invoke<Question[]>("memory_questions")).length === 0, { timeout: 60_000, timeoutMsg: "Both are true did not close the question" });
    const all = await invoke<Seen[]>("memory_items");
    expect(all.find((i) => i.id === older)!.item.state).toBe("current");
    expect(all.find((i) => i.id === newer)!.item.state).toBe("current");
    expect(await $('[data-testid="memory-question"]').isExisting()).toBe(false);
  });
});
