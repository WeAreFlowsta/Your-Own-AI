// Memory's search, on the built app with the made-up bakery history:
// a first message on an empty profile carries no search wording; the
// history is read into the index; the planted answers are found; a typed
// question is sent with what memory found ahead of it and nothing of that
// is stored in the conversation; the reply is stored with what it was
// given; the line under it opens the panel; a planted secret never
// reaches a prompt; a deleted source can no longer be shown.
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { waitForChat, ask, waitForReplyDone, invoke, shot } from "./_helpers";

const here = path.dirname(fileURLToPath(import.meta.url));
const fixture = path.resolve(here, "../../fixtures/memory");
const BASE = "http://127.0.0.1:11435/internal";
const history = JSON.parse(fs.readFileSync(path.join(fixture, "history.json"), "utf8")) as { conversations: { id: string; started_at: string | number; messages: unknown[] }[] };
const secrets = (JSON.parse(fs.readFileSync(path.join(fixture, "secrets.json"), "utf8")).secrets as { text: string }[]).map((s) => s.text);

type Q = { n: number; question: string; trap: boolean; where: { conversation: string; from: number; to: number }[] };

/** The made-up set, read the way the harness reads a set. */
function questions(): Q[] {
  const text = fs.readFileSync(path.join(fixture, "questions.md"), "utf8").replace(/\r\n/g, "\n");
  return [...text.matchAll(/^\*\*(\d+)\. ([A-F])\. (.+?)\*\*\n((?:- .*\n?)+)/gm)].map((m) => ({
    n: Number(m[1]),
    question: m[3],
    trap: m[2] === "F",
    where: [...m[4].matchAll(/(conv-[0-9a-z]+) seq (\d+)(?:-(\d+))?/g)].map((w) => ({ conversation: w[1], from: Number(w[2]), to: Number(w[3] ?? w[2]) })),
  }));
}

const micros = (when: string | number) => (typeof when === "number" ? when : Date.parse(when) * 1000);
const startOf = new Map(history.conversations.map((c) => [c.id, micros(c.started_at)]));

async function dev(method: "GET" | "POST", route: string, query: Record<string, string> = {}, body?: unknown): Promise<any> {
  const q = new URLSearchParams(query).toString();
  const res = await fetch(`${BASE}/${route}${q ? `?${q}` : ""}`, {
    method,
    headers: body === undefined ? undefined : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const answer = await res.json().catch(() => ({ error: `HTTP ${res.status}` }));
  if (!res.ok || answer?.error) throw new Error(`${route}: ${JSON.stringify(answer?.error ?? res.status)}`);
  return answer;
}

const lastUserText = (request: { messages: { role: string; content: unknown }[] }) => {
  const last = [...request.messages].reverse().find((m) => m.role === "user");
  return typeof last?.content === "string" ? last.content : JSON.stringify(last?.content ?? "");
};
const systemText = (request: { messages: { role: string; content: unknown }[] }) => request.messages.filter((m) => m.role === "system").map((m) => String(m.content)).join("\n");

async function newChat() {
  await waitForChat();
  if (await browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length > 0)) {
    await (await $('[data-testid="new-chat"]')).click();
    await browser.waitUntil(async () => browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length === 0), { timeout: 15_000 });
  }
}

/** The open conversation's entries as recorded. */
async function recorded(): Promise<{ role: string; content: string; given?: any }[]> {
  const turn = ((await browser.execute(() => (window as any).__yoaiTurnsFull?.() ?? [])) as { agentKey: string; conversationHash: string }[]).slice(-1)[0];
  return invoke("get_conversation_transcript", { agentKey: turn.agentKey, conversationHash: turn.conversationHash });
}

let ai = { id: "", name: "", agent: "" };
const DELIVERY = "What is the free delivery threshold?";

