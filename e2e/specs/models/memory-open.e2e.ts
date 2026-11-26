// The Open board, on the built app: what is waiting is listed oldest
// first; a later item that reports an open thing done moves it under "May
// already be done" with what suggests it, and closes nothing; "Still
// waiting" takes the suggestion away for good; Done closes it and Recently
// offers Reopen; "Ask ... to do it now" opens the chat with the request
// ready to send.
import { waitForChat, ask, waitForReplyDone, invoke, shot } from "./_helpers";

const BASE = "http://127.0.0.1:11435/internal";
async function dev(method: "GET" | "POST", route: string, query: Record<string, string> = {}): Promise<any> {
  const q = new URLSearchParams(query).toString();
  const res = await fetch(`${BASE}/${route}${q ? `?${q}` : ""}`, { method });
  const answer = await res.json().catch(() => ({ error: `HTTP ${res.status}` }));
  if (!res.ok || answer?.error) throw new Error(`${route}: ${JSON.stringify(answer?.error ?? res.status)}`);
  return answer;
}

type Item = { id: string; text: string; item: { state: string } };
const item = async (id: string) => (await invoke<Item[]>("memory_items")).find((i) => i.id === id)!;
const DAY = 86_400_000;
const textOf = async (sel: string) => (await browser.execute((s) => (document.querySelector(s) as HTMLElement | null)?.innerText ?? "", sel)) as string;
const press = async (sel: string) => {
  const el = await $(sel);
  await el.waitForExist({ timeout: 30_000 });
  await browser.execute((e) => (e as HTMLElement).click(), el);
};

/** Keep words as an item of a type, as of some days ago. */
async function keep(text: string, itemType: "open" | "decision", daysAgo: number): Promise<string> {
  const at = Date.now() - daysAgo * DAY;
  const kept = await invoke<{ item: Item; already: boolean }>("memory_keep", {
    input: { text, itemType, ptr: { c: at * 1000, s: 0, t: at * 1000, a: 0, b: 0 }, asOf: at, kind: "exchange", why: "pressed" },
  });
  expect(kept.already).toBe(false);
  return kept.item.id;
}

const OPEN = "Ask the fitter for a quote on a second oven deck.";
const LATER = "The fitter sent the quote for the second oven deck on Tuesday: $1,850 fitted.";
const OTHER = "Decide which flour supplier to use from January.";
let open = "";
let later = "";
let other = "";

async function openBoard() {
  await browser.url("tauri://localhost/chat/");
  await browser.url("tauri://localhost/your-memory/#open");
  await (await $('[data-testid="open-board"]')).waitForExist({ timeout: 60_000 });
}

