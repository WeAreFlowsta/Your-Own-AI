// Diagnostic, changes nothing: opens Edit on the first AI of Your AIs and lists the dialog's buttons and tabs.
import { appUrl, SHOTS } from "../wdio.conf";
import { resolve } from "node:path";

describe("capture", () => {
  it("peek edit", async () => {
    await browser.url(appUrl("/your-ais/"));
    await browser.waitUntil(async () => (await $$('button[title^="Edit AI"]')).length >= 1, { timeout: 60_000 });
    console.log("peek: titles " + JSON.stringify(await browser.execute(() => [...document.querySelectorAll("[title]")].map((e) => e.getAttribute("title")).filter((t) => t && t.length < 30))));
    await (await $$('button[title^="Edit AI"]'))[0].click();
    await browser.pause(3000);
    await browser.saveScreenshot(resolve(SHOTS, "peek-edit.png"));
    console.log("peek: buttons " + JSON.stringify(await browser.execute(() => [...document.querySelectorAll("button, [role=tab]")].map((b) => (b as HTMLElement).innerText.trim()).filter(Boolean))));
  });
});
