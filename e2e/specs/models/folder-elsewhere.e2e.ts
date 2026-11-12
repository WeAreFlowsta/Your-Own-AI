// A conversation that worked in a project folder, opened on a computer that
// does not have that folder: the app says where the folder is and offers to
// point at it here or to read without it. It never makes the folder, never
// shows an error, and does not ask again once the person chose to read.
import { shot, waitForChat, invoke, ask, waitForReplyDone } from "./_helpers";

const STAMP = Date.now().toString(36);
const ELSEWHERE = `/home/another-computer/projects/ferry-timetable-${STAMP}`;
const TITLE = "This project's folder is not on this computer";

const dialogUp = () => browser.execute((t: string) => [...document.querySelectorAll("h2")].some((h) => (h.textContent || "").trim() === t), TITLE);
async function openFromDrawer() {
  await (await $('button[title^="Conversations"]')).click();
  const item = await $(`//*[@data-testid="conversation-item"][contains(., "Folder test ${STAMP}")]`);
  await item.waitForDisplayed({ timeout: 60_000, timeoutMsg: "the drawer never listed the conversation" });
  await (await item.$("button")).click();
}
const messagesShown = () => browser.execute(() => [...document.querySelectorAll('[data-testid="chat-message"]')].map((m) => (m as HTMLElement).innerText.slice(0, 80)));

describe("a project conversation whose folder is not on this computer", () => {
  it("says so, offers both choices, and reads without the folder without asking again", async function () {
    this.timeout(900_000);
    await waitForChat();
    await ask(`Folder test ${STAMP}: name one metal and nothing else.`);
    await waitForReplyDone(300_000);
    const turns = (await browser.execute(() => (window as any).__yoaiTurnsFull?.() ?? [])) as { conversationHash: string }[];
    const hash = turns[turns.length - 1]?.conversationHash ?? "";
    expect(hash.length).toBeGreaterThan(10);

    // The conversation's folder, as a computer that has it recorded it.
    const path = await invoke<string>("profile_store_path", { name: "ai-data.json" });
    const scope = path.split(/[\\/]/).slice(-2, -1)[0];
    await browser.execute((s: string, h: string, folder: string) => {
      const key = `conversation-folders:${s}`;
      const map = JSON.parse(localStorage.getItem(key) || "{}");
      map[h] = folder;
      localStorage.setItem(key, JSON.stringify(map));
      localStorage.removeItem(`conversation-read-without-folder:${s}`);
    }, scope, hash, ELSEWHERE);
    expect(await invoke<boolean>("path_is_dir", { path: ELSEWHERE })).toBe(false);

    await browser.execute(() => location.reload());
    await browser.pause(2000);
    await waitForChat();
    // The page may reopen its last conversation by itself; else the drawer.
    const byItself = await browser.waitUntil(dialogUp, { timeout: 8_000, interval: 500 }).catch(() => false);
    if (!byItself) await openFromDrawer();
    await browser.waitUntil(dialogUp, { timeout: 60_000, timeoutMsg: "the folder dialog never showed" });
    const said = await browser.execute(() => (document.body.innerText || "").replace(/\s+/g, " "));
    expect(said).toContain(`This conversation worked in the folder ${ELSEWHERE} on another of your computers.`);
    expect(said).toContain("Files do not travel between your computers yet.");
    expect(await $("button*=Choose its folder on this computer").isDisplayed()).toBe(true);
    expect(await $("button*=Read without the folder").isDisplayed()).toBe(true);
    await shot("folder-elsewhere-dialog");

    await (await $("button*=Read without the folder")).click();
    await browser.waitUntil(async () => !(await dialogUp()), { timeout: 10_000, timeoutMsg: "the dialog stayed after Read without the folder" });
    await browser.waitUntil(async () => (await messagesShown()).some((t: string) => t.includes(`Folder test ${STAMP}`)), { timeout: 60_000, timeoutMsg: "the conversation's messages did not show" });
    // Never made here, and no error on the page.
    expect(await invoke<boolean>("path_is_dir", { path: ELSEWHERE })).toBe(false);
    expect(await browser.execute(() => document.body.innerText)).not.toContain("cannot create workspace dir");
    await shot("folder-elsewhere-reading");

    // Opened again: read, not asked.
    await (await $('[data-testid="new-chat"]')).click();
    await browser.pause(1000);
    await openFromDrawer();
    await browser.waitUntil(async () => (await messagesShown()).some((t: string) => t.includes(`Folder test ${STAMP}`)), { timeout: 60_000, timeoutMsg: "the conversation did not open the second time" });
    await browser.pause(3000);
    expect(await dialogUp()).toBe(false);
  });
});
