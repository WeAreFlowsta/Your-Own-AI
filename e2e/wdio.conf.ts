// WebdriverIO against the REAL app (build-docs planning/UI_AUTOMATION.md).
// `npm run e2e` builds the app with the `e2e` cargo feature (the embedded
// WebDriver server, never in a release build), wipes the scratch profile,
// and runs every spec in e2e/specs. Screenshots land in e2e/shots.
import { execFileSync } from "node:child_process";
import { mkdirSync, rmSync } from "node:fs";
import { resolve } from "node:path";

const here = resolve(import.meta.dirname ?? ".");
export const SHOTS = resolve(here, "shots");
const PROFILE = resolve(here, "profile");
const launcher = resolve(here, process.platform === "win32" ? "launch-app.cmd" : "launch-app.sh");

export const config: WebdriverIO.Config = {
  runner: "local",
  // Two spec sets, one per launch mode (see launch-app.sh): fresh profile
  // (the welcome flow) or the installed models (the chat flows).
  specs: [process.env.YOAI_E2E_WITH_MODELS ? "./specs/models/**/*.e2e.ts" : "./specs/fresh/**/*.e2e.ts"],
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
  before: async function () {
    // The window must be ACTIVE on the desktop, or the compositor withholds
    // frame callbacks: WebKit then produces no frames, requestAnimationFrame
    // and IntersectionObserver never fire, and Qwik's visible tasks (the
    // chat page's first-run check among them) never run. The app's own
    // set_focus is refused by focus-stealing prevention; xdotool's
    // activation is honored. Linux desktop only; harmless where absent.
    if (process.platform === "linux" && process.env.DISPLAY) {
      try {
        const id = execFileSync("xdotool", ["search", "--sync", "--name", "^Your Own AI$"], { encoding: "utf8", timeout: 30_000 }).trim().split("\n")[0];
        if (id) execFileSync("xdotool", ["windowactivate", "--sync", id], { timeout: 10_000, stdio: "ignore" });
      } catch (e) {
        console.warn("e2e: could not activate the app window (xdotool):", (e as Error).message);
      }
    }
  },
  afterTest: async function (test, _context, { passed }) {
    // One picture of where it ended, pass or fail - the thing Claude reads.
    const name = `${test.parent} - ${test.title}`.replace(/[^a-z0-9]+/gi, "_").slice(0, 80);
    await browser.saveScreenshot(resolve(SHOTS, `${passed ? "ok" : "FAIL"}_${name}.png`));
  },
};
