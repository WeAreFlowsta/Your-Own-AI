// A new computer replaces one that is gone (e2e/replace-live.sh):
//   a  A chats and backs up, then is never started again.
//   b  B joins with the recovery phrase on its own Vault. Its restore is
//      refused while A is listed, naming A; B stops waiting for A (what the
//      restore's "A is gone: restore here" does), and the restore brings
//      A's conversations into B's own records from the copy B's Vault holds,
//      and A's memory with them, as A held it: fifty kept items with
//      their states, the rules, the named subjects, the choices on
//      conversations; and Memory says once that it is back.
// REPLACE_PHASE picks the phase.
import { readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
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
// restore: with A gone, the backup alone must bring all of it back. A
// made-up profile: 50 kept items from four conversations (ten the
// person's own, forty written by an AI, of which five are then confirmed,
// five replaced by a later item and three forgotten), three rules for
// every AI and two for a project, two subjects named by the person, one
// conversation kept private, one given to one AI only, and two messages
// not to be used. What A holds is written to a file; B must hold the same.
const KEPT = "Replace test: the bakery's flour comes on Tuesdays.";
const RULE = "Replace test: answer in one short sentence.";
const RULES = [RULE, "Replace test: use metric units.", "Replace test: never use tables."];
const PROJECT = "replace-test-project";
const PROJECT_RULES = ["Replace test: run the tests before every release.", "Replace test: never push on Fridays."];
const PRIVATE_CONVERSATION = 1_700_000_000_000_000;
const ONE_AI_CONVERSATION = 1_700_000_050_000_000;
const NOT_USED = [1_700_000_060_000_000, 1_700_000_070_000_000];
const FROM = [1_700_000_100_000_000, 1_700_000_200_000_000, 1_700_000_300_000_000, 1_700_000_400_000_000];
const NAMES = ["Replace test: the bakery", "Replace test: the garden"];
const SNAPSHOT = process.env.REPLACE_SNAPSHOT ?? join(tmpdir(), "yoai-replace-memory.json");

type ItemSeen = { id: string; text: string; savedBy: string; item: { state: string; confirmed?: { how: string } | null; subject?: string | null; audience?: { kind: string } | null } };
type Use = { startedAt: number; private: boolean; excluded?: number[]; audience?: unknown };
type Held = {
  items: { id: string; text: string; savedBy: string; state: string; confirmed: string | null; subject: string | null; audience: string }[];
  rules: string[];
  projectRules: string[];
  subjects: { id: string; name: string; items: number }[];
  uses: Use[];
};

const items = () => invoke<ItemSeen[]>("memory_items");
const ruleTexts = async (scope: string) => (await invoke<{ kind: string; text: string }[]>("list_memory_rows", { agentKey: null, scope })).filter((r) => r.kind === "rule" && r.text.startsWith("Replace test")).map((r) => r.text).sort();

// An object written with its keys in order: the same value reads the same
// whatever order its keys arrived in.
const sorted = (v: unknown) => JSON.stringify(v, v && typeof v === "object" ? Object.keys(v as object).sort() : undefined);

// Everything memory holds, in a shape that reads the same on both computers.
async function held(): Promise<Held> {
  const all = (await items()).filter((i) => i.text.startsWith("Replace test"));
  const map = await invoke<{ subjects: { id: string; name: string; items: number }[] }>("memory_map", { view: null });
  const uses: Use[] = [];
  for (const startedAt of [PRIVATE_CONVERSATION, ONE_AI_CONVERSATION, ...NOT_USED]) {
    const u = await invoke<Use>("memory_conversation_use", { startedAt });
    uses.push({ startedAt, private: u.private, excluded: u.excluded ?? [], audience: u.audience ?? null });
  }
  return {
    items: all.map((i) => ({ id: i.id, text: i.text, savedBy: i.savedBy, state: i.item.state, confirmed: i.item.confirmed?.how ?? null, subject: i.item.subject ?? null, audience: sorted(i.item.audience ?? null) })).sort((a, b) => a.id.localeCompare(b.id)),
    rules: await ruleTexts("person"),
    projectRules: await ruleTexts(`project:${PROJECT}`),
    subjects: map.subjects.map((s) => ({ id: s.id, name: s.name, items: s.items })).sort((a, b) => a.id.localeCompare(b.id)),
    uses,
  };
}

const count = (h: Held) => {
  const by = (f: (i: Held["items"][number]) => boolean) => h.items.filter(f).length;
  return `${h.items.length} listed; states ${JSON.stringify(Object.fromEntries([...new Set(h.items.map((i) => i.state))].map((s) => [s, by((i) => i.state === s)])))}; confirmed ${by((i) => !!i.confirmed)}; ${h.rules.length} rules, ${h.projectRules.length} for the project; ${h.subjects.length} subjects`;
};

async function makeMemory(): Promise<void> {
  await browser.waitUntil(async () => invoke<unknown[]>("memory_items").then(() => true, () => false), { timeout: 600_000, interval: 5_000, timeoutMsg: "memory did not answer within ten minutes" });
  const keep = async (n: number, text: string, extra: Record<string, unknown> = {}) => {
    const c = FROM[n % FROM.length];
    const kept = await invoke<{ item: ItemSeen }>("memory_keep", {
      input: { text, itemType: "detail", ptr: { c, s: 1 + n, t: c, a: 0, b: 0 }, asOf: Math.round(c / 1000) + n, kind: "exchange", why: "pressed", ...extra },
    });
    return kept.item;
  };
  // Ten of the person's own; the first is the one the note is checked on.
  const own = [await keep(0, KEPT)];
  for (let n = 1; n < 10; n++) own.push(await keep(n, `Replace test: own item ${n}, the shop opens at ${n} on day ${n}.`));
  await invoke("memory_item_change", { id: own[0].id, edit: { do: "confirm" } });
  // One of them is for a project's turns only: the project is then one of
  // those the backup carries, with its rules.
  await invoke("memory_item_change", { id: own[9].id, edit: { do: "audience", audience: { kind: "project", project: PROJECT } } });
  // Forty written by an AI.
  const theirs: ItemSeen[] = [];
  for (let n = 10; n < 50; n++) theirs.push(await keep(n, `Replace test: AI item ${n}, order number ${n * 7} is for bay ${n}.`, { why: "settled", by: "ai:replace-test" }));
  for (const i of theirs.slice(0, 5)) await invoke("memory_item_change", { id: i.id, edit: { do: "confirm" } });
  for (let k = 0; k < 5; k++) await invoke("memory_replaces", { newer: theirs[10 + k].id, older: theirs[20 + k].id });
  for (const i of theirs.slice(30, 33)) await invoke("memory_item_change", { id: i.id, edit: { do: "forget" } });
  const now = Date.now();
  const rule = (id: string, scope: string, text: string) =>
    invoke("save_memory_row", { agentKey: null, row: { id, scope, kind: "rule", text, structured: null, as_of: now, saved_by: "person", provenance: "authored", pinned: false, topics: [], replaces: [], deleted: false, updated_at: now } });
  for (const [k, text] of RULES.entries()) await rule(k === 0 ? "replace-test-rule" : `replace-test-rule-${k}`, "person", text);
  for (const [k, text] of PROJECT_RULES.entries()) await rule(`replace-test-project-rule-${k}`, `project:${PROJECT}`, text);
  // Two of the four subjects are named by the person.
  const map = await invoke<{ subjects: { id: string; kind: string }[] }>("memory_map", { view: null });
  const fromConversations = map.subjects.filter((s) => s.kind === "conversation").sort((a, b) => a.id.localeCompare(b.id));
  expect(fromConversations.length).toBe(FROM.length);
  for (const [k, name] of NAMES.entries()) await invoke("memory_subject_rename", { id: fromConversations[k].id, name });
  // Choices on conversations.
  const ai = (await aiAgents())[0];
  await invoke("memory_conversation_use_set", { change: { startedAt: PRIVATE_CONVERSATION, private: true } });
  await invoke("memory_conversation_use_set", { change: { startedAt: ONE_AI_CONVERSATION, audience: { kind: "ai", ai } } });
  for (const startedAt of NOT_USED) await invoke("memory_conversation_use_set", { change: { startedAt, exclude: 2 } });
  const made = await held();
  console.log(`e2e: A holds: ${count(made)}`);
  expect(made.items.filter((i) => i.confirmed).length).toBeGreaterThanOrEqual(6);
  expect(made.rules).toEqual([...RULES].sort());
  expect(made.projectRules).toEqual([...PROJECT_RULES].sort());
  expect(made.subjects.filter((s) => NAMES.includes(s.name)).length).toBe(2);
  writeFileSync(SNAPSHOT, JSON.stringify(made, null, 1));
}

async function memoryIsBack(): Promise<void> {
  await browser.waitUntil(async () => ((await items().catch(() => [])) as ItemSeen[]).some((i) => i.text === KEPT), { timeout: 120_000, interval: 5_000, timeoutMsg: "the kept item did not come back with the restore" });
  const item = (await items()).find((i) => i.text === KEPT)!;
  console.log(`e2e: the kept item is back: ${JSON.stringify(item.item).slice(0, 200)}`);
  expect(item.item.state).toBe("current");
  expect(item.item.confirmed?.how).toBe("pressed");
  const use = await invoke<{ private: boolean }>("memory_conversation_use", { startedAt: PRIVATE_CONVERSATION });
  expect(use.private).toBe(true);
  // Everything A held, as A held it: each item with its state, who wrote
  // it, whether it is confirmed, its subject and who may use it; the
  // rules; the subjects with the names the person gave; the choices on
  // conversations.
  const was = JSON.parse(readFileSync(SNAPSHOT, "utf8")) as Held;
  let now = await held();
  await browser
    .waitUntil(async () => { now = await held(); return now.items.length === was.items.length && now.projectRules.length === was.projectRules.length; }, { timeout: 120_000, interval: 5_000 })
    .catch(() => undefined);
  console.log(`e2e: A held: ${count(was)}`);
  console.log(`e2e: B holds: ${count(now)}`);
  const differ = was.items.filter((a) => JSON.stringify(a) !== JSON.stringify(now.items.find((b) => b.id === a.id)));
  for (const a of differ.slice(0, 6)) console.log(`e2e: differs: was ${JSON.stringify(a)} now ${JSON.stringify(now.items.find((b) => b.id === a.id) ?? null)}`);
  expect(now.items.length).toBe(was.items.length);
  expect(differ.length).toBe(0);
  expect(now.rules).toEqual(was.rules);
  expect(now.projectRules).toEqual(was.projectRules);
  expect(now.subjects).toEqual(was.subjects);
  expect(now.uses).toEqual(was.uses);
  console.log("e2e: the items, the rules, the subjects and the choices on conversations are back as they were");
  // Memory says so once, and not again after "Got it".
  await browser.url("tauri://localhost/your-memory/");
  const note = await $('[data-testid="memory-restore-note"]');
  await note.waitForDisplayed({ timeout: 60_000, timeoutMsg: "Memory did not say that it is back" });
  const said = (await note.getText()).replace(/\s+/g, " ");
  console.log(`e2e: Memory says: ${said.slice(0, 300)}`);
  expect(said).toContain("Your memory is back.");
  expect(said).toMatch(/: \d+ kept items? and \d+ rules?\./);
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
