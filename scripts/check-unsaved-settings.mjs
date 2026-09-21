// "Not saved yet" must come AND go: typing then deleting leaves nothing
// unsaved; a secret's text is always new; a setting that does not apply
// (its `unless` switch is on) is never unsaved. Runs in `npm run build`.
import { readFileSync } from "node:fs";
import { transformSync } from "esbuild";

const src = readFileSync("src/utils/mcp.ts", "utf8");
const start = src.indexOf("export function unsavedSettings(");
const end = src.indexOf("\n}\n", start) + 3;
const js = transformSync(src.slice(start, end).replace("export ", ""), { loader: "ts" }).code;
const unsavedSettings = new Function(`${js}; return unsavedSettings;`)();

const config = [
  { key: "TOKEN", label: "API token", kind: "secret", required: true },
  { key: "READ_ONLY", label: "Read only - look, not change", kind: "toggle", default: "on" },
  { key: "DB", label: "Database graph", kind: "toggle", default: "off" },
  { key: "PATH", label: "Graph folder - files only", kind: "path", unless: "DB" },
];
const stored = { READ_ONLY: "off", DB: "on" };
const clean = { TOKEN: "", READ_ONLY: "off", DB: "on", PATH: "" };
const cases = [
  ["nothing touched", clean, []],
  ["typed a token", { ...clean, TOKEN: "abc" }, ["API token"]],
  ["typed then deleted", { ...clean, TOKEN: "" }, []],
  ["only spaces in the token", { ...clean, TOKEN: "   " }, []],
  ["ticked read only", { ...clean, READ_ONLY: "on" }, ["Read only"]],
  ["ticked and unticked", { ...clean, READ_ONLY: "off" }, []],
  ["a folder while the database switch is on", { ...clean, PATH: "/x" }, []],
  ["a folder with the switch off", { ...clean, DB: "off", PATH: "/x" }, ["Database graph", "Graph folder"]],
  ["a default that was never stored", { TOKEN: "", READ_ONLY: "on", DB: "off", PATH: "" }, [], {}],
];
let bad = 0;
for (const [name, draft, want, st] of cases) {
  const got = unsavedSettings(config, draft, st ?? stored);
  if (JSON.stringify(got) !== JSON.stringify(want)) {
    console.error(`unsaved settings: "${name}" gave ${JSON.stringify(got)}, wanted ${JSON.stringify(want)}`);
    bad++;
  }
}
if (bad) process.exit(1);
console.log(`unsaved settings check: ${cases.length} cases pass`);
