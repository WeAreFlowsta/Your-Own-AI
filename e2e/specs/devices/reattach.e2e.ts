// The 0.6 line stops when the person stops waiting for a computer, and
// starts again by itself when that computer comes back (e2e/reattach-live.sh):
//   stopwait  A (this build): B, still on the earlier release, holds the 0.6
//             line; the person stops waiting for it.
//   stopped   A (this build), the next start: a full pass, then the 0.6 line
//             stops (nothing needs it, nothing is left on it).
//   back      B (the earlier release), A off: B chats and backs up, so its
//             backups change.
//   return    A (this build), B online: A reads that B wrote again, waits for
//             it once more, starts the 0.6 line, and B's new conversation
//             arrives in A's own records (not a read-only Vault copy).
// REATTACH_PHASE picks the phase.
import { waitForChat, invoke, ask, waitForReplyDone, shot } from "../models/_helpers";

const PHASE = process.env.REATTACH_PHASE ?? "";
const MESSAGE_BACK = "Reattach test: name one planet and nothing else.";

type Status = { old_line_wanted: boolean; old_line_running?: boolean; why: string; holding: { id: string; name: string | null; needs_update: boolean }[]; stopped_waiting: string[] };
const status = () => invoke<Status>("line_status");

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

/** Whether a conversation in this device's OWN records carries the text. */
async function inOwnRecords(text: string): Promise<boolean> {
  for (const agentKey of await aiAgents()) {
    const convs = await invoke<{ hash: string; source?: string | null }[]>("get_conversations", { agentKey, maxAgeSecs: null, caller: "e2e reattach" }).catch(() => []);
    for (const c of convs.filter((x) => !x.source?.startsWith("copy:"))) {
      const entries = await invoke<unknown[]>("get_conversation_transcript", { agentKey, conversationHash: c.hash }).catch(() => []);
      if (JSON.stringify(entries).includes(text)) return true;
    }
  }
  return false;
}

describe(`the 0.6 line stops and starts again, phase ${PHASE}`, () => {
  it(`phase ${PHASE}`, async function () {
    this.timeout(1_800_000);
    await waitForChat();
    if (PHASE === "stopwait") {
      let st: Status | null = null;
      await browser.waitUntil(async () => { st = await status(); return st.holding.length > 0; }, {
        timeout: 300_000, interval: 10_000, timeoutMsg: "the computer on the earlier release was never listed as holding the 0.6 line",
      });
      console.log(`e2e: before: ${JSON.stringify(st)}`);
      const b = st!.holding[0];
      expect(b.needs_update).toBe(true);
      // Settings > Devices says which computer holds the older background
      // service and why, and offers the two ways out. Through its buttons.
      await browser.url("tauri://localhost/settings/");
      const panel = await $('[data-testid="settings-devices"]');
      await panel.waitForExist({ timeout: 60_000, timeoutMsg: "Settings > Devices never showed" });
      await panel.scrollIntoView();
      const holding = await $('[data-testid="device-holding"]');
      await holding.waitForDisplayed({ timeout: 60_000, timeoutMsg: "Settings > Devices never named the computer on the older version" });
      const callout = (await holding.getText()).replace(/\s+/g, " ");
      console.log(`e2e: Settings > Devices says ${JSON.stringify(callout)}`);
      expect(callout).toContain("is on an older version of Your Own AI");
      expect(callout).toContain("This computer keeps a second background service running so the two stay in sync.");
      expect(await (await $('[data-testid="device-how-to-update"]')).getText()).toBe("How to update");
      await shot("reattach-devices-holding");
      await (await $('[data-testid="device-stop-waiting"]')).click();
      await browser.waitUntil(async () => !(await $('[data-testid="device-holding"]').isExisting()), { timeout: 30_000, timeoutMsg: "the callout stayed after Stop waiting" });
      const again = await $('[data-testid="device-wait-again"]');
      await again.waitForDisplayed({ timeout: 30_000, timeoutMsg: "the row never offered to wait again" });
      expect(await again.getText()).toContain("not waiting for it: wait again");
      const after = await status();
      console.log(`e2e: after stopping the wait: ${JSON.stringify(after)}`);
      expect(after.stopped_waiting).toContain(b.id);
      expect(after.holding.length).toBe(0);
      await shot("reattach-devices-not-waiting");
    } else if (PHASE === "stopped") {
      let st: Status | null = null;
      // Not wanted once the full pass has carried everything; stopped at the
      // watch's next round (every 30 s).
      await browser.waitUntil(async () => { st = await status(); return !st.old_line_wanted && st.old_line_running === false; }, {
        timeout: 600_000, interval: 10_000, timeoutMsg: "the 0.6 line did not stop after the full pass",
      });
      console.log(`e2e: stopped: ${JSON.stringify(st)}`);
    } else if (PHASE === "back") {
      await (await $('[data-testid="new-chat"]')).click();
      await ask(MESSAGE_BACK);
      await waitForReplyDone(300_000);
      const backup = await invoke<Record<string, unknown>>("vault_backup_now");
      console.log(`e2e: B's backup ${JSON.stringify(backup).slice(0, 300)}`);
      await shot("reattach-b-back");
    } else if (PHASE === "return") {
      let st: Status | null = null;
      await browser.waitUntil(async () => { st = await status(); return st.old_line_wanted && st.stopped_waiting.length === 0 && st.old_line_running === true; }, {
        timeout: 900_000, interval: 10_000, timeoutMsg: "the wait for the computer that came back never resumed, or the 0.6 line never started again",
      });
      console.log(`e2e: waiting again: ${JSON.stringify(st)}`);
      await browser.waitUntil(() => inOwnRecords("Reattach test"), {
        timeout: 900_000, interval: 15_000, timeoutMsg: "B's new conversation never reached this device's own records",
      });
      console.log("e2e: B's new conversation is in this device's own records");
      await shot("reattach-a-return");
    } else {
      throw new Error(`no phase ${PHASE}`);
    }
  });
});
