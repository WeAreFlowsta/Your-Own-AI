// Qwik gotcha #30 guard: a `$()` closure that calls another `const x = $(...)`
// declared LATER in the same component throws "x is not defined" at runtime -
// no typecheck or build error. Exit 1 with the offenders. Run before commits.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
const files = [];
const walk = (d) => { for (const n of readdirSync(d)) { const p = join(d, n); if (statSync(p).isDirectory()) walk(p); else if (/\.tsx?$/.test(n)) files.push(p); } };
walk("src");
let bad = 0;
for (const f of files) {
  const s = readFileSync(f, "utf8");
  const decl = [...s.matchAll(/^([ \t]*)const (\w+) = \$\(/gm)].map((m) => ({ name: m[2], at: m.index, indent: m[1] }));
  for (const d of decl) {
    // the closure body: from its declaration to the matching `});` at the same indent
    const endRe = new RegExp(`^${d.indent}\\}\\);`, "m");
    endRe.lastIndex = d.at;
    const rest = s.slice(d.at);
    const endMatch = endRe.exec(rest);
    const body = endMatch ? rest.slice(0, endMatch.index) : rest.slice(0, 4000);
    for (const other of decl) {
      if (other.name === d.name || other.at < d.at) continue;
      if (new RegExp(`\\b${other.name}\\(`).test(body)) {
        console.log(`${f}: ${d.name} calls ${other.name}, which is declared later (line ${s.slice(0, other.at).split("\n").length})`);
        bad++;
      }
    }
  }
}
console.log(bad ? `${bad} closure-order problem(s)` : "closure order: ok");
process.exit(bad ? 1 : 0);
