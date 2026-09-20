// Checks src/utils/replyBoundary.ts (run by `npm run build`).
import assert from "node:assert";
import { readFileSync } from "node:fs";
import ts from "typescript";

const src = readFileSync(new URL("../src/utils/replyBoundary.ts", import.meta.url), "utf8");
const js = ts.transpileModule(src, { compilerOptions: { module: "ESNext", target: "ES2020" } }).outputText;
const { reachedBoundary, boundaryIndex, insideCodeBlock, BOUNDARY_CAP_CHARS } = await import(
  "data:text/javascript;base64," + Buffer.from(js).toString("base64")
);

const at = (before, after) => reachedBoundary(before + after, before.length);

assert.equal(at("The tide is", " pulled by"), false, "mid-sentence keeps going");
assert.equal(at("The tide is", " pulled by the moon. "), true, "sentence end");
assert.equal(at("The tide is", " pulled by the moon."), false, "a final dot may be a decimal or an ellipsis - wait for the space");
assert.equal(at("Pi is about", " 3.14 and"), false, "a decimal is not a sentence end");
assert.equal(at("She said", ' "stop!" and'), true, "punctuation inside a closing quote");
assert.equal(at("First point", "\n"), true, "a line end");
assert.equal(at("x", "y".repeat(BOUNDARY_CAP_CHARS)), true, "the cap");
assert.equal(at("Already ended. ", "Next"), false, "only text AFTER the interruption counts");

const code = "Here:\n```js\nconst a = 1;";
assert.equal(insideCodeBlock(code), true);
assert.equal(insideCodeBlock(code + "\n```\nDone"), false);
assert.equal(at(code, " // note. more"), false, "inside code a dot is not a boundary");
assert.equal(at(code, " // note\n"), true, "inside code the line is the unit");

const run = "It rains a lot. The next sen";
assert.equal(run.slice(0, boundaryIndex(run, 3)), "It rains a lot.", "a chunk that ran on is trimmed to the sentence");
assert.equal(boundaryIndex("no end yet", 0), null);

console.log("reply boundary check: 14 cases pass");
