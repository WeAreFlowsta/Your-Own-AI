// A new computer replaces one that is gone (e2e/replace-live.sh):
//   a  A chats and backs up, then is never started again.
//   b  B joins with the recovery phrase on its own Vault. Its restore is
//      refused while A is listed, naming A; B stops waiting for A (what the
//      restore's "A is gone: restore here" does), and the restore brings
//      A's conversations into B's own records from the copy B's Vault holds.
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

describe(`a new computer replaces one that is gone, phase ${PHASE}`, () => {
  it(`phase ${PHASE}`, async function () {
    this.timeout(1_200_000);
    await waitForChat();
    if (PHASE === "a") {
      await (await $('[data-testid="new-chat"]')).click();
      await ask(MESSAGE);
      await waitForReplyDone(300_000);
      const backup = await invokeLong<Record<string, unknown>>("vault_backup_now", {}, 300_000);
      console.log(`e2e: A's backup ${JSON.stringify(backup).slice(0, 300)}`);
      expect(backup.manifest).toBe("written");
      expect(Number(backup.conversations)).toBeGreaterThan(0);
    } else if (PHASE === "b") {
      // Refused while A is listed, and A is named.
      const refused = await invokeLong<unknown>("vault_restore_conversations", {}, 300_000).then(() => "", (e) => String(e));
      console.log(`e2e: first restore: ${refused.slice(0, 240)}`);
      expect(refused).toContain("siblings_hold_data:");
      const rest = refused.slice(refused.indexOf("siblings_hold_data:") + "siblings_hold_data:".length);
      let list: { id: string; name: string | null }[] = [];
      for (let end = rest.indexOf("]"); end >= 0; end = rest.indexOf("]", end + 1)) {
        try { list = JSON.parse(rest.slice(0, end + 1)); break; } catch { /* not complete yet */ }
      }
      console.log(`e2e: named: ${JSON.stringify(list)}`);
      expect(list.length).toBe(1);
      // "A is gone: restore here": stop waiting for it, then the restore again.
      await invoke("line_stop_waiting", { device: list[0].id });
      const stats = await invokeLong<Record<string, number>>("vault_restore_conversations", {}, 600_000);
      console.log(`e2e: restore after stopping the wait ${JSON.stringify(stats).slice(0, 300)}`);
      expect(stats.conversations_restored).toBeGreaterThan(0);
      expect(stats.missing_objects ?? 0).toBe(0);
      await browser.waitUntil(() => inOwnRecords("Replace test"), { timeout: 300_000, interval: 10_000, timeoutMsg: "A's conversation never reached B's own records" });
      console.log("e2e: A's conversation is in B's own records");
      await shot("replace-b-restored");
    } else {
      throw new Error(`no phase ${PHASE}`);
    }
  });
});
