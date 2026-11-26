// Checks onlyADateLine in src/utils/memoryDateLine.ts (run by `npm run build`):
// a reply that is only the line above a passage memory gave is asked once
// more; a date followed by an answer is an answer.
import assert from "node:assert";
import { readFileSync } from "node:fs";
import ts from "typescript";

const src = readFileSync(new URL("../src/utils/memoryDateLine.ts", import.meta.url), "utf8");
const js = ts.transpileModule(src, { compilerOptions: { module: "ESNext", target: "ES2022" } }).outputText;
const { onlyADateLine } = await import("data:text/javascript;base64," + Buffer.from(js).toString("base64"));

const given = [
  "[From memory. A search of all their conversations was run for this question. It returned the 2 dated exchange(s) below.",
  "",
  '[2026-03-08, conversation "Pricing", with Atlas]',
  "They said: the site is on the small plan",
  "",
  '[2026-04-02, conversation "Hosting notes 2", with Atlas]',
  "AI: The medium plan starts on April 9.]",
].join("\n");

// The line itself, with or without its brackets, and pieces of it.
for (const reply of ['[2026-03-08, conversation "Pricing", with Atlas]', '2026-03-08, conversation "Pricing", with Atlas]', "2026-03-08", '2026-04-02 | "Hosting notes 2"', " [2026-04-02, conversation \"Hosting notes 2\", with Atlas]: "]) {
  assert.equal(onlyADateLine(reply, given), true, reply);
}
// A date with an answer after it, an answer alone, a date memory did not give.
for (const reply of [
  "2026-04-02: the medium plan starts on April 9.",
  '[2026-03-08, conversation "Pricing", with Atlas] They said the site is on the small plan.',
  "The small plan.",
  "2025-01-01",
  "9",
  "",
  '[2026-03-08, conversation "Pricing", with Atlas]\nYou said the site is on the small plan.',
]) {
  assert.equal(onlyADateLine(reply, given), false, reply);
}
// Nothing was given: nothing to compare with.
assert.equal(onlyADateLine("2026-03-08", null), false);
assert.equal(onlyADateLine("2026-03-08", ""), false);
console.log("check-date-line: ok");
