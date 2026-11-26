// Questions whose answers are only in the library, on the built app, on a
// profile that already holds the library (YOAI_E2E_KEEP_PROFILE=1 after
// library-large). A measurement, not a gate: it runs only when
// YOAI_E2E_DOC_QUESTIONS names a JSON file of questions, which is kept
// outside the repository. For each question it records the reply, which
// documents had passages given, and whether the reply holds the answer
// (and, for something that changed over time, the stale value).
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { waitForChat, ask, waitForReplyDone } from "./_helpers";

type Q = { n: number; type: string; question: string; keys: string[][]; source: string; stale: string[] };
const FILE = process.env.YOAI_E2E_DOC_QUESTIONS ?? "";
const OUT = process.env.YOAI_MEMORY_OUT ?? "";
const ONLY = (process.env.YOAI_E2E_DOC_ONLY ?? "").split(",").filter(Boolean).map(Number);

const lastAssistant = '(//*[@data-testid="chat-message"][@data-role="assistant"])[last()]';

/** Where the search itself puts the document that holds the answer: its
 *  best passage's place among the first 300, and the place of the first
 *  passage holding the answer's words, with their scores. Search only; no
 *  model reply. Runs when YOAI_E2E_DOC_RANKS is set. */
describe("where the search ranks the answering document", () => {
  it("prints the place of the source document for each question", async function () {
    if (!FILE || !process.env.YOAI_E2E_DOC_RANKS) this.skip();
    this.timeout(3_600_000);
    const all = JSON.parse(readFileSync(FILE, "utf8")) as Q[];
    await waitForChat();
    const rows: Record<string, unknown>[] = [];
    for (const q of process.env.YOAI_E2E_DOC_DUMP ? all : all.filter((x) => x.source)) {
      const row = (await browser.executeAsync(
        (question: string, source: string, keys: string[][], prefix: string, model: string, dump: boolean, done: (v: unknown) => void) => {
          (async () => {
            const t = (window as any).__TAURI__;
            const store = await t.store.load(await t.core.invoke("profile_store_path", { name: "ai-data.json" }));
            const aiId = (((await store.get("custom-ais")) || []) as { id?: string }[])[0]?.id ?? "";
            const [vec] = await t.core.invoke("embed_texts", { texts: [prefix + question], model });
            const hits = (await t.core.invoke("corpus_recall", { aiId, query: vec, maxPassages: 300 })) as { filename: string; text: string; score: number; as_of: number }[];
            const holds = (text: string) => keys.length > 0 && keys.every((g) => g.some((k) => text.toLowerCase().includes(k.toLowerCase())));
            const src = hits.findIndex((h) => h.filename === source);
            const ans = hits.findIndex((h) => holds(h.text));
            const ansSrc = hits.findIndex((h) => h.filename === source && holds(h.text));
            return {
              // Every candidate, for trying other orderings outside the app.
              candidates: dump ? hits.map((h) => ({ f: h.filename, s: h.score, d: h.as_of, t: h.text })) : undefined,
              top: hits[0]?.score ?? 0,
              eighth: hits[7]?.score ?? 0,
              source: src < 0 ? null : { place: src + 1, score: hits[src].score },
              answer: ans < 0 ? null : { place: ans + 1, score: hits[ans].score, file: hits[ans].filename, day: new Date(hits[ans].as_of * 1000).toISOString().slice(0, 10) },
              answerInSource: ansSrc < 0 ? null : { place: ansSrc + 1, score: hits[ansSrc].score },
            };
          })().then(done, (e) => done({ error: String(e) }));
        },
        q.question, q.source, q.keys, "task: search result | query: ", "embeddinggemma-300M-Q8_0.gguf", !!process.env.YOAI_E2E_DOC_DUMP,
      )) as Record<string, unknown>;
      rows.push({ n: q.n, type: q.type, ...row });
      console.log(`e2e: RANK Q${q.n} ${q.type}: ${JSON.stringify({ ...row, candidates: undefined })}`);
    }
    if (OUT) writeFileSync(resolve(OUT, "document-question-ranks.json"), JSON.stringify(rows, null, 1));
  });
});

describe("questions answered from the library", () => {
  it("asks each question and records what was given and what was said", async function () {
    if (!FILE || process.env.YOAI_E2E_DOC_RANKS === "only") this.skip();
    this.timeout(6 * 3_600_000);
    const all = JSON.parse(readFileSync(FILE, "utf8")) as Q[];
    const questions = ONLY.length ? all.filter((q) => ONLY.includes(q.n)) : all;
    await waitForChat();
    // The library is what is measured: no conversation is searched, so a
    // run does not find its own questions or an earlier run's.
    const until = await fetch("http://127.0.0.1:11435/internal/memory-search-until", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ at: 1000 }) });
    console.log(`e2e: conversations are not searched in this run (HTTP ${until.status})`);
    const results: Record<string, unknown>[] = [];
    for (const q of questions) {
      // Each question in a conversation of its own.
      if (await browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length > 0)) {
        await (await $('[data-testid="new-chat"]')).click();
        await browser.waitUntil(async () => browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length === 0), { timeout: 30_000 });
      }
      const started = Date.now();
      await ask(q.question);
      const r = await waitForReplyDone(900_000);
      const seconds = Math.round((Date.now() - started) / 1000);
      // Which documents had passages given: the reply's own Sources list.
      let given = "";
      const sources = await $(`${lastAssistant}//*[@data-testid="reply-sources"]`);
      if (await sources.isExisting()) {
        await browser.execute((e) => (e as HTMLElement).click(), sources);
        const library = await $(`${lastAssistant}//*[@data-testid="reply-library"]`);
        if (await library.waitForExist({ timeout: 10_000 }).then(() => true, () => false)) {
          given = ((await browser.execute((e) => (e as HTMLElement).innerText, library)) as string).replace(/\s+/g, " ").trim();
        }
      }
      const said = r.text.toLowerCase();
      const right = q.keys.length > 0 && q.keys.every((group) => group.some((k) => said.includes(k.toLowerCase())));
      const stale = q.stale.some((k) => said.includes(k.toLowerCase()));
      const stem = q.source.replace(/\.[a-z]+$/i, "").toLowerCase();
      const sourceGiven = !!stem && given.toLowerCase().includes(stem);
      const row = { n: q.n, type: q.type, seconds, right, stale, sourceGiven, given: given.slice(0, 400), reply: r.text.slice(0, 700) };
      results.push(row);
      console.log(`e2e: Q${q.n} ${q.type}: right ${right}, stale ${stale}, source given ${sourceGiven}, ${seconds} s; given: ${JSON.stringify(given.slice(0, 160))}; reply: ${JSON.stringify(r.text.slice(0, 220))}`);
    }
    const count = (f: (r: any) => boolean) => results.filter(f).length;
    console.log(
      `e2e: DOCUMENT QUESTIONS: details right ${count((r) => r.type === "D" && r.right)} of ${count((r) => r.type === "D")}; changed over time, current value given ${count((r) => r.type === "E" && r.right && !r.stale)} of ${count((r) => r.type === "E")} (stale value said ${count((r) => r.type === "E" && r.stale)}); source document given ${count((r) => r.type !== "F" && r.sourceGiven)} of ${count((r) => r.type !== "F")}; traps: read the replies`,
    );
    if (OUT) writeFileSync(resolve(OUT, "document-questions-results.json"), JSON.stringify(results, null, 1));
  });
});
