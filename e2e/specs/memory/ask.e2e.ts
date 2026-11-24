// Asks a question set of the app's memory and saves every reply with the
// request that produced it, for the reading sheets of tools/memory-harness.mjs.
// Each question is asked twice: typed into the chat page in a new
// conversation ("chat"), and sent through the local port's assembly
// ("port"). Before every question the memory stores are put back as they
// were at the start, so no question is answered from an earlier one.
//
// Runs on the installed models, one spec in its own run:
//   node e2e/run.mjs models --spec e2e/specs/memory/ask.e2e.ts
//
// With nothing set it asks the made-up set in e2e/fixtures/memory of the
// made-up history. Everything else is given by path, outside the repository:
//   YOAI_MEMORY_OUT       folder for replies.jsonl (default: a temp folder)
//   YOAI_MEMORY_ASK       [{ n, question }] (memory-harness.mjs ask-list)
//   YOAI_MEMORY_HISTORY   a history file to load ("none" loads nothing)
//   YOAI_MEMORY_STORES    a stores file to seed (the facts, an older index)
//   YOAI_MEMORY_PATHS     "chat,port" (default), or one of them
//   YOAI_MEMORY_SETUP     the setup name the replies carry (default "today")
//   YOAI_MEMORY_READ      1: have the history read into memory's index and
//                         save what is found per question, before asking
//   YOAI_MEMORY_READ_MINUTES  how long the reading may take (default 30)
import * as fs from "node:fs";
import * as os from "node:os";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { waitForChat, ask, waitForReplyDone, invoke, shot } from "../models/_helpers";

const here = path.dirname(fileURLToPath(import.meta.url));
const fixture = path.resolve(here, "../../fixtures/memory");
const repo = path.resolve(here, "../../..");
const URL_BASE = process.env.YOAI_URL ?? "http://127.0.0.1:11435";
const OUT = path.resolve(process.env.YOAI_MEMORY_OUT ?? fs.mkdtempSync(path.join(os.tmpdir(), "memory-ask-")));
const HISTORY = process.env.YOAI_MEMORY_HISTORY ?? path.join(fixture, "history.json");
const STORES = process.env.YOAI_MEMORY_STORES ?? "";
const PATHS = (process.env.YOAI_MEMORY_PATHS ?? "chat,port").split(",").map((p) => p.trim()).filter(Boolean);
const SETUP = process.env.YOAI_MEMORY_SETUP ?? "today";
const REPLIES = path.join(OUT, "replies.jsonl");
// Have the history read into memory's index, and save what is found for
// each question, before asking (builds that have the index).
const READ = !!process.env.YOAI_MEMORY_READ;
const READ_MINUTES = Number(process.env.YOAI_MEMORY_READ_MINUTES ?? 30);
// A request to the loader carries about this many messages: at the pace
// records are written, well inside the time a request may wait.
const LOAD_MESSAGES = 600;
const SETTLE_MS = 4000;
// The history and the stores are in place (set by the step that loads them).
let ready = false;

type Asked = { n: number; question: string };

/** The made-up set's questions, read the way the harness reads a set. */
function fixtureQuestions(): Asked[] {
  const text = fs.readFileSync(path.join(fixture, "questions.md"), "utf8").replace(/\r\n/g, "\n");
  return [...text.matchAll(/^\*\*(\d+)\. [A-F]\. (.+?)\*\*$/gm)].map((m) => ({ n: Number(m[1]), question: m[2] }));
}

const QUESTIONS: Asked[] = process.env.YOAI_MEMORY_ASK ? JSON.parse(fs.readFileSync(process.env.YOAI_MEMORY_ASK, "utf8")) : fixtureQuestions();

