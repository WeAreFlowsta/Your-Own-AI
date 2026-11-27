// The part of the app that searches is not on this computer, and it
// cannot be downloaded just now. A question about the past is still
// answered; the reply's Memory button says memory did not look and why,
// and offers the download in place. Pressed while the download cannot
// succeed, it says the download failed and offers it again; the chat
// goes on working.
//
// For a launch whose models folder holds a chat model and no search part,
// with downloads refused (YOAI_BLOCK_MODEL_DOWNLOADS=1):
//   YOAI_E2E_PART_MISSING=1
import { waitForChat, ask, waitForReplyDone, shot, invoke } from "../models/_helpers";

const lastLine = () =>
  browser.execute(() => {
    const all = document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]');
    const line = all[all.length - 1]?.querySelector('[data-testid="given-line"]') as HTMLElement | null;
    return line ? { kind: line.getAttribute("data-kind"), text: line.innerText.replace(/\s+/g, " ").trim() } : null;
  });

describe("memory's search part is missing and cannot be downloaded", () => {
  it("the reply says memory did not look, and the download offered in place fails in plain words", async function () {
    if (process.env.YOAI_E2E_PART_MISSING !== "1") this.skip();
    this.timeout(900_000);
    await waitForChat();
    // With no history at all nothing is said about memory (there is nothing
    // it could have looked through): one earlier conversation first.
    await ask("We decided the oven goes to the left of the sink. Say OK and nothing else.");
    await waitForReplyDone(600_000);
    await browser.execute(() => (document.querySelector('[data-testid="new-chat"]') as HTMLElement | null)?.click());
    await browser.waitUntil(async () => (await browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length)) === 0, { timeout: 15_000 });
    await browser.waitUntil(async () => (await invoke<{ total: number }>("memory_reading_status")).total > 0, { timeout: 120_000, interval: 3_000, timeoutMsg: "the earlier conversation never counted as something to read" });
    await ask("What did we decide last week about the oven?");
    const reply = await waitForReplyDone(600_000);
    expect(reply.text.trim().length).toBeGreaterThan(0);
    const line = await lastLine();
    console.log(`e2e: PART MISSING line: ${JSON.stringify(line)}`);
    expect(line?.kind).toBe("did-not-look");
    // Nothing about memory shows at rest: the reply's Memory button opens it.
    await browser.execute(() => {
      const all = document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]');
      (all[all.length - 1]?.querySelector('[data-testid="given-line"] button') as HTMLElement | null)?.click();
    });
    const opened = await $('[data-testid="given-opened"]');
    await opened.waitForExist({ timeout: 30_000, timeoutMsg: "the Memory button opened nothing" });
    console.log(`e2e: PART MISSING opened: ${(await opened.getText()).replace(/\s+/g, " ").trim()}`);
    const box = await $('[data-testid="memory-download"]');
    await box.waitForExist({ timeout: 30_000, timeoutMsg: "the line offered no download" });
    const start = await $('[data-testid="memory-download-start"]');
    expect(await start.getText()).toMatch(/^Download it \(\d+ MB\)$/);
    await shot("part-missing-offer");
    await browser.execute((e) => (e as HTMLElement).click(), start);
    await browser.waitUntil(async () => (await box.getAttribute("data-state")) === "failed", { timeout: 120_000, timeoutMsg: "the refused download never showed as failed" });
    const after = (await box.getText()).replace(/\s+/g, " ").trim();
    console.log(`e2e: PART MISSING after the press: ${after}`);
    expect(after).toContain("The download failed.");
    expect(after).toContain("Download again");
    await shot("part-missing-failed");
  });

  it("the chat goes on working", async function () {
    if (process.env.YOAI_E2E_PART_MISSING !== "1") this.skip();
    this.timeout(600_000);
    await ask("Name one primary color and nothing else.");
    const reply = await waitForReplyDone(300_000);
    expect(reply.text.trim().length).toBeGreaterThan(0);
  });
});
