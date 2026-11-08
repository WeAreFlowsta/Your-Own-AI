// One computer with a Vault on the 0.7 line alone, live (e2e/alone-live.sh):
// after the update from the previous release and the 0.6 stop, everything
// the person does still works with only the 0.7 line running.
//   ALONE_PHASE=alone    the conversations from before the move are listed;
//                        a new chat is recorded; Back up now writes them all;
//                        search finds an old and a new one; an export is
//                        imported once, and a second import adds nothing.
//   ALONE_PHASE=restore  a reinstall under the same Vault (the harness wiped
//                        the profile, its link kept): the backup brings every
//                        conversation back onto the 0.7 line.
// ALONE_MIN_CONVERSATIONS: what the earlier launches wrote.
import { writeFileSync, mkdtempSync } from "node:fs";
import { join } from "node:path";
import { tmpdir } from "node:os";
import { shot, waitForChat, invoke, invokeLong, ask, waitForReplyDone } from "../models/_helpers";

const PHASE = process.env.ALONE_PHASE ?? "";
const MIN = Number(process.env.ALONE_MIN_CONVERSATIONS ?? 0);
const STAMP = Date.now().toString(36);

type Ai = { id: string; name: string; agent: string };
async function ais(): Promise<Ai[]> {
  const path = await invoke<string>("profile_store_path", { name: "ai-data.json" });
  return browser.execute(async (p: string) => {
    const t = (window as any).__TAURI__.core;
    const rid = await t.invoke("plugin:store|load", { path: p, autoSave: false });
    const got = await t.invoke("plugin:store|get", { rid, key: "custom-ais" });
    const list = Array.isArray(got) ? got[0] : got;
    return (Array.isArray(list) ? list : []).map((a: any) => ({ id: a.id ?? "", name: a.name ?? "", agent: a.agentPubKey ?? "" })).filter((a: Ai) => !!a.agent);
  }, path) as Promise<Ai[]>;
}

async function conversations(): Promise<{ agent: string; hash: string }[]> {
  const out: { agent: string; hash: string }[] = [];
  for (const ai of await ais()) {
    for (const c of await invokeLong<{ hash: string }[]>("get_conversations", { agentKey: ai.agent, maxAgeSecs: null, caller: "e2e alone" }, 120_000)) {
      out.push({ agent: ai.agent, hash: c.hash });
    }
  }
  return out;
}

async function atLeast(n: number, what: string) {
  let got = 0;
  await browser.waitUntil(async () => { got = (await conversations()).length; return got >= n; }, {
    timeout: 180_000, interval: 10_000, timeoutMsg: `${what}: fewer than ${n} conversations listed`,
  }).catch(() => undefined);
  console.log(`e2e: ${what}: ${got} conversation(s) listed, ${n} expected`);
  expect(got).toBeGreaterThanOrEqual(n);
  return got;
}

/** Search every AI for a word; the hits' conversation hashes. */
async function search(word: string): Promise<string[]> {
  const hits: string[] = [];
  for (const ai of await ais()) {
    await invoke("transcript_search_build", { agentKey: ai.agent });
    await browser.waitUntil(async () => !(await invoke<{ building: boolean }>("transcript_search_status")).building, { timeout: 180_000, interval: 3_000, timeoutMsg: "the search index never finished" });
    const answer = await invoke<{ hits: { hash: string }[] }>("transcript_search", { aiId: ai.id, agentKey: ai.agent, query: word, limit: 20 });
    hits.push(...answer.hits.map((h) => h.hash));
  }
  return hits;
}

describe(`on the 0.7 line alone, phase ${PHASE}`, () => {
  it(PHASE === "restore" ? "a reinstall under the same Vault brings every conversation back" : "lists, records, backs up, searches and imports with only the 0.7 line running", async function () {
    this.timeout(1_800_000);
    await waitForChat();
    if (PHASE === "restore") {
      const before = (await conversations()).length;
      console.log(`e2e: before the restore: ${before} conversation(s)`);
      const out = await invokeLong<Record<string, unknown>>("vault_restore_conversations", {}, 600_000);
      console.log(`e2e: restore ${JSON.stringify(out).slice(0, 400)}`);
      await atLeast(MIN, "after the restore");
      await shot("alone-restored");
      return;
    }
    // Everything from before the move is listed.
    const start = await atLeast(MIN, "at the start");
    // A new conversation is recorded on the 0.7 line.
    await (await $('[data-testid="new-chat"]')).click();
    await ask(`Alone test ${STAMP}: name one fruit and nothing else.`);
    await waitForReplyDone(300_000);
    const turns = (await browser.execute(() => (window as any).__yoaiTurnsFull?.() ?? [])) as { conversationHash: string }[];
    const newHash = turns[turns.length - 1]?.conversationHash ?? "";
    console.log(`e2e: new conversation ${newHash}`);
    expect(newHash.length).toBeGreaterThan(10);
    await atLeast(start + 1, "after the new chat");
    // Back up now writes them all from the 0.7 line.
    const backup = await invokeLong<Record<string, unknown>>("vault_backup_now", {}, 300_000);
    console.log(`e2e: backup ${JSON.stringify(backup).slice(0, 400)}`);
    expect(backup.manifest).toBe("written");
    expect(Number(backup.records)).toBeGreaterThan(0);
    expect(Number(backup.conversations)).toBeGreaterThanOrEqual(start + 1);
    expect(backup.warning ?? null).toBe(null);
    // Search finds a conversation from before the move and the new one.
    const old = await search("Paris");
    const fresh = await search(STAMP);
    console.log(`e2e: search: "Paris" ${old.length} hit(s), the new chat's stamp ${fresh.length}`);
    expect(old.length).toBeGreaterThan(0);
    expect(fresh).toContain(newHash);
    // An export imported once; the same export again adds nothing.
    const dir = mkdtempSync(join(tmpdir(), "yoai-alone-import-"));
    const file = join(dir, "conversations.json");
    writeFileSync(file, JSON.stringify([{
      title: `Imported ${STAMP}`, create_time: 1714000000.0, id: `alone-${STAMP}`,
      mapping: {
        root: { message: null, parent: null, children: ["a"] },
        a: { message: { author: { role: "user" }, create_time: 1714000001.0, content: { parts: [`Imported question ${STAMP}`] } }, parent: "root", children: ["b"] },
        b: { message: { author: { role: "assistant" }, create_time: 1714000002.0, content: { parts: ["Imported answer"] } }, parent: "a", children: [] },
      },
    }]));
    const ai = (await ais())[0];
    const scan = await invokeLong<{ archive_id: string; conversation_count: number }>("import_conversations_scan", { path: file, mode: null, sourceHint: null }, 120_000);
    console.log(`e2e: import scan ${JSON.stringify(scan)}`);
    expect(scan.conversation_count).toBe(1);
    const first = await invokeLong<number>("import_archive_adopt", { archiveId: scan.archive_id, aiId: ai.agent, aiName: ai.name }, 300_000);
    const afterFirst = await atLeast(start + 2, "after the import");
    const again = await invokeLong<number>("import_archive_adopt", { archiveId: scan.archive_id, aiId: ai.agent, aiName: ai.name }, 300_000).catch((e) => { console.log(`e2e: second adopt said ${String(e).slice(0, 200)}`); return 0; });
    const afterAgain = (await conversations()).length;
    console.log(`e2e: import adopted ${first}, again ${again}; ${afterFirst} -> ${afterAgain} conversation(s)`);
    expect(first).toBeGreaterThan(0);
    expect(afterAgain).toBe(afterFirst);
    await shot("alone-done");
  });
});
