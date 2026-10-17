// The activity tray's row for a download says what is happening at every
// step: the percent and bytes while it runs, "Installing..." while an
// engine or the Projects helper is unpacked, then how it ended - never a
// running figure next to Dismiss (Windows, 10-17: "11% · 80 MB of 728 MB"
// with Dismiss after the CUDA engine had installed). The app's own events
// are sent from the page, so no real download is needed; another row comes
// and goes partway through, as notes do.
import { shot, waitForChat } from "./_helpers";


const rowText = () =>
  browser.execute(() => {
    const tray = document.querySelector('[data-testid="activity-tray"]') as HTMLElement | null;
    return tray ? tray.innerText : "";
  });

const emit = (event: string, payload: unknown) =>
  browser.execute(async (e: string, p: unknown) => {
    await (window as any).__TAURI__.event.emit(e, p);
  }, event, payload as never);

// Each kind of download: the file, the name the tray gives it, how it ends
// (the engine and the Projects helper are unpacked after their download).
const KINDS = [
  { file: "llama-server-cuda-test.zip", name: "CUDA engine", installed: { event: "engine-installed", payload: "llama-test" as unknown, says: "Installed - your models use it now" } },
  { file: "your-own-ai-build-test.zip", name: "Projects helper", installed: { event: "build-install-done", payload: { path: "/tmp/none" } as unknown, says: "Installed - open a project folder to use it" } },
  { file: "embeddinggemma-300M-Q8_0.gguf", name: "Memory model", installed: null },
  { file: "e2e-test-model-Q4_K_M.gguf", name: "e2e-test-model-Q4_K_M", installed: null },
];

describe("the tray's download rows", () => {
  for (const k of KINDS) {
    it(`${k.name}: running figures while it runs, then how it ended`, async function () {
      this.timeout(180_000);
      await waitForChat();
      const total = 300 * 1024 * 1024;
      let mid = "";
      for (let pct = 0; pct <= 100; pct += 2) {
        await emit("model-download-progress", { filename: k.file, downloaded: Math.round((total * pct) / 100), total, percent: pct });
        if (pct === 10) {
          // Another row arrives and leaves while the download runs.
          await browser.execute(() => window.dispatchEvent(new CustomEvent("yoai-activity", { detail: { id: "e2e-note", title: "A note", detail: "Comes and goes", state: "done", ttlMs: 1000 } })));
        }
        if (pct === 60) {
          await browser.pause(150);
          mid = await rowText();
        }
      }
      await browser.pause(300);
      const full = await rowText();
      await emit("model-download-complete", { filename: k.file });
      await browser.pause(500);
      const ended = await rowText();
      let installed = "";
      if (k.installed) {
        await emit(k.installed.event, k.installed.payload);
        await browser.pause(500);
        installed = await rowText();
      }
      console.log(`e2e: ${k.name} at 60% ${JSON.stringify(mid)} | at 100% ${JSON.stringify(full)} | ended ${JSON.stringify(ended)} | installed ${JSON.stringify(installed)}`);
      expect(mid).toContain(k.name);
      expect(mid).toContain("60%");
      expect(full).toContain("100%");
      if (k.installed) {
        expect(ended).toContain("Installing");
        expect(ended).not.toContain("Dismiss");
        expect(installed).toContain(k.installed.says);
        expect(installed).not.toMatch(/\d+% ·/);
      } else {
        expect(ended).toContain("Downloaded");
        expect(ended).not.toMatch(/\d+% ·/);
      }
      await shot(`tray-${k.name.replace(/\W+/g, "-").toLowerCase()}`);
      // Clear the row before the next kind.
      await browser.execute(() => {
        for (const b of document.querySelectorAll('[data-testid="activity-tray"] button')) if (b.textContent?.trim() === "Dismiss") (b as HTMLElement).click();
      });
      await browser.pause(300);
    });
  }
});
