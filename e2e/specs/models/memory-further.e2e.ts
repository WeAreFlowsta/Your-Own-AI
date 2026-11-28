// "Look further", on the built app with the made-up bakery history, on a
// computer that reads slowly (a debug build told its reading speed):
// memory gives a reply only the closest of what it found and tells the
// model so; when the reply did not have what it needed, one button above
// the message box offers to look further. Pressing it sends its words as
// the person's message and that question is searched again with everything
// found. Saying it in other words does the same. Any other message is an
// ordinary message and the offer is gone.
//
// Launch with a slow reading speed, in tokens a second:
//   YOAI_TEST_READ_SPEED=30
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { waitForChat, ask, waitForReplyDone, invoke, shot } from "./_helpers";

const here = path.dirname(fileURLToPath(import.meta.url));
const fixture = path.resolve(here, "../../fixtures/memory");
const BASE = "http://127.0.0.1:11435/internal";
const SLOW = !!process.env.YOAI_TEST_READ_SPEED;

async function dev(route: string, query: Record<string, string>): Promise<any> {
  const res = await fetch(`${BASE}/${route}?${new URLSearchParams(query).toString()}`, { method: "POST" });
  const answer = await res.json().catch(() => ({ error: `HTTP ${res.status}` }));
  if (!res.ok || answer?.error) throw new Error(`${route}: ${JSON.stringify(answer?.error ?? res.status)}`);
  return answer;
}

async function newChat() {
  await waitForChat();
  if (await browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length > 0)) {
    await (await $('[data-testid="new-chat"]')).click();
    await browser.waitUntil(async () => browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length === 0), { timeout: 15_000 });
  }
}

type Entry = { role: string; content: string; given?: { search?: { ran?: boolean; found?: number; given?: number; closest_only?: boolean } } };

/** The open conversation's entries as recorded. */
async function recorded(): Promise<Entry[]> {
  const turn = ((await browser.execute(() => (window as any).__yoaiTurnsFull?.() ?? [])) as { agentKey: string; conversationHash: string }[]).slice(-1)[0];
  return invoke("get_conversation_transcript", { agentKey: turn.agentKey, conversationHash: turn.conversationHash });
}

/** The newest reply's search, once it is in the records. */
async function lastSearch(replies: number) {
  let found: Entry | undefined;
  await browser.waitUntil(
    async () => {
      const all = (await recorded().catch(() => [])).filter((e) => e.role === "assistant");
      found = all.length >= replies ? all[all.length - 1] : undefined;
      return !!found?.given?.search;
    },
    { timeout: 240_000, interval: 3_000, timeoutMsg: "the reply was not recorded with what it was given" },
  );
  return found!.given!.search!;
}

const offer = () => $('[data-testid="open-offer"]');
const userMessages = () => browser.execute(() => [...document.querySelectorAll('[data-testid="chat-message"][data-role="user"]')].map((m) => (m as HTMLElement).innerText.trim()));

// Questions the history holds no answer to, though it holds much that is near.
const ASKS = [
  "What did we decide about opening a second shop in Rosedale?",
  "Which accountant did we switch to after Bellamy?",
  "What price did we set for the Croissant Loaf?",
];

/** Ask in a new conversation until a reply is offered "Look further";
 *  what memory gave that reply. */
async function untilOffered(): Promise<{ question: string; given: number; found: number }> {
  for (const question of ASKS) {
    await newChat();
    await ask(question);
    await waitForReplyDone(600_000);
    const s = await lastSearch(1);
    console.log(`e2e: LOOK FURTHER "${question}": given ${s.given} of ${s.found}, closest only ${s.closest_only === true}`);
    expect(s.ran).toBe(true);
    expect(s.closest_only).toBe(true);
    expect(s.given!).toBeLessThan(s.found!);
    if (await (await offer()).isExisting()) return { question, given: s.given!, found: s.found! };
  }
  throw new Error("no reply was offered Look further, though each was given only the closest results");
}

