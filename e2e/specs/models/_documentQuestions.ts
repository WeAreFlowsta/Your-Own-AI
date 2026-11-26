// Asking a set of questions whose answers are only in the library, and
// recording for each: the reply, which documents had passages given, and
// whether the reply holds the answer (and, for something that changed over
// time, an earlier value).
import { ask, waitForReplyDone } from "./_helpers";

export type DocQuestion = { n: number; type: string; question: string; keys: string[][]; source: string; stale: string[] };
export type DocResult = { n: number; type: string; seconds: number; right: boolean; stale: boolean; sourceGiven: boolean; given: string; reply: string };

const lastAssistant = '(//*[@data-testid="chat-message"][@data-role="assistant"])[last()]';

/** No conversation is searched from here on: the library is what is
 *  measured, and a run must not find its own questions. */
export async function leaveConversationsOut(): Promise<void> {
  const until = await fetch("http://127.0.0.1:11435/internal/memory-search-until", { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ at: 1000 }) });
  console.log(`e2e: conversations are not searched in this run (HTTP ${until.status})`);
}

export async function askDocumentQuestions(questions: DocQuestion[]): Promise<DocResult[]> {
  const results: DocResult[] = [];
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
  return results;
}

export const count = (results: DocResult[], f: (r: DocResult) => boolean) => results.filter(f).length;

export function summary(results: DocResult[]): string {
  const c = (f: (r: DocResult) => boolean) => count(results, f);
  return `details right ${c((r) => r.type === "D" && r.right)} of ${c((r) => r.type === "D")}; changed over time, the current value is in the reply ${c((r) => r.type === "E" && r.right)} of ${c((r) => r.type === "E")} (an earlier value is named in ${c((r) => r.type === "E" && r.stale)}: read those); source document given ${c((r) => r.type !== "F" && r.sourceGiven)} of ${c((r) => r.type !== "F")}; traps: read the replies`;
}

/** Where the search itself puts the document that holds the answer: its
 *  best passage's place among the first 300 candidates, and the place of
 *  the first passage holding the answer's words, with their scores. Search
 *  only; no model reply. `dump` keeps every candidate in the answer. */
export async function rankDocumentQuestions(questions: DocQuestion[], dump: boolean): Promise<Record<string, unknown>[]> {
  const rows: Record<string, unknown>[] = [];
  for (const q of questions) {
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
      q.question, q.source, q.keys, "task: search result | query: ", "embeddinggemma-300M-Q8_0.gguf", dump,
    )) as Record<string, unknown>;
    rows.push({ n: q.n, type: q.type, ...row });
    console.log(`e2e: RANK Q${q.n} ${q.type}: ${JSON.stringify({ ...row, candidates: undefined })}`);
  }
  return rows;
}