async function dev(method: "GET" | "POST", route: string, query: Record<string, string> = {}, body?: unknown): Promise<any> {
  const q = new URLSearchParams(query).toString();
  const res = await fetch(`${URL_BASE}/internal/${route}${q ? `?${q}` : ""}`, {
    method,
    headers: body === undefined ? undefined : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const answer = await res.json().catch(() => ({ error: `HTTP ${res.status}` }));
  if (!res.ok || answer?.error) throw new Error(`${route}: ${JSON.stringify(answer?.error ?? res.status)}`);
  return answer;
}

function done(): Set<string> {
  if (!fs.existsSync(REPLIES)) return new Set();
  return new Set(fs.readFileSync(REPLIES, "utf8").split("\n").filter((l) => l.trim()).map((l) => { const r = JSON.parse(l); return `${r.n}|${r.setup}`; }));
}

function save(row: Record<string, unknown>) {
  fs.appendFileSync(REPLIES, JSON.stringify(row) + "\n");
}

let ai = { id: "", name: "", agent: "" };

describe("memory, asked a question set", () => {
  before(() => {
    const inside = path.relative(repo, OUT);
    if (!inside.startsWith("..") && !path.isAbsolute(inside)) throw new Error(`the output folder must be outside the repository: ${OUT}`);
    fs.mkdirSync(OUT, { recursive: true });
    console.log(`e2e: ${QUESTIONS.length} question(s), paths ${PATHS.join("+")}, replies to ${REPLIES}`);
  });

  it("has a model loaded and an AI that has answered once", async () => {
    await waitForChat();
    // The first reply of a new profile gives the AI its place in the
    // records, which the loader writes into.
    await ask("What colour is the sky on a clear day?");
    await waitForReplyDone();
    const found = await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const p = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(p);
      const ais = ((await store.get("custom-ais")) || []) as { id?: string; name?: string; agentPubKey?: string }[];
      const turns = ((window as any).__yoaiTurnsFull?.() ?? []) as { agentKey: string }[];
      const asked = turns[turns.length - 1]?.agentKey ?? "";
      const a = ais.find((x) => x.agentPubKey === asked) ?? ais[0];
      return a ? { id: a.id ?? "", name: a.name ?? "", agent: a.agentPubKey ?? "" } : null;
    });
    expect(found?.agent).toBeTruthy();
    ai = found!;
    console.log(`e2e: asking ${ai.name} (${ai.id})`);
  });

  it("holds the history and its memory stores", async () => {
    if (HISTORY !== "none") {
      const model = (await invoke<string | null>("get_current_model")) ?? "fixture";
      const started = Date.now();
      // A long history is written a part at a time, so no one request
      // waits longer than a request may; a part already there is skipped.
      const all = JSON.parse(fs.readFileSync(path.resolve(HISTORY), "utf8")) as { conversations: { messages?: unknown[] }[] };
      const total = { conversations: 0, messages: 0, skipped: 0 };
      const part = path.join(OUT, "history-part.json");
      let said = Date.now();
      for (let i = 0; i < all.conversations.length; ) {
        // Whole conversations, up to the message count (one long
        // conversation goes alone).
        let j = i;
        let messages = 0;
        do {
          messages += all.conversations[j].messages?.length ?? 0;
          j += 1;
        } while (j < all.conversations.length && messages + (all.conversations[j].messages?.length ?? 0) <= LOAD_MESSAGES);
        fs.writeFileSync(part, JSON.stringify({ ...all, conversations: all.conversations.slice(i, j) }));
        const loaded = await dev("POST", "memory-fixture-load", { path: part, all: ai.id, model });
        total.conversations += loaded.conversations;
        total.messages += loaded.messages;
        total.skipped += loaded.skipped;
        i = j;
        if (Date.now() - said > 120_000) {
          said = Date.now();
          console.log(`e2e: history: ${i} of ${all.conversations.length} conversation(s), ${total.messages} message(s) written, ${Math.round((Date.now() - started) / 1000)} s`);
        }
      }
      fs.rmSync(part, { force: true });
      console.log(`e2e: history: ${total.conversations} conversation(s), ${total.messages} message(s) written, ${total.skipped} already there, ${Math.round((Date.now() - started) / 1000)} s`);
    }
    // Every AI the stores file names stands for the AI being asked: a
    // profile AI that happens to share a name must not take its index.
    const named = STORES ? ((JSON.parse(fs.readFileSync(path.resolve(STORES), "utf8")).recall ?? []) as { ai: string }[]).map((r) => `${r.ai}:${ai.id}`).join(",") : "";
    const stores = STORES ? await dev("POST", "memory-stores", { do: "seed", path: path.resolve(STORES), all: ai.id, ...(named ? { map: named } : {}) }) : {};
    if (STORES) {
      const held = await invoke<unknown[]>("get_transcript_embeddings", { aiId: ai.id }).catch(() => []);
      console.log(`e2e: the asked AI's index of earlier turns holds ${held.length} entr(ies)`);
    }
    console.log(`e2e: memory stores: ${JSON.stringify(stores)}`);
    const kept = await dev("POST", "memory-stores", { do: "keep" });
    console.log(`e2e: ${kept.kept} store file(s) kept as the starting state`);
    ready = true;
  });

  it("has the history read into memory's index", async function () {
    if (!READ || !ready) return this.skip();
    await invoke("memory_read_now", { on: true });
    let status: any = {};
    let said = 0;
    const started = Date.now();
    await browser.waitUntil(async () => {
      status = await invoke("memory_reading_status");
      if (Date.now() - said > 300_000) {
        said = Date.now();
        console.log(`e2e: reading: ${status.read} of ${status.total} message(s), ${status.passages} passage(s), ${Math.round((Date.now() - started) / 60_000)} min${status.waiting ? `, waiting: ${status.why}` : ""}`);
      }
      // "All read" counts only once the reader has looked at the records
      // again after this step began: a count taken while the history was
      // still being written covers part of it.
      return status.total > 0 && status.read >= status.total && status.levelAt > started;
    }, { timeout: READ_MINUTES * 60_000, interval: 15_000, timeoutMsg: `the history was not read within ${READ_MINUTES} minutes` });
    const root = await invoke<string>("profile_store_path", { name: "memory-index.sqlite" });
    const size = fs.existsSync(root) ? fs.statSync(root).size : 0;
    const line = { read: status.read, total: status.total, passages: status.passages, shortened: status.shortened, seconds: Math.round((Date.now() - started) / 1000), index_bytes: size };
    console.log(`e2e: read: ${JSON.stringify(line)}`);
    fs.writeFileSync(path.join(OUT, "reading.json"), JSON.stringify(line, null, 1));
  });

  it("what memory finds for each question, with no model", async function () {
    if (!READ || !ready) return this.skip();
    const rows = [];
    for (const q of QUESTIONS) {
      const started = Date.now();
      const r = await dev("GET", "memory-search", { q: q.question });
      rows.push({ n: q.n, question: q.question, ms: Date.now() - started, read: r.read, total: r.total, found: r.found });
    }
    fs.writeFileSync(path.join(OUT, "found.json"), JSON.stringify(rows));
    console.log(`e2e: found for ${rows.length} question(s); the slowest search took ${Math.max(...rows.map((r) => r.ms))} ms`);
  });

  for (const q of QUESTIONS) {
    it(`question ${q.n}`, async function () {
      // Without the history and the stores in place an answer measures nothing.
      if (!ready) return this.skip();
      const have = done();
      if (PATHS.includes("chat") && !have.has(`${q.n}|${SETUP}-chat`)) {
        await browser.pause(SETTLE_MS);
        await dev("POST", "memory-stores", { do: "back" });
        await waitForChat();
        const started = Date.now();
        await ask(q.question);
        const r = await waitForReplyDone(1_200_000);
        const { request } = await dev("GET", "last-chat-request");
        const last = [...(request?.messages ?? [])].reverse().find((m: { role: string }) => m.role === "user");
        const lastText = typeof last?.content === "string" ? last.content : JSON.stringify(last?.content ?? "");
        // The request kept must be this question's: a turn that never
        // reached the model would leave the one before it.
        expect(lastText).toContain(q.question.slice(0, 40));
        // The reply's own words: the bubble's body, without the AI's name
        // and the buttons that stand around it.
        const body = await browser.execute(() => {
          const m = [...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined;
          const b = m?.querySelector(".markdown-content:not(.thinking-markdown)") as HTMLElement | null;
          return b ? b.innerText : null;
        });
        save({ n: q.n, setup: `${SETUP}-chat`, question: q.question, answer: (body ?? r.text).trim(), ms: Date.now() - started, request });
      }
      if (PATHS.includes("port") && !have.has(`${q.n}|${SETUP}-port`)) {
        // What the chat turn writes after its reply (the turn's own entry
        // in an index, a fact taken from it) lands first, then the stores
        // go back, so the next ask never finds this one.
        await browser.pause(SETTLE_MS);
        await dev("POST", "memory-stores", { do: "back" });
        const started = Date.now();
        const r = await dev("POST", "memory-ask", {}, { ai: ai.id, question: q.question });
        save({ n: q.n, setup: `${SETUP}-port`, question: q.question, answer: r.reply?.content ?? "", reasoning: r.reply?.reasoning_content ?? "", ms: Date.now() - started, request: r.request });
      }
    });
  }

  after(async () => {
    await shot("memory-ask-end").catch(() => undefined);
    console.log(`e2e: ${done().size} repl(ies) in ${REPLIES}`);
  });
});
