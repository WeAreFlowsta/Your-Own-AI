#!/usr/bin/env node
/**
 * Build check: a project's older memory text is not sent to a session and
 * is not rewritten by a model when a session ends. What a session learns
 * is kept as memory items, each shown to the person with an undo; a
 * rewrite of the whole text replaced it in one step, with no way back.
 * Fails when the page's source calls the rewrite or builds the old block.
 */
import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { fileURLToPath } from "node:url";

const REPO = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const GONE = ["reviseWorkspaceMemory", "[Project memory - durable notes"];

function files(dir) {
  return readdirSync(dir).flatMap((name) => {
    const p = join(dir, name);
    return statSync(p).isDirectory() ? files(p) : [p];
  });
}

const problems = [];
for (const path of files(join(REPO, "src"))) {
  if (!/\.(ts|tsx)$/.test(path)) continue;
  const body = readFileSync(path, "utf8");
  for (const text of GONE) {
    if (body.includes(text)) problems.push(`${relative(REPO, path)} holds "${text}"`);
  }
}
if (problems.length) {
  console.error(`check-no-project-rewrite: FAILED\n  ${problems.join("\n  ")}`);
  process.exit(1);
}
console.log("check-no-project-rewrite: ok (no session-end rewrite, no project memory text on the wire)");
