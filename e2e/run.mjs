// Runs WebdriverIO with a launch mode set, the same way on every platform
// (Windows' npm shell has no `VAR=1 cmd` syntax).
//   node e2e/run.mjs              fresh profile: the welcome flow
//   node e2e/run.mjs models       the machine's installed models: the chat flows
//   node e2e/run.mjs capture      screen captures for videos (see e2e/CAPTURE.md)
// Anything after the mode goes to wdio (e.g. --spec e2e/specs/models/chat.e2e.ts).
import { spawnSync } from "node:child_process";

const [mode = "fresh", ...rest] = process.argv.slice(2);
const env = { ...process.env };
if (mode === "models" || mode === "capture") env.YOAI_E2E_WITH_MODELS = "1";
if (mode === "capture") {
  env.YOAI_CAPTURE = "1";
  // Videos run on the machine's real engine: the installed CUDA build when there is one.
  env.YOAI_E2E_WITH_CUDA ??= "1";
}
const r = spawnSync("npx", ["wdio", "run", "e2e/wdio.conf.ts", ...rest], { stdio: "inherit", env, shell: process.platform === "win32" });
process.exit(r.status ?? 1);
