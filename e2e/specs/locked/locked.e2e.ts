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

/** Offer an update the way the updater does once a download is ready,
 *  and start recording everything the notice says from here on. */
async function offerUpdate() {
  await browser.execute(() => location.reload());
  await browser.pause(1500);
  await waitForChat();
  // The notice starts listening a moment after the page is up: offer until it shows.
  await browser.waitUntil(async () => {
    await browser.execute(async () => {
      const w = window as any;
      w.__updateSaid = [];
      await w.__TAURI__.event.emit("update-ready", { version: "9.9.9", notes_url: "", changes_line: false, asks_password: false });
    });
    return $('[data-testid="restart-to-update"]').isDisplayed();
  }, { timeout: 30_000, interval: 1_000, timeoutMsg: "the update notice never showed Restart to update" });
  await browser.execute(() => {
    const w = window as any;
    const card = document.querySelector('[data-testid="update-notices"]') as HTMLElement;
    const note = () => { const t = (card.innerText || "").replace(/\s+/g, " ").trim(); if (w.__updateSaid[w.__updateSaid.length - 1] !== t) w.__updateSaid.push(t); };
    note();
    new MutationObserver(note).observe(card, { childList: true, subtree: true, characterData: true });
  });
}
const said = () => browser.execute(() => ((window as any).__updateSaid ?? []) as string[]);
const noticeText = () => browser.execute(() => ((document.querySelector('[data-testid="update-ready"]') as HTMLElement | null)?.innerText ?? "").replace(/\s+/g, " ").trim());
const backupAt = async () => Number(((await invoke<Record<string, unknown> | null>("last_backup_outcome")) ?? {}).at_secs ?? 0);
const nowSecs = () => Math.floor(Date.now() / 1000);
async function chat(what: string): Promise<number> {
  const at = nowSecs();
  await ask(`Locked test ${STAMP}: name one ${what} and nothing else.`);
  await waitForReplyDone(300_000);
  return at;
}

/** What Settings > Devices shows for this computer: the lock's own words
 *  when nothing travels, null when there is no lock. */
async function devicesLock(): Promise<{ kind: string | null; says: string | null }> {
  await browser.url("tauri://localhost/settings/");
  const panel = await $('[data-testid="settings-devices"]');
  await panel.waitForExist({ timeout: 60_000, timeoutMsg: "Settings > Devices never showed" });
  await panel.scrollIntoView();
  await (await $('[data-testid="device-row"][data-this="yes"]')).waitForExist({ timeout: 30_000 });
  await browser.pause(3000); // the panel reads the Vault once it is in view
  const got = await browser.execute(() => {
    const lock = document.querySelector('[data-testid="device-locked"]');
    return { kind: lock?.getAttribute("data-kind") ?? null, says: lock?.getAttribute("title") ?? null };
  });
  await browser.url("tauri://localhost/chat/");
  await waitForChat();
  return got;
}

