// WebdriverIO against the REAL app (build-docs planning/UI_AUTOMATION.md).
// `npm run e2e` builds the app with the `e2e` cargo feature (the embedded
// WebDriver server, never in a release build), wipes the scratch profile,
// and runs every spec in e2e/specs. Screenshots land in e2e/shots.
import { mkdirSync, rmSync } from "node:fs";
import { resolve } from "node:path";

const here = resolve(import.meta.dirname ?? ".");
export const SHOTS = resolve(here, "shots");
const PROFILE = resolve(here, "profile");
const launcher = resolve(here, process.platform === "win32" ? "launch-app.cmd" : "launch-app.sh");

export const config: WebdriverIO.Config = {
  runner: "local",
  specs: ["./specs/**/*.e2e.ts"],
  maxInstances: 1,
  capabilities: [
    {
      browserName: "tauri",
      "tauri:options": { application: launcher },
    },
  ],
  services: [
    [
      "@wdio/tauri-service",
      {
        driverProvider: "embedded",
        appBinaryPath: launcher,
        captureBackendLogs: true,
        captureFrontendLogs: true,
        startTimeout: 90_000,
      },
    ],
  ],
  framework: "mocha",
  mochaOpts: { ui: "bdd", timeout: 120_000 },
  reporters: ["spec"],
  logLevel: "warn",
  waitforTimeout: 30_000,
  onPrepare() {
    rmSync(PROFILE, { recursive: true, force: true });
    rmSync(SHOTS, { recursive: true, force: true });
    mkdirSync(SHOTS, { recursive: true });
  },
  afterTest: async function (test, _context, { passed }) {
    // One picture of where it ended, pass or fail - the thing Claude reads.
    const name = `${test.parent} - ${test.title}`.replace(/[^a-z0-9]+/gi, "_").slice(0, 80);
    await browser.saveScreenshot(resolve(SHOTS, `${passed ? "ok" : "FAIL"}_${name}.png`));
  },
};
