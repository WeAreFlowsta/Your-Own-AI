// A question about now, on an AI that cannot look anything up (Offline
// Only, no plan): the reply carries one line above the composer saying it
// may be out of date, with the door to the Online Models page. Once a
// conversation, never for a health question, gone for good on "Don't ask
// again for this AI". Web access off changes nothing here: turning the web
// on would not fetch live information without online models, so the live
// line takes the place of the web-off line.
import { shot, waitForChat, ask, waitForReplyDone, resetAiModels } from "./_helpers";

const offer = () =>
  browser.execute(() => {
    const el = document.querySelector('[data-testid="live-offer"]') as HTMLElement | null;
    return el
      ? { plan: el.dataset.plan ?? "", text: el.innerText, link: el.querySelector("a")?.getAttribute("href") ?? "", web: !!document.querySelector('[data-testid="web-offer"]') }
      : null;
  });

async function firstAi(patch: Record<string, unknown>) {
  await browser.execute(async (p: Record<string, unknown>) => {
    const T = (window as any).__TAURI__;
    const store = await T.store.load(await T.core.invoke("profile_store_path", { name: "ai-data.json" }));
    const ais = ((await store.get("custom-ais")) || []) as Record<string, unknown>[];
    const a = ais.find((x) => x.agentPubKey);
    if (a) for (const [k, v] of Object.entries(p)) if (v === null) delete a[k]; else a[k] = v;
    await store.set("custom-ais", ais);
    await store.save();
  }, patch);
}

async function newChat() {
  await browser.url("tauri://localhost/chat/");
  await waitForChat();
}

/** The reply, then a moment for the check that runs beside it. */
async function answered(q: string) {
  await ask(q);
  await waitForReplyDone(300_000);
  await browser.pause(2_500);
  return offer();
}

const clearDontAsk = () =>
  browser.execute(() => {
    for (const k of Object.keys(localStorage)) if (k.startsWith("live-offer-off:")) localStorage.removeItem(k);
  });

describe("a question about now, on an AI that can't look it up", () => {
  before(async () => {
    await resetAiModels();
    await clearDontAsk();
  });
  after(async () => {
    await firstAi({ webAccess: null });
    await clearDontAsk();
    await resetAiModels();
  });

  it("says the answer may be out of date, once a conversation, with the door to Online Models", async function () {
    this.timeout(900_000);
    await newChat();
    const first = await answered("What's the weather today in Chicago?");
    console.log(`e2e: live offer ${JSON.stringify(first)}`);
    expect(first).not.toBeNull();
    expect(first!.plan).toBe("no");
    expect(first!.text).toContain("may be out of date");
    expect(first!.text).toContain("optional paid service");
    expect(first!.text).toContain("Everything offline stays free");
    expect(first!.link).toBe("/online-models");
    await shot("live-offer-no-plan");

    // The same conversation, another question about now: no second offer.
    const second = await answered("And what's the forecast for tomorrow there?");
    console.log(`e2e: second live question ${JSON.stringify(second)}`);
    expect(second).toBeNull();
  });

  it("never offers online for a health question", async function () {
    this.timeout(600_000);
    await newChat();
    const health = await answered("What's the latest advice today on blood pressure medication?");
    console.log(`e2e: health question ${JSON.stringify(health)}`);
    expect(health).toBeNull();
  });

  it("takes the place of the web-off line when web access is off", async function () {
    this.timeout(600_000);
    await firstAi({ webAccess: false });
    await newChat();
    const off = await answered("Who won the football game last night?");
    console.log(`e2e: web off ${JSON.stringify(off)}`);
    expect(off).not.toBeNull();
    expect(off!.web).toBe(false);
    await shot("live-offer-web-off");
    await firstAi({ webAccess: null });
  });

  it("stays away after Don't ask again for this AI", async function () {
    this.timeout(600_000);
    await newChat();
    expect(await answered("What's the price of bitcoin right now?")).not.toBeNull();
    await browser.execute(() => {
      const b = [...(document.querySelector('[data-testid="live-offer"]')?.querySelectorAll("button") ?? [])].find((x) => x.textContent?.trim() === "Don't ask again for this AI") as HTMLElement | undefined;
      b?.click();
    });
    await browser.waitUntil(async () => (await offer()) === null, { timeout: 10_000, timeoutMsg: "the live offer stayed" });
    await newChat();
    const after = await answered("What's the weather today in Chicago?");
    console.log(`e2e: after don't ask again ${JSON.stringify(after)}`);
    expect(after).toBeNull();
  });
});
