// "Restart to update" with Flowsta Vault not running, and an install that
// fails, live: `LOCKED_SPEC=e2e/specs/locked/vault-closed.e2e.ts
// YOAI_TEST_UPDATE_INSTALL_FAILS=1 e2e/locked-live.sh`. A person who backs
// up to the Vault is never updated without their backup and without a word:
// with the Vault closed the notice says so and offers two things; and when
// the install itself fails, it says nothing changed and offers it again.
import { execFileSync } from "node:child_process";
import { shot, waitForChat, ask, waitForReplyDone } from "../models/_helpers";

const PORT = process.env.YOAI_VAULT_PORT ?? "";
const STAMP = Date.now().toString(36);

async function offerUpdate() {
  await browser.execute(() => location.reload());
  await browser.pause(1500);
  await waitForChat();
  // The notice starts listening a moment after the page is up: offer until it shows.
  await browser.waitUntil(async () => {
    await browser.execute(async () => {
      await (window as any).__TAURI__.event.emit("update-ready", { version: "9.9.9", notes_url: "", changes_line: false, asks_password: false });
    });
    return $('[data-testid="restart-to-update"]').isDisplayed();
  }, { timeout: 30_000, interval: 1_000, timeoutMsg: "the update notice never showed Restart to update" });
}
const noticeText = () => browser.execute(() => ((document.querySelector('[data-testid="update-ready"]') as HTMLElement | null)?.innerText ?? "").replace(/\s+/g, " ").trim());

/** Close the test Vault as a person quitting it would leave it: the one
 *  process listening on its port, by its pid. */
function closeTheVault(): number {
  const out = execFileSync("ss", ["-ltnpH", `sport = :${PORT}`], { encoding: "utf8" });
  const pid = Number(/pid=(\d+)/.exec(out)?.[1] ?? 0);
  if (!pid) throw new Error(`no process listens on the test Vault's port ${PORT}`);
  process.kill(pid, "SIGTERM");
  return pid;
}

describe("Restart to update with the Vault closed, and an install that fails", () => {
  it("says the Vault is not running, offers to open it first, and after a failed install says nothing changed", async () => {
    expect(PORT).not.toBe("");
    await waitForChat();
    // Something to back up, so the update's backup is owed.
    await ask(`Vault closed test ${STAMP}: name one fruit and nothing else.`);
    await waitForReplyDone(300_000);

    const pid = closeTheVault();
    console.log(`e2e: the test Vault (pid ${pid}) was closed`);
    await browser.waitUntil(async () => !(await fetch(`http://127.0.0.1:${PORT}/status`).then((r) => r.ok, () => false)), { timeout: 30_000, timeoutMsg: "the test Vault still answers" });

    // 1. The Vault is not running: no install, and the two choices.
    await offerUpdate();
    await (await $('[data-testid="restart-to-update"]')).click();
    const why = await $('[data-testid="update-no-backup"]');
    await why.waitForDisplayed({ timeout: 400_000, timeoutMsg: `the notice never said no backup could be made: ${await noticeText()}` });
    expect(await why.getText()).toContain("Flowsta Vault is not running, so this update cannot back up to it first.");
    expect(await (await $('[data-testid="update-without-backup"]')).getText()).toContain("Update without a backup");
    expect(await (await $('[data-testid="update-unlock-first"]')).getText()).toContain("Open the Vault first");
    await shot("update-vault-closed");

    // 2. "Open the Vault first" returns to the offer and installs nothing.
    await (await $('[data-testid="update-unlock-first"]')).click();
    await (await $('[data-testid="restart-to-update"]')).waitForDisplayed({ timeout: 10_000 });
    expect(await $('[data-testid="update-no-backup"]').isExisting()).toBe(false);

    // 3. Asked again and told to go on without a backup: the install runs,
    //    fails (the test build was asked to fail it), and the notice says
    //    nothing changed and offers it again.
    await (await $('[data-testid="restart-to-update"]')).click();
    await (await $('[data-testid="update-without-backup"]')).waitForDisplayed({ timeout: 400_000 });
    await (await $('[data-testid="update-without-backup"]')).click();
    await browser.waitUntil(async () => (await noticeText()).includes("It was not installed, and nothing changed."), { timeout: 120_000, timeoutMsg: `the notice never said the install failed: ${await noticeText()}` });
    expect(await (await $('[data-testid="restart-to-update"]')).isDisplayed()).toBe(true);
    expect(await $('[data-testid="update-no-backup"]').isExisting()).toBe(false);
    await shot("update-install-failed");
    console.log(`e2e: after the failed install the notice reads: ${await noticeText()}`);
  });
});
