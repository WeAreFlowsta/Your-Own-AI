// Take: leave the chat, it keeps going. A long question is sent; while the reply streams the person opens Settings
// from the header menu; the activity tray's row says the AI is still answering; Open goes back to the reply.
// Kept shoot profile, models seeded.
//   YOAI_CAPTURE_QUESTION  the question (long enough to still be streaming after Settings opens)
import { appUrl } from "../wdio.conf";
import { Take } from "./_capture";

type Session = { agent_pub_key: string | null; display_name?: string | null; username?: string | null; web_username?: string | null };
// the identity a take must run as (its agent key, YOAI_CAPTURE_IDENTITY), so a take never records anyone's own
const DEMO = process.env.YOAI_CAPTURE_IDENTITY;
const session = () => browser.execute(async () => (window as any).__TAURI__.core.invoke("flowsta_session")) as Promise<Session>;
const QUESTION = process.env.YOAI_CAPTURE_QUESTION ??
  "Write a 1,000-word onboarding guide for a new project manager at a 20-person consulting firm.";
const reply = () =>
  browser.execute(() => {
    const m = [...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined;
    return m ? { state: m.getAttribute("data-state") ?? "", len: m.innerText.length } : null;
  });
const tag = (css: string, startsWith: string, as: string) =>
  browser.execute((c: string, t: string, a: string) => {
    const el = [...document.querySelectorAll(c)].find((e) => (e as HTMLElement).innerText.trim().startsWith(t));
    el?.setAttribute("data-shoot", a);
    return !!el;
  }, css, startsWith, as);

describe("capture", () => {
  it("keeps going", async () => {
    await browser.url(appUrl("/chat/"));
    await (await $('[data-testid="chat-input"]')).waitForDisplayed({ timeout: 60_000 });
    // the demo identity only, never anyone's own
    if (!DEMO) throw new Error("set YOAI_CAPTURE_IDENTITY to the demo identity's agent key");
    let s: Session = { agent_pub_key: null };
    await browser.waitUntil(async () => !!(s = await session().catch(() => s)).agent_pub_key, { timeout: 60_000, interval: 1_000, timeoutMsg: "not signed in" });
    console.log(`keeps-going: session ${s.display_name ?? "?"} (@${s.username ?? s.web_username ?? "?"}) ${s.agent_pub_key}`);
    if (s.agent_pub_key !== DEMO) throw new Error("not the demo identity: switch the Vault to it");
    if (await browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length > 0)) {
      await (await $('[data-testid="new-chat"]')).click().catch(() => undefined);
      await browser.pause(1500);
    }

    const take = await Take.start("keeps-going");
    try {
      await take.hold(800);
      await take.typeEditable('[data-testid="chat-input"]', QUESTION, 28);
      await take.hold(300);
      await take.click('[data-testid="chat-send"]');
      await browser.waitUntil(async () => { const r = await reply(); return !!r && r.state !== "done" && r.len > 400; }, { timeout: 180_000, interval: 250, timeoutMsg: "the reply never streamed" });
      await take.hold(1200);
      await take.still("01-streaming");
      // leave through the app's own menu (an in-app route change, as a person does it)
      await take.click('button[title^="Menu"]');
      await browser.waitUntil(async () => tag('[role="menuitem"]', "Settings", "settings"), { timeout: 10_000, timeoutMsg: "no Settings in the menu" });
      await take.hold(400);
      await take.click('[data-shoot="settings"]');
      await browser.waitUntil(async () => (await browser.execute(() => location.pathname)) === "/settings/", { timeout: 30_000 });
      const row = await $('[data-testid="turn-row"]');
      await row.waitForDisplayed({ timeout: 15_000 });
      console.log(`keeps-going: tray row (${await row.getAttribute("data-turn-state")}) ${JSON.stringify((await row.getText()).replace(/\s+/g, " "))}`);
      await take.hover('[data-testid="turn-row"]', 1200);
      await take.still("02-tray");
      await browser.waitUntil(async () => tag('[data-testid="turn-row"] button', "Open", "open"), { timeout: 5_000 });
      await take.click('[data-shoot="open"]');
      await browser.waitUntil(async () => (await browser.execute(() => location.pathname)) === "/chat/", { timeout: 30_000 });
      await take.hold(2500);
      await take.still("03-back");
      console.log(`keeps-going: on return ${JSON.stringify(await reply())}`);
      await take.hold(1500);
    } finally {
      await take.stop();
    }
  });
});
