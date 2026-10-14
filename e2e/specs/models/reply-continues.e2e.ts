// Replies keep going when you leave the chat (build-docs ALWAYS_ON_AND_THREADS.md,
// step 1; the Holochain team's 0.8.0 report): a long reply is started, the
// person opens Settings while it streams, comes back - the reply is still
// streaming or finished, and it is in the records.
import { shot, waitForChat, invoke, typeInto } from "./_helpers";

// Long enough that the reply is still streaming when the tray is checked -
// the WebDriver crawls (every command waits on the window-state hook) and a
// 300-word answer was done before the check (10-14).
const QUESTION = "Write about 1200 words on the history of tea, in plain paragraphs.";

describe("a reply keeps going when you leave the chat", () => {
  it("streams on while Settings is open and is in the records on return", async function () {
    this.timeout(600_000);
    await waitForChat();
    await typeInto("chat-input", QUESTION);
    await (await $('[data-testid="chat-send"]')).click();
    // Streaming has begun: an assistant bubble that is still loading, with some text.
    await browser.waitUntil(async () => {
      const t = await browser.execute(() => {
        const m = [...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined;
        return m ? { state: m.getAttribute("data-state"), len: m.innerText.length } : null;
      });
      return !!t && t.state !== "done" && t.len > 20;
    }, { timeout: 180_000, interval: 500, timeoutMsg: "the reply never started streaming" });
    // The turn is registered once the request starts (after routing, memory and
    // documents): leave only once it is, so the reply is really in flight.
    await browser.waitUntil(async () => ((await browser.execute(() => (window as any).__yoaiTurns?.() ?? [])) as unknown[]).length > 0, { timeout: 120_000, interval: 500, timeoutMsg: "no turn was registered at the root" });
    const live = await browser.execute(() => (window as any).__yoaiTurns?.() ?? []);
    console.log(`e2e: turns while streaming ${JSON.stringify(live)}`);
    await shot("reply-continues-streaming");
    // Leave for Settings THROUGH THE APP'S OWN LINK - an in-app route change, as a
    // person does it. (browser.url() reloads the document and wipes every JS
    // object, the reply's loop included - not the field case.)
    await (await $('button[title^="Menu"]')).click();
    await browser.execute(() => {
      const item = [...document.querySelectorAll('[role="menuitem"]')].find((b) => (b as HTMLElement).innerText.trim().startsWith("Settings")) as HTMLElement | undefined;
      item?.click();
    });
    await browser.waitUntil(async () => (await browser.execute(() => location.pathname)) === "/settings/", { timeout: 30_000, timeoutMsg: "Settings never opened" });
    await new Promise((r) => setTimeout(r, 6000));
    console.log(`e2e: turns while on Settings ${JSON.stringify(await browser.execute(() => (window as any).__yoaiTurns?.() ?? []))}`);
    // The activity tray is where the reply shows while you are elsewhere: a row
    // "Reeves is answering.." with Stop and Open.
    // Still streaming = "X is answering.." with Stop; if the engine raced
    // ahead and finished, the row reads "X answered" with Open - either way
    // the reply is shown where the person is, never lost.
    const row = await $('[data-testid="turn-row"]');
    await row.waitForDisplayed({ timeout: 10_000 });
    const rowText = await row.getText();
    const rowState = await row.getAttribute("data-turn-state");
    console.log(`e2e: tray row (${rowState}) ${JSON.stringify(rowText.replace(/\s+/g, " ").slice(0, 160))}`);
    if (rowState === "running") {
      expect(rowText).toContain("is answering");
      expect(rowText).toContain("Stop");
    } else {
      expect(rowText).toContain("answered");
    }
    expect(rowText).toContain("Open");
    await shot("reply-continues-settings");
    // Back to the chat through the row's Open: the page opens the reply's conversation.
    await browser.execute(() => {
      const r = document.querySelector('[data-testid="turn-row"]');
      const open = [...(r?.querySelectorAll("button") ?? [])].find((b) => b.textContent?.trim() === "Open") as HTMLElement | undefined;
      open?.click();
    });
    await browser.waitUntil(async () => (await browser.execute(() => location.pathname)) === "/chat/", { timeout: 30_000, timeoutMsg: "the chat never reopened" });
    await new Promise((r) => setTimeout(r, 4000));
    console.log(`e2e: turns on return ${JSON.stringify(await browser.execute(() => (window as any).__yoaiTurns?.() ?? []))}`);
    console.log(`e2e: page on return: ${JSON.stringify(await browser.execute(() => document.body.innerText.replace(/\s+/g, " ").slice(0, 240)))}`);
    // The reply is there: still streaming or done, never missing.
    await browser.waitUntil(async () => {
      const t = await browser.execute(() => {
        const m = [...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined;
        return m ? { state: m.getAttribute("data-state"), len: m.innerText.length } : null;
      });
      return !!t && t.len > 20;
    }, { timeout: 60_000, interval: 500, timeoutMsg: "the reply was not shown after coming back" });
    const seen = await browser.execute(() => {
      const m = [...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement;
      return { state: m.getAttribute("data-state"), len: m.innerText.length };
    });
    console.log(`e2e: on return the reply is ${JSON.stringify(seen)}`);
    // One question, one answer: no "never got an answer" bubble above the live reply, no second question.
    const bubbles = await browser.execute(() => ({
      unanswered: document.body.innerText.includes("never got an answer"),
      users: document.querySelectorAll('[data-testid="chat-message"][data-role="user"]').length,
      assistants: document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]').length,
    }));
    console.log(`e2e: bubbles on return ${JSON.stringify(bubbles)}`);
    expect(bubbles.unanswered).toBe(false);
    expect(bubbles.users).toBe(1);
    expect(bubbles.assistants).toBe(1);
    await shot("reply-continues-back");
    // Let it finish, then the records must hold it.
    await browser.waitUntil(async () => browser.execute(() => {
      const m = [...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined;
      return !!m && m.getAttribute("data-state") === "done";
    }), { timeout: 300_000, interval: 1000, timeoutMsg: "the reply never finished" });
    const finalLen = await browser.execute(() => ([...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement).innerText.length);
    console.log(`e2e: finished, ${finalLen} chars on screen`);
    expect(finalLen).toBeGreaterThan(200);
    // Records: the turn's conversation holds an assistant entry of real length.
    const done = (await browser.execute(() => (window as any).__yoaiTurns?.() ?? [])) as { conv: string | null }[];
    console.log(`e2e: turns at the end ${JSON.stringify(done)}`);
    const full = await browser.execute(() => {
      const w = window as any;
      return w.__yoaiTurnsFull ? w.__yoaiTurnsFull() : null;
    });
    if (full && full.length) {
      const t = full[full.length - 1];
      const entries = await invoke<{ role: string; content: string }[]>("get_conversation_transcript", { agentKey: t.agentKey, conversationHash: t.conversationHash }).catch((e) => { console.log(`e2e: transcript read said ${String(e).slice(0, 160)}`); return []; });
      const assistant = entries.filter((e) => e.role === "assistant");
      console.log(`e2e: records hold ${entries.length} entries, ${assistant.length} assistant, last ${assistant.at(-1)?.content.length ?? 0} chars`);
      expect(assistant.length).toBeGreaterThan(0);
      expect(assistant.at(-1)!.content.length).toBeGreaterThan(200);
    }
    await shot("reply-continues-done");
  });
});
