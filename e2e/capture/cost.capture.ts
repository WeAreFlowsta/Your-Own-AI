// Take: see what each answer cost. First (not recorded) a few everyday and harder questions in a new chat, so the
// routing log and this plan period's online use have something to show; then Settings > Routing, "Where your
// answers came from" opened: the last questions by where they were answered, and the online cost by kind of model.
// Kept shoot profile, signed in with the plan; the AI on Auto - Online and Offline.
//   YOAI_CAPTURE_SKIP_SEED=1  straight to the take (the questions were asked before)
import { appUrl } from "../wdio.conf";
import { Take } from "./_capture";

type Session = { agent_pub_key: string | null; display_name?: string | null; username?: string | null; web_username?: string | null };
// the identity a take must run as (its agent key, YOAI_CAPTURE_IDENTITY), so a take never records anyone's own
const DEMO = process.env.YOAI_CAPTURE_IDENTITY;
const session = () => browser.execute(async () => (window as any).__TAURI__.core.invoke("flowsta_session")) as Promise<Session>;
const QUESTIONS = [
  "What is 15% of 2,400?",
  "Give me three subject lines for a project kickoff email.",
  "Rewrite this to sound friendlier: Send me the report by Friday.",
  "Draft a short risk register for moving a 20-person firm to a new CRM, with likelihood, impact and an owner for each risk.",
  "Compare fixed-fee, hourly and retainer pricing for a small consulting firm, with the trade-offs of each.",
  "Turn these notes into a tidy agenda: budget review, hiring plan, Q3 targets, office move.",
  "Explain the difference between gross margin and net margin in two sentences.",
  "Write a one-paragraph summary of what a project charter should contain.",
];
const lastReply = () =>
  browser.execute(() => {
    const m = [...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined;
    return m ? { state: m.dataset.state ?? "", n: document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]').length } : null;
  });

async function ask(q: string) {
  const before = (await lastReply())?.n ?? 0;
  await browser.execute((t: string) => {
    const el = document.querySelector('[data-testid="chat-input"]') as HTMLElement;
    el.focus();
    el.textContent = t;
    el.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: t.slice(-1) }));
  }, q);
  await browser.pause(300);
  await (await $('[data-testid="chat-send"]')).click();
  await browser.waitUntil(async () => { const r = await lastReply(); return !!r && r.n > before && r.state === "done"; }, { timeout: 300_000, interval: 700, timeoutMsg: `no answer to: ${q}` });
}

describe("capture", () => {
  it("cost", async () => {
    await browser.url(appUrl("/chat/"));
    await (await $('[data-testid="chat-input"]')).waitForDisplayed({ timeout: 60_000 });
    // the demo identity only, never anyone's own
    if (!DEMO) throw new Error("set YOAI_CAPTURE_IDENTITY to the demo identity's agent key");
    let s: Session = { agent_pub_key: null };
    await browser.waitUntil(async () => !!(s = await session().catch(() => s)).agent_pub_key, { timeout: 60_000, interval: 1_000, timeoutMsg: "not signed in" });
    console.log(`cost: session ${s.display_name ?? "?"} (@${s.username ?? s.web_username ?? "?"}) ${s.agent_pub_key}`);
    if (s.agent_pub_key !== DEMO) throw new Error("not the demo identity: switch the Vault to it");
    if (!process.env.YOAI_CAPTURE_SKIP_SEED) {
      if (await browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length > 0)) {
        await (await $('[data-testid="new-chat"]')).click().catch(() => undefined);
        await browser.pause(1500);
      }
      for (const q of QUESTIONS) { const t0 = Date.now(); await ask(q); console.log(`cost: answered in ${((Date.now() - t0) / 1000).toFixed(1)} s: ${q}`); }
    }
    await browser.url(appUrl("/settings/"));
    const summary = await $('[data-testid="routing-breakdown"] summary');
    await summary.waitForExist({ timeout: 90_000, timeoutMsg: "no breakdown (no questions in the routing log?)" });
    await browser.pause(1200);
    // the routing share choices and the breakdown in the middle of the screen before the take starts
    await browser.execute(() => document.querySelector('[data-testid="routing-breakdown"]')!.scrollIntoView({ block: "center" }));
    await browser.pause(800);

    const take = await Take.start("cost");
    try {
      await take.hold(1000);
      await take.click('[data-testid="routing-breakdown"] summary');
      await take.hold(1500);
      await take.still("01-breakdown");
      const text = await browser.execute(() => (document.querySelector('[data-testid="routing-breakdown"]') as HTMLElement).innerText);
      console.log(`cost: ${JSON.stringify(text)}`);
      await take.hold(2500);
    } finally {
      await take.stop();
    }
  });
});
