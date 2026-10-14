// Flow 1b: with the installed models the app goes straight to the chat, the
// composer is ready, and the first local reply arrives. Runs with
// YOAI_E2E_WITH_MODELS=1 (npm run e2e:models): the scratch profile reads
// the person's real models folder - this spec never downloads or deletes.
import { shot, waitForChat, ask, waitForReplyDone, invoke } from "./_helpers";

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

  it("leaves one episode in the AI's memory when a conversation is left", async function () {
    this.timeout(600_000);
    await waitForChat();
    await (await $('[data-testid="new-chat"]')).click();
    await ask("Name one primary colour and nothing else.");
    await waitForReplyDone(300_000);
    await ask("And one more, nothing else.");
    await waitForReplyDone(300_000);
    const turns = (await browser.execute(() => (window as any).__yoaiTurnsFull?.() ?? [])) as { agentKey: string; conversationHash: string }[];
    const hash = turns[turns.length - 1]?.conversationHash ?? "";
    expect(hash.length).toBeGreaterThan(10);
    await (await $('[data-testid="new-chat"]')).click();
    const ai = await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const path = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(path);
      const ais = ((await store.get("custom-ais")) || []) as { agentPubKey?: string; foundingKey?: string }[];
      return ais[0] ? { agent: ais[0].agentPubKey ?? "", founding: ais[0].foundingKey ?? ais[0].agentPubKey ?? "" } : null;
    });
    expect(ai?.agent).toBeTruthy();
    let rows: { kind: string; text: string; provenance: string }[] = [];
    await browser.waitUntil(async () => {
      rows = await invoke<{ kind: string; text: string; provenance: string }[]>("list_memory_rows", { agentKey: ai!.agent, scope: `ai:${ai!.founding}` });
      return rows.some((r) => r.kind === "episode" && r.provenance === `distilled:chat:${hash}`);
    }, { timeout: 60_000, interval: 2000, timeoutMsg: "no episode row for the conversation that was left" });
    const ep = rows.find((r) => r.kind === "episode" && r.provenance === `distilled:chat:${hash}`)!;
    console.log(`e2e: episode ${JSON.stringify(ep.text.slice(0, 160))}`);
    expect(ep.text).toContain("Conversation: Name one primary colour");
    expect(ep.text).toContain("(2 questions)");
  });
});
