// Flow 1b: with the installed models the app goes straight to the chat, the
// composer is ready, and the first local reply arrives. Runs with
// YOAI_E2E_WITH_MODELS=1 (npm run e2e:models): the scratch profile reads
// the person's real models folder - this spec never downloads or deletes.
import { shot, waitForChat, ask, waitForReplyDone, invoke } from "./_helpers";
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// YOAI_E2E_MIN_CONVERSATIONS (move-live.sh): the conversations earlier
// launches in this scratch home wrote, which every AI's list must hold.
const MIN_CONVERSATIONS = Number(process.env.YOAI_E2E_MIN_CONVERSATIONS ?? 0);

async function conversationCount(): Promise<number> {
  const path = await invoke<string>("profile_store_path", { name: "ai-data.json" });
  const agents = await browser.execute(async (p: string) => {
    const t = (window as any).__TAURI__.core;
    const rid = await t.invoke("plugin:store|load", { path: p, autoSave: false });
    const got = await t.invoke("plugin:store|get", { rid, key: "custom-ais" });
    const list = Array.isArray(got) ? got[0] : got;
    return (Array.isArray(list) ? list : []).map((a: any) => a.agentPubKey ?? "").filter((k: string) => !!k);
  }, path) as string[];
  let n = 0;
  for (const agentKey of agents) {
    n += (await invoke<unknown[]>("get_conversations", { agentKey, maxAgeSecs: null, caller: "e2e count" })).length;
  }
  return n;
}

// YOAI_E2E_WAIT_LOG (move-live.sh and the like): a pattern the app's log
// must say during this launch before the spec ends. The move's pass waits
// while the person uses their conversations, so it finishes after the chat,
// and a launch that ended with the chat would close the app in the middle.
const WAIT_LOG = process.env.YOAI_E2E_WAIT_LOG ?? "";
const LOG_DIR = path.join(process.env.YOAI_E2E_HOME ?? path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../../profile"), ".local/share/com.solar.yourowai/logs");
function logSaid(pattern: string): number {
  const re = new RegExp(pattern);
  let n = 0;
  for (const f of fs.existsSync(LOG_DIR) ? fs.readdirSync(LOG_DIR) : []) {
    if (!f.endsWith(".log")) continue;
    for (const line of fs.readFileSync(path.join(LOG_DIR, f), "utf8").replace(/\x1b\[[0-9;]*m/g, "").split("\n")) if (re.test(line)) n++;
  }
  return n;
}
let saidBefore = 0;

describe("chat with installed models", () => {
  before(() => { if (WAIT_LOG) saidBefore = logSaid(WAIT_LOG); });

  it("lands on the chat with the composer ready", async () => {
    await waitForChat();
    await shot("10-chat-ready");
    expect(await browser.execute(() => location.pathname)).toBe("/chat/");
  });

  it("lists every conversation the earlier launches wrote", async function () {
    if (!MIN_CONVERSATIONS) return this.skip();
    let n = 0;
    await browser.waitUntil(async () => { n = await conversationCount(); return n >= MIN_CONVERSATIONS; }, {
      timeout: 120_000, interval: 5_000, timeoutMsg: `the AIs list fewer conversations than earlier launches wrote`,
    }).catch(() => undefined);
    console.log(`e2e: ${n} conversation(s) listed, ${MIN_CONVERSATIONS} expected`);
    expect(n).toBeGreaterThanOrEqual(MIN_CONVERSATIONS);
  });

  it("answers a short question with a local model", async () => {
    // An ordinary question. "Reply with exactly the word: ready" was the
    // question until 0.9.1: the 2B test model answers a bare "say only X"
    // with a bracketed outline of how it would answer in 10 to 30 of 100
    // tries, with or without the warm-up (measured against the engine,
    // 2026-11-13), and never does that for a question like this one.
    await ask("What colour is the sky on a clear day?");
    await shot("11-sent");
    // A model may need to load first (cold start on this card): up to three
    // minutes. The ASSISTANT's finished message must carry the word - never
    // the page as a whole, which also holds the question.
    const r = await waitForReplyDone();
    await shot("12-replied");
    expect(r.text.toLowerCase()).toContain("blue");
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

  (WAIT_LOG ? it : it.skip)("the app's log says what this launch waits for", async function () {
    this.timeout(1_500_000);
    await browser.waitUntil(async () => logSaid(WAIT_LOG) > saidBefore, {
      timeout: 1_200_000, interval: 5000, timeoutMsg: `the app's log never said /${WAIT_LOG}/ in this launch`,
    });
  });
});
