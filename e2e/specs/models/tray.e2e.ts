// A download running during a conversation: the activity tray floats
// above the composer and never covers the Ask button (the wizard case of
// the same problem is in specs/fresh). The download is faked through the
// engine's own progress event - nothing is downloaded.
import { shot, waitForChat, ask, waitForReplyDone } from "./_helpers";

describe("activity tray and the composer", () => {
  it("lifts above the composer while a download runs", async () => {
    await waitForChat();
    await ask("Reply with exactly the word: ready");
    await waitForReplyDone();
    await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      await t.event.emit("model-download-progress", { filename: "e2e-fake-model.gguf", downloaded: 900_000_000, total: 2_600_000_000, percent: 35 });
    });
    const tray = await $('[data-testid="activity-tray"]');
    await tray.waitForDisplayed({ timeout: 10_000 });
    // Give the tray its measuring tick, then check geometry and the hit test.
    await browser.pause(1_500);
    const geo = await browser.execute(() => {
      const tray = document.querySelector('[data-testid="activity-tray"]')!.getBoundingClientRect();
      const bar = document.querySelector("[data-bottom-bar]")!.getBoundingClientRect();
      const send = document.querySelector('[data-testid="chat-send"]') as HTMLElement;
      const r = send.getBoundingClientRect();
      const hit = document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2);
      return { trayBottom: tray.bottom, barTop: bar.top, sendOnTop: !!hit && send.contains(hit) };
    });
    await shot("50-tray-above-composer");
    expect(geo.trayBottom).toBeLessThanOrEqual(geo.barTop + 1);
    expect(geo.sendOnTop).toBe(true);
  });
});
