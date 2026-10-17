// The activity tray's row for an engine download says what is happening at
// every step: the percent and bytes while it runs, "Installing..." while the
// engine is unpacked, "Installed" when it is in use - never a running figure
// next to Dismiss (Windows, 10-17: "11% · 80
// MB of 728 MB" with Dismiss after the CUDA engine had installed). The app's
// own events are sent from the page, so no real download is needed; another
// row comes and goes partway through, as notes do.
import { shot, waitForChat } from "./_helpers";

const FILE = "llama-server-cuda-test.zip";
const TOTAL = 728 * 1024 * 1024;

const rowText = () =>
  browser.execute(() => {
    const tray = document.querySelector('[data-testid="activity-tray"]') as HTMLElement | null;
    return tray ? tray.innerText : "";
  });

const emit = (event: string, payload: unknown) =>
  browser.execute(async (e: string, p: unknown) => {
    await (window as any).__TAURI__.event.emit(e, p);
  }, event, payload as never);

describe("the tray's download row", () => {
  it("shows the running figures while it runs and Downloaded when it ends", async function () {
    this.timeout(180_000);
    await waitForChat();
    const seen: string[] = [];
    for (let pct = 0; pct <= 100; pct += 1) {
      await emit("model-download-progress", { filename: FILE, downloaded: Math.round((TOTAL * pct) / 100), total: TOTAL, percent: pct });
      if (pct === 11) {
        // Another row arrives and leaves while the download runs.
        await browser.execute(() => window.dispatchEvent(new CustomEvent("yoai-activity", { detail: { id: "e2e-note", title: "A note", detail: "Comes and goes", state: "done", ttlMs: 1500 } })));
      }
      if (pct % 10 === 1) {
        await browser.pause(120);
        const t = await rowText();
        seen.push(`${pct}: ${t.replace(/\n/g, " | ")}`);
      }
    }
    await browser.pause(300);
    const before = await rowText();
    await emit("model-download-complete", { filename: FILE });
    await browser.pause(500);
    const unpacking = await rowText();
    // The engine is unpacked and put in use next: the row says so.
    await emit("engine-installed", "llama-test");
    await browser.pause(500);
    const after = await rowText();
    await shot("tray-download-done");
    for (const s of seen) console.log(`e2e: ${s}`);
    console.log(`e2e: at 100% ${JSON.stringify(before)}`);
    console.log(`e2e: after the download ${JSON.stringify(unpacking)}`);
    console.log(`e2e: after the install ${JSON.stringify(after)}`);
    expect(before).toContain("100%");
    expect(unpacking).toContain("Installing");
    expect(unpacking).not.toContain("Dismiss");
    expect(after).toContain("Installed - your models use it now");
    expect(after).not.toMatch(/\d+% ·/);
  });
});
