// WebdriverIO against the REAL app (see e2e/README.md).
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

/**
 * Each spec file is its own session, and the service ends the app between
 * them - but not the app's sidecars (the engine, the embedding server, the
 * conductor). An orphaned engine kept 2.3 GB of a 4 GB card between two
 * sessions and the next spec's model had no room. Kill, by pid, every
 * process whose program lives in src-tauri/bin and whose parent is not a
 * live test app (the desktop's session manager adopts orphans, so a parent
 * of 1 is not the test). Linux and macOS; Windows later.
 */
function reapSidecars() {
  if (process.platform === "win32") return;
  try {
    const bin = resolve(here, "..", "src-tauri", "bin") + "/";
    const appBin = resolve(here, "..", "src-tauri", "target", "debug", "app");
    const rows = execFileSync("ps", ["-eo", "pid=,ppid=,args="], { encoding: "utf8" }).split("\n");
    const parsed = rows.map((r) => r.trim().match(/^(\d+)\s+(\d+)\s+(.*)$/)).filter(Boolean) as RegExpMatchArray[];
    const liveApps = new Set(parsed.filter((m) => m[3] === appBin || m[3].startsWith(appBin + " ")).map((m) => m[1]));
    for (const m of parsed) {
      const [, pid, ppid, args] = m;
      if (args.startsWith(bin) && !liveApps.has(ppid)) {
        try { process.kill(Number(pid), "SIGTERM"); console.log(`e2e: reaped orphaned sidecar ${pid}: ${args.slice(bin.length, bin.length + 40)}`); } catch { /* gone */ }
      }
    }
  } catch (e) {
    console.warn("e2e: sidecar reap skipped:", (e as Error).message);
  }
}

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
  // A cold model load plus a reply can take three minutes on a small card.
  mochaOpts: { ui: "bdd", timeout: 300_000 },
  reporters: ["spec"],
  logLevel: "warn",
  waitforTimeout: 30_000,
  onPrepare() {
    reapSidecars();
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
  afterSession() {
    reapSidecars();
  },
  afterTest: async function (test, _context, { passed }) {
    // One picture of where it ended, pass or fail - the thing Claude reads.
    const name = `${test.parent} - ${test.title}`.replace(/[^a-z0-9]+/gi, "_").slice(0, 80);
    await browser.saveScreenshot(resolve(SHOTS, `${passed ? "ok" : "FAIL"}_${name}.png`));
  },
};
