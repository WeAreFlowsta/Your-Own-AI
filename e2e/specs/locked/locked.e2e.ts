// "Restart to update" with the Vault locked, live (e2e/locked-live.sh): the
// backup before the install is made against a Vault that is locked and
// still syncing; against one that stopped syncing the step says so and
// installs nothing until the person says to go on without a backup.
// The launch sets YOAI_TEST_UPDATE_BACKUP_ONLY (debug builds): the step
// runs its backup and stops before the install.
import { shot, waitForChat, invoke, invokeLong, ask, waitForReplyDone } from "../models/_helpers";

const VAULT = `http://127.0.0.1:${process.env.YOAI_VAULT_PORT}`;
const HEADERS = { "content-type": "application/json", origin: "https://ourtest.flowsta.com" };
const STAMP = Date.now().toString(36);

async function vault(path: string, body?: unknown): Promise<{ status: number; data: any }> {
  const r = await fetch(`${VAULT}${path}`, { method: "POST", headers: HEADERS, body: JSON.stringify(body ?? {}) });
  return { status: r.status, data: await r.json().catch(() => null) };
}
async function kind(want: string): Promise<string> {
  let got = "";
  await browser.waitUntil(async () => (got = await invoke<string>("vault_lock_kind")) === want, { timeout: 60_000, interval: 2_000, timeoutMsg: `the Vault never read as "${want}"` }).catch(() => undefined);
  return got;
}
async function update(withoutBackup: boolean): Promise<string> {
  try {
    await invokeLong("restart_to_update", { withoutBackup }, 600_000);
    return "ok";
  } catch (e) {
    return String((e as Error).message ?? e);
  }
}

describe("Restart to update with the Vault locked", () => {
  it("backs up to a locked, still-syncing Vault and never skips the backup without a word", async () => {
    await waitForChat();
    await ask(`Locked test ${STAMP}: name one fruit and nothing else.`);
    await waitForReplyDone(300_000);

    expect(await kind("unlocked")).toBe("unlocked");
    const first = await invokeLong<Record<string, unknown>>("vault_backup_now", {}, 300_000);
    console.log(`e2e: unlocked backup ${JSON.stringify(first).slice(0, 300)}`);
    expect(first.manifest).toBe("written");

    // The ordinary lock: locked, still syncing.
    const locked = await vault("/dev/devices", { op: "lock" });
    console.log(`e2e: lock ${locked.status} ${JSON.stringify(locked.data)}`);
    expect(await kind("syncing")).toBe("syncing");
    const whileSyncing = await update(false);
    console.log(`e2e: update step, locked and still syncing: ${whileSyncing}`);
    expect(whileSyncing).toBe("ok");
    const again = await invokeLong<Record<string, unknown>>("vault_backup_now", {}, 300_000);
    console.log(`e2e: locked backup ${JSON.stringify(again).slice(0, 300)}`);
    expect(again.manifest).toBe("written");
    await shot("locked-still-syncing");

    // Lock and stop syncing.
    expect((await vault("/dev/unlock")).status).toBe(200);
    expect(await kind("unlocked")).toBe("unlocked");
    expect((await vault("/dev/lock")).status).toBe(200);
    expect(await kind("stopped")).toBe("stopped");
    const stopped = await update(false);
    console.log(`e2e: update step, syncing stopped: ${stopped}`);
    expect(stopped).toContain("no_backup:vault_locked");
    const goOn = await update(true);
    console.log(`e2e: update step, syncing stopped, "Update without a backup": ${goOn}`);
    expect(goOn).toBe("ok");
    await shot("locked-stopped");
    await vault("/dev/unlock");
  });
});
