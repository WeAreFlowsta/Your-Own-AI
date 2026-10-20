// Take: the memory beat. In the chat with the AI (Nova), an instruction said in passing is offered as a rule; Keep
// saves it as a dated rule every AI follows. Then Your Memory: the row of faces (You and each AI, each with its own
// memory) and the new rule at the top of yours. Runs on the kept shoot profile with the models seeded.
//   YOAI_CAPTURE_AI_NAME  (default Nova)   YOAI_CAPTURE_RULE  the instruction (default: action items first)
import { appUrl } from "../wdio.conf";
import { Take } from "./_capture";

const name = process.env.YOAI_CAPTURE_AI_NAME ?? "Nova";
const RULE = process.env.YOAI_CAPTURE_RULE ?? "From now on, put the action items first in every summary.";
const invoke = <T>(cmd: string, args: Record<string, unknown> = {}) =>
  browser.execute(async (c: string, a: Record<string, unknown>) => (window as any).__TAURI__.core.invoke(c, a), cmd, args) as Promise<T>;
const tag = (css: string, text: string, as: string) =>
  browser.execute((c: string, t: string, a: string) => {
    const el = [...document.querySelectorAll(c)].find((e) => (e as HTMLElement).innerText.trim() === t || e.getAttribute("title") === t);
    el?.setAttribute("data-shoot", a);
    return !!el;
  }, css, text, as);

describe("capture", () => {
  it("memory", async () => {
    await browser.url(appUrl("/chat/"));
    await (await $('[data-testid="chat-input"]')).waitForDisplayed({ timeout: 60_000 });
    // A fresh conversation, a model loaded, the records answering.
    if (await browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length > 0)) {
      await (await $('[data-testid="new-chat"]')).click().catch(() => undefined);
      await browser.pause(1500);
    }
    await browser.waitUntil(async () => !!(await invoke<string | null>("get_current_model")), { timeout: 240_000, interval: 2_000, timeoutMsg: "no model loaded" });
    await browser.waitUntil(async () => { try { await invoke("list_memory_rows", { agentKey: null, scope: "person" }); return true; } catch { return false; } }, { timeout: 180_000 });
    console.log(`memory: model ${await invoke<string>("get_current_model")}`);
    const gotIt = await $("button=Got it");
    if (await gotIt.isExisting()) { await gotIt.click(); await browser.pause(500); }

    const take = await Take.start("memory");
    try {
      await take.hold(1000);
      // Nova, in the selector.
      if (!(await (await $('[data-testid="ai-selector"]')).getText()).includes(name)) {
        await take.click('[data-testid="ai-selector"]');
        await browser.waitUntil(async () => (await tag('[data-testid="ai-option"]', name, "ai")) || (await tag('[data-testid="ai-option"] *', name, "ai")), { timeout: 10_000 });
        await take.click('[data-shoot="ai"]');
        await take.hold(700);
      }
      await take.still("01-chat");
      await take.typeEditable('[data-testid="chat-input"]', RULE);
      await take.hold(600);
      await take.click('[data-testid="chat-send"]');
      // The offer under the message: keep it as a rule.
      await (await $('[data-testid="rule-offer"]')).waitForDisplayed({ timeout: 30_000 });
      await take.hold(1200);
      await take.still("02-offer");
      await take.click('[data-testid="rule-offer-keep"]');
      await (await $('[data-testid="rule-kept"]')).waitForDisplayed({ timeout: 30_000 });
      await take.hold(1500);
      await take.still("03-kept");
      await browser.waitUntil(async () => browser.execute(() => {
        const m = [...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined;
        return m?.dataset.state === "done";
      }), { timeout: 120_000, interval: 500 }).catch(() => undefined);
      await take.hold(1500);
      await take.still("04-reply");

      // Your Memory: the faces, and the rule at the top of yours.
      await take.click('[data-testid="header-menu"]');
      await take.hold(400);
      await browser.waitUntil(async () => tag("a, button", "Your Memory", "memory"), { timeout: 10_000 });
      await take.click('[data-shoot="memory"]');
      await (await $('[data-testid="memory-owners"]')).waitForDisplayed({ timeout: 30_000 });
      await (await $(`[data-testid="memory-rule"]*=${RULE.slice(0, 24)}`)).waitForDisplayed({ timeout: 60_000 });
      await take.hold(1500);
      await take.still("05-your-memory");
      await take.hover('[data-testid="memory-owners"] [data-testid="memory-owner-you"]', 1200);
      for (const t of ["Nova", "Coder", "Analyst"]) {
        if (await tag('[data-testid="memory-owner-ai"]', `${t}'s own memory`, `face-${t}`)) await take.hover(`[data-shoot="face-${t}"]`, 700);
      }
      await take.hover('[data-testid="memory-rule"]', 2000);
      await take.still("06-rule");
    } finally {
      await take.stop();
    }
  });
});
