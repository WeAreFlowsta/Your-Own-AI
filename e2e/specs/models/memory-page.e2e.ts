// The Memory page, on the built app: what the person writes about
// themselves is kept as an item of theirs and reaches the next answer; a
// conversation kept out of memory is listed under "Not used" and "Use
// again" puts it back.
import { waitForChat, ask, waitForReplyDone, invoke, shot } from "./_helpers";

const BASE = "http://127.0.0.1:11435/internal";
type Item = { id: string; text: string; savedBy: string; item: { state: string; confirmed?: unknown; subject?: string | null; source?: { kind: string } | null } };
const items = () => invoke<Item[]>("memory_items");

let ai = { id: "", agent: "" };

async function add(kind: string, text: string) {
  await (await $('[data-testid="memory-add"]')).click();
  await (await $('[data-testid="memory-add-kind"]')).click();
  await (await $(`[data-testid="memory-add-option"][data-kind="${kind}"]`)).click();
  const field = kind === "note" ? "memory-add-note" : "memory-add-value";
  await (await $(`[data-testid="${field}"]`)).waitForExist({ timeout: 20_000 });
  await browser.execute((id: string, t: string) => {
    const el = document.querySelector(`[data-testid="${id}"]`) as HTMLInputElement;
    el.focus();
    el.value = t;
    el.dispatchEvent(new Event("input", { bubbles: true }));
  }, field, text);
  await browser.execute((e) => (e as HTMLElement).click(), await $('[data-testid="memory-add-save"]'));
}

describe("the Memory page", () => {
  it("what the person writes about themselves is kept as theirs and is given to an AI", async function () {
    this.timeout(600_000);
    await waitForChat();
    // The first reply of a new profile gives the AI its place in the records.
    await ask("Name one primary colour and nothing else.");
    await waitForReplyDone();
    const found = await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const p = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(p);
      const ais = ((await store.get("custom-ais")) || []) as { id?: string; agentPubKey?: string }[];
      const asked = (((window as any).__yoaiTurnsFull?.() ?? []) as { agentKey: string }[]).slice(-1)[0]?.agentKey ?? "";
      const a = ais.find((x) => x.agentPubKey === asked) ?? ais[0];
      return a ? { id: a.id ?? "", agent: a.agentPubKey ?? "" } : null;
    });
    expect(found?.agent).toBeTruthy();
    ai = found!;
    await browser.waitUntil(async () => !!(await invoke("memory_update_report").catch(() => null)), { timeout: 120_000, timeoutMsg: "the update's pass never finished" });

    await browser.url("tauri://localhost/your-memory/");
    await (await $('[data-testid="memory-add"]')).waitForDisplayed({ timeout: 60_000 });
    const before = (await items()).length;
    await add("likes", "rye sourdough with caraway");
    await browser.waitUntil(async () => (await items()).length === before + 1, { timeout: 60_000, timeoutMsg: "what was written was not kept as an item" });
    const kept = (await items()).find((i) => i.text === "Likes: rye sourdough with caraway")!;
    console.log(`e2e: kept from the page: ${JSON.stringify(kept)}`);
    expect(kept.item.state).toBe("current");
    expect(kept.savedBy).toBe("person");
    expect(kept.item.confirmed ?? null).not.toBe(null);
    expect(kept.item.source?.kind).toBe("note");
    // Recently lists it, with Undo.
    await browser.execute((e) => (e as HTMLElement).click(), await $('[data-testid="memory-tab"][data-tab="recently"]'));
    await browser.waitUntil(async () => ((await browser.execute(() => Array.from(document.querySelectorAll('[data-testid="recently-row"]')).map((r) => (r as HTMLElement).innerText).join("\n"))) as string).includes("rye sourdough with caraway"), { timeout: 30_000, timeoutMsg: "Recently did not list what was kept" });
    await shot("memory-page-kept-words");
    await browser.execute((e) => (e as HTMLElement).click(), await $('[data-testid="memory-tab"][data-tab="map"]'));
    await (await $('[data-testid="memory-add"]')).waitForDisplayed({ timeout: 60_000 });

    // A note, in free words.
    await add("note", "My oven is a Rofco B40, bought in 2024.");
    await browser.waitUntil(async () => (await items()).some((i) => i.text === "My oven is a Rofco B40, bought in 2024."), { timeout: 60_000, timeoutMsg: "the note was not kept as an item" });
    // The same words again are not kept twice.
    const again = await invoke<{ already: boolean }>("memory_keep_words", { text: "my oven is a Rofco B40, bought in 2024" });
    expect(again.already).toBe(true);

    // The next answer of any AI is given both, as written.
    const res = await fetch(`${BASE}/memory-assemble`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ ai: ai.id, question: "What bread should I bake this weekend?", surface: "chat" }) });
    const { memory } = await res.json();
    console.log(`e2e: the builder's part:\n${memory.systemPart}`);
    expect(memory.systemPart).toContain("About them, kept by them");
    expect(memory.systemPart).toContain("- Likes: rye sourdough with caraway");
    expect(memory.systemPart).toContain("- My oven is a Rofco B40, bought in 2024.");
  });

  it("a conversation kept out of memory is listed under Not used, and Use again puts it back", async function () {
    this.timeout(300_000);
    const hash = (((await browser.execute(() => (window as any).__yoaiTurnsFull?.() ?? [])) as { conversationHash: string }[]).slice(-1)[0]?.conversationHash) ?? "";
    let started = 0;
    await browser.waitUntil(async () => {
      const list = await invoke<{ hash: string; started_at: number }[]>("get_conversations_cached", { agentKey: ai.agent });
      started = list.find((c) => c.hash === hash)?.started_at ?? list[0]?.started_at ?? 0;
      return started > 0;
    }, { timeout: 60_000, timeoutMsg: "the conversation is not in the list" });
    const set = await invoke<{ private: boolean }>("memory_conversation_use_set", { change: { startedAt: started, private: true } });
    expect(set.private).toBe(true);
    await browser.url("tauri://localhost/chat/");
    await browser.url("tauri://localhost/your-memory/#recently");
    const row = await $('[data-testid="not-used-row"]');
    await row.waitForDisplayed({ timeout: 60_000 });
    const words = (await browser.execute((e) => (e as HTMLElement).innerText, row)) as string;
    console.log(`e2e: not used: ${JSON.stringify(words)}`);
    expect(words).toContain("Kept out of memory. It is never searched and nothing is kept from it.");
    await shot("memory-page-not-used");
    await browser.execute((e) => (e as HTMLElement).click(), await $('[data-testid="not-used-again"]'));
    await (await $('[data-testid="not-used-list"]')).waitForExist({ reverse: true, timeout: 60_000 });
    expect((await invoke<unknown[]>("memory_conversations_not_used")).length).toBe(0);
    const back = await invoke<{ private: boolean; excluded?: number[] }>("memory_conversation_use", { startedAt: started });
    expect(back.private).toBe(false);
  });
});
