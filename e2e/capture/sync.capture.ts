// Take: device A's half of the sync beat. On a kept shoot profile signed in to Flowsta, Nova gets a new picture from
// the gallery on the Your AIs page; the identity's other device (recorded there) shows it arrive about a minute
// later. Logs the save's wall-clock time (UTC) so the two recordings line up in the edit.
//   YOAI_CAPTURE_AI_NAME     the AI (default Nova)    YOAI_CAPTURE_NEW_PICTURE  the gallery picture (default Ocean)
import { appUrl } from "../wdio.conf";
import { Take } from "./_capture";

const name = process.env.YOAI_CAPTURE_AI_NAME ?? "Nova";
const picture = process.env.YOAI_CAPTURE_NEW_PICTURE ?? "Ocean";

/** Tags the first element matching `css` whose text includes `text` (or the card titled `text`) so a take can aim at it. */
const tag = (css: string, text: string, as: string) =>
  browser.execute((c: string, t: string, a: string) => {
    const el = [...document.querySelectorAll(c)].find((e) => (e as HTMLElement).innerText.includes(t) || e.getAttribute("title") === t);
    el?.setAttribute("data-shoot", a);
    return !!el;
  }, css, text, as);

describe("capture", () => {
  it("sync", async () => {
    await browser.url(appUrl("/your-ais/"));
    await browser.waitUntil(async () => (await $$('button[title^="Edit AI"]')).length >= 3, { timeout: 60_000, timeoutMsg: "Your AIs never listed the AIs" });
    // Nova's card: the Edit button in the card that carries its name.
    const found = await browser.execute((n: string) => {
      const label = [...document.querySelectorAll("[title]")].find((e) => e.getAttribute("title") === n);
      let card: Element | null = label ?? null;
      while (card && !card.querySelector('button[title^="Edit AI"]')) card = card.parentElement;
      card?.querySelector('button[title^="Edit AI"]')?.setAttribute("data-shoot", "edit");
      card?.setAttribute("data-shoot-card", "1");
      return !!card;
    }, name);
    if (!found) throw new Error(`capture: no AI called ${name} on Your AIs`);

    // The page's first-visit tip, out of the shot.
    const gotIt = await $("button=Got it");
    if (await gotIt.isExisting()) { await gotIt.click(); await browser.pause(500); }

    const take = await Take.start("sync-a");
    await take.hold(1200);
    await take.still("01-your-ais");
    await take.hover("[data-shoot-card]", 900);
    await take.click('[data-shoot="edit"]');
    await browser.waitUntil(async () => tag("button", "Appearance", "appearance"), { timeout: 15_000, timeoutMsg: "the edit dialog never opened" });
    await take.hold(700);
    await take.click('[data-shoot="appearance"]');
    await browser.waitUntil(async () => tag("button", "Choose from Gallery", "gallery"), { timeout: 15_000, timeoutMsg: "no Choose from Gallery on Appearance" });
    await take.hold(800);
    await take.scrollTo('[data-shoot="gallery"]');
    await take.hold(500);
    await take.click('[data-shoot="gallery"]');
    await (await $(`button[title="${picture}"]`)).waitForDisplayed({ timeout: 10_000 });
    await take.hold(800);
    await take.still("02-gallery");
    await take.hover(`button[title="${picture}"]`, 500);
    await take.click(`button[title="${picture}"]`);
    await browser.waitUntil(async () => tag("button", "Save Changes", "save"), { timeout: 15_000 });
    await take.hold(900);
    await take.scrollTo('[data-shoot="save"]');
    await take.click('[data-shoot="save"]');
    console.log(`sync: SAVED at ${new Date().toISOString()}`);
    await (await $('[data-shoot="save"]')).waitForExist({ reverse: true, timeout: 30_000 });
    await take.hold(1500);
    await take.still("03-saved");
    await take.hover("[data-shoot-card]", 2500);
    await take.stop();
  });
});
