// Flow 1b: with the installed models the app goes straight to the chat, the
// composer is ready, and the first local reply arrives. Runs with
// YOAI_E2E_WITH_MODELS=1 (npm run e2e:models): the scratch profile reads
// the person's real models folder - this spec never downloads or deletes.
import { shot, waitForChat, ask, waitForReplyDone } from "./_helpers";

describe("chat with installed models", () => {
  it("lands on the chat with the composer ready", async () => {
    await waitForChat();
    await shot("10-chat-ready");
    expect(await browser.execute(() => location.pathname)).toBe("/chat/");
  });

  it("answers a short question with a local model", async () => {
    await ask("Reply with exactly the word: ready");
    await shot("11-sent");
    // A model may need to load first (cold start on this card): up to three
    // minutes. The ASSISTANT's finished message must carry the word - never
    // the page as a whole, which also holds the question.
    const r = await waitForReplyDone();
    await shot("12-replied");
    expect(r.text.toLowerCase()).toContain("ready");
  });

  it("answers a real short question, not with a greeting", async () => {
    // A persona template once held a quoted example reply ("Doing well - ready
    // when you are..") and the small model parroted it for short questions.
    await ask("In one sentence, what is the capital of France?");
    const r = await waitForReplyDone();
    console.log(`e2e: short answer ${JSON.stringify(r.text.slice(0, 160))}`);
    expect(r.text.toLowerCase()).toContain("paris");
  });
});
