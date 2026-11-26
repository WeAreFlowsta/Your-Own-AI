// One of the person's devices tidies at a time, chosen by the app with
// nothing to see or set. While another device holds the turn, a pass here
// judges nothing; once that turn has run out (the other device is off or
// offline), a pass here takes it. At no point does a Memory board name a
// device that tidies or offer a way to choose one. The other device's part
// is its row, written here as it would have synced.
import { waitForChat, invoke, shot } from "./_helpers";

const BASE = "http://127.0.0.1:11435/internal";
type Row = { id: string; structured?: { sys?: string; lease?: { device: string; score: number; until: number } } | null };
type Report = { ran: boolean; whyNot: string | null };

async function tidy(queue: string): Promise<Report> {
  const res = await fetch(`${BASE}/memory-tidy?${new URLSearchParams({ force: "1", queue })}`, { method: "POST" });
  return (await res.json()).report as Report;
}

const OTHER = "another-device-of-theirs";
async function otherHolds(until: number): Promise<void> {
  const now = Date.now();
  await invoke("save_memory_row", {
    agentKey: null,
    row: { id: "sys-lease", scope: "memory", kind: "note", text: "", structured: { sys: "lease", lease: { device: OTHER, score: 30, until } }, as_of: now, valid_to: null, saved_by: "person", provenance: "authored", pinned: false, topics: [], replaces: [], deleted: false, updated_at: now },
  });
}
const turn = async () => (await invoke<Row[]>("list_memory_rows", { agentKey: null, scope: "memory" })).find((r) => r.structured?.sys === "lease")?.structured?.lease;

async function boardsSay(): Promise<string> {
  await browser.url("tauri://localhost/your-memory/");
  await (await $('[data-testid="memory-tabs"]')).waitForExist({ timeout: 60_000 });
  let all = "";
  for (const tab of ["map", "recently", "open", "rules"]) {
    const button = await $(`[data-testid="memory-tab"][data-tab="${tab}"]`);
    await button.waitForDisplayed({ timeout: 60_000 });
    await button.click();
    await browser.pause(1500);
    all += ` ${await (await $("body")).getText()}`;
  }
  return all.replace(/\s+/g, " ");
}

describe("which device tidies", () => {
  it("is another device's turn until it runs out, and no board says so", async function () {
    this.timeout(900_000);
    await waitForChat();
    await browser.waitUntil(async () => invoke<unknown[]>("memory_items").then(() => true, () => false), { timeout: 600_000, interval: 5_000, timeoutMsg: "memory did not answer within ten minutes" });
    const c = 1_700_000_100_000_000;
    const kept = await invoke<{ item: { id: string } }>("memory_keep", {
      input: { text: "Tidy-turn test: the shop closes at five.", itemType: "decision", ptr: { c, s: 1, t: c, a: 0, b: 0 }, asOf: Math.round(c / 1000), kind: "exchange", why: "pressed" },
    });

    // Another device holds the turn for hours yet: nothing is judged here.
    await otherHolds(Date.now() + 5 * 60 * 60 * 1000);
    const waited = await tidy(kept.item.id);
    console.log(`e2e: while another device holds the turn: ${JSON.stringify({ ran: waited.ran, why: waited.whyNot })}`);
    expect(waited.ran).toBe(false);
    expect(waited.whyNot).toBe("another device tidies");
    expect((await turn())?.device).toBe(OTHER);
    const during = await boardsSay();
    await (await $('[data-testid="memory-tab"][data-tab="recently"]')).click();
    const line = await $('[data-testid="tidying-line"]');
    await line.waitForDisplayed({ timeout: 60_000 });
    console.log(`e2e: the tidying line meanwhile: ${(await line.getText()).replace(/\s+/g, " ").slice(0, 240)}`);
    await shot("memory-tidy-turn-elsewhere");

    // Its turn ran out a minute ago: this device takes it and the pass runs.
    await otherHolds(Date.now() - 60_000);
    const took = await tidy(kept.item.id);
    console.log(`e2e: after the turn ran out: ${JSON.stringify({ ran: took.ran, why: took.whyNot })}`);
    expect(took.ran).toBe(true);
    const now = await turn();
    expect(now?.device).not.toBe(OTHER);
    expect(now!.until).toBeGreaterThan(Date.now() + 5 * 60 * 60 * 1000);
    const after = await boardsSay();

    // Nothing names the device that tidies or offers to choose it.
    for (const said of [during, after]) {
      expect(said).not.toContain(OTHER);
      expect(said).not.toMatch(/lease|takes? over|tidies memory|choose (the|which) device|this (computer|device) tidies/i);
    }
  });
});