describe("look further, when memory gave only the closest results", () => {
  it("the made-up history is read into the index", async function () {
    if (!SLOW) this.skip();
    this.timeout(1_200_000);
    await waitForChat();
    await ask("What colour is the sky on a clear day?");
    await waitForReplyDone();
    // Nothing to look through yet: nothing is offered.
    expect(await (await offer()).isExisting()).toBe(false);
    const ai = await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const p = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(p);
      const ais = ((await store.get("custom-ais")) || []) as { id?: string; agentPubKey?: string }[];
      const asked = (((window as any).__yoaiTurnsFull?.() ?? []) as { agentKey: string }[]).slice(-1)[0]?.agentKey ?? "";
      return (ais.find((x) => x.agentPubKey === asked) ?? ais[0])?.id ?? "";
    });
    expect(ai).toBeTruthy();
    const model = (await invoke<string | null>("get_current_model")) ?? "fixture";
    const loaded = await dev("memory-fixture-load", { path: path.join(fixture, "history.json"), all: ai, model });
    expect(loaded.messages).toBe(66);
    await invoke("memory_read_now", { on: true });
    await browser.waitUntil(
      async () => {
        const s = await invoke<{ total: number; read: number }>("memory_reading_status");
        return s.total >= 66 && s.read >= s.total;
      },
      { timeout: 900_000, interval: 5_000, timeoutMsg: "the history was not read into the index within 15 minutes" },
    );
  });

  it("the button sends its words as the person's message, and everything found is then given", async function () {
    if (!SLOW) this.skip();
    this.timeout(2_400_000);
    const first = await untilOffered();
    const button = await $('[data-testid="open-offer-take"]');
    expect(await button.getText()).toBe("Look further");
    // One button, no sentence.
    expect((await (await offer()).getText()).trim()).toBe("Look further");
    await shot("look-further-offered");
    await browser.execute((e) => (e as HTMLElement).click(), button);
    await browser.waitUntil(async () => (await userMessages()).length === 2, { timeout: 30_000, timeoutMsg: "pressing the offer sent no message" });
    expect((await userMessages())[1]).toContain("Look further");
    await waitForReplyDone(900_000);
    const again = await lastSearch(2);
    console.log(`e2e: LOOK FURTHER after the press: given ${again.given} of ${again.found}, closest only ${again.closest_only === true}`);
    expect(again.closest_only ?? false).toBe(false);
    expect(again.given!).toBeGreaterThan(first.given);
    // It is recorded as the person's own message, as typed.
    const entries = await recorded();
    expect(entries.filter((e) => e.role === "user").map((e) => e.content.trim())).toEqual([first.question, "Look further"]);
    // Everything was given: there is nothing further to offer.
    expect(await (await offer()).isExisting()).toBe(false);
  });

  it("said in other words, it does the same", async function () {
    if (!SLOW) this.skip();
    this.timeout(2_400_000);
    const first = await untilOffered();
    await ask("Yes, keep looking please");
    await waitForReplyDone(900_000);
    const again = await lastSearch(2);
    console.log(`e2e: LOOK FURTHER after it was said: given ${again.given} of ${again.found}`);
    expect(again.closest_only ?? false).toBe(false);
    expect(again.given!).toBeGreaterThan(first.given);
  });

  it("any other message is an ordinary message, and the offer is gone", async function () {
    if (!SLOW) this.skip();
    this.timeout(2_400_000);
    await untilOffered();
    await ask("Should I look further into the flour prices instead?");
    await browser.waitUntil(async () => !(await (await offer()).isExisting()), { timeout: 30_000, timeoutMsg: "the offer stayed after another message" });
    await waitForReplyDone(900_000);
    // Searched as its own question, held to the wait like any other.
    const s = await lastSearch(2);
    expect(s.ran).toBe(true);
    expect(s.given!).toBeLessThanOrEqual(s.found!);
    expect(s.given === s.found ? true : s.closest_only === true).toBe(true);
  });
});
