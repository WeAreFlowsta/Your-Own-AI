// Deleting, on the built app: "Delete all memory" says what goes and asks
// for the word DELETE, takes every kept item and subject, and leaves the
// conversations and the rules; deleting a conversation says how many
// memory items go with it, and they go.
import { waitForChat, ask, waitForReplyDone, invoke, shot } from "./_helpers";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

// What the app's log said: the removal of earlier copies happens in the
// background and says how many went.
const LOG_DIR = path.join(process.env.YOAI_E2E_HOME ?? path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../profile"), ".local/share/com.solar.yourowai/logs");
function removedSoFar(): number {
  let n = 0;
  for (const f of fs.existsSync(LOG_DIR) ? fs.readdirSync(LOG_DIR) : []) {
    if (!f.endsWith(".log")) continue;
    for (const m of fs.readFileSync(path.join(LOG_DIR, f), "utf8").matchAll(/\[memory\] (\d+) earlier copies of deleted rows removed from the record/g)) n += Number(m[1]);
  }
  return n;
}

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

    const removedBefore = removedSoFar();
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
    // The deleted items' earlier copies leave the stored record too: the two
    // items, and the subject made for the conversation.
    await browser.waitUntil(async () => removedSoFar() >= removedBefore + 3, { timeout: 180_000, interval: 3_000, timeoutMsg: `the earlier copies were not removed from the record (${removedSoFar() - removedBefore} so far)` });
    console.log(`e2e: earlier copies removed from the record: ${removedSoFar() - removedBefore}`);
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
    const removedBefore = removedSoFar();

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
    await browser.waitUntil(async () => removedSoFar() > removedBefore, { timeout: 180_000, interval: 3_000, timeoutMsg: "the item's earlier copy was not removed from the record" });
  });
  it("deleting an AI says what happens to what it kept: its own items go, and an item kept from its conversation says where its source went", async function () {
    this.timeout(900_000);
    await browser.url("tauri://localhost/chat/");
    await waitForChat();
    await ask("We order yeast from Lesaffre every second Monday. Reply with one short sentence.");
    await waitForReplyDone();
    await keepTheLastReply();
    await browser.waitUntil(async () => (await live()).length === 1, { timeout: 60_000, timeoutMsg: "the reply was not kept" });
    const fromConversation = (await live())[0];
    const who = (await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const store = await t.store.load(await t.core.invoke("profile_store_path", { name: "ai-data.json" }));
      const ais = ((await store.get("custom-ais")) || []) as { id: string; name: string; agentPubKey?: string; foundingKey?: string }[];
      const asked = (((window as any).__yoaiTurnsFull?.() ?? []) as { agentKey: string }[]).slice(-1)[0]?.agentKey ?? "";
      const a = ais.find((x) => x.agentPubKey === asked) ?? ais[0];
      return { id: a.id, name: a.name, founding: a.foundingKey || a.agentPubKey || "", count: ais.length };
    })) as { id: string; name: string; founding: string; count: number };
    expect(who.founding).toBeTruthy();
    // One more item, for this AI alone.
    const ONLY = "Only one AI needs this note about the proofing room.";
    await invoke("memory_keep_words", { text: ONLY });
    const only = (await live()).find((i) => i.text === ONLY)!;
    await invoke("memory_item_change", { id: only.id, edit: { do: "audience", audience: { kind: "ai", ai: who.founding } } });
    expect(await invoke("memory_ai_delete_preview", { founding: who.founding })).toEqual({ stay: 0, go: 1 });

    await browser.url("tauri://localhost/your-ais/");
    await press(`[data-ai-id="${who.id}"] [title="Delete AI"]`);
    await browser.waitUntil(async () => (await textOf('[data-testid="delete-ai-words"]')).includes(`${who.name} kept 1 memory item, for ${who.name} only. It will be deleted.`), { timeout: 30_000, timeoutMsg: "the dialog did not say what happens to what the AI kept" });
    console.log(`e2e: the dialog: ${JSON.stringify((await textOf('[data-testid="delete-ai-words"]')).replace(/\s+/g, " "))}`);
    await shot("memory-delete-ai-dialog");
    await press('[data-testid="delete-ai-purge"]');
    await browser.waitUntil(
      async () =>
        (await browser.execute(async (id) => {
          const t = (window as any).__TAURI__;
          const store = await t.store.load(await t.core.invoke("profile_store_path", { name: "ai-data.json" }));
          return !(((await store.get("custom-ais")) || []) as { id: string }[]).some((a) => a.id === id);
        }, who.id)) as boolean,
      { timeout: 180_000, interval: 2_000, timeoutMsg: "the AI was not deleted" },
    );
    // Its own item is gone; the one kept from its conversation stays and says where its source went.
    await browser.waitUntil(async () => !(await live()).some((i) => i.id === only.id), { timeout: 60_000, timeoutMsg: "the item for the AI alone was not deleted with it" });
    let stays: (Item & { item: { source_gone?: { with?: string } } }) | undefined;
    await browser.waitUntil(async () => {
      stays = (await live()).find((i) => i.id === fromConversation.id) as typeof stays;
      return stays?.item.source_gone?.with === who.name;
    }, { timeout: 60_000, timeoutMsg: "the item kept from the AI's conversation does not say its source went with the AI" });
    await browser.url("tauri://localhost/your-memory/");
    await browser.waitUntil(async () => browser.execute(() => typeof (window as any).__yoaiMap?.select === "function"), { timeout: 60_000, timeoutMsg: "the map never came up" });
    await browser.execute((id) => (window as any).__yoaiMap.select(id), stays!.item.subject ?? "sub-about-you");
    await browser.waitUntil(async () => (await textOf(`[data-testid="map-panel-item"][data-id="${fromConversation.id}"] [data-testid="item-source-gone"]`)) === `The conversation this came from was deleted with ${who.name}.`, { timeout: 30_000, timeoutMsg: "the item does not say on the page where its source went" });
    await shot("memory-delete-ai-source-gone");
  });

  it("Memory and every conversation takes both, and leaves the AIs and the rules", async function () {
    this.timeout(900_000);
    await browser.url("tauri://localhost/chat/");
    await waitForChat();
    await ask("The oven is serviced each March. Reply with one short sentence.");
    await waitForReplyDone();
    await keepTheLastReply();
    await browser.waitUntil(async () => (await live()).length >= 2, { timeout: 60_000, timeoutMsg: "the reply was not kept" });
    const state = async () =>
      (await browser.execute(async () => {
        const t = (window as any).__TAURI__;
        const store = await t.store.load(await t.core.invoke("profile_store_path", { name: "ai-data.json" }));
        const ais = ((await store.get("custom-ais")) || []) as { agentPubKey?: string }[];
        let conversations = 0;
        for (const a of ais) if (a.agentPubKey) conversations += ((await t.core.invoke("get_conversations_cached", { agentKey: a.agentPubKey })) as unknown[]).length;
        return { ais: ais.length, conversations };
      })) as { ais: number; conversations: number };
    const before = await state();
    expect(before.conversations).toBeGreaterThanOrEqual(1);

    await browser.url("tauri://localhost/your-memory/");
    await press('[data-testid="delete-all-open"]');
    await (await $('[data-testid="delete-all-dialog"]')).waitForDisplayed({ timeout: 30_000 });
    await press('[data-testid="delete-all-with-conversations"]');
    await browser.execute(() => {
      const el = document.querySelector('[data-testid="delete-all-word"]') as HTMLInputElement;
      el.focus();
      el.value = "DELETE";
      el.dispatchEvent(new Event("input", { bubbles: true }));
    });
    await press('[data-testid="delete-all-go"]');
    await (await $('[data-testid="delete-all-dialog"]')).waitForExist({ reverse: true, timeout: 300_000 });
    expect((await invoke<Item[]>("memory_items")).length).toBe(0);
    const after = await state();
    console.log(`e2e: before ${JSON.stringify(before)}, after ${JSON.stringify(after)}`);
    expect(after).toEqual({ ais: before.ais, conversations: 0 });
    const rules = await invoke<{ kind: string; text: string; deleted: boolean }[]>("list_memory_rows", { agentKey: null, scope: "person" });
    expect(rules.some((r) => r.kind === "rule" && r.text === RULE && !r.deleted)).toBe(true);
  });
});
