#!/usr/bin/env node
/**
 * Build check: the text memory adds to a request is written in one place,
 * src-tauri/src/memory_prompt.rs. Fails when a heading of that text appears
 * in the page's source, or in any other backend file: a second copy drifts
 * (two copies once gave different memory to the chat page and the local
 * port).
 */
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const ONE = join("src-tauri", "src", "memory_prompt.rs");
const HEADINGS = [
  "Things you've learned about them",
  "From earlier conversations with this person",
  "Rules from them, in their words",
  "Relevant notes they've saved",
  "Knowledge this AI has been given",
  "Rules for this project:",
];

function files(dir) {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    return statSync(p).isDirectory() ? files(p) : [p];
  });
}

const problems = [];
let builder = "";
for (const path of [...files(join(REPO, "src")), ...files(join(REPO, "src-tauri", "src"))]) {
  if (!/\.(ts|tsx|rs)$/.test(path)) continue;
  const rel = relative(REPO, path);
  const body = readFileSync(path, "utf8");
  if (rel === ONE) {
    builder = body;
    continue;
  }
  body.split(/\r?\n/).forEach((line, i) => {
    for (const h of HEADINGS) if (line.includes(h)) problems.push(`${rel}:${i + 1} holds "${h}"`);
  });
}
for (const h of HEADINGS) if (!builder.includes(h)) problems.push(`${ONE} no longer holds "${h}" (update this check with the builder)`);

if (problems.length) {
  console.error("check-one-builder: memory's prompt text belongs in one file\n  " + problems.join("\n  "));
  process.exit(1);
}
console.log(`check-one-builder: ok (${HEADINGS.length} headings, only in ${ONE})`);
