// WebdriverIO against the REAL app (see e2e/README.md).
// `npm run e2e` builds the app with the `e2e` cargo feature (the embedded
// WebDriver server, never in a release build), wipes the scratch profile,
// and runs every spec in e2e/specs. Screenshots land in e2e/shots.
import { execFileSync, spawn, type ChildProcess } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, symlinkSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { SevereServiceError } from "webdriverio";

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

/**
 * The window must be ACTIVE on the desktop, or the compositor withholds
 * frame callbacks: WebKit then produces no frames, requestAnimationFrame
 * and IntersectionObserver never fire, and Qwik's visible tasks (the chat
 * page's first-run check among them) never run. The app's own set_focus is
 * refused by focus-stealing prevention; xdotool's activation is honored.
 * The window can take a while to map on a cold debug build, so poll for it
 * rather than `search --sync` with one long wait. Linux desktop only.
 */
function activateWindow() {
  if (process.platform !== "linux" || !process.env.DISPLAY) return;
  const deadline = Date.now() + 60_000;
  while (Date.now() < deadline) {
    try {
      const ids = execFileSync("xdotool", ["search", "--onlyvisible", "--name", "^Your Own AI$"], { encoding: "utf8", timeout: 5_000 }).trim().split("\n").filter(Boolean);
      if (ids.length) {
        execFileSync("xdotool", ["windowactivate", "--sync", ids[ids.length - 1]], { timeout: 10_000, stdio: "ignore" });
        return;
      }
    } catch { /* not mapped yet, or search found nothing (exit 1) */ }
    execFileSync("sleep", ["1"]);
  }
  console.warn("e2e: could not activate the app window (xdotool found no visible 'Your Own AI' window in 60 s)");
}

/**
 * A locked or blanked screen gives the app no frames either (the same stall
 * as an inactive window, found 2026-10-02: GNOME locked after its 5-minute
 * idle delay mid-run and every later step hung). Refuse to start on a locked
 * screen, and hold off the idle lock for as long as the run lasts. GNOME
 * only; elsewhere both are no-ops.
 */
let idleInhibitor: ChildProcess | null = null;

function screenLocked(): boolean {
  if (process.platform !== "linux") return false;
  try {
    const out = execFileSync("gdbus", ["call", "--session", "--dest", "org.gnome.ScreenSaver", "--object-path", "/org/gnome/ScreenSaver", "--method", "org.gnome.ScreenSaver.GetActive"], { encoding: "utf8", timeout: 5_000 });
    return out.includes("true");
  } catch {
    return false;
  }
}

function holdIdleLock() {
  if (process.platform !== "linux") return;
  try {
    idleInhibitor = spawn("gnome-session-inhibit", ["--inhibit", "idle", "--reason", "Your Own AI UI tests", "sleep", "infinity"], { stdio: "ignore" });
    idleInhibitor.on("error", () => { idleInhibitor = null; });
  } catch { /* not GNOME */ }
}

/**
 * Windows: what launch-app.sh does per launch on Linux, done once per run
 * (cmd cannot write JSON safely). The scratch profile reads the person's
 * real models folder (their own settings' modelsDir, else the default) and,
 * with YOAI_E2E_WITH_CUDA, links their installed engines folder (a
 * junction: no admin rights needed). Nothing is downloaded or written there.
 */
function seedWindowsProfile() {
  if (process.platform !== "win32" || !process.env.APPDATA) return;
  const realData = resolve(process.env.APPDATA, "com.solar.yourowai");
  const testData = resolve(PROFILE, "AppData", "Roaming", "com.solar.yourowai");
  mkdirSync(testData, { recursive: true });
  if (process.env.YOAI_E2E_WITH_MODELS) {
    let models = process.env.YOAI_E2E_MODELS_DIR ?? "";
    if (!models) {
      try { models = JSON.parse(readFileSync(resolve(realData, "settings.json"), "utf8")).modelsDir ?? ""; } catch { /* no settings */ }
    }
    writeFileSync(resolve(testData, "settings.json"), JSON.stringify({ modelsDir: models || resolve(realData, "models") }));
  }
  if (process.env.YOAI_E2E_WITH_CUDA) {
    const engines = process.env.YOAI_E2E_ENGINES_DIR ?? resolve(realData, "engines");
    if (existsSync(engines)) symlinkSync(engines, resolve(testData, "engines"), "junction");
    else console.warn(`e2e: no engines folder at ${engines} - the bundled engine runs (install CUDA in the app first)`);
  }
}

const CAPTURE = !!process.env.YOAI_CAPTURE;

export const config: WebdriverIO.Config = {
  runner: "local",
  // Spec sets by launch mode (e2e/run.mjs): fresh profile (the welcome
  // flow), the installed models (the chat flows), or the video captures.
  specs: [CAPTURE ? "./capture/**/*.capture.ts" : process.env.YOAI_E2E_WITH_MODELS ? "./specs/models/**/*.e2e.ts" : "./specs/fresh/**/*.e2e.ts"],
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
  // A capture take records whole flows, measurements included.
  mochaOpts: { ui: "bdd", timeout: CAPTURE ? 1_800_000 : 300_000 },
  reporters: ["spec"],
  logLevel: "warn",
  waitforTimeout: 30_000,
  onPrepare() {
    if (screenLocked()) throw new SevereServiceError("e2e: the screen is locked - the app gets no frames while it is. Unlock it and run again.");
    holdIdleLock();
    reapSidecars();
    rmSync(PROFILE, { recursive: true, force: true });
    rmSync(SHOTS, { recursive: true, force: true });
    mkdirSync(SHOTS, { recursive: true });
    seedWindowsProfile();
  },
  before: async function () {
    activateWindow();
  },
  beforeTest: async function () {
    // Something else on the desktop may have taken focus mid-run.
    activateWindow();
  },
  onComplete() {
    idleInhibitor?.kill();
  },
  afterSession() {
    reapSidecars();
  },
  afterTest: async function (test, _context, { passed }) {
    if (CAPTURE) return; // a take's stills are its own
    // One picture of where it ended, pass or fail - the thing Claude reads.
    const name = `${test.parent} - ${test.title}`.replace(/[^a-z0-9]+/gi, "_").slice(0, 80);
    await browser.saveScreenshot(resolve(SHOTS, `${passed ? "ok" : "FAIL"}_${name}.png`));
  },
};
