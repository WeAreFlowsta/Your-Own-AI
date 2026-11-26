// Checks keepNearBest in src/utils/memory.ts (run by `npm run build`).
import assert from "node:assert";
import { readFileSync } from "node:fs";
import ts from "typescript";

const src = readFileSync(new URL("../src/utils/memory.ts", import.meta.url), "utf8");
const start = src.indexOf("export const RECALL_BEST_AT_LEAST");
const end = src.indexOf("/**", src.indexOf("export function withNewerNotes"));
const js = ts.transpileModule(src.slice(start, end), { compilerOptions: { module: "ESNext", target: "ES2020" } }).outputText;
const { keepNearBest, withNewerNotes, RECALL_MARGIN, RECALL_BEST_AT_LEAST } = await import("data:text/javascript;base64," + Buffer.from(js).toString("base64"));

const s = (...scores) => scores.map((score, i) => ({ score, i }));
// Scores on EmbeddingGemma's scale (2026-10-12 matrix: right passages 0.21-0.75, mean 0.57;
// the best wrong one <= 0.32). The real case: one survey passage at 0.57, an unrelated
// document filling the rest just under the bar.
assert.deepEqual(keepNearBest(s(0.57, 0.32, 0.31, 0.3, 0.29)).map((h) => h.score), [0.57]);
// Three passages of the right document ride together; the fourth is too far behind.
assert.equal(keepNearBest(s(0.62, 0.58, 0.5, 0.4)).length, 3);
// Nothing in the library fits (best 0.28, the rest just under it): give NOTHING.
assert.deepEqual(keepNearBest(s(0.28, 0.27, 0.26, 0.26, 0.25, 0.24, 0.24, 0.23)), []);
// A real question about a long document keeps its passages (best 0.60, within the margin).
assert.equal(keepNearBest(s(0.6, 0.55, 0.5, 0.47, 0.44, 0.3)).length, 4);
assert.equal(RECALL_BEST_AT_LEAST, 0.30);
assert.deepEqual(keepNearBest([]), []);
assert.equal(RECALL_MARGIN, 0.15);

// A newer note keeps a place. An old note discusses the subject at length
// (0.69, 0.62, 0.55); the current one states the value in one row (0.47).
const DAY = 86_400;
const h = (score, day, doc_id) => ({ score, as_of: day * DAY + 100, doc_id });
{
  const found = [h(0.69, 10, "old"), h(0.62, 10, "old"), h(0.55, 10, "old"), h(0.47, 400, "current"), h(0.46, 400, "current"), h(0.3, 500, "other")];
  const chosen = keepNearBest(found.slice(0, 8));
  assert.deepEqual(chosen.map((x) => x.score), [0.69, 0.62, 0.55]);
  // The current note's best passage joins, once; a newer note too far below the best does not.
  assert.deepEqual(withNewerNotes(chosen, found, 8).map((x) => x.score), [0.69, 0.62, 0.55, 0.47]);
  // No room: the lowest of the chosen makes way.
  assert.deepEqual(withNewerNotes(chosen, found, 3).map((x) => x.score), [0.69, 0.62, 0.47]);
  // One passage only: it stays the best match.
  assert.deepEqual(withNewerNotes(chosen.slice(0, 1), found, 1).map((x) => x.score), [0.69]);
}
// Nothing newer than what was chosen: nothing is added.
{
  const found = [h(0.6, 400, "current"), h(0.5, 10, "old"), h(0.44, 10, "older")];
  assert.deepEqual(withNewerNotes(keepNearBest(found), found, 8).map((x) => x.score), [0.6, 0.5]);
}
// At most two, from two different notes, and nothing when nothing was chosen.
{
  const found = [h(0.7, 10, "old"), h(0.5, 20, "a"), h(0.49, 20, "a"), h(0.48, 30, "b"), h(0.47, 40, "c")];
  assert.deepEqual(withNewerNotes([found[0]], found, 8).map((x) => x.doc_id), ["old", "a", "b"]);
  assert.deepEqual(withNewerNotes([], found, 8), []);
}
// The same day is not newer (many files carry one day after a folder is moved).
{
  const found = [h(0.7, 10, "old"), { score: 0.5, as_of: 10 * DAY + 80_000, doc_id: "same-day" }];
  assert.deepEqual(withNewerNotes([found[0]], found, 8).length, 1);
}
