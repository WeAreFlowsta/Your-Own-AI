// A new computer replaces one that is gone (e2e/replace-live.sh):
//   a  A chats and backs up, then is never started again.
//   b  B joins with the recovery phrase on its own Vault. Its restore is
//      refused while A is listed, naming A; B stops waiting for A (what the
//      restore's "A is gone: restore here" does), and the restore brings
//      A's conversations into B's own records from the copy B's Vault holds,
//      and A's memory with them: a kept item still confirmed, a rule, a
//      conversation still private, and Memory says once that it is back.
// REPLACE_PHASE picks the phase.
import { waitForChat, invoke, invokeLong, ask, waitForReplyDone, shot } from "../models/_helpers";

const PHASE = process.env.REPLACE_PHASE ?? "";
const MESSAGE = "Replace test: name one river and nothing else.";

async function aiAgents(): Promise<string[]> {
  const path = await invoke<string>("profile_store_path", { name: "ai-data.json" });
  return browser.execute(async (p: string) => {
    const t = (window as any).__TAURI__.core;
    const rid = await t.invoke("plugin:store|load", { path: p, autoSave: false });
    const got = await t.invoke("plugin:store|get", { rid, key: "custom-ais" });
    const list = Array.isArray(got) ? got[0] : got;
    return (Array.isArray(list) ? list : []).map((a: any) => a.agentPubKey ?? "").filter((k: string) => !!k);
  }, path) as Promise<string[]>;
}

async function inOwnRecords(text: string): Promise<boolean> {
  for (const agentKey of await aiAgents()) {
    const convs = await invoke<{ hash: string; source?: string | null }[]>("get_conversations", { agentKey, maxAgeSecs: null, caller: "e2e replace" }).catch(() => []);
    for (const c of convs.filter((x) => !x.source?.startsWith("copy:"))) {
      const entries = await invoke<unknown[]>("get_conversation_transcript", { agentKey, conversationHash: c.hash }).catch(() => []);
      if (JSON.stringify(entries).includes(text)) return true;
    }
  }
  return false;
}

// Memory made on A before its backup, and looked for on B after the
// restore: with A gone, the backup alone must bring all of it back.
const KEPT = "Replace test: the bakery's flour comes on Tuesdays.";
const RULE = "Replace test: answer in one short sentence.";
const PRIVATE_CONVERSATION = 1_700_000_000_000_000;
type ItemSeen = { id: string; text: string; item: { state: string; confirmed?: { how: string } | null } };

async function makeMemory(): Promise<void> {
  await browser.waitUntil(async () => invoke<unknown[]>("memory_items").then(() => true, () => false), { timeout: 600_000, interval: 5_000, timeoutMsg: "memory did not answer within ten minutes" });
  const kept = await invoke<{ item: ItemSeen }>("memory_keep", {
    input: { text: KEPT, itemType: "detail", ptr: { c: 1_700_000_100_000_000, s: 1, t: 1_700_000_100_000_000, a: 0, b: 0 }, asOf: 1_700_000_100_000, kind: "exchange", why: "pressed" },
  });
  await invoke("memory_item_change", { id: kept.item.id, edit: { do: "confirm" } });
  const now = Date.now();
  await invoke("save_memory_row", {
    agentKey: null,
    row: { id: "replace-test-rule", scope: "person", kind: "rule", text: RULE, structured: null, as_of: now, saved_by: "person", provenance: "authored", pinned: false, topics: [], replaces: [], deleted: false, updated_at: now },
  });
  await invoke("memory_conversation_use_set", { change: { startedAt: PRIVATE_CONVERSATION, private: true } });
}

