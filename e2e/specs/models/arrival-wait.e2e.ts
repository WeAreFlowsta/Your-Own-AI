// A conversation written on another of the person's computers, opened here
// before its messages have arrived: the page waits where the person can see
// why, says both computers need to be on, and anything else they start ends
// the wait. A debug build answers for one made-up conversation as if it
// were written elsewhere with three messages still to come
// (`conversation_arrival`, TEST_ARRIVING_HASH), so no second device is
// needed here; devices-live.sh has the real arrival.
import { ask, invoke, shot, waitForChat, waitForReplyDone } from "./_helpers";

const HASH = "uhCkkArrivalWaitTest";

async function reloadToChat() {
  await browser.execute(() => location.reload());
  await browser.pause(2000);
  await waitForChat();
  // The notes are scoped once the profile is known (a few hundred ms).
  await browser.pause(3000);
}

describe("a conversation still arriving from another computer", () => {
  it("waits in view, says why and what to do, and gives way to a new chat", async function () {
    this.timeout(600_000);
    await waitForChat();
    const path = await invoke<string>("profile_store_path", { name: "ai-data.json" });
    const scope = path.split(/[\\/]/).slice(-2, -1)[0];
    const own = await browser.execute(async (p: string) => {
      const t = (window as any).__TAURI__.core;
      const rid = await t.invoke("plugin:store|load", { path: p, autoSave: false });
      const got = await t.invoke("plugin:store|get", { rid, key: "custom-ais" });
      const list = Array.isArray(got) ? got[0] : got;
      return (Array.isArray(list) ? list : []).map((a: any) => ({ key: a.agentPubKey ?? "", id: a.id ?? "" })).find((a: any) => !!a.key) ?? null;
    }, path);
    expect(own).toBeTruthy();

    // The continue pointer names the conversation that is not here yet.
    await browser.execute((s: string, o: { key: string; id: string }, h: string) => {
      localStorage.setItem(`last-conversation:${s}`, JSON.stringify({ hash: h, agentKey: o.key, aiId: o.id, title: "Written on my other computer" }));
    }, scope, own, HASH);
    await reloadToChat();
    const offer = await $('[data-testid="continue-last"]');
    await offer.waitForDisplayed({ timeout: 60_000, timeoutMsg: "the start page never offered the conversation" });
    expect(await offer.getText()).toContain("Written on my other computer");
    await offer.click();

    // The wait, in view: what is happening, and what the person can do.
    const waiting = await $('[data-testid="conversation-arriving"]');
    await waiting.waitForDisplayed({ timeout: 180_000, timeoutMsg: "the page never said the conversation was arriving" });
    const said = (await waiting.getText()).replace(/\s+/g, " ").trim();
    console.log(`e2e: the wait reads: ${said}`);
    expect(said).toContain("Getting this conversation from Test computer");
    expect(said).toContain("It opens here by itself when it arrives. Both computers need to be on.");
    expect(await offer.isEnabled()).toBe(false);
    await shot("arrival-wait");

    // Starting something else ends the wait: a new message is answered.
    await ask("Arrival test: name one fruit and nothing else.");
    await waitForReplyDone(300_000);
    expect(await $('[data-testid="conversation-arriving"]').isExisting()).toBe(false);
    await browser.execute((s: string) => localStorage.removeItem(`last-conversation:${s}`), scope);
  });
});
