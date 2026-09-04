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
writeFileSync(file, out.join("\n"));
