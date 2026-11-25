// Who can use a conversation, on the built app: by default every AI of the
// person's finds it; "Only <one AI>", set in the conversation's record,
// keeps it from the others at once, with nothing read again; "All your AIs"
// gives it back.
import { waitForChat, ask, waitForReplyDone, invoke, shot } from "./_helpers";

const BASE = "http://127.0.0.1:11435/internal";
const PLATE = `KLM-${String(Date.now()).slice(-4)}`;
const QUESTION = "What is the number plate of our delivery van?";

type Ai = { id: string; name: string; agent: string; founding: string };
let asked: Ai;
let other: Ai;
let started = 0;

const textOf = async (sel: string) => (await browser.execute((s) => (document.querySelector(s) as HTMLElement | null)?.innerText ?? "", sel)) as string;
const press = async (sel: string) => {
  const el = await $(sel);
  await el.waitForExist({ timeout: 30_000 });
  await browser.execute((e) => (e as HTMLElement).click(), el);
};

/** What the builder would hand this AI for the question. */
async function givenTo(ai: Ai): Promise<string> {
  const res = await fetch(`${BASE}/memory-assemble`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ ai: ai.id, question: QUESTION, surface: "chat", windowTokens: 16384 }) });
  const { memory } = await res.json();
  return `${memory?.systemPart ?? ""}\n${memory?.beforeQuestion ?? ""}`;
}

async function openTheRecord() {
  await browser.url("tauri://localhost/chat/");
  await (await $('[data-testid="chat-input"]')).waitForDisplayed({ timeout: 60_000 });
  await browser.execute((e) => (e as HTMLElement).click(), await $('button[title^="Conversations"]'));
  await press('[data-testid="conversation-item"] [data-testid="conversation-record"]');
  await browser.waitUntil(async () => (await browser.execute(() => location.pathname)) === "/memory/", { timeout: 30_000, timeoutMsg: "the record did not open" });
  await (await $('[data-testid="record-conversation"][data-open="1"] [data-testid="conversation-use"]')).waitForExist({ timeout: 120_000 });
}

describe("who can use a conversation", () => {
  it("every AI finds a conversation by default", async function () {
    this.timeout(600_000);
    await waitForChat();
    await ask(`The number plate of our delivery van is ${PLATE}. Reply with one short sentence.`);
    await waitForReplyDone();
    const found = (await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const store = await t.store.load(await t.core.invoke("profile_store_path", { name: "ai-data.json" }));
      const ais = (((await store.get("custom-ais")) || []) as { id: string; name: string; agentPubKey?: string; foundingKey?: string; status?: string }[]).filter((a) => a.agentPubKey && a.status !== "archived");
      const turn = (((window as any).__yoaiTurnsFull?.() ?? []) as { agentKey: string; conversationHash: string }[]).slice(-1)[0];
      const list = ais.map((a) => ({ id: a.id, name: a.name, agent: a.agentPubKey!, founding: a.foundingKey || a.agentPubKey! }));
      return { list, askedKey: turn?.agentKey ?? "", hash: turn?.conversationHash ?? "" };
    })) as { list: Ai[]; askedKey: string; hash: string };
    asked = found.list.find((a) => a.agent === found.askedKey) ?? found.list[0];
    other = found.list.find((a) => a.id !== asked.id)!;
    expect(other).toBeTruthy();
    await browser.waitUntil(async () => {
      const list = await invoke<{ hash: string; started_at: number }[]>("get_conversations_cached", { agentKey: asked.agent });
      started = list.find((c) => c.hash === found.hash)?.started_at ?? list[0]?.started_at ?? 0;
      return started > 0;
    }, { timeout: 60_000, timeoutMsg: "the conversation is not in the list" });
    // The other AI is given the exchange once the index holds it.
    await browser.waitUntil(async () => (await givenTo(other)).includes(PLATE), { timeout: 240_000, interval: 5_000, timeoutMsg: "the other AI was never given the exchange" });
  });

  it("Only one AI, set in the conversation's record, keeps it from the others at once", async function () {
    this.timeout(600_000);
    await openTheRecord();
    expect(await textOf('[data-testid="conversation-use-words"]')).toBe("All your AIs can use this conversation.");
    await press('[data-testid="conversation-use-change"]');
    await shot("memory-audience-choices");
    await press(`[data-testid="conversation-use-to"][data-to="${asked.founding}"]`);
    await browser.waitUntil(async () => (await textOf('[data-testid="conversation-use-words"]')) === `Only ${asked.name} can use this conversation.`, { timeout: 30_000, timeoutMsg: "the record did not say who can use the conversation" });
    await shot("memory-audience-one-ai");
    const use = await invoke<{ audience?: { kind: string; ai?: string } }>("memory_conversation_use", { startedAt: started });
    expect(use.audience).toEqual({ kind: "ai", ai: asked.founding });
    // The other AI is no longer given it; the one it is for still is.
    expect(await givenTo(other)).not.toContain(PLATE);
    expect(await givenTo(asked)).toContain(PLATE);
    // Memory lists it under "Not used", in words.
    await browser.url("tauri://localhost/your-memory/#recently");
    const row = await $('[data-testid="not-used-row"]');
    await row.waitForDisplayed({ timeout: 60_000 });
    expect((await browser.execute((e) => (e as HTMLElement).innerText, row)) as string).toContain("Only one AI may use it.");
  });

  it("All your AIs gives it back", async function () {
    this.timeout(600_000);
    await openTheRecord();
    await press('[data-testid="conversation-use-change"]');
    await press('[data-testid="conversation-use-to"][data-to="all"]');
    await browser.waitUntil(async () => (await textOf('[data-testid="conversation-use-words"]')) === "All your AIs can use this conversation.", { timeout: 30_000, timeoutMsg: "the record did not go back to all the AIs" });
    expect(await givenTo(other)).toContain(PLATE);
    expect((await invoke<unknown[]>("memory_conversations_not_used")).length).toBe(0);
  });
});
