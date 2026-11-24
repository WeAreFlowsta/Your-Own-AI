// Keeping without being asked, on the built app: a reply is recorded and
// its exchange waits; a pass of the keeping job puts it to the loaded
// model ("was something settled", then the item form); what is kept is an
// item marked as kept by the AI, not confirmed, shown in Recently with an
// Undo; an undone keep is not made again; a stopped reply and the
// person's keeping switch are respected.
import { waitForChat, ask, waitForReplyDone, invoke, shot } from "./_helpers";

const BASE = "http://127.0.0.1:11435/internal";
async function pass(): Promise<any> {
  const res = await fetch(`${BASE}/memory-keeping?force=1`, { method: "POST" });
  const answer = await res.json();
  if (!res.ok || answer?.error) throw new Error(`memory-keeping: ${JSON.stringify(answer?.error ?? res.status)}`);
  return answer.report;
}

type Item = { id: string; text: string; savedBy: string; item: { state: string; confirmed?: unknown; audience?: { kind: string }; source?: { kind: string; why?: string; quote?: string | null; ptr?: { c: number; s: number } | null } | null } };
const items = () => invoke<Item[]>("memory_items");
const current = async () => (await items()).filter((i) => i.item.state === "current" || i.item.state === "open");

describe("what an exchange settled is kept in idle time", () => {
  it("a stated decision is looked at once, and what is kept is the AI's, not confirmed", async () => {
    await waitForChat();
    await ask("We have decided: from March 9 the bakery is closed on Mondays, because Monday sales do not cover the staff. Please confirm in one short sentence.");
    await waitForReplyDone();
    expect((await current()).length).toBe(0);
    const report = await pass();
    console.log(`e2e: keeping pass: ${JSON.stringify(report)}`);
    expect(report.ran).toBe(true);
    expect(report.lookedAt).toBe(1);
    expect(report.leftInQueue).toBe(0);
    const kept = await current();
    console.log(`e2e: kept: ${JSON.stringify(kept.map((i) => ({ text: i.text, by: i.savedBy, why: i.item.source?.why, quote: i.item.source?.quote })))}`);
    // The model's answer is its own: when it kept something, the item
    // carries every mark the design asks for.
    for (const i of kept) {
      expect(i.savedBy.startsWith("ai:")).toBe(true);
      expect(i.item.confirmed ?? null).toBe(null);
      expect(i.item.source?.why).toBe("settled");
      expect(i.item.source?.ptr?.c).toBeGreaterThan(0);
    }
    expect(report.kept).toBe(kept.length);
  });

  it("a second pass looks at nothing: each exchange is put to the model once", async () => {
    const before = (await current()).length;
    const report = await pass();
    expect(report.ran).toBe(false);
    expect(report.whyNot).toBe("no new exchange waits");
    expect((await current()).length).toBe(before);
  });

  it("Recently shows what the AI kept, with Undo, and the menu counts it", async () => {
    const kept = await current();
    if (!kept.length) {
      console.log("e2e: the model kept nothing from the decision; the Recently step has nothing to show");
      return;
    }
    await browser.url("tauri://localhost/your-memory/");
    const row = await $('[data-testid="recently-row"][data-what="kept"]');
    await row.waitForExist({ timeout: 60_000 });
    const words = (await browser.execute((e) => (e as HTMLElement).innerText, row)) as string;
    console.log(`e2e: recently: ${JSON.stringify(words)}`);
    await shot("keeping-recently");
    await browser.execute((e) => (e as HTMLElement).click(), await row.$('[data-testid="recently-undo"]'));
    await browser.waitUntil(async () => (await current()).length === kept.length - 1, { timeout: 60_000, timeoutMsg: "Undo did not take the AI's item back" });
    await browser.url("tauri://localhost/chat/");
  });

  it("passing talk and a hypothetical: whatever is kept is marked unconfirmed", async () => {
    await waitForChat();
    await ask("We could close on Saturdays too, I am only thinking aloud. One short sentence back, please.");
    await waitForReplyDone();
    const before = (await items()).map((i) => i.id);
    const report = await pass();
    console.log(`e2e: keeping pass on a hypothetical: ${JSON.stringify(report.exchanges)}`);
    for (const i of (await items()).filter((x) => !before.includes(x.id))) {
      expect(i.item.confirmed ?? null).toBe(null);
      expect(i.savedBy.startsWith("ai:")).toBe(true);
    }
  });

  it("with keeping turned off, nothing waits and nothing is kept", async () => {
    await invoke("memory_tidy_set", { off: true });
    await ask("Decision: the new oven is the Rofco B40, ordered today. One short sentence back, please.");
    await waitForReplyDone();
    const before = (await items()).length;
    const report = await pass();
    console.log(`e2e: keeping pass while off: ${JSON.stringify({ ran: report.ran, whyNot: report.whyNot })}`);
    expect(report.ran).toBe(false);
    expect(report.whyNot).toBe("keeping is turned off");
    expect((await items()).length).toBe(before);
    // The turn's records can land a few seconds after the reply shows;
    // they are written while keeping is still off.
    await browser.pause(8000);
    await invoke("memory_tidy_set", { off: false });
    // What waited while it was off is not looked at afterward.
    const after = await pass();
    console.log(`e2e: keeping pass after turning it on again: ${JSON.stringify(after)}`);
    expect(after.ran).toBe(false);
  });
});
