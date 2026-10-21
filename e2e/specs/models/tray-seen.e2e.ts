// The activity tray lists replies that finished while the person was
// elsewhere - never one they watched finish. 10-17: after New, the
// tray showed "Emrys answered" twice, for replies the person had just watched end.
// A reply left mid-way (New while it streams) is news when it ends.
import { ask, invoke, lastReply, shot, waitForChat, waitForReplyDone } from "./_helpers";

const turnRows = () =>
  browser.execute(() =>
    [...document.querySelectorAll('[data-testid="turn-row"]')].map((r) => ({
      state: (r as HTMLElement).dataset.turnState ?? "",
      text: (r as HTMLElement).innerText.replace(/\n/g, " | "),
    })),
  );

async function newChat() {
  await (await $('[data-testid="new-chat"]')).click();
  await browser.waitUntil(async () => browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length === 0), { timeout: 15_000, timeoutMsg: "New did not clear the page" });
}

describe("the tray and replies the person saw", () => {
  it("does not list a reply the person watched finish", async function () {
    this.timeout(400_000);
    await waitForChat();
    await ask("In one short sentence: why do birds sing?");
    await waitForReplyDone(300_000);
    await browser.pause(1_500);
    await newChat();
    await browser.pause(2_000);
    const rows = await turnRows();
    console.log(`e2e: after New, watched reply: ${JSON.stringify(rows)}`);
    await shot("tray-seen-after-new");
    expect(rows.filter((r) => r.state === "done")).toEqual([]);
  });

  it("lists a reply that finished after the person left it, recorded where it started", async function () {
    this.timeout(700_000);
    await waitForChat();
    await ask("Write four short sentences about the sea, one per line.");
    // Leave as soon as the reply has started.
    await browser.waitUntil(async () => (await lastReply())?.state === "streaming" || (await lastReply())?.state === "thinking", { timeout: 180_000, interval: 200, timeoutMsg: "the reply never started" });
    await newChat();
    try {
      await browser.waitUntil(async () => (await turnRows()).some((r) => r.state === "done"), { timeout: 300_000, interval: 1_000, timeoutMsg: "the reply that finished elsewhere was never listed" });
    } catch (e) {
      // Say what the root held and what the tray showed, so a miss can be read.
      console.log(`e2e: root turns at the miss: ${JSON.stringify(await browser.execute(() => (window as any).__yoaiTurns?.() ?? []))}`);
      console.log(`e2e: tray rows at the miss: ${JSON.stringify(await turnRows())} path=${await browser.execute(() => location.pathname)} bubbles=${await browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length)}`);
      throw e;
    }
    const rows = await turnRows();
    console.log(`e2e: after New mid-reply: ${JSON.stringify(rows)}`);
    await shot("tray-news-after-leaving");
    expect(rows.some((r) => r.state === "done" && r.text.includes("answered"))).toBe(true);

    // The reply recorded in ITS conversation: the question, then the answer.
    type T = { state: string; agentKey: string; conversationHash: string | null; question?: string };
    const left = ((await browser.execute(() => (window as any).__yoaiTurnsFull?.() ?? [])) as T[]).find((t) => !!t.question?.startsWith("Write four short sentences about the sea"));
    expect(left?.conversationHash).toBeTruthy();
    await browser.pause(3_000);
    const entries = await invoke<{ role: string; content: string }[]>("get_conversation_transcript", { agentKey: left!.agentKey, conversationHash: left!.conversationHash });
    console.log(`e2e: its conversation ${left!.conversationHash!.slice(0, 12)}: ${JSON.stringify(entries.map((e) => [e.role, e.content.slice(0, 40)]))}`);
    expect(entries.map((e) => e.role)).toEqual(["user", "assistant"]);
    expect(entries[1].content.length).toBeGreaterThan(20);

    // The new page is a new conversation: the next question does not join the old one.
    expect(await browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length)).toBe(0);
    await ask("Reply with the single word: ok");
    await waitForReplyDone(300_000);
    await browser.pause(3_000);
    const next = ((await browser.execute(() => (window as any).__yoaiTurnsFull?.() ?? [])) as T[]).find((t) => !!t.question?.startsWith("Reply with the single word"));
    console.log(`e2e: the next question's conversation ${next?.conversationHash?.slice(0, 12)} (the left one ${left!.conversationHash!.slice(0, 12)})`);
    expect(next?.conversationHash).toBeTruthy();
    expect(next!.conversationHash).not.toBe(left!.conversationHash);
    const still = await invoke<{ role: string }[]>("get_conversation_transcript", { agentKey: left!.agentKey, conversationHash: left!.conversationHash });
    expect(still.length).toBe(2);
  });
});
