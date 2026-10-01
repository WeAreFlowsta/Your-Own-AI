// Flow 3: a reply-style setting applies and survives a reload. The sliders
// apply instantly (no Save on pages; Save lives in modals).
import { shot } from "./_helpers";

const slider = '[data-testid="reply-temperature"]';

describe("settings", () => {
  it("moves the temperature slider and finds it kept after a reload", async () => {
    await browser.url("tauri://localhost/settings/");
    const s = await $(slider);
    await s.waitForExist({ timeout: 60_000 });
    await s.scrollIntoView();
    await shot("30-settings");
    const min = Number(await s.getAttribute("min"));
    const max = Number(await s.getAttribute("max"));
    const target = Math.round((min + (max - min) * 0.25) * 100) / 100;
    await browser.execute((sel: string, v: number) => {
      const el = document.querySelector(sel) as HTMLInputElement;
      el.value = String(v);
      el.dispatchEvent(new Event("input", { bubbles: true }));
      el.dispatchEvent(new Event("change", { bubbles: true }));
    }, slider, target);
    await browser.pause(1_000); // the 400 ms save debounce
    await shot("31-moved");
    await browser.url("tauri://localhost/settings/");
    const again = await $(slider);
    await again.waitForExist({ timeout: 60_000 });
    // The section reads its saved values when it scrolls INTO VIEW (a Qwik
    // visible task on an intersection observer), so scroll there first, then
    // wait for the slider to settle on the kept value.
    await again.scrollIntoView();
    let kept = NaN;
    await browser.waitUntil(async () => { kept = Number(await again.getValue()); return Math.abs(kept - target) < 0.011; }, {
      timeout: 15_000,
      timeoutMsg: () => `the slider settled on ${kept}, wanted ${target}`,
    });
  });
});
