// A picture of each place the app offers online models to someone without a
// plan (signed out): the model picker's footer, Settings' routing text, the
// Online Models page, the offer above the composer when a question needs live
// information and web access is off, and the card for an online model while
// signed out. Pictures only (e2e/shots/notice-*.png) - the review of the
// notices reads them; nothing here passes or fails on copy.
import { shot, invoke, waitForChat, ask, waitForReplyDone, resetAiModels } from "./_helpers";

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

async function scrollToText(text: string) {
  return browser.execute((t: string) => {
    const el = [...document.querySelectorAll("h1,h2,h3,h4,p,span,div,label")].find((e) => e.childElementCount === 0 && e.textContent?.includes(t)) as HTMLElement | undefined;
    el?.scrollIntoView({ block: "center", behavior: "instant" as ScrollBehavior });
    return !!el;
  }, text);
}

describe("online-model offers, as someone without a plan sees them", () => {
  after(async () => {
    await firstAi({ webAccess: null });
    await resetAiModels();
  });

  it("captures each notice", async function () {
    this.timeout(900_000);
    await waitForChat();
    await shot("notice-01-start-view");

    // The model picker beside the composer ("Auto - Offline Only"), scrolled to its footer.
    const opened = await browser.execute(() => {
      const b = [...document.querySelectorAll("button")].find((x) => /Offline Only|Online and Offline/.test(x.textContent ?? "")) as HTMLElement | undefined;
      b?.click();
      return !!b;
    });
    console.log(`e2e: model picker opened: ${opened}`);
    await browser.pause(800);
    console.log(`e2e: picker footer found: ${await scrollToText("come with a plan")}`);
    await browser.pause(300);
    await shot("notice-02-model-picker");
    await browser.keys("Escape");

    // Settings: the routing section's no-plan text.
    await browser.url("tauri://localhost/settings/");
    await browser.pause(2_500);
    console.log(`e2e: settings routing text found: ${await scrollToText("Plans are an optional paid")}`);
    await browser.pause(300);
    await shot("notice-03-settings-routing");

    // The Online Models page, signed out.
    await browser.url("tauri://localhost/online-models/");
    await browser.pause(3_000);
    await shot("notice-04-online-models-page");

    // A live-information question with web access off: the offer above the composer.
    await firstAi({ webAccess: false });
    await browser.url("tauri://localhost/chat/");
    await waitForChat();
    await ask("what's the weather today in Chicago");
    await waitForReplyDone(240_000);
    await browser.pause(800);
    await shot("notice-05-live-question-web-off");

    // An online model while signed out: the card under the conversation.
    await firstAi({ webAccess: null, model: "online:gpt-6-luna" });
    await browser.url("tauri://localhost/chat/");
    await (await $('[data-testid="chat-input"]')).waitForDisplayed({ timeout: 60_000 });
    await (await $('[data-testid="new-chat"]')).click().catch(() => {});
    await ask("hello");
    await browser.pause(6_000);
    await shot("notice-06-online-model-signed-out");
  });
});
