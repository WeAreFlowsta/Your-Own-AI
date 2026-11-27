// Where the passage that answers a question stands in memory's search, on
// the profile as it is (YOAI_E2E_KEEP_PROFILE=1), with no chat model: a
// measuring run for questions whose answer is known to be in the history.
// For each question the search is asked for many more messages than a
// reply is given, and the place of the first one from the answer's own
// passage is written down, with what stood in the first places instead.
// Everything is given by path, outside the repository:
//   YOAI_E2E_SOURCES      [{ n, kind, question, sources: [{ title, lo, hi, date }] }]
//   YOAI_MEMORY_OUT       folder for source-rank.json
//   YOAI_MEMORY_UNTIL_MS  search nothing begun after this (the run's own questions)
// Nothing is asserted about the places: this is a measure.
import * as fs from "node:fs";
import * as path from "node:path";
import { waitForChat, invoke } from "../models/_helpers";

const FILE = process.env.YOAI_E2E_SOURCES ?? "";
const OUT = process.env.YOAI_MEMORY_OUT ?? "";
const UNTIL_MS = Number(process.env.YOAI_MEMORY_UNTIL_MS ?? 0);
const BASE = "http://127.0.0.1:11435/internal";
const DEEP = 400;
const GIVEN = 12;

type Source = { title: string; lo: number; hi: number; date: string | null };
type Q = { n: number; kind: string; question: string; sources: Source[] };
type Found = { conversation: string; started_at: number; title: string | null; seq: number; at: number; role: string; score: number };

const sameTitle = (a: string | null, b: string) => (a ?? "").trim().toLowerCase() === b.trim().toLowerCase();
// A message of the answer's passage, or right beside it (the reply to it).
const fromSource = (f: Found, s: Source) => sameTitle(f.title, s.title) && f.seq >= s.lo - 1 && f.seq <= s.hi + 1;

describe("where the answer's passage stands in the search", () => {
  it("writes the place of each answer's passage", async function () {
    if (!FILE) this.skip();
    this.timeout(3 * 3_600_000);
    const questions = JSON.parse(fs.readFileSync(FILE, "utf8")) as Q[];
    await waitForChat();
    // A kept profile's records take a while to open: wait until memory answers.
    await browser.waitUntil(async () => invoke<unknown[]>("memory_items").then(() => true, () => false), { timeout: 600_000, interval: 5_000, timeoutMsg: "memory did not answer within ten minutes" });
    if (UNTIL_MS > 0) await fetch(`${BASE}/memory-search-until`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ at: UNTIL_MS * 1000 }) });
    const rows: unknown[] = [];
    let inGiven = 0;
    let inDeep = 0;
    let asked = 0;
    for (const q of questions) {
      if (q.sources.length === 0) continue;
      const r = await fetch(`${BASE}/memory-search?limit=${DEEP}&q=${encodeURIComponent(q.question)}`);
      const body = (await r.json()) as { found?: Found[]; error?: string; read?: number; total?: number };
      if (!body.found) {
        console.log(`e2e: SOURCE RANK Q${q.n}: the search did not answer: ${body.error ?? r.status}`);
        continue;
      }
      asked += 1;
      const at = body.found.findIndex((f) => q.sources.some((s) => fromSource(f, s)));
      const sameConversation = body.found.findIndex((f) => q.sources.some((s) => sameTitle(f.title, s.title)));
      const place = at < 0 ? null : at + 1;
      if (place !== null && place <= GIVEN) inGiven += 1;
      if (place !== null) inDeep += 1;
      const top = body.found.slice(0, 5).map((f) => `${(f.title ?? "").slice(0, 28)}#${f.seq} ${f.score.toFixed(3)}`);
      const scoreAtPlace = at < 0 ? null : body.found[at].score;
      const scoreAtCut = body.found[GIVEN - 1]?.score ?? null;
      rows.push({ n: q.n, kind: q.kind, place, firstOfItsConversation: sameConversation < 0 ? null : sameConversation + 1, scoreAtPlace, scoreAtCut, topScore: body.found[0]?.score ?? null, found: body.found.length, read: body.read, total: body.total, top });
      console.log(`e2e: SOURCE RANK Q${q.n} ${q.kind}: place ${place ?? `not in ${body.found.length}`}; its conversation first at ${sameConversation < 0 ? "none" : sameConversation + 1}; score ${scoreAtPlace?.toFixed(3) ?? "-"} against ${scoreAtCut?.toFixed(3) ?? "-"} at place ${GIVEN}`);
      if (OUT) fs.writeFileSync(path.resolve(OUT, "source-rank.json"), JSON.stringify(rows, null, 1));
    }
    console.log(`e2e: SOURCE RANK TOTAL: of ${asked} questions, the answer's passage is in the first ${GIVEN} for ${inGiven}, in the first ${DEEP} for ${inDeep}`);
    expect(asked).toBeGreaterThan(0);
  });
});
