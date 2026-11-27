// Controls said in the chat, on the built app. "Keep this conversation
// out of memory" makes the conversation private and says so under the
// message, with Undo. "Forget that" after an exchange in which nothing was
// kept leaves that exchange out of memory. "That's right" confirms the one
// thing an AI kept from the exchange before. A message that only looks
// like one of them changes nothing.
import { waitForChat, ask, waitForReplyDone, invoke } from "./_helpers";

type Use = { startedAt: number; private: boolean; excluded: number[] };
type Seen = { id: string; text: string; item: { state: string; confirmed?: unknown; source?: { ptr?: { c: number; s: number } | null } | null } };

const notUsed = () => invoke<Use[]>("memory_conversations_not_used");
const lastSaid = '(//*[@data-testid="chat-message"][@data-role="user"])[last()]//*[@data-testid="kept-said"]';

async function saidLine(timeout = 60_000): Promise<{ said: string; text: string }> {
  const el = await $(lastSaid);
  await el.waitForExist({ timeout, timeoutMsg: "no line under the message says what was done" });
  return { said: (await el.getAttribute("data-said")) ?? "", text: await el.getText() };
}

async function newChat() {
  await browser.execute(() => (document.querySelector('[data-testid="new-chat"]') as HTMLElement | null)?.click());
  await browser.waitUntil(async () => browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length === 0), { timeout: 15_000 });
}

describe("controls said in the chat", () => {
  it("an ordinary message that looks like one changes nothing", async function () {
    this.timeout(600_000);
    await waitForChat();
    await ask("My delivery van is blue. One short sentence back, please.");
    await waitForReplyDone();
    const before = (await notUsed()).length;
    await ask("Is that right?");
    await waitForReplyDone();
    await browser.pause(8_000);
    expect(await (await $(lastSaid)).isExisting()).toBe(false);
    expect((await notUsed()).length).toBe(before);
  });

  it("forget that, with nothing kept, leaves the exchange before out of memory, and Undo puts it back", async function () {
    this.timeout(600_000);
    await ask("Forget that.");
    await waitForReplyDone();
    const line = await saidLine();
    console.log(`e2e: under "Forget that.": ${JSON.stringify(line)}`);
    // Nothing was kept from the exchange, or one thing was: either the
    // exchange is left out, or that item is forgotten.
    expect(["left_out", "forgotten"]).toContain(line.said);
    if (line.said === "left_out") {
      const use = (await notUsed()).find((u) => u.excluded.length > 0);
      expect(use?.excluded.length).toBeGreaterThan(0);
      await browser.execute((e) => (e as HTMLElement).click(), await $('(//*[@data-testid="chat-message"][@data-role="user"])[last()]//*[@data-testid="kept-undo"]'));
      await browser.waitUntil(async () => !(await notUsed()).some((u) => u.excluded.length > 0), { timeout: 30_000, timeoutMsg: "Undo did not put the exchange back" });
    }
  });

  it("keep this conversation out of memory makes it private, and Undo takes that back", async function () {
    this.timeout(600_000);
    await newChat();
    await ask("Name one primary color and nothing else.");
    await waitForReplyDone();
    await ask("Keep this conversation out of memory.");
    await waitForReplyDone();
    const line = await saidLine();
    console.log(`e2e: under the request: ${JSON.stringify(line)}`);
    expect(line.said).toBe("kept_out");
    expect(line.text).toContain("kept out of memory");
    await browser.waitUntil(async () => (await notUsed()).some((u) => u.private), { timeout: 30_000, timeoutMsg: "the conversation was not made private" });
    await browser.execute((e) => (e as HTMLElement).click(), await $('(//*[@data-testid="chat-message"][@data-role="user"])[last()]//*[@data-testid="kept-undo"]'));
    await browser.waitUntil(async () => !(await notUsed()).some((u) => u.private), { timeout: 30_000, timeoutMsg: "Undo did not make it usable again" });
  });

  it("that's right confirms the one thing an AI kept from the exchange before", async function () {
    this.timeout(600_000);
    await newChat();
    await ask("Name one primary color and nothing else.");
    const first = await waitForReplyDone();
    // Where the exchange is in the records: keeping the reply by hand
    // answers with its place, and that keep is then forgotten, so the one
    // thing left from the exchange is the AI's item below.
    const turn = (await browser.execute(() => ((window as any).__yoaiTurnsFull?.() ?? []).slice(-1)[0])) as { agentKey?: string; conversationHash?: string } | undefined;
    if (!turn?.conversationHash) this.skip();
    let mine: { item: Seen } | null = null;
    await browser.waitUntil(
      async () => {
        mine = await invoke<{ item: Seen }>("memory_keep_message", { input: { conversation: turn!.conversationHash, agentKey: turn!.agentKey ?? null, seq: null, role: "assistant", text: first.text, why: "pressed" } }).catch(() => null);
        return !!mine;
      },
      { timeout: 60_000, interval: 3_000, timeoutMsg: "the reply could not be found in the records" },
    );
    const at = mine!.item.item.source?.ptr;
    const c = at?.c ?? 0;
    const s = at?.s ?? 1;
    await invoke("memory_item_change", { id: mine!.item.id, edit: { do: "forget" } });
    const kept = await invoke<{ item: Seen }>("memory_keep", {
      input: { text: "Said-controls test: the first color named was a primary one.", itemType: "detail", ptr: { c, s, t: c, a: 0, b: 0 }, asOf: Math.round(c / 1000), kind: "exchange", why: "settled", by: "ai:said-controls-test" },
    });
    await ask("Yes, that's right.");
    await waitForReplyDone();
    const line = await saidLine();
    console.log(`e2e: under "that's right": ${JSON.stringify(line)}`);
    expect(line.said).toBe("confirmed");
    const now = (await invoke<Seen[]>("memory_items")).find((i) => i.id === kept.item.id);
    expect(!!now?.item.confirmed).toBe(true);
  });
});