describe("the Open board", () => {
  it("lists what is waiting, oldest first", async function () {
    this.timeout(600_000);
    await waitForChat();
    await ask("Name one primary colour and nothing else.");
    await waitForReplyDone();
    other = await keep(OTHER, "open", 3);
    open = await keep(OPEN, "open", 9);
    await openBoard();
    await browser.waitUntil(async () => (await $$('[data-testid="open-row"]')).length === 2, { timeout: 30_000, timeoutMsg: "the two open things are not listed" });
    const ids = (await browser.execute(() => Array.from(document.querySelectorAll('[data-testid="open-row"]')).map((r) => r.getAttribute("data-id")))) as string[];
    expect(ids).toEqual([open, other]);
    expect(await textOf('[data-testid="open-board"]')).toContain("Everything still waiting, oldest first.");
    expect(await (await $('[data-testid="open-maybe-done"]')).isExisting()).toBe(false);
    await shot("memory-open-waiting");
  });

  it("a later item that reports it done moves it under May already be done, and closes nothing", async function () {
    this.timeout(600_000);
    later = await keep(LATER, "decision", 1);
    const { report } = await dev("POST", "memory-tidy", { force: "1", queue: later });
    console.log(`e2e: tidying: ${JSON.stringify(report)}`);
    expect(report.ran).toBe(true);
    const pair = (report.pairs as { earlier: string; later: string; verdict: string }[]).find((p) => p.earlier === open && p.later === later);
    expect(pair?.verdict).toBe("DONE");
    expect(report.mayBeDone).toBe(1);
    expect(report.replaced).toBe(0);
    // Nothing was closed for the person.
    expect((await item(open)).item.state).toBe("open");
    await openBoard();
    const row = `[data-testid="open-maybe-row"][data-id="${open}"]`;
    await (await $(row)).waitForExist({ timeout: 30_000 });
    const words = await textOf(row);
    console.log(`e2e: may already be done: ${JSON.stringify(words.replace(/\s+/g, " "))}`);
    expect(words).toContain(OPEN);
    expect(words).toContain("It was never marked done.");
    expect(words).toContain(LATER);
    // The other open thing is still under what is waiting.
    expect(await (await $(`[data-testid="open-row"][data-id="${other}"]`)).isExisting()).toBe(true);
    await shot("memory-open-may-be-done");
  });

  it("Still waiting takes the suggestion away, and the same two are not put together again", async function () {
    this.timeout(600_000);
    await press(`[data-testid="open-maybe-row"][data-id="${open}"] [data-testid="open-still-waiting"]`);
    await (await $(`[data-testid="open-row"][data-id="${open}"]`)).waitForExist({ timeout: 30_000 });
    expect(await (await $('[data-testid="open-maybe-done"]')).isExisting()).toBe(false);
    expect((await invoke<unknown[]>("memory_may_be_done")).length).toBe(0);
    expect((await item(open)).item.state).toBe("open");
    const { report } = await dev("POST", "memory-tidy", { force: "1", queue: later });
    console.log(`e2e: tidying again: ${JSON.stringify(report)}`);
    expect((report.pairs as { earlier: string; later: string }[]).some((p) => p.earlier === open && p.later === later)).toBe(false);
  });

  it("Done closes it, and Recently offers Reopen", async function () {
    this.timeout(600_000);
    await press(`[data-testid="open-row"][data-id="${open}"] [data-testid="open-done"]`);
    await (await $(`[data-testid="open-row"][data-id="${open}"]`)).waitForExist({ reverse: true, timeout: 30_000 });
    expect((await item(open)).item.state).toBe("done");
    await press('[data-testid="memory-tab"][data-tab="recently"]');
    const closed = '[data-testid="recently-row"][data-what="closed"]';
    await (await $(closed)).waitForExist({ timeout: 30_000 });
    expect(await textOf(`${closed} [data-testid="recently-undo"]`)).toBe("Reopen");
    await shot("memory-open-closed");
    await press(`${closed} [data-testid="recently-undo"]`);
    await browser.waitUntil(async () => (await item(open)).item.state === "open", { timeout: 30_000, timeoutMsg: "Reopen did not open it again" });
  });

  it("That's done, said in the chat, marks the one thing the conversation left open as done, with Undo", async function () {
    this.timeout(600_000);
    await browser.url("tauri://localhost/chat/");
    await waitForChat();
    await ask("We still need to book the yearly oven service. Reply with one short sentence.");
    await waitForReplyDone();
    const turn = (((await browser.execute(() => (window as any).__yoaiTurnsFull?.() ?? [])) as { agentKey: string; conversationHash: string }[]).slice(-1)[0])!;
    let started = 0;
    await browser.waitUntil(async () => {
      const list = await invoke<{ hash: string; started_at: number }[]>("get_conversations_cached", { agentKey: turn.agentKey });
      started = list.find((c) => c.hash === turn.conversationHash)?.started_at ?? 0;
      return started > 0;
    }, { timeout: 60_000, timeoutMsg: "the conversation is not in the list" });
    const SERVICE = "Book the yearly oven service.";
    const kept = await invoke<{ item: Item }>("memory_keep", {
      input: { text: SERVICE, itemType: "open", ptr: { c: started, s: 0, t: started, a: 0, b: 0 }, asOf: Date.now(), kind: "exchange", why: "pressed" },
    });
    expect((await item(kept.item.id)).item.state).toBe("open");
    await ask("That's done.");
    await waitForReplyDone();
    const line = '[data-testid="kept-closed"]';
    await (await $(line)).waitForExist({ timeout: 120_000 });
    expect(await textOf(line)).toBe(`Marked done: ${SERVICE}`);
    expect((await item(kept.item.id)).item.state).toBe("done");
    await shot("memory-open-said-done");
    await press('[data-testid="kept-line"] [data-testid="kept-undo"]');
    await browser.waitUntil(async () => (await item(kept.item.id)).item.state === "open", { timeout: 30_000, timeoutMsg: "Undo did not open it again" });
    // Closed for good this time, so the last step's board holds what it expects.
    await invoke("memory_item_change", { id: kept.item.id, edit: { do: "close", done: true } });
  });

  it("Ask to do it now opens the chat with the request ready to send", async function () {
    this.timeout(300_000);
    await openBoard();
    await press(`[data-testid="open-row"][data-id="${other}"] [data-testid="open-ask-now"]`);
    await browser.waitUntil(async () => (await browser.execute(() => location.pathname)) === "/chat/", { timeout: 30_000, timeoutMsg: "the chat did not open" });
    const box = await $('[data-testid="chat-input"]');
    await box.waitForDisplayed({ timeout: 60_000 });
    await browser.waitUntil(async () => ((await browser.execute((e) => (e as HTMLElement).innerText || (e as HTMLTextAreaElement).value || "", box)) as string).includes(`Please do this now: ${OTHER}`), { timeout: 30_000, timeoutMsg: "the request was not ready in the box" });
    await shot("memory-open-ask-now");
    // Nothing was sent: the person sends it.
    const sent = (await browser.execute(() => Array.from(document.querySelectorAll('[data-testid="chat-message"][data-role="user"]')).map((m) => (m as HTMLElement).innerText).join("\n"))) as string;
    expect(sent).not.toContain("Please do this now");
  });
});
