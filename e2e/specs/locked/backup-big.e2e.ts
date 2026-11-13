// A Vault backup of a long-used AI when nothing was written to it: the pass
// does not ask its cell for the list of its conversations again. A person's
// profile, 2026-11-11: one AI's 1,046 conversations took 14 minutes to list,
// with none of them read. Here an AI is given YOAI_E2E_BIG conversations
// (1,000), backed up, and then: nothing written; a write to another AI; a
// write to it. backup-big-live.sh reads the app's log for which passes
// listed it.
import { waitForChat, invoke, invokeLong, seedConversations } from "../models/_helpers";

const BIG = Number(process.env.YOAI_E2E_BIG ?? 1000);
type Ai = Record<string, any>;
type Backup = { conversations?: number; manifest?: string; skipped?: string };

async function ais(): Promise<Ai[]> {
  const path = await invoke<string>("profile_store_path", { name: "ai-data.json" });
  return browser.execute(async (p: string) => {
    const t = (window as any).__TAURI__.core;
    const rid = await t.invoke("plugin:store|load", { path: p, autoSave: false });
    const got = await t.invoke("plugin:store|get", { rid, key: "custom-ais" });
    const list = Array.isArray(got) && got.length === 2 && typeof got[1] === "boolean" ? got[0] : got;
    return Array.isArray(list) ? list : [];
  }, path) as unknown as Promise<Ai[]>;
}
/** "Back up now", waiting out a pass the app started by itself; timed. */
async function backupNow(what: string): Promise<Backup> {
  const until = Date.now() + 3_600_000;
  for (;;) {
    const began = Date.now();
    try {
      const r = await invokeLong<Backup>("vault_backup_now", {}, 3_600_000);
      console.log(`e2e: big backup ${what}: ${Math.round((Date.now() - began) / 1000)} s ${JSON.stringify(r).slice(0, 200)}`);
      return r;
    } catch (e) {
      if (!String(e).includes("backup_in_progress") || Date.now() > until) throw e;
      await browser.pause(2_000);
    }
  }
}
const start = (agentKey: string, aiName: string, title: string) => invoke<string>("start_conversation", { agentKey, aiName, model: "e2e", title, source: null });

describe("a Vault backup of a long-used AI", () => {
  it("does not list it again until something is written to it", async function () {
    this.timeout(4 * 3_600_000);
    await waitForChat();
    // A new profile gives its starters their agents in its first minute.
    let list: Ai[] = [];
    await browser.waitUntil(async () => { list = (await ais()).filter((a) => a.agentPubKey && !a.foldedInto); return list.length >= 2; },
      { timeout: 300_000, interval: 3_000, timeoutMsg: "fewer than two AIs were given an agent" });
    const [big, other] = list;
    console.log(`e2e: big AI ${big.name} ${String(big.agentPubKey).slice(0, 12)}; other AI ${other.name} ${String(other.agentPubKey).slice(0, 12)}`);

    const seeded = Date.now();
    const made = await seedConversations(big.agentPubKey, big.name, BIG);
    console.log(`e2e: big seeded ${made} conversation(s) in ${Math.round((Date.now() - seeded) / 1000)} s`);
    expect(made).toBe(BIG);
    // The list the app keeps of them (the page would read it; here, asked for).
    const listed = await invokeLong<unknown[]>("get_conversations", { agentKey: big.agentPubKey, maxAgeSecs: null, caller: "e2e big" }, 1_800_000);
    expect(listed.length).toBeGreaterThanOrEqual(BIG);

    // 1. The first pass reads and uploads every one of them.
    const first = await backupNow("first (everything is new)");
    expect(first.manifest).toBe("written");
    expect(first.conversations ?? 0).toBeGreaterThanOrEqual(BIG);

    // 2. Nothing written: nothing is listed again, and nothing is lost.
    const second = await backupNow("second (nothing written)");
    expect(second.conversations).toBe(first.conversations);

    // 3. A write to another AI: the big one still stands as backed up.
    await start(other.agentPubKey, other.name, "written to the other AI");
    const third = await backupNow("third (the other AI was written to)");
    expect(third.conversations).toBe((first.conversations ?? 0) + 1);

    // 4. A write to the big AI: it is listed again, and the new one is in.
    await start(big.agentPubKey, big.name, "written to the big AI");
    const fourth = await backupNow("fourth (the big AI was written to)");
    expect(fourth.conversations).toBe((first.conversations ?? 0) + 2);

    // 5. And once more with nothing written.
    const fifth = await backupNow("fifth (nothing written)");
    expect(fifth.conversations).toBe(fourth.conversations);
  });
});
