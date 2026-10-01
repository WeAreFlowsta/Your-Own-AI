// Flow 2: a reply can be stopped mid-stream. The message keeps what it got,
// is marked as stopped, and the composer is ready again.
import { shot, waitForChat, ask, lastReply, waitForReplyDone } from "./_helpers";

describe("stop mid-reply", () => {
  it("stops a long reply and keeps what arrived", async () => {
    await waitForChat();
    await ask("Write a 600 word story about a lighthouse keeper. Do not stop early.");
    await browser.waitUntil(async () => (await lastReply())?.state === "streaming", { timeout: 180_000, timeoutMsg: "the reply never started streaming" });
    await browser.pause(1_500);
    await shot("20-streaming");
    const stop = await $('[data-testid="chat-stop"]');
    await stop.waitForDisplayed({ timeout: 10_000 });
    await stop.click();
    const r = await waitForReplyDone(30_000);
    await shot("21-stopped");
    expect(r.stopped).toBe(true);
    expect(r.text.length).toBeGreaterThan(20);
    // The composer is ready for the next question.
    await (await $('[data-testid="chat-send"]')).waitForDisplayed({ timeout: 10_000 });
  });
});
