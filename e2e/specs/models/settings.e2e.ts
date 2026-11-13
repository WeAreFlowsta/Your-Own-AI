// Flow 3: a reply-style setting applies and survives a reload. The sliders
// apply instantly (no Save on pages; Save lives in modals).
import { shot, invoke } from "./_helpers";

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

  // Settings > General > Automatic updates: the switch is the person's
  // choice in the updater itself (not a page-only flag), kept across a
  // reload, and turning it back on is as plain.
  it("turns automatic updates off and on, and the updater keeps the choice", async () => {
    const sw = '[data-testid="auto-updates"]';
    const open = async () => {
      await browser.url("tauri://localhost/settings/");
      const el = await $(sw);
      await el.waitForExist({ timeout: 60_000 });
      await el.scrollIntoView();
      return el;
    };
    const saved = async () => (await invoke<{ auto: boolean }>("update_status")).auto;
    const shown = async (el: WebdriverIO.Element) => (await el.getAttribute("aria-pressed")) === "true";

    let el = await open();
    await browser.waitUntil(async () => (await shown(el)) === (await saved()), { timeout: 15_000, timeoutMsg: "the switch never showed the saved choice" });
    expect(await saved()).toBe(true);
    await el.click();
    await browser.waitUntil(async () => !(await saved()), { timeout: 10_000, timeoutMsg: "the updater still says automatic updates are on" });
    expect(await shown(el)).toBe(false);
    await shot("32-auto-updates-off");

    el = await open();
    await browser.waitUntil(async () => !(await shown(el)), { timeout: 15_000, timeoutMsg: "after a reload the switch showed on again" });
    expect(await saved()).toBe(false);

    await el.click();
    await browser.waitUntil(async () => saved(), { timeout: 10_000, timeoutMsg: "the updater never took automatic updates back on" });
    expect(await shown(el)).toBe(true);
  });
});
