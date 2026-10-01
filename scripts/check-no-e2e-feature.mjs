// The `e2e` cargo feature compiles an embedded WebDriver server into the
// app (planning/UI_AUTOMATION.md). It must never reach a release: this
// check fails the build if the feature is on by default or if any release
// workflow passes it. `npm run e2e` is the only caller allowed to use it.
import { readFileSync, readdirSync } from "node:fs";
const cargo = readFileSync("src-tauri/Cargo.toml", "utf8");
const features = cargo.split("[features]")[1]?.split(/\n\[/)[0] ?? "";
const defaultLine = features.match(/^default\s*=\s*\[(.*)\]/m)?.[1] ?? "";
let bad = [];
if (/e2e/.test(defaultLine)) bad.push("Cargo.toml: `e2e` is in the default features");
if (!/^e2e\s*=/m.test(features)) bad.push("Cargo.toml: the `e2e` feature is missing (the plugin would be unconditional)");
if (!/optional = true/.test(cargo.match(/tauri-plugin-wdio-webdriver[^\n]*/)?.[0] ?? "")) bad.push("Cargo.toml: tauri-plugin-wdio-webdriver is not optional");
for (const f of readdirSync(".github/workflows")) {
  const y = readFileSync(`.github/workflows/${f}`, "utf8");
  if (/--features[^\n]*e2e|features:[^\n]*e2e/.test(y)) bad.push(`.github/workflows/${f}: passes the e2e feature`);
}
if (bad.length) { console.error("check-no-e2e-feature:\n  " + bad.join("\n  ")); process.exit(1); }
console.log("check-no-e2e-feature: ok (e2e is optional, off by default, and no workflow passes it)");
