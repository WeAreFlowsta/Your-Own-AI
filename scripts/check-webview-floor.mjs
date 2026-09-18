#!/usr/bin/env node
// The app's webview is the operating system's own on macOS (WKWebView) and
// Linux (WebKitGTK): as old as the OS, never "evergreen". macOS 12 runs
// WebKit at the Safari 15.6 level. One regex LOOKBEHIND LITERAL is a parse
// error there (supported from Safari 16.4), and a parse error takes down the
// whole chunk plus every chunk that imports it - buttons stop working while
// plain links still do. The bundler does not flag it (`build.target` covers
// syntax it can transform; a regex body is not one of them), so this reads
// every built chunk and fails the build on what that webview cannot parse.
//
//   node scripts/check-webview-floor.mjs [dist/build]
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { parse } from "acorn";

const dir = process.argv[2] ?? "dist/build";
const problems = [];

function walk(node, visit) {
  if (!node || typeof node.type !== "string") return;
  visit(node);
  for (const key of Object.keys(node)) {
    const v = node[key];
    if (Array.isArray(v)) v.forEach((c) => walk(c, visit));
    else if (v && typeof v === "object") walk(v, visit);
  }
}

for (const name of readdirSync(dir).filter((f) => f.endsWith(".js"))) {
  const src = readFileSync(join(dir, name), "utf8");
  let ast;
  try {
    ast = parse(src, { ecmaVersion: "latest", sourceType: "module" });
  } catch (e) {
    problems.push(`${name}: does not parse (${e.message})`);
    continue;
  }
  walk(ast, (n) => {
    if (n.type !== "Literal" || !n.regex) return;
    const { pattern, flags } = n.regex;
    if (/\(\?<[=!]/.test(pattern)) {
      problems.push(`${name}: regex lookbehind literal /${pattern.slice(0, 60)}/ - Safari < 16.4 cannot parse it; rewrite without lookbehind, or build it with new RegExp() behind a feature test`);
    }
    if (flags.includes("v")) {
      problems.push(`${name}: regex flag "v" on /${pattern.slice(0, 40)}/ - Safari < 17 cannot parse it`);
    }
  });
  // Belt and braces for a build that was NOT held to the Safari 15 target.
  walk(ast, (n) => {
    if (n.type === "StaticBlock") problems.push(`${name}: class static block - Safari < 16.4 cannot parse it`);
  });
}

if (problems.length) {
  console.error(`webview floor check FAILED (${problems.length}):`);
  for (const p of problems) console.error("  " + p);
  process.exit(1);
}
console.log(`webview floor check: ${readdirSync(dir).filter((f) => f.endsWith(".js")).length} chunks parse for the Safari 15 webview`);
