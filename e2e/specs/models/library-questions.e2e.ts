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

describe("questions answered from the library", () => {
  it("asks each question and records what was given and what was said", async function () {
    if (!FILE) this.skip();
    this.timeout(6 * 3_600_000);
    const all = JSON.parse(readFileSync(FILE, "utf8")) as Q[];
    const questions = ONLY.length ? all.filter((q) => ONLY.includes(q.n)) : all;
    await waitForChat();
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
