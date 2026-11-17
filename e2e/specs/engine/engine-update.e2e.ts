// After an app update that pins a newer NVIDIA engine: the profile holds the
// engine of an earlier version, the app fetches the pinned one by itself as
// it starts, and the model loads on it when it is in place. Nothing is
// pressed. Run by e2e/engine-update-live.sh, which seeds the earlier
// engine and serves this machine's installed one as the download.
import { waitForChat, invoke } from "../models/_helpers";

type Engine = { installed: boolean; stale_version_installed: boolean; arriving: boolean; running_backend: string | null; active_backend: string };
const engine = () => invoke<Engine>("engine_status");

describe("the NVIDIA engine after an update", () => {
  it("arrives by itself and the model loads on it", async function () {
    if (!process.env.YOAI_TEST_CUDA_ZIP_URL) this.skip();
    this.timeout(900_000);
    const first = await engine();
    console.log(`e2e: engine at start ${JSON.stringify(first)}`);
    // Either still on its way or already in place; never left for a button.
    expect(first.arriving || first.installed).toBe(true);

    // The tray says what the download is part of, while it runs.
    let sawRow = false;
    await browser.waitUntil(
      async () => {
        if (!sawRow) {
          sawRow = await browser.execute(() =>
            Array.from(document.querySelectorAll("[data-testid='activity-tray'] *")).some((n) => n.textContent === "Finishing the update: NVIDIA engine"),
          );
        }
        return (await engine()).installed;
      },
      { timeout: 600_000, interval: 500, timeoutMsg: "the engine did not arrive by itself" },
    );
    const arrived = await engine();
    console.log(`e2e: engine arrived ${JSON.stringify(arrived)}; tray row seen: ${sawRow}`);
    expect(arrived.arriving).toBe(false);
    expect(arrived.stale_version_installed).toBe(false); // the earlier one is gone
    expect(arrived.active_backend).toBe("cuda");

    // The model that was asked for at start is up, on the NVIDIA engine.
    await waitForChat();
    await browser.waitUntil(async () => (await engine()).running_backend === "cuda", {
      timeout: 300_000,
      interval: 1_000,
      timeoutMsg: "the model is not running on the NVIDIA engine",
    });
    expect(sawRow).toBe(true);
    // No offer to update it by hand anywhere on the home page.
    const offer = await browser.execute(() => document.body.innerText.includes("Your NVIDIA engine needs an update"));
    expect(offer).toBe(false);
  });
});
