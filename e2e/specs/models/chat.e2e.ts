// Flow 1b (planning/UI_AUTOMATION.md): with the installed models the app
// goes straight to the chat, the composer is ready, and the first local
// reply arrives. Runs with YOAI_E2E_WITH_MODELS=1 (npm run e2e:models):
// the scratch profile reads the person's real models folder - this spec
// never downloads or deletes anything.
import { resolve } from "node:path";
import { SHOTS } from "../../wdio.conf";

const shot = (name: string) => browser.saveScreenshot(resolve(SHOTS, `${name}.png`));

// The composer is a contenteditable; the embedded driver's key events do not
// reach its input handler, so type the way a paste would: set the text and
// fire a real InputEvent, which Qwik's onInput$ reads.
async function typeInto(testId: string, text: string) {
  await browser.execute((id: string, t: string) => {
    const el = document.querySelector(`[data-testid="${id}"]`) as HTMLElement;
    el.focus();
    el.textContent = t;
    el.dispatchEvent(new InputEvent("input", { bubbles: true, inputType: "insertText", data: t }));
  }, testId, text);
}

describe("chat with installed models", () => {
  it("lands on the chat with the composer ready", async () => {
    await browser.waitUntil(async () => (await browser.execute(() => location.pathname)) === "/chat/", { timeout: 60_000 });
    const input = await $('[data-testid="chat-input"]');
    await input.waitForDisplayed({ timeout: 60_000 });
    await shot("10-chat-ready");
    // No welcome: the models folder had models.
    expect(await browser.execute(() => location.pathname)).toBe("/chat/");
  });

  it("answers a short question with a local model", async () => {
    await typeInto("chat-input", "Reply with exactly the word: ready");
    const send = await $('[data-testid="chat-send"]');
    await send.waitForEnabled({ timeout: 10_000 });
    await send.click();
    await shot("11-sent");
    // A model may need to load first (cold start on this card): give the
    // reply up to three minutes. The ASSISTANT's finished message must carry
    // the word - never the page as a whole, which also holds the question.
    const reply = async () =>
      browser.execute(() => {
        const m = [...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined;
        return m ? { state: m.dataset.state, text: m.innerText } : null;
      });
    await browser.waitUntil(async () => (await reply())?.state === "done", { timeout: 180_000, timeoutMsg: "the assistant never finished within 3 minutes" });
    const r = await reply();
    await shot("12-replied");
    expect((r?.text || "").toLowerCase()).toContain("ready");
  });
});
