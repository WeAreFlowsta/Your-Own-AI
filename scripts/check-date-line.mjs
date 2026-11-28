// Checks onlyADateLine in src/utils/memoryDateLine.ts (run by `npm run build`):
// a reply that is only the line above a passage memory gave is asked once
// more; a date followed by an answer is an answer.
import assert from "node:assert";
import { readFileSync } from "node:fs";
import ts from "typescript";

const src = readFileSync(new URL("../src/utils/memoryDateLine.ts", import.meta.url), "utf8");
const js = ts.transpileModule(src, { compilerOptions: { module: "ESNext", target: "ES2022" } }).outputText;
const { onlyADateLine, opensWithADateLine, withAPlainCitation } = await import("data:text/javascript;base64," + Buffer.from(js).toString("base64"));

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
// A reply that opens with the line and goes on: a copy of the passage is
// asked again, the model's own words keep a plain citation.
assert.equal(opensWithADateLine('2026-03-08, conversation "Pricing", with Atlas] They said: the site is on the small plan', given)?.kind, "copy");
assert.equal(opensWithADateLine('[2026-04-02, conversation "Hosting notes 2", with Atlas]: AI: The medium plan starts on April 9.', given)?.kind, "copy");
assert.equal(opensWithADateLine('2026-03-08, conversation "Pricing", with Atlas]\n\nYou are on the small plan.', given)?.kind, "lead");
assert.equal(opensWithADateLine('[2026-03-08, conversation "Pricing", with Atlas]', given), null);
assert.equal(opensWithADateLine('2025-01-01, conversation "Pricing", with Atlas] You are on the small plan.', given), null);
assert.equal(opensWithADateLine("You are on the small plan.", given), null);
assert.equal(withAPlainCitation('2026-03-08, conversation "Pricing", with Atlas]\n\nYou are on the small plan.', given), 'From "Pricing", 2026-03-08:\n\nYou are on the small plan.');
assert.equal(withAPlainCitation('<think>look</think>[2026-03-08, conversation "Pricing", with Atlas]: You are on the small plan.', given), '<think>look</think>\nFrom "Pricing", 2026-03-08:\n\nYou are on the small plan.');
assert.equal(withAPlainCitation("You are on the small plan.", given), "You are on the small plan.");
assert.equal(withAPlainCitation('2026-03-08, conversation "Pricing", with Atlas] They said: the site is on the small plan', given), '2026-03-08, conversation "Pricing", with Atlas] They said: the site is on the small plan');
console.log("check-date-line: ok");