describe("memory's search, on the made-up history", () => {
  it("a first message on an empty profile carries no search wording", async () => {
    await waitForChat();
    await ask("What colour is the sky on a clear day?");
    await waitForReplyDone();
    const { request } = await dev("GET", "last-chat-request");
    expect(lastUserText(request)).toBe("What colour is the sky on a clear day?");
    expect(JSON.stringify(request)).not.toContain("From memory");
    expect(await $('[data-testid="given-line"]').isExisting()).toBe(false);
    const found = await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const p = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(p);
      const ais = ((await store.get("custom-ais")) || []) as { id?: string; name?: string; agentPubKey?: string }[];
      const asked = (((window as any).__yoaiTurnsFull?.() ?? []) as { agentKey: string }[]).slice(-1)[0]?.agentKey ?? "";
      const a = ais.find((x) => x.agentPubKey === asked) ?? ais[0];
      return a ? { id: a.id ?? "", name: a.name ?? "", agent: a.agentPubKey ?? "" } : null;
    });
    expect(found?.agent).toBeTruthy();
    ai = found!;
  });

  it("reads the history into the index, newest first, and says how far it is", async () => {
    const model = (await invoke<string | null>("get_current_model")) ?? "fixture";
    const loaded = await dev("POST", "memory-fixture-load", { path: path.join(fixture, "history.json"), all: ai.id, model });
    console.log(`e2e: history loaded: ${loaded.conversations} conversation(s), ${loaded.messages} message(s)`);
    expect(loaded.messages).toBe(66);
    await invoke("memory_read_now", { on: true });
    let status: any = {};
    const started = Date.now();
    await browser.waitUntil(async () => {
      status = await invoke("memory_reading_status");
      return status.total >= 66 && status.read >= status.total;
    }, { timeout: 900_000, interval: 5_000, timeoutMsg: "the history was not read into the index within 15 minutes" });
    console.log(`e2e: read ${status.read} of ${status.total} messages, ${status.passages} passage(s), in ${Math.round((Date.now() - started) / 1000)} s; ${status.shortened} shortened`);
    expect(status.waiting).toBe(false);
    expect(status.passages).toBeGreaterThanOrEqual(66);
  });

  it("finds the planted answers among the first twelve messages", async () => {
    const answerable = questions().filter((q) => !q.trap);
    let first3 = 0;
    let first12 = 0;
    const missed: number[] = [];
    for (const q of answerable) {
      const r = await dev("GET", "memory-search", { q: q.question });
      expect(r.read).toBe(r.total);
      const rank = (r.found as { started_at: number; seq: number }[]).findIndex((f) => q.where.some((w) => f.started_at === startOf.get(w.conversation) && f.seq >= w.from - 2 && f.seq <= w.to + 2));
      if (rank >= 0 && rank < 3) first3++;
      if (rank >= 0) first12++;
      else missed.push(q.n);
    }
    console.log(`e2e: of ${answerable.length} planted answers, ${first3} in the first 3 and ${first12} in the first 12; missed: ${missed.join(", ") || "none"}`);
    expect(first12).toBeGreaterThanOrEqual(answerable.length - 2);
  });

  it("no planted secret reaches a prompt, for any question", async () => {
    for (const q of questions()) {
      const { memory } = await dev("POST", "memory-assemble", {}, { ai: ai.id, question: q.question, surface: "chat", windowTokens: 16384 });
      const sent = `${memory.systemPart}\n${memory.beforeQuestion ?? ""}`;
      for (const s of secrets) expect(sent).not.toContain(s);
    }
    // The question that is about the planted key finds its exchange with the key hidden.
    const { memory } = await dev("POST", "memory-assemble", {}, { ai: ai.id, question: "What is the test key for the payment provider?", surface: "chat", windowTokens: 16384 });
    console.log(`e2e: hidden in that block: ${memory.given?.masked}`);
    expect(memory.beforeQuestion).not.toContain(secrets[0]);
  });

  it("sends a typed question with what memory found ahead of it, and stores the question as typed", async () => {
    await newChat();
    await ask(DELIVERY);
    const reply = await waitForReplyDone(600_000);
    await shot("memory-search-answered");
    const { request } = await dev("GET", "last-chat-request");
    const sent = lastUserText(request);
    expect(sent.startsWith("[From memory. A search of all their conversations was run for this question. It returned the ")).toBe(true);
    expect(sent.endsWith(`]\n\n${DELIVERY}`)).toBe(true);
    expect(sent).toContain("Rule for questions about their past work: answer only from a line you can quote");
    expect(sent).toContain("$60");
    expect(sent).not.toContain("[User question]");
    // The system prompt no longer carries retrieved lines: it is the same every turn.
    expect(systemText(request)).not.toContain("From earlier conversations with this person");
    expect(systemText(request)).toMatch(/Today is \d{4}-\d{2}-\d{2}\./);
    // The bubble and the record keep the question as typed.
    const bubble = await browser.execute(() => ([...document.querySelectorAll('[data-testid="chat-message"][data-role="user"]')].pop() as HTMLElement).innerText);
    expect(bubble).toContain(DELIVERY);
    expect(bubble).not.toContain("From memory");
    let entries: { role: string; content: string; given?: any }[] = [];
    await browser.waitUntil(async () => { entries = await recorded(); return entries.filter((e) => e.role === "assistant").length >= 1; }, { timeout: 60_000, interval: 2_000, timeoutMsg: "the reply was not recorded" });
    expect(entries.find((e) => e.role === "user")!.content).toBe(DELIVERY);
    // The reply is recorded with what it was given: a report and pointers, no text.
    const given = entries.find((e) => e.role === "assistant")!.given;
    console.log(`e2e: the reply: ${JSON.stringify(reply.text.slice(0, 240))}`);
    console.log(`e2e: given: ${JSON.stringify({ ...given, exchanges: given?.exchanges?.length })}`);
    expect(given.search.ran).toBe(true);
    expect(given.search.given).toBeGreaterThanOrEqual(1);
    expect(given.exchanges.length).toBe(given.search.given - (given.lines?.length ?? 0));
    expect(given.fingerprint).toMatch(/^[0-9a-f]{64}$/);
    expect(given.block).toMatch(/^[0-9a-f]{64}$/);
    expect(JSON.stringify(given)).not.toContain("delivery");
    const hit = given.exchanges[0].hit;
    expect([...startOf.values()]).toContain(hit.c);
    // The model answers from what it was given.
    expect(reply.text).toContain("60");
  });

  it("the line under the reply opens exactly what was given", async () => {
    const line = await $('[data-testid="given-line"]');
    const hasLine = await line.isExisting();
    console.log(`e2e: line under the reply: ${hasLine ? await line.getText() : "(none: the reply quoted nothing)"}`);
    // With a line: Show. Without (the reply quoted nothing): the way in is in Model details.
    if (hasLine && (await $('[data-testid="given-show"]').isExisting())) {
      await (await $('[data-testid="given-show"]')).click();
    } else {
      const lastReplyEl = (await $$('[data-testid="chat-message"][data-role="assistant"]')).slice(-1)[0];
      await (await lastReplyEl.$("button*=Model")).click();
      await (await $('[data-testid="given-open"]')).click();
    }
    const checked = await $('[data-testid="given-checked"]');
    await checked.waitForDisplayed({ timeout: 30_000 });
    await shot("memory-given-panel");
    expect(await checked.getAttribute("data-checked")).toBe("exact");
    expect(await checked.getText()).toContain("This is exactly what");
    const passages = await $$('[data-testid="given-passage"]');
    expect(passages.length).toBeGreaterThanOrEqual(1);
    const text = await browser.execute(() => (document.querySelector('[data-testid="given-panel"]') as HTMLElement).innerText);
    expect(text).toContain("$60");
    expect(text).toContain("Also given");
    for (const s of secrets) expect(text).not.toContain(s);
  });

  it("a short follow-up is searched together with the question before it", async () => {
    await ask("and when was that?");
    const reply = await waitForReplyDone(600_000);
    console.log(`e2e: the follow-up's reply: ${JSON.stringify(reply.text.slice(0, 240))}`);
    let entries: { role: string; given?: any }[] = [];
    await browser.waitUntil(async () => { entries = await recorded(); return entries.filter((e) => e.role === "assistant").length >= 2; }, { timeout: 60_000, interval: 2_000, timeoutMsg: "the follow-up's reply was not recorded" });
    const given = entries.filter((e) => e.role === "assistant")[1].given;
    expect(given.search.follow_up).toBe(true);
    // The open conversation's own messages are not "found": they are already sent.
    const { request } = await dev("GET", "last-chat-request");
    const open = lastUserText(request);
    expect(open).toContain("March 9");
    expect(open.split(DELIVERY).length).toBe(1);
  });

  it("a trap is answered from a search that found nothing about it", async () => {
    await newChat();
    const trap = questions().find((q) => q.trap)!;
    await ask(trap.question);
    const reply = await waitForReplyDone(600_000);
    console.log(`e2e: the trap's reply: ${JSON.stringify(reply.text.slice(0, 300))}`);
    const { request } = await dev("GET", "last-chat-request");
    expect(lastUserText(request)).toContain("No conversation anywhere mentions: Rosedale.");
    const line = await $('[data-testid="given-line"]');
    if (await line.isExisting()) console.log(`e2e: line: ${await line.getText()}`);
  });

  it("a deleted source can no longer be shown, and the reply stays", async () => {
    // What the delivery reply was given, then its source conversation is deleted.
    const r = await dev("POST", "memory-assemble", {}, { ai: ai.id, question: DELIVERY, surface: "chat", windowTokens: 16384 });
    const given = r.memory.given;
    const before = await invoke<any>("memory_given_texts", { given });
    expect(before.checked).toBe("exact");
    const first = before.passages[0];
    await invoke("delete_conversation", { agentKey: ai.agent, conversationHash: first.conversationId });
    const after = await invoke<any>("memory_given_texts", { given });
    console.log(`e2e: after the delete: ${after.checked}, ${after.deleted} deleted, ${after.notHere} not here`);
    expect(after.checked).toBe("some-gone");
    expect(after.passages[0].state === "deleted" || after.passages[0].state === "not-here").toBe(true);
    // And the index no longer finds it.
    const again = await dev("GET", "memory-search", { q: DELIVERY });
    expect((again.found as { conversation: string }[]).some((f) => f.conversation === first.conversationId)).toBe(false);
  });
});
