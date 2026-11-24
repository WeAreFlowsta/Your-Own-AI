#!/usr/bin/env node
/**
 * Build check: every route under /internal/ on the local server answers
 * "not found" in a release build. Each handler registered for such a route
 * must turn away, before anything else, when the build is not a debug one.
 */
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const file = join(dirname(fileURLToPath(import.meta.url)), "..", "src-tauri", "src", "inference_server.rs");
// Line endings are normalised here, so a Windows checkout reads the same.
const src = readFileSync(file, "utf8").replace(/\r\n/g, "\n");
const routes = [...src.matchAll(/\.route\("(\/internal\/[^"]+)",\s*(?:get|post)\((\w+)\)/g)].map((m) => ({ path: m[1], handler: m[2] }));
const problems = [];
if (routes.length === 0) problems.push("no /internal/ route found: has the router moved?");
for (const { path, handler } of routes) {
  const at = src.indexOf(`async fn ${handler}(`);
  if (at < 0) {
    problems.push(`${path}: handler ${handler} not found`);
    continue;
  }
  const open = src.indexOf("{\n", src.indexOf(") -> Response", at));
  const first = src.slice(open + 2, open + 400).trimStart();
  if (!/^if !cfg!\(debug_assertions\) \{\s*return err\(StatusCode::NOT_FOUND/.test(first)) {
    problems.push(`${path}: ${handler} does not begin by answering "not found" outside a debug build`);
  }
}
if (problems.length) {
  console.error("check-dev-routes:\n  " + problems.join("\n  "));
  process.exit(1);
}
console.log(`check-dev-routes: ok (${routes.length} /internal/ routes, each closed in a release build)`);
