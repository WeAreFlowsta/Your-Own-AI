// Qwik gotcha #30 guard: a `$()` closure that calls another `const x = $(...)`
// declared LATER in the same component throws "x is not defined" at runtime -
// no typecheck or build error. The same for a closure that READS a signal,
// store, computed or resource declared later ("Can't find variable: x" on
// the closure's first run; 2026-10-21: every project question since
// 0.9.0-beta.11 died that way, and the Detailed rail never followed its
// newest row since 09-23). Exit 1 with the offenders. Runs first in `npm run build`.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
const files = [];
const walk = (d) => { for (const n of readdirSync(d)) { const p = join(d, n); if (statSync(p).isDirectory()) walk(p); else if (/\.tsx?$/.test(n)) files.push(p); } };
walk("src");
let bad = 0;
for (const f of files) {
  const s = readFileSync(f, "utf8").replace(/\r\n/g, "\n"); // the Windows runner checks out CRLF
  const decl = [...s.matchAll(/^([ \t]*)const (\w+\$?) = \$\(/gm)].map((m) => ({ name: m[2], at: m.index, indent: m[1] }));
  // hook values a closure may read: they exist only once their line has run
  const values = [...s.matchAll(/^([ \t]*)const (\w+) = use(?:Signal|Store|Computed\$|Resource\$|Context|Ref)(?:<[^\n]*?>)?\(/gm)].map((m) => ({ name: m[2], at: m.index }));
  // a task is a closure too: it takes what it uses when the component renders
  const tasks = [...s.matchAll(/^([ \t]*)use(?:Visible)?Task\$\(/gm)].map((m) => ({ name: `a task (line ${s.slice(0, m.index).split("\n").length})`, at: m.index, indent: m[1] }));
  for (const d of [...decl, ...tasks]) {
    // the closure body: a one-line closure is its own line; otherwise from
    // its declaration to the matching `});` or `}, {` (a task with options)
    // at the same indent
    const rest = s.slice(d.at);
    const firstLine = rest.slice(0, rest.indexOf("\n") < 0 ? rest.length : rest.indexOf("\n"));
    const endRe = new RegExp(`^${d.indent}\\}(\\);|, \\{)`, "m");
    const endMatch = endRe.exec(rest);
    const raw = /\)\);?\s*$/.test(firstLine) && !/\{\s*$/.test(firstLine)
      ? firstLine
      : endMatch ? rest.slice(0, endMatch.index) : rest.slice(0, 4000);
    // comments are not reads
    const body = raw.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^[ \t]*\/\/.*$/gm, "");
    for (const other of decl) {
      if (other.name === d.name || other.at < d.at) continue;
      if (new RegExp(`(?<![.\\w])${other.name.replace(/\$/g, "\\$")}\\(`).test(body)) {
        console.log(`${f}: ${d.name} calls ${other.name}, which is declared later (line ${s.slice(0, other.at).split("\n").length})`);
        bad++;
      }
    }
    for (const v of values) {
      if (v.at < d.at) continue;
      // a read: the bare name, not a property, not an object key or type field
      if (new RegExp(`(?<![.\\w"'])(?<!const )${v.name}(?![\\w"'])(?!\\s*[:?]\\s*[\\w{(<])`).test(body)) {
        console.log(`${f}: ${d.name} reads ${v.name}, which is declared later (line ${s.slice(0, v.at).split("\n").length}) - move the declaration above the closure`);
        bad++;
      }
    }
  }
}
console.log(bad ? `${bad} closure-order problem(s)` : "closure order: ok");
process.exit(bad ? 1 : 0);
