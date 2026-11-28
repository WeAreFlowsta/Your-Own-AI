// Questions whose answers are only in the library, on the built app, on a
// profile that already holds the library (YOAI_E2E_KEEP_PROFILE=1 after
// library-large). A measurement, not a gate: it runs only when
// YOAI_E2E_DOC_QUESTIONS names a JSON file of questions, which is kept
// outside the repository. For each question it records the reply, which
// documents had passages given, and whether the reply holds the answer
// (and, for something that changed over time, the stale value).
import { readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { waitForChat } from "./_helpers";
import { askDocumentQuestions, leaveConversationsOut, rankDocumentQuestions, summary, type DocQuestion as Q } from "./_documentQuestions";

const FILE = process.env.YOAI_E2E_DOC_QUESTIONS ?? "";
const OUT = process.env.YOAI_MEMORY_OUT ?? "";
const ONLY = (process.env.YOAI_E2E_DOC_ONLY ?? "").split(",").filter(Boolean).map(Number);

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
    const rows = await rankDocumentQuestions(process.env.YOAI_E2E_DOC_DUMP ? all : all.filter((x) => x.source), !!process.env.YOAI_E2E_DOC_DUMP);
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
    await leaveConversationsOut();
    const results = await askDocumentQuestions(questions);
    console.log(`e2e: DOCUMENT QUESTIONS: ${summary(results)}`);
    if (OUT) writeFileSync(resolve(OUT, "document-questions-results.json"), JSON.stringify(results, null, 1));
  });
});
