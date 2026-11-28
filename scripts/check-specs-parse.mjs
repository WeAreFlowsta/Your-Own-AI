// The UI test files are not part of the type check (tsconfig covers src),
// so a broken edit in one shows only when its run starts, minutes into a
// queue. This parses every spec the way the test runner does.
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { transformSync } from "esbuild";

const root = fileURLToPath(new URL("../e2e/", import.meta.url));
const files = [];
const walk = (dir) => {
  for (const name of readdirSync(dir)) {
    const path = join(dir, name);
    if (statSync(path).isDirectory()) {
      if (name !== "profile" && name !== "shots" && name !== "node_modules") walk(path);
    } else if (name.endsWith(".ts")) files.push(path);
  }
};
walk(root);
const bad = [];
for (const file of files) {
  try {
    transformSync(readFileSync(file, "utf8"), { loader: "ts", sourcefile: file });
  } catch (e) {
    bad.push(`${file.replace(root, "e2e/")}: ${String(e.errors?.[0]?.text ?? e.message)} (line ${e.errors?.[0]?.location?.line ?? "?"})`);
  }
}
if (bad.length) {
  console.error(bad.join("\n"));
  console.error(`${bad.length} UI test file(s) do not parse`);
  process.exit(1);
}
console.log(`check-specs-parse: ok (${files.length} files)`);
