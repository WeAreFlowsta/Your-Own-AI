// Deleting, on the built app: "Delete all memory" says what goes and asks
// for the word DELETE, takes every kept item and subject, and leaves the
// conversations and the rules; deleting a conversation says how many
// memory items go with it, and they go.
import { waitForChat, ask, waitForReplyDone, invoke, shot } from "./_helpers";

type Item = { id: string; text: string; item: { state: string; subject?: string | null; source?: { ptr?: { c: number } | null } | null } };
const live = async () => (await invoke<Item[]>("memory_items")).filter((i) => i.item.state === "current");
const textOf = async (sel: string) => (await browser.execute((s) => (document.querySelector(s) as HTMLElement | null)?.innerText ?? "", sel)) as string;
const press = async (sel: string) => {
  const el = await $(sel);
  await el.waitForExist({ timeout: 30_000 });
  await browser.execute((e) => (e as HTMLElement).click(), el);
};
const RULE = `Always answer in plain words (delete ${Date.now().toString(36)})`;

async function keepTheLastReply() {
  await press('(//*[@data-testid="chat-message"][@data-role="assistant"])[last()]//*[@data-testid="remember-reply"]');
}

describe("deleting memory", () => {
  let agent = "";

  it("Delete all memory takes every kept item and subject, and leaves conversations and rules", async function () {
    this.timeout(600_000);
    await waitForChat();
    await ask("Our flour supplier is Mill and Sons and they deliver on Tuesdays. Reply with one short sentence.");
    await waitForReplyDone();
    await browser.waitUntil(
      async () => !!(await invoke("memory_update_report").catch(() => null)) || (await invoke("memory_update_run", { learningWasPaused: false }).then(() => true, () => false)),
      { timeout: 180_000, interval: 5_000, timeoutMsg: "the update's pass never finished" },
    );
    await keepTheLastReply();
    await browser.waitUntil(async () => (await live()).length === 1, { timeout: 60_000, timeoutMsg: "the reply was not kept" });
    await invoke("memory_keep_words", { text: "I bake on Saturdays." });
    await invoke("save_memory_row", {
      agentKey: null,
      row: { id: `delete-rule-${Date.now()}`, scope: "person", kind: "rule", text: RULE, structured: null, as_of: Date.now(), saved_by: "person", provenance: "authored", pinned: true, topics: [], replaces: [], deleted: false, updated_at: Date.now(), action_hash: null },
    });
    agent = (await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const store = await t.store.load(await t.core.invoke("profile_store_path", { name: "ai-data.json" }));
      const ais = ((await store.get("custom-ais")) || []) as { agentPubKey?: string }[];
      const asked = (((window as any).__yoaiTurnsFull?.() ?? []) as { agentKey: string }[]).slice(-1)[0]?.agentKey ?? "";
      return asked || ais[0]?.agentPubKey || "";
    })) as string;
    const conversationsBefore = (await invoke<unknown[]>("get_conversations_cached", { agentKey: agent })).length;
    expect(conversationsBefore).toBeGreaterThanOrEqual(1);

    await browser.url("tauri://localhost/your-memory/");
    await press('[data-testid="delete-all-open"]');
    await (await $('[data-testid="delete-all-dialog"]')).waitForDisplayed({ timeout: 30_000 });
    await browser.waitUntil(async () => (await textOf('[data-testid="delete-all-counts"]')).includes("All 2 kept items, all 2 subjects"), { timeout: 30_000, timeoutMsg: "the dialog did not count what would go" });
    const words = await textOf('[data-testid="delete-all-dialog"]');
    console.log(`e2e: the dialog: ${JSON.stringify(words.replace(/\s+/g, " ").slice(0, 700))}`);
    expect(words).toContain("Your conversations and your rules stay");
    expect(words).toContain("In Vault backups made before today: still there.");
    await shot("memory-delete-all-dialog");
    // Nothing happens until the word is typed.
    expect(await (await $('[data-testid="delete-all-go"]')).getAttribute("disabled")).not.toBe(null);
    await browser.execute(() => {
      const el = document.querySelector('[data-testid="delete-all-word"]') as HTMLInputElement;
      el.focus();
      el.value = "DELETE";
      el.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await press('[data-testid="delete-all-go"]');
    await (await $('[data-testid="delete-all-dialog"]')).waitForExist({ reverse: true, timeout: 120_000 });

    // Every kept item and subject is gone...
    expect((await invoke<Item[]>("memory_items")).length).toBe(0);
    const map = await invoke<{ subjects: { kind: string; items: number }[] }>("memory_map", { view: null });
    expect(map.subjects.map((s) => [s.kind, s.items])).toEqual([["about_you", 0]]);
    // ...the Map says so, with the day...
    await browser.waitUntil(async () => (await textOf('[data-testid="map-grow-line"]')).startsWith("Memory is empty. You deleted all memory on "), { timeout: 30_000, timeoutMsg: "the Map did not say memory was deleted" });
    console.log(`e2e: the Map after: ${JSON.stringify(await textOf('[data-testid="map-grow-line"]'))}`);
    await shot("memory-delete-all-after");
    // ...and the conversations and the rules are as they were.
    expect((await invoke<unknown[]>("get_conversations_cached", { agentKey: agent })).length).toBe(conversationsBefore);
    const rules = await invoke<{ kind: string; text: string; deleted: boolean }[]>("list_memory_rows", { agentKey: null, scope: "person" });
    expect(rules.some((r) => r.kind === "rule" && r.text === RULE && !r.deleted)).toBe(true);
  });

  it("deleting a conversation says how many memory items go with it, and they go", async function () {
    this.timeout(600_000);
    // Something is kept again from the same conversation.
    await browser.url("tauri://localhost/chat/");
    await (await $('[data-testid="chat-input"]')).waitForDisplayed({ timeout: 60_000 });
    await browser.execute((e) => (e as HTMLElement).click(), await $('button[title^="Conversations"]'));
    await press('[data-testid="conversation-item"] [data-testid="conversation-open"]');
    await (await $('(//*[@data-testid="chat-message"][@data-role="assistant"])[last()]')).waitForExist({ timeout: 60_000 });
    await keepTheLastReply();
    await browser.waitUntil(async () => (await live()).length === 1, { timeout: 60_000, timeoutMsg: "the reply was not kept again" });

    // Its record, and the delete there.
    await browser.execute((e) => (e as HTMLElement).click(), await $('button[title^="Conversations"]'));
    await press('[data-testid="conversation-item"] [data-testid="conversation-record"]');
    await browser.waitUntil(async () => (await browser.execute(() => location.pathname)) === "/memory/", { timeout: 30_000, timeoutMsg: "the record did not open" });
    await press('[data-testid="record-conversation"] [data-testid="record-delete"]');
    const dialog = await $('[data-testid="conversation-delete-confirm"]');
    await dialog.waitForDisplayed({ timeout: 30_000 });
    await browser.waitUntil(async () => (await textOf('[data-testid="conversation-delete-confirm"]')).includes("1 memory item made from it will be deleted too."), { timeout: 30_000, timeoutMsg: "the dialog did not say what goes with the conversation" });
    console.log(`e2e: the dialog: ${JSON.stringify((await textOf('[data-testid="conversation-delete-confirm"]')).replace(/\s+/g, " ").slice(0, 500))}`);
    await shot("memory-delete-conversation-dialog");
    await press('[data-testid="conversation-delete-confirm"] [data-testid="confirm-ok"]');
    await browser.waitUntil(async () => (await invoke<Item[]>("memory_items")).length === 0, { timeout: 120_000, timeoutMsg: "the item made from the conversation was not deleted with it" });
    expect((await invoke<unknown[]>("get_conversations_cached", { agentKey: agent })).length).toBe(0);
  });
});
