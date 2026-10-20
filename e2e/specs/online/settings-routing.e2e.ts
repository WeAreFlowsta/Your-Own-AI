// Settings > Routing, as someone without a plan sees it and then with one:
// the offline part first (for everyone), the online part second, how
// routing decides last. Pictures (e2e/shots/routing-*.png) plus the order
// of the section's headings, which must hold in both states.
import { appUrl } from "../../wdio.conf";
import { shot, waitForChat } from "../models/_helpers";
import { haveIdentity, signInAsTestIdentity } from "./_online";

const headings = () =>
  browser.execute(() =>
    [...document.querySelectorAll("#settings-routing h3, #settings-routing h4")].map((h) => (h as HTMLElement).innerText.trim()),
  ) as Promise<string[]>;

async function scrollToHeading(text: string) {
  await browser.execute((t: string) => {
    const h = [...document.querySelectorAll("#settings-routing h2, #settings-routing h3, #settings-routing h4")].find((x) => (x as HTMLElement).innerText.trim() === t) as HTMLElement | undefined;
    h?.scrollIntoView({ block: "start", behavior: "instant" as ScrollBehavior });
  }, text);
  await browser.pause(400);
}

async function openRouting() {
  await browser.url(appUrl("/settings/"));
  await (await $("#settings-routing")).waitForExist({ timeout: 60_000 });
  await browser.pause(2_000);
}

describe("Settings > Routing, offline part first, online part second", () => {
  it("without a plan: offline settings for everyone, one line for online", async function () {
    this.timeout(180_000);
    await waitForChat();
    await openRouting();
    const h = await headings();
    console.log(`e2e: no plan headings ${JSON.stringify(h)}`);
    expect(h.indexOf("Offline models")).toBeGreaterThanOrEqual(0);
    expect(h.indexOf("Offline models")).toBeLessThan(h.indexOf("Online models"));
    expect(h.indexOf("Online models")).toBeLessThan(h.indexOf("How routing decides"));
    // The offline settings are there without a plan.
    expect(h).toContain("Choosing a model");
    expect(h).toContain("Health questions");
    // No online controls without a plan.
    expect(h).not.toContain("How much goes online");
    await scrollToHeading("Routing");
    await shot("routing-noplan-1");
    await scrollToHeading("Online models");
    await shot("routing-noplan-2");
  });

  it("with a plan: the online settings in order under Online models", async function () {
    if (!haveIdentity()) this.skip();
    this.timeout(240_000);
    const session = await signInAsTestIdentity();
    expect(session.tier).toBe("pro");
    // The page reads the plan when it loads.
    await browser.url(appUrl("/online-models/"));
    await browser.pause(3_000);
    await openRouting();
    await browser.waitUntil(async () => (await headings()).includes("How much goes online"), { timeout: 60_000, timeoutMsg: "the online settings never showed" });
    await browser.pause(1_500);
    const h = await headings();
    console.log(`e2e: plan headings ${JSON.stringify(h)}`);
    const at = (t: string) => h.indexOf(t);
    expect(at("Offline models")).toBeLessThan(at("Online models"));
    for (const [a, b] of [
      ["Online models", "How much goes online"],
      ["How much goes online", "Online models by job"],
      ["Online models by job", "Models automatic routing may use"],
      ["Models automatic routing may use", "How routing decides"],
    ]) expect(at(a)).toBeLessThan(at(b));
    await scrollToHeading("Routing");
    await shot("routing-plan-1");
    await scrollToHeading("Online models");
    await shot("routing-plan-2");
    await scrollToHeading("Online models by job");
    await shot("routing-plan-3");
    await scrollToHeading("How routing decides");
    await shot("routing-plan-4");
  });
});
