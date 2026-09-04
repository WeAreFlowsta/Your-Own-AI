// Unwrap hard-wrapped CHANGELOG lines for a GitHub release body.
//
// CHANGELOG.md is wrapped at ~76 columns (easy to read raw, clean diffs).
// GitHub renders release bodies like comments, where a single newline is a
// line break - so wrapped entries showed ragged breaks while single-line
// ones flowed (0.7.0 release page, Eric 2026-09-04). This joins each
// paragraph and list item back into one line; headings, blank lines,
// tables and fenced code pass through untouched.
//
//   node scripts/unwrap-release-notes.mjs <file>   (rewrites the file)
import { readFileSync, writeFileSync } from "node:fs";

const file = process.argv[2];
const lines = readFileSync(file, "utf8").split("\n");
const out = [];
let inFence = false;
const isBlock = (l) => /^\s*(#|\||```|---)/.test(l) || /^\s*$/.test(l);
const startsItem = (l) => /^\s*([-*+]|\d+[.)])\s+/.test(l);
for (const line of lines) {
  if (/^\s*```/.test(line)) { inFence = !inFence; out.push(line); continue; }
  if (inFence || isBlock(line) || startsItem(line) || out.length === 0) { out.push(line); continue; }
  const prev = out[out.length - 1];
  if (prev !== undefined && !isBlock(prev) && !/^\s*```/.test(prev)) {
    out[out.length - 1] = prev.replace(/\s+$/, "") + " " + line.trim();
  } else {
    out.push(line);
  }
}
// A block that opens with "### Highlights" keeps the highlights in view and
// folds everything after them: a stable's full entry is hundreds of lines,
// and a release page that is all of it at once is harder to read than a
// short list with the detail one click away (Eric, 0.7.0 release page).
const text = out.join("\n");
const hl = text.indexOf("### Highlights");
let result = text;
if (hl >= 0) {
  const afterHl = text.indexOf("\n### ", hl + 5);
  if (afterHl > 0) {
    const head = text.slice(0, afterHl).replace(/\s+$/, "");
    const rest = text.slice(afterHl).replace(/^\s+/, "");
    const sections = (rest.match(/^### /gm) || []).length;
    result = `${head}\n\n<details>\n<summary><strong>Everything in this release</strong> (${sections} sections)</summary>\n\n${rest.replace(/\s+$/, "")}\n\n</details>`;
  }
}
writeFileSync(file, result);
