#!/usr/bin/env node
/**
 * Build check: the measuring harness and its fixtures hold made-up data
 * only. Fails when a file under tools/ or e2e/fixtures/:
 *   - names a folder where private question sets or histories are kept, or
 *   - contains a line whose hash is in a list of private lines.
 *
 * The list is never in this repository. It is a text file of sha256 hashes,
 * one per line, given by YOAI_PRIVATE_LINE_HASHES; without it only the
 * first check runs (as in CI). To make a list from private files:
 *
 *   node scripts/check-harness-private.mjs --make-list FILE... > hashes.txt
 *
 * A line is hashed after trimming and collapsing whitespace; lines shorter
 * than 24 characters are skipped (they are too common to mean anything).
 */
import { createHash } from "node:crypto";
import { readFileSync, readdirSync, statSync, existsSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ROOTS = ["tools", join("e2e", "fixtures")];
/** Folder names that mark a private store. Built from parts so this file
 *  does not trip its own check when it is scanned from another root. */
const PRIVATE_MARKS = [["flowsta", "kept"].join("-"), ["memory", "exploration"].join("-")];
const MIN_LINE = 24;

const norm = (line) => line.trim().split(/\s+/).join(" ");
const hash = (line) => createHash("sha256").update(norm(line)).digest("hex");

function files(dir) {
  if (!existsSync(dir)) return [];
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    return statSync(p).isDirectory() ? files(p) : [p];
  });
}

function text(path) {
  const raw = readFileSync(path);
  return raw.includes(0) ? null : raw.toString("utf8");
}

if (process.argv[2] === "--make-list") {
  const seen = new Set();
  for (const f of process.argv.slice(3)) {
    for (const line of readFileSync(f, "utf8").split("\n")) {
      if (norm(line).length >= MIN_LINE) seen.add(hash(line));
    }
  }
  console.log([...seen].join("\n"));
  process.exit(0);
}

const listPath = process.env.YOAI_PRIVATE_LINE_HASHES;
const privateLines = listPath && existsSync(listPath) ? new Set(readFileSync(listPath, "utf8").split("\n").map((l) => l.trim()).filter(Boolean)) : null;

const problems = [];
for (const root of ROOTS) {
  for (const path of files(join(REPO, root))) {
    const body = text(path);
    if (body === null) continue;
    const rel = relative(REPO, path);
    body.split(/\r?\n/).forEach((line, i) => {
      for (const mark of PRIVATE_MARKS) {
        if (line.includes(mark)) problems.push(`${rel}:${i + 1} names a private folder (${mark})`);
      }
      if (privateLines && norm(line).length >= MIN_LINE && privateLines.has(hash(line))) {
        problems.push(`${rel}:${i + 1} is a line from a private set`);
      }
    });
  }
}

if (problems.length) {
  console.error("check-harness-private: the harness must hold made-up data only\n  " + problems.join("\n  "));
  process.exit(1);
}
console.log(`check-harness-private: ok (${privateLines ? `${privateLines.size} private line hashes checked` : "folder names only; no private line list given"})`);
