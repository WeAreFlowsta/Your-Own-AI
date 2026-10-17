// A plan holder's AI set to Offline Only, asked about now: the line above
// the composer says the answer may be out of date and offers "Look it up
// online" - the same question again on the live-web model, this once, with
// the AI still Offline Only afterwards.
import { waitForChat, ask, lastReply, shot, resetAiModels } from "../models/_helpers";
import { cheapPicks, haveIdentity, setAiModel, setRoutingPrefs, signInAsTestIdentity } from "./_online";

const offer = () =>
  browser.execute(() => {
    const el = document.querySelector('[data-testid="live-offer"]') as HTMLElement | null;
    return el ? { plan: el.dataset.plan ?? "", text: el.innerText, buttons: [...el.querySelectorAll("button")].map((b) => b.textContent?.trim() ?? "") } : null;
  });

const served = () =>
  browser.execute(() => {
    const m = [...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined;
    return m?.dataset.served ?? "";
  });

describe("the live line for a plan holder", () => {
  let key = "";

  before(async function () {
    this.timeout(300_000);
    if (!haveIdentity()) return;
    await waitForChat();
    const session = await signInAsTestIdentity();
    expect(session.tier).toBe("pro");
    key = (await browser.execute(async () => {
      const T = (window as any).__TAURI__;
      const store = await T.store.load(await T.core.invoke("profile_store_path", { name: "ai-data.json" }));
      return (((await store.get("custom-ais")) || []) as { agentPubKey?: string }[]).find((a) => a.agentPubKey)?.agentPubKey ?? "";
    })) as string;
    await setAiModel(key, "auto:offline");
    await setRoutingPrefs(cheapPicks());
    await browser.execute(() => {
      for (const k of Object.keys(localStorage)) if (k.startsWith("live-offer-off:")) localStorage.removeItem(k);
    });
  });
  after(async () => {
    await resetAiModels();
  });

  it("looks it up online once, and leaves the AI Offline Only", async function () {
    this.timeout(900_000);
    if (!haveIdentity()) this.skip();
    await browser.url("tauri://localhost/chat/");
    await waitForChat();
    await ask("What's the weather today in Chicago?");
    await browser.waitUntil(async () => (await lastReply())?.state === "done", { timeout: 300_000, timeoutMsg: "the device reply never finished" });
    const device = await served();
    expect(device.startsWith("online:")).toBe(false);
    await browser.waitUntil(async () => !!(await offer()), { timeout: 15_000, timeoutMsg: "the live line did not show" });
    const o = (await offer())!;
    console.log(`e2e: live offer ${JSON.stringify(o)}`);
    expect(o.plan).toBe("yes");
    expect(o.buttons).toContain("Look it up online");
    expect(o.buttons).toContain("Let this AI go online");
    await shot("live-offer-plan");

    await browser.execute(() => {
      const b = [...(document.querySelector('[data-testid="live-offer"]')?.querySelectorAll("button") ?? [])].find((x) => x.textContent?.trim() === "Look it up online") as HTMLElement | undefined;
      b?.click();
    });
    await browser.waitUntil(async () => (await served()).startsWith("online:") && (await lastReply())?.state === "done", {
      timeout: 240_000,
      interval: 1000,
      timeoutMsg: "the question was not answered online",
    });
    const online = await served();
    const reply = await lastReply();
    console.log(`e2e: looked up on ${online}: ${JSON.stringify(reply?.text.slice(0, 200))}`);
    // One question, one answer: the device answer was replaced.
    const counts = await browser.execute(() => ({
      users: document.querySelectorAll('[data-testid="chat-message"][data-role="user"]').length,
      assistants: document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]').length,
    }));
    expect(counts).toEqual({ users: 1, assistants: 1 });
    expect(await offer()).toBeNull();
    // The AI itself stays as it was.
    const model = await browser.execute(async (k: string) => {
      const T = (window as any).__TAURI__;
      const store = await T.store.load(await T.core.invoke("profile_store_path", { name: "ai-data.json" }));
      return (((await store.get("custom-ais")) || []) as { agentPubKey?: string; model?: string }[]).find((a) => a.agentPubKey === k)?.model ?? "";
    }, key);
    expect(model).toBe("auto:offline");
    await shot("live-offer-plan-looked-up");
  });
});
