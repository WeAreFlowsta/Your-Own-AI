// Take: the online beat. The AI (Nova) is switched from Auto - Offline Only to Auto - Online and Offline in the chat's
// model chip; a harder question goes online; the reply's Model details say which model answered, where it ran and
// why auto routing sent it there. Needs the identity's plan (online models). Kept shoot profile, models seeded.
//   YOAI_CAPTURE_QUESTION  the question (default: a business comparison)
import { appUrl } from "../wdio.conf";
import { Take } from "./_capture";

const QUESTION = process.env.YOAI_CAPTURE_QUESTION ??
  "Compare three ways a 20-person consulting firm could track client projects, with rough costs, and recommend one.";
const invoke = <T>(cmd: string, args: Record<string, unknown> = {}) =>
  browser.execute(async (c: string, a: Record<string, unknown>) => (window as any).__TAURI__.core.invoke(c, a), cmd, args) as Promise<T>;
const tag = (css: string, startsWith: string, as: string) =>
  browser.execute((c: string, t: string, a: string) => {
    const el = [...document.querySelectorAll(c)].find((e) => (e as HTMLElement).innerText.trim().startsWith(t));
    el?.setAttribute("data-shoot", a);
    return !!el;
  }, css, startsWith, as);
const lastReply = () =>
  browser.execute(() => {
    const m = [...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined;
    if (m) m.setAttribute("data-shoot-reply", "1");
    return m ? { state: m.dataset.state ?? "", text: m.innerText } : null;
  });

describe("capture", () => {
  it("online", async () => {
    await browser.url(appUrl("/chat/"));
    await (await $('[data-testid="chat-input"]')).waitForDisplayed({ timeout: 60_000 });
    if (await browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length > 0)) {
      await (await $('[data-testid="new-chat"]')).click().catch(() => undefined);
      await browser.pause(1500);
    }
    await browser.waitUntil(async () => !!(await invoke<string | null>("get_current_model")), { timeout: 240_000, interval: 2_000, timeoutMsg: "no model loaded" });

    const take = await Take.start("online");
    try {
      await take.hold(1000);
      // Online and offline, in the model chip.
      const chip = 'button[title="Change which model answers as this AI"]';
      const chipText = await (await $(chip)).getText();
      console.log(`online: chip "${chipText}"`);
      if (!/Online and Offline/i.test(chipText)) {
        await take.click(chip);
        await browser.waitUntil(async () => tag("div.fixed.z-30 div", "Auto - Online and Offline", "online-offline"), { timeout: 15_000, timeoutMsg: "no Auto - Online and Offline option (plan?)" });
        await take.hold(900);
        await take.still("01-chip-menu");
        await take.click('[data-shoot="online-offline"]');
        await take.hold(900);
      }
      await take.still("02-online-offline");
      await take.typeEditable('[data-testid="chat-input"]', QUESTION, 35);
      await take.hold(500);
      await take.click('[data-testid="chat-send"]');
      const t0 = Date.now();
      await browser.waitUntil(async () => (await lastReply())?.state === "done", { timeout: 300_000, interval: 500, timeoutMsg: "the reply never finished" });
      console.log(`online: reply done in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
      await take.hold(1200);
      await take.still("03-reply");
      // Where it came from: the reply's Model details.
      await browser.waitUntil(async () => tag("[data-shoot-reply] button", "Model", "model"), { timeout: 10_000 });
      await take.click('[data-shoot="model"]');
      await take.hold(2500);
      await take.still("04-model-details");
      const details = await browser.execute(() => (document.querySelector("[data-shoot-reply]") as HTMLElement).innerText.split("\n").filter((l) => /routing|online|·/i.test(l)).slice(0, 6));
      console.log(`online: details ${JSON.stringify(details)}`);
      await take.hold(2000);
    } finally {
      await take.stop();
    }
  });
});
