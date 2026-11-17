// The parts a new version needs, fetched ahead of the update. Run twice by
// e2e/parts-ahead-live.sh:
//   PARTS_AHEAD_PHASE=before  this launch is "the version doing the update":
//     it is handed a manifest's parts (a test-build door) and fetches the
//     ones this profile has, each into its own place, checked by its hash.
//   PARTS_AHEAD_PHASE=after   this launch is "the new version": what was
//     fetched ahead is put in place at start, with nothing downloaded.
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { waitForChat, invoke } from "../models/_helpers";

const phase = process.env.PARTS_AHEAD_PHASE;
const data = process.env.PARTS_AHEAD_DATA ?? "";
const pinnedEngine = process.env.PARTS_AHEAD_ENGINE ?? "";
type Build = { installed: boolean; installed_version: string | null; pinned_version: string; downloading: boolean };
type Engine = { installed: boolean; stale_version_installed: boolean; arriving: boolean; running_backend: string | null };

describe(`parts fetched ahead of an update (${phase})`, () => {
  it("the version doing the update fetches the named parts this profile has", async function () {
    if (phase !== "before") this.skip();
    this.timeout(900_000);
    const helperNext = join(data, "build.next");
    const engineNext = join(data, "engines", "cuda-b99999");
    await browser.waitUntil(
      async () => existsSync(join(helperNext, "VERSION")) && existsSync(join(engineNext, "fetched-ahead")),
      { timeout: 780_000, interval: 1_000, timeoutMsg: "the named parts were not fetched ahead" },
    );
    // The helper: kept beside the installed one, at the named version.
    const build = await invoke<Build>("build_install_status");
    expect(readFileSync(join(helperNext, "VERSION"), "utf8").trim()).toBe(build.pinned_version);
    expect(existsSync(join(helperNext, "your-own-ai-build"))).toBe(true);
    // The engine: in its own versioned folder, which this version does not use.
    expect(existsSync(join(engineNext, "llama-server"))).toBe(true);
    // This version's own engine arrived as well (the earlier one was stale),
    // and its clean-up left the one fetched ahead alone.
    await browser.waitUntil(async () => (await invoke<Engine>("engine_status")).installed, { timeout: 600_000, interval: 1_000 });
    expect(existsSync(join(engineNext, "fetched-ahead"))).toBe(true);
    expect(existsSync(join(data, "engines", "cuda-b0000"))).toBe(false);
    console.log(`e2e: fetched ahead: helper ${build.pinned_version} in build.next, engine in cuda-b99999`);
  });

  it("the new version puts them in place at start, with nothing downloaded", async function () {
    if (phase !== "after") this.skip();
    this.timeout(600_000);
    await waitForChat();
    const build = await invoke<Build>("build_install_status");
    console.log(`e2e: helper at the new version's start ${JSON.stringify(build)}`);
    expect(build.installed_version).toBe(build.pinned_version);
    expect(build.downloading).toBe(false);
    expect(existsSync(join(data, "build.next"))).toBe(false);
    const engine = await invoke<Engine>("engine_status");
    console.log(`e2e: engine at the new version's start ${JSON.stringify(engine)}`);
    expect(engine.installed).toBe(true);
    expect(engine.arriving).toBe(false);
    expect(engine.stale_version_installed).toBe(false);
    expect(existsSync(join(data, "engines", `cuda-${pinnedEngine}`, "fetched-ahead"))).toBe(false);
    expect(existsSync(join(data, "engines", "cuda-b99999"))).toBe(false);
    await browser.waitUntil(async () => (await invoke<Engine>("engine_status")).running_backend === "cuda", {
      timeout: 300_000,
      interval: 1_000,
      timeoutMsg: "the model is not running on the NVIDIA engine",
    });
  });
});