async function memoryIsBack(): Promise<void> {
  await browser.waitUntil(async () => ((await invoke<ItemSeen[]>("memory_items").catch(() => [])) as ItemSeen[]).some((i) => i.text === KEPT), { timeout: 120_000, interval: 5_000, timeoutMsg: "the kept item did not come back with the restore" });
  const item = (await invoke<ItemSeen[]>("memory_items")).find((i) => i.text === KEPT)!;
  console.log(`e2e: the kept item is back: ${JSON.stringify(item.item).slice(0, 200)}`);
  expect(item.item.state).toBe("current");
  expect(item.item.confirmed?.how).toBe("pressed");
  const rules = await invoke<{ kind: string; text: string }[]>("list_memory_rows", { agentKey: null, scope: "person" });
  expect(rules.filter((r) => r.kind === "rule" && r.text === RULE).length).toBe(1);
  const use = await invoke<{ private: boolean }>("memory_conversation_use", { startedAt: PRIVATE_CONVERSATION });
  expect(use.private).toBe(true);
  console.log("e2e: the rule and the private mark are back");
  // Memory says so once, and not again after "Got it".
  await browser.url("tauri://localhost/your-memory/");
  const note = await $('[data-testid="memory-restore-note"]');
  await note.waitForDisplayed({ timeout: 60_000, timeoutMsg: "Memory did not say that it is back" });
  const said = (await note.getText()).replace(/\s+/g, " ");
  console.log(`e2e: Memory says: ${said.slice(0, 300)}`);
  expect(said).toContain("Your memory is back.");
  expect(said).toMatch(/: 1 kept item and \d+ rules?\./);
  await shot("replace-b-memory-back");
  await (await $('[data-testid="memory-restore-seen"]')).click();
  await browser.waitUntil(async () => !(await $('[data-testid="memory-restore-note"]').isExisting()), { timeout: 10_000, timeoutMsg: "the note stayed after Got it" });
  await browser.refresh();
  await (await $('[data-testid="memory-tabs"]')).waitForExist({ timeout: 60_000 });
  expect(await $('[data-testid="memory-restore-note"]').isExisting()).toBe(false);
}

describe(`a new computer replaces one that is gone, phase ${PHASE}`, () => {
  it(`phase ${PHASE}`, async function () {
    this.timeout(1_200_000);
    await waitForChat();
    if (PHASE === "a") {
      await (await $('[data-testid="new-chat"]')).click();
      await ask(MESSAGE);
      await waitForReplyDone(300_000);
      await makeMemory();
      const backup = await invokeLong<Record<string, unknown>>("vault_backup_now", {}, 300_000);
      console.log(`e2e: A's backup ${JSON.stringify(backup).slice(0, 300)}`);
      expect(backup.manifest).toBe("written");
      expect(Number(backup.conversations)).toBeGreaterThan(0);
    } else if (PHASE === "b") {
      // As a person does it: Settings > Backups & recovery > "Restore
      // conversations from Vault". Refused while A is listed, and A is named
      // on the one button that goes on: "<A> is gone: restore here".
      await browser.url("tauri://localhost/settings/");
      const restore = await $('[data-testid="restore-conversations"]');
      await restore.waitForExist({ timeout: 120_000, timeoutMsg: "Settings never showed Restore conversations from Vault" });
      await restore.scrollIntoView();
      await restore.click();
      await (await $('[data-testid="confirm-ok"]')).waitForDisplayed({ timeout: 10_000 });
      await (await $('[data-testid="confirm-ok"]')).click();
      const gone = await $('[data-testid="gone-restore-here"]');
      await gone.waitForDisplayed({ timeout: 300_000, timeoutMsg: "the restore was not refused with a way on (no \"is gone: restore here\" button)" });
      const label = (await gone.getText()).trim();
      console.log(`e2e: the button reads: ${label}`);
      expect(label).toMatch(/ is gone: restore here$/);
      await gone.scrollIntoView();
      await shot("replace-b-gone-offer");
      // The button stops waiting for A and runs the same restore again.
      await gone.click();
      await browser.waitUntil(async () => !(await $('[data-testid="gone-restore-here"]').isExisting()), { timeout: 30_000, timeoutMsg: "the offer stayed after it was taken" });
      await browser.waitUntil(async () => !(await (await $('[data-testid="restore-conversations"]')).getText()).includes("Restoring"), { timeout: 600_000, interval: 5_000, timeoutMsg: "the restore never finished" });
      const stopped = await invoke<{ stopped_waiting: string[] }>("line_status");
      console.log(`e2e: no longer waited for: ${JSON.stringify(stopped.stopped_waiting)}`);
      expect(stopped.stopped_waiting.length).toBe(1);
      await browser.waitUntil(() => inOwnRecords("Replace test"), { timeout: 300_000, interval: 10_000, timeoutMsg: "A's conversation never reached B's own records" });
      console.log("e2e: A's conversation is in B's own records");
      await shot("replace-b-restored");
      await memoryIsBack();
    } else {
      throw new Error(`no phase ${PHASE}`);
    }
  });
});
