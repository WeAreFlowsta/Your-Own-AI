// Every build check must pass on a Windows checkout too. The Windows release
// runner checks the repo out with CRLF line endings (no .gitattributes), and
// 0.8.0-beta.1 died there: a check cut a function out of the source by its
// "\n}\n" end, found nothing, and failed only on Windows - after every other
// platform had built. This runs each source-reading check against a copy of
// the tree with every line ending turned into CRLF. Runs in `npm run build`.
//
// A new scripts/check-*.mjs joins by being listed below. check-webview-floor
// reads the built output, not the source, and is left out.
import { cpSync, mkdtempSync, readdirSync, readFileSync, rmSync, statSync, symlinkSync, writeFileSync, existsSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { spawnSync } from "node:child_process";

const CHECKS = [
  "check-closure-order.mjs",
  "check-reply-boundary.mjs",
  "check-recall-margin.mjs",
  "check-unsaved-settings.mjs",
  "check-tools-gate.mjs",
  "check-action-labels.mjs",
];
// Every check script must be listed here or named as skipped.
const SKIPPED = ["check-webview-floor.mjs", "check-crlf.mjs", "check-catalog-urls.mjs", "check-online-catalog.mjs"];
const all = readdirSync("scripts").filter((f) => /^check-.*\.mjs$/.test(f));
const unlisted = all.filter((f) => !CHECKS.includes(f) && !SKIPPED.includes(f));
if (unlisted.length) {
  console.error(`crlf check: ${unlisted.join(", ")} is not listed in scripts/check-crlf.mjs - add it to CHECKS (or SKIPPED, with the reason)`);
  process.exit(1);
}

const root = mkdtempSync(join(tmpdir(), "yoai-crlf-"));
const toCrlf = (dir) => {
  for (const name of readdirSync(dir)) {
    const p = join(dir, name);
    if (statSync(p).isDirectory()) toCrlf(p);
    else if (/\.(ts|tsx|mjs|js|rs|json|css)$/.test(name)) {
      const text = readFileSync(p, "utf8");
      writeFileSync(p, text.replace(/\r?\n/g, "\r\n"));
    }
  }
};
let bad = 0;
try {
  cpSync("src", join(root, "src"), { recursive: true });
  cpSync("scripts", join(root, "scripts"), { recursive: true });
  cpSync("src-tauri/src/router.rs", join(root, "src-tauri/src/router.rs"));
  toCrlf(join(root, "src"));
  toCrlf(join(root, "src-tauri"));
  if (existsSync("node_modules")) symlinkSync(join(process.cwd(), "node_modules"), join(root, "node_modules"), "dir");
  for (const c of CHECKS) {
    const r = spawnSync(process.execPath, [join("scripts", c)], { cwd: root, encoding: "utf8" });
    if (r.status !== 0) {
      bad++;
      console.error(`crlf check: ${c} fails on a Windows (CRLF) checkout:\n${(r.stderr || r.stdout).trim().split("\n").slice(-6).join("\n")}`);
    }
  }
} finally {
  rmSync(root, { recursive: true, force: true });
}
if (bad) process.exit(1);
console.log(`crlf check: ${CHECKS.length} build checks pass on CRLF line endings`);
