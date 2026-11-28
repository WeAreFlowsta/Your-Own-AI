// How long a question about the past waits for its first word, on the
// profile as it is (YOAI_E2E_KEEP_PROFILE=1): a measuring run. Run once
// with the search part in the models folder and once without it (memory
// then answers "did not look"), and the difference is what memory adds.
// Each question is asked in a new conversation, after one warm-up question
// has loaded the chat model. Everything is given by path, outside the
// repository:
//   YOAI_E2E_SOURCES       [{ n, question }]: the questions
//   YOAI_E2E_FIRST_WORD_N  how many of them to ask (default 5)
//   YOAI_MEMORY_OUT        folder for first-word.json
// Nothing is asserted about the times: this is a measure.
import * as fs from "node:fs";
import * as path from "node:path";
import { waitForChat, ask, lastReply, invoke } from "../models/_helpers";

const FILE = process.env.YOAI_E2E_SOURCES ?? "";
const OUT = process.env.YOAI_MEMORY_OUT ?? "";
const N = Number(process.env.YOAI_E2E_FIRST_WORD_N ?? 5);
const LIMIT = Number(process.env.YOAI_E2E_TIMEOUT_MS) || 3_600_000;

type Q = { n: number; question: string };
type Row = { n: number; firstWordMs: number; doneMs: number; line: string | null; chars: number };

const replies = () => browser.execute(() => document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]').length) as Promise<number>;

async function newChat() {
  await browser.execute(() => (document.querySelector('[data-testid="new-chat"]') as HTMLElement | null)?.click());
  await browser.waitUntil(async () => (await browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length)) === 0, { timeout: 15_000 });
}

// Ask, and time the first text of the new reply and its end.
async function timed(question: string): Promise<{ firstWordMs: number; doneMs: number; chars: number }> {
  const before = await replies();
  const t0 = Date.now();
  await ask(question);
  let first = 0;
  let chars = 0;
  await browser.waitUntil(
    async () => {
      const n = await replies();
      const r = await lastReply();
      // "thinking" is the bubble before the model has written anything.
      if (n > before && r && (r.state === "streaming" || r.state === "done") && !first) first = Date.now() - t0;
      chars = r?.text.length ?? 0;
      return n > before && r?.state === "done";
    },
    { timeout: 900_000, interval: 200, timeoutMsg: "the reply never finished" },
  );
  return { firstWordMs: first, doneMs: Date.now() - t0, chars };
}

describe("the wait for the first word of an answer about the past", () => {
  it("writes the wait for each question", async function () {
    if (!FILE) this.skip();
    this.timeout(LIMIT);
    const questions = (JSON.parse(fs.readFileSync(FILE, "utf8")) as Q[]).slice(0, N);
    await waitForChat();
    await browser.waitUntil(async () => invoke<unknown[]>("memory_items").then(() => true, () => false), { timeout: 600_000, interval: 5_000, timeoutMsg: "memory did not answer within ten minutes" });
    const model = await invoke<string | null>("get_current_model").catch(() => null);
    const warm = await timed("Name one primary color and nothing else.");
    console.log(`e2e: FIRST WORD warm-up on ${model}: first word ${warm.firstWordMs} ms, done ${warm.doneMs} ms`);
    const rows: Row[] = [];
    for (const q of questions) {
      await newChat();
      const t = await timed(q.question);
      const line = await browser.execute(() => {
        const all = document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]');
        return all[all.length - 1]?.querySelector('[data-testid="given-line"]')?.getAttribute("data-kind") ?? null;
      });
      rows.push({ n: q.n, ...t, line });
      console.log(`e2e: FIRST WORD Q${q.n}: first word ${t.firstWordMs} ms, done ${t.doneMs} ms, ${t.chars} characters, line ${line ?? "none"}`);
      if (OUT) fs.writeFileSync(path.resolve(OUT, "first-word.json"), JSON.stringify({ model, warm, rows }, null, 1));
    }
    const sorted = rows.map((r) => r.firstWordMs).sort((a, b) => a - b);
    console.log(`e2e: FIRST WORD TOTAL: ${rows.length} questions on ${model}; first word: least ${sorted[0]} ms, middle ${sorted[Math.floor(sorted.length / 2)]} ms, most ${sorted[sorted.length - 1]} ms`);
    expect(rows.length).toBeGreaterThan(0);
  });
});
