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
// The real case: one survey passage at 0.71, an unrelated document filling the rest.
assert.deepEqual(keepNearBest(s(0.71, 0.52, 0.51, 0.5, 0.49)).map((h) => h.score), [0.71]);
// Several passages of the same subject stay together.
assert.equal(keepNearBest(s(0.78, 0.74, 0.7, 0.55)).length, 3);
// Nothing in the library fits (the bond-markets question: best 0.62, the rest just under it): give NOTHING.
assert.deepEqual(keepNearBest(s(0.62, 0.61, 0.6, 0.6, 0.59, 0.58, 0.58, 0.57)), []);
// A real question about a long document keeps its passages (best 0.70).
assert.equal(keepNearBest(s(0.7, 0.68, 0.66, 0.63, 0.6, 0.55)).length, 5);
assert.equal(RECALL_BEST_AT_LEAST, 0.66);
assert.deepEqual(keepNearBest([]), []);
assert.equal(RECALL_MARGIN, 0.12);
console.log("recall check: 7 cases pass");