describe("Restart to update with the Vault locked", () => {
  it("says each step at the button, backs up to a locked and still-syncing Vault, and never skips the backup without a word", async () => {
    await waitForChat();
    await chat("fruit");
    expect(await kind("unlocked")).toBe("unlocked");
    const first = await invokeLong<Record<string, unknown>>("vault_backup_now", {}, 300_000);
    console.log(`e2e: unlocked backup ${JSON.stringify(first).slice(0, 300)}`);
    expect(first.manifest).toBe("written");

    // 1. The button, the Vault unlocked, something new to save: the notice
    //    says it is backing up, and a backup newer than the chat exists
    //    when the step is over.
    const wrote1 = await chat("color");
    await offerUpdate();
    expect(await noticeText()).toContain("Your Own AI 9.9.9 is ready to install");
    await (await $('[data-testid="restart-to-update"]')).click();
    await browser.waitUntil(async () => (await backupAt()) >= wrote1 && (await said()).some((t) => t.includes("Installing Your Own AI 9.9.9")), { timeout: 400_000, interval: 2_000, timeoutMsg: "the update step never finished its backup" });
    console.log(`e2e: the notice said ${JSON.stringify(await said())}`);
    expect((await said()).some((t) => t.includes("Backing up to your Vault first."))).toBe(true);
    await shot("update-backing-up");

    // 2. A backup already running when the button is pressed: the notice
    //    says it is finishing that one.
    await chat("river");
    await offerUpdate();
    await browser.execute(() => { (window as any).__TAURI__.core.invoke("vault_backup_now").catch(() => {}); });
    await browser.pause(500);
    await (await $('[data-testid="restart-to-update"]')).click();
    await browser.waitUntil(async () => (await said()).some((t) => t.includes("Finishing a backup to your Vault that was already running.")), { timeout: 120_000, interval: 500, timeoutMsg: `the notice never said it was finishing a running backup: ${JSON.stringify(await said())}` });
    await shot("update-finishing-a-backup");

    // 3. The ordinary lock (locked, still syncing): the step backs up.
    const locked = await vault("/dev/devices", { op: "lock" });
    console.log(`e2e: lock ${locked.status} ${JSON.stringify(locked.data)}`);
    expect(await kind("syncing")).toBe("syncing");
    const wrote3 = await chat("tree");
    await offerUpdate();
    await (await $('[data-testid="restart-to-update"]')).click();
    await browser.waitUntil(async () => (await backupAt()) >= wrote3, { timeout: 400_000, interval: 2_000, timeoutMsg: "no backup newer than the chat was made against the locked, still-syncing Vault" });
    expect(await $('[data-testid="update-no-backup"]').isExisting()).toBe(false);
    const again = await invokeLong<Record<string, unknown>>("vault_backup_now", {}, 300_000);
    console.log(`e2e: locked backup ${JSON.stringify(again).slice(0, 300)}`);
    expect(again.manifest).toBe("written");
    await shot("locked-still-syncing");
    // Settings > Devices shows no lock: everything still travels.
    const whileSyncing = await devicesLock();
    console.log(`e2e: Settings > Devices, locked and still syncing: ${JSON.stringify(whileSyncing)}`);
    expect(whileSyncing.says).toBeNull();

    // 4. Locked with syncing stopped: the notice says why, offers both
    //    choices, and installs nothing until one is taken.
    expect((await vault("/dev/unlock")).status).toBe(200);
    expect(await kind("unlocked")).toBe("unlocked");
    expect((await vault("/dev/lock")).status).toBe(200);
    expect(await kind("stopped")).toBe("stopped");
    const whileStopped = await devicesLock();
    console.log(`e2e: Settings > Devices, syncing stopped: ${JSON.stringify(whileStopped)}`);
    expect(whileStopped.says).toBe("Syncing is stopped on this computer. Unlock Flowsta Vault to start it again.");
    await chat("animal");
    await offerUpdate();
    await (await $('[data-testid="restart-to-update"]')).click();
    const why = await $('[data-testid="update-no-backup"]');
    await why.waitForDisplayed({ timeout: 400_000, timeoutMsg: "the notice never said no backup could be made" });
    expect(await why.getText()).toContain("Flowsta Vault is locked, so this update cannot back up to it first.");
    expect(await (await $('[data-testid="update-without-backup"]')).getText()).toContain("Update without a backup");
    expect(await (await $('[data-testid="update-unlock-first"]')).getText()).toContain("Unlock the Vault first");
    await shot("update-no-backup");
    // "Unlock the Vault first" returns to the offer.
    await (await $('[data-testid="update-unlock-first"]')).click();
    await (await $('[data-testid="restart-to-update"]')).waitForDisplayed({ timeout: 10_000 });
    // Asked again, then "Update without a backup" goes on.
    await (await $('[data-testid="restart-to-update"]')).click();
    await (await $('[data-testid="update-without-backup"]')).waitForDisplayed({ timeout: 400_000 });
    await (await $('[data-testid="update-without-backup"]')).click();
    await browser.waitUntil(async () => (await noticeText()).includes("Installing Your Own AI 9.9.9"), { timeout: 60_000, timeoutMsg: `the notice did not go on to the install: ${await noticeText()}` });
    expect(await $('[data-testid="update-no-backup"]').isExisting()).toBe(false);
    await shot("locked-stopped");
    // The Vault stays stopped until the install's own backup attempt has
    // been refused (the test build holds each backup for a few seconds):
    // unlocked sooner, a backup would be made after all.
    await browser.pause(10_000);
    await vault("/dev/unlock");
  });
});
