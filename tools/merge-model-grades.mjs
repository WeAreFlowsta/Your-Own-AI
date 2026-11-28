#!/usr/bin/env node
// Adds the results of a matrix grading run to the data the app ships
// (src-tauri/src/local_model_results.json). A report holds one line per
// graded model that begins "figure: " and carries {"kind":"model-grade"}:
// the file, its hash, how many of the harder questions were right, and
// the memory check's result.
//
//   node tools/merge-model-grades.mjs <report.txt> [more reports]   what would change
//   node tools/merge-model-grades.mjs --write <report.txt> ...      change the data file
//   node tools/merge-model-grades.mjs --replace --write ...         let a report replace a result that differs
//
// A result belongs to a model file. Two reports of the same file should
// agree; where they do not, the difference is printed and the result in
// the data file stays unless --replace is given.
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const DATA = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "src", "local_model_results.json");
const args = process.argv.slice(2);
const write = args.includes("--write");
const replace = args.includes("--replace");
const reports = args.filter((a) => !a.startsWith("--"));
if (reports.length === 0) {
  console.error("usage: node tools/merge-model-grades.mjs [--write] [--replace] <report.txt> ...");
  process.exit(2);
}

/** The grades a report holds, as the data file keeps them. */
export function gradesIn(text) {
  const out = [];
  for (const line of text.replace(/\r\n/g, "\n").split("\n")) {
    const at = line.indexOf("figure: {");
    if (at < 0) continue;
    let f;
    try {
      f = JSON.parse(line.slice(at + "figure: ".length));
    } catch {
      continue;
    }
    if (f.kind !== "model-grade" || typeof f.file !== "string") continue;
    const entry = { file: f.file };
    if (typeof f.sha256 === "string" && f.sha256.length === 64) entry.sha256 = f.sha256;
    const q = f.questions;
    // Only a full pass of the set is a result: a run that was stopped is not.
    if (q && q.of > 0 && q.asked === q.of) entry.questions = { set: q.set, right: q.right, of: q.of };
    const m = f.memory;
    if (m && entry.sha256) entry.memory = { set: m.set, may_keep: !!m.may_keep, may_close: !!m.may_close, may_tidy: !!m.may_tidy, judge_right: m.judge_right ?? 0 };
    if (entry.questions || entry.memory) out.push(entry);
  }
  return out;
}

const same = (a, b) => JSON.stringify(a ?? null) === JSON.stringify(b ?? null);
const data = JSON.parse(readFileSync(DATA, "utf8"));
const byFile = new Map(data.results.map((r) => [r.file.toLowerCase(), r]));
let added = 0;
let changed = 0;
let differs = 0;
for (const path of reports) {
  for (const g of gradesIn(readFileSync(path, "utf8"))) {
    const have = byFile.get(g.file.toLowerCase());
    if (!have) {
      byFile.set(g.file.toLowerCase(), g);
      added += 1;
      console.log(`new: ${g.file}: ${g.questions ? `${g.questions.right} of ${g.questions.of}` : "no questions"}; memory ${g.memory ? `keep ${g.memory.may_keep}, close ${g.memory.may_close}, replace ${g.memory.may_tidy}` : "not checked"}`);
      continue;
    }
    // Another file under the same name is another model: never merged into it.
    if (have.sha256 && g.sha256 && have.sha256 !== g.sha256) {
      console.log(`DIFFERENT FILE under the name ${g.file}: the data file has ${have.sha256.slice(0, 12)}, the report ${g.sha256.slice(0, 12)}. Not merged.`);
      differs += 1;
      continue;
    }
    for (const part of ["questions", "memory"]) {
      if (!g[part]) continue;
      if (!have[part]) {
        have[part] = g[part];
        if (g.sha256) have.sha256 = g.sha256;
        changed += 1;
        console.log(`added ${part} for ${g.file}`);
      } else if (!same(have[part], g[part])) {
        differs += 1;
        console.log(`DIFFERS: ${g.file} ${part}: the data file has ${JSON.stringify(have[part])}, the report ${JSON.stringify(g[part])}${replace ? " (replaced)" : " (kept; --replace takes the report's)"}`);
        if (replace) {
          have[part] = g[part];
          changed += 1;
        }
      }
    }
  }
}
data.results = [...byFile.values()].sort((a, b) => a.file.toLowerCase().localeCompare(b.file.toLowerCase()));
console.log(`${added} new, ${changed} changed, ${differs} that differ; ${data.results.length} models in all`);
if (write) {
  writeFileSync(DATA, JSON.stringify(data, null, 1) + "\n");
  console.log("the data file is written");
} else {
  console.log("nothing written (give --write)");
}
