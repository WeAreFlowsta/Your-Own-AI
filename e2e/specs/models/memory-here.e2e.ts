// Using memory in replies, turned off and on for this computer. Said in
// the chat in fixed words, it is turned off and the line under the message
// says so, with Undo. While it is off a question about the past is still
// answered; the reply's Memory button says memory is turned off on this
// computer and offers to turn it on, in place. Turned on there, the next
// question is looked up again. Nothing about it shows at rest.
import { waitForChat, ask, waitForReplyDone, invoke, shot } from "./_helpers";

const lastSaid = '(//*[@data-testid="chat-message"][@data-role="user"])[last()]//*[@data-testid="kept-said"]';

async function saidLine(timeout = 60_000): Promise<{ said: string; text: string }> {
  const el = await $(lastSaid);
  await el.waitForExist({ timeout, timeoutMsg: "no line under the message says what was done" });
  return { said: (await el.getAttribute("data-said")) ?? "", text: await el.getText() };
}

const lastLine = () =>
  browser.execute(() => {
    const all = document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]');
    const line = all[all.length - 1]?.querySelector('[data-testid="given-line"]') as HTMLElement | null;
    return line ? { kind: line.getAttribute("data-kind"), text: line.innerText.replace(/\s+/g, " ").trim() } : null;
  });

async function newChat() {
  await browser.execute(() => (document.querySelector('[data-testid="new-chat"]') as HTMLElement | null)?.click());
  await browser.waitUntil(async () => browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length === 0), { timeout: 15_000 });
}

const usesMemory = () => invoke<boolean>("memory_in_replies");

describe("memory in replies, off and on for this computer", () => {
  it("is on until the person turns it off, and a message that only looks like the request changes nothing", async function () {
    this.timeout(900_000);
    await waitForChat();
    expect(await usesMemory()).toBe(true);
    // Something for memory to look through.
    await ask("We decided the oven goes to the left of the sink. Say OK and nothing else.");
    await waitForReplyDone(600_000);
    await ask("How do I stop using memory on this computer?");
    await waitForReplyDone(600_000);
    await browser.pause(6_000);
    expect(await (await $(lastSaid)).isExisting()).toBe(false);
    expect(await usesMemory()).toBe(true);
  });

  it("said in the chat, it is turned off, the line says so, and Undo turns it back on", async function () {
    this.timeout(900_000);
    await newChat();
    await ask("Stop using memory on this computer.");
    await waitForReplyDone(600_000);
    const line = await saidLine();
    console.log(`e2e: MEMORY HERE under the request: ${JSON.stringify(line)}`);
    expect(line.said).toBe("off_here");
    expect(line.text).toContain("no longer look in memory");
    expect(await usesMemory()).toBe(false);
    await browser.execute((e) => (e as HTMLElement).click(), await $('(//*[@data-testid="chat-message"][@data-role="user"])[last()]//*[@data-testid="kept-undo"]'));
    await browser.waitUntil(usesMemory, { timeout: 30_000, timeoutMsg: "Undo did not turn memory back on" });
  });

  it("while it is off, the reply's Memory button says so and turns it on in place", async function () {
    this.timeout(1_200_000);
    await newChat();
    await ask("Turn memory off on this computer.");
    await waitForReplyDone(600_000);
    expect((await saidLine()).said).toBe("off_here");
    await browser.waitUntil(async () => (await invoke<{ total: number }>("memory_reading_status")).total > 0, { timeout: 120_000, interval: 3_000, timeoutMsg: "the earlier conversation never counted as something to read" });
    await newChat();
    await ask("What did we decide about where the oven goes?");
    const reply = await waitForReplyDone(600_000);
    expect(reply.text.trim().length).toBeGreaterThan(0);
    const off = await lastLine();
    console.log(`e2e: MEMORY HERE line while off: ${JSON.stringify(off)}`);
    expect(off?.kind).toBe("did-not-look");
    await browser.execute(() => {
      const all = document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]');
      (all[all.length - 1]?.querySelector('[data-testid="given-line"] button') as HTMLElement | null)?.click();
    });
    const opened = await $('[data-testid="given-opened"]');
    await opened.waitForExist({ timeout: 30_000, timeoutMsg: "the Memory button opened nothing" });
    const words = (await opened.getText()).replace(/\s+/g, " ").trim();
    console.log(`e2e: MEMORY HERE opened: ${words}`);
    expect(words).toContain("Memory is turned off on this computer");
    await shot("memory-here-off");
    const box = await $('[data-testid="memory-here"]');
    await box.waitForExist({ timeout: 30_000, timeoutMsg: "the line offered no way to turn it on" });
    await browser.execute((e) => (e as HTMLElement).click(), await $('[data-testid="memory-here-on"]'));
    await browser.waitUntil(async () => (await box.getAttribute("data-state")) === "on", { timeout: 30_000, timeoutMsg: "pressing it did not turn memory on" });
    expect(await usesMemory()).toBe(true);
    // The next question is looked up again.
    await newChat();
    await ask("Where did we decide the oven goes?");
    await waitForReplyDone(600_000);
    const on = await lastLine();
    console.log(`e2e: MEMORY HERE line once on: ${JSON.stringify(on)}`);
    expect(on?.kind).not.toBe("did-not-look");
  });

  it("said in the chat, it is turned on again", async function () {
    this.timeout(900_000);
    await invoke("memory_in_replies_set", { on: false });
    await newChat();
    await ask("Use memory on this computer again.");
    await waitForReplyDone(600_000);
    const line = await saidLine();
    expect(line.said).toBe("on_here");
    expect(await usesMemory()).toBe(true);
  });
});
