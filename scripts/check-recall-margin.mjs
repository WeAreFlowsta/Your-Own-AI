// Checks keepNearBest in src/utils/memory.ts (run by `npm run build`).
import assert from "node:assert";
import { readFileSync } from "node:fs";
import ts from "typescript";

const src = readFileSync(new URL("../src/utils/memory.ts", import.meta.url), "utf8");
const start = src.indexOf("export const RECALL_BEST_AT_LEAST");
const end = src.indexOf("/**", src.indexOf("export function keepNearBest"));
const js = ts.transpileModule(src.slice(start, end), { compilerOptions: { module: "ESNext", target: "ES2020" } }).outputText;
const { keepNearBest, RECALL_MARGIN, RECALL_BEST_AT_LEAST } = await import("data:text/javascript;base64," + Buffer.from(js).toString("base64"));

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
