// Take: you choose which providers answer. Settings > Routing, "Models automatic routing may use": one row per
// provider with where it is based; one provider's switch is turned off. The switch is turned back on after the
// take (not recorded). Kept shoot profile, signed in with the plan.
//   YOAI_CAPTURE_PROVIDER  the provider's name to switch (default: the last one that is on)
import { appUrl } from "../wdio.conf";
import { Take } from "./_capture";

type Session = { agent_pub_key: string | null; display_name?: string | null; username?: string | null; web_username?: string | null };
// the identity a take must run as (its agent key, YOAI_CAPTURE_IDENTITY), so a take never records anyone's own
const DEMO = process.env.YOAI_CAPTURE_IDENTITY;
const session = () => browser.execute(async () => (window as any).__TAURI__.core.invoke("flowsta_session")) as Promise<Session>;
const rows = () =>
  browser.execute(() =>
    [...document.querySelectorAll('[data-testid="auto-routing-models"] [data-provider]')].map((r) => ({
      id: r.getAttribute("data-provider")!,
      text: (r as HTMLElement).innerText.replace(/\s+/g, " ").trim(),
      on: r.querySelector('[data-testid="auto-provider-switch"]')?.getAttribute("aria-checked") === "true",
    })),
  ) as Promise<{ id: string; text: string; on: boolean }[]>;

describe("capture", () => {
  it("providers", async () => {
    await browser.url(appUrl("/settings/"));
    // the demo identity only, never anyone's own
    if (!DEMO) throw new Error("set YOAI_CAPTURE_IDENTITY to the demo identity's agent key");
    let s: Session = { agent_pub_key: null };
    await browser.waitUntil(async () => !!(s = await session().catch(() => s)).agent_pub_key, { timeout: 60_000, interval: 1_000, timeoutMsg: "not signed in" });
    console.log(`providers: session ${s.display_name ?? "?"} (@${s.username ?? s.web_username ?? "?"}) ${s.agent_pub_key}`);
    if (s.agent_pub_key !== DEMO) throw new Error("not the demo identity: switch the Vault to it");
    const list = await $('[data-testid="auto-routing-models"]');
    await list.waitForExist({ timeout: 90_000, timeoutMsg: "no provider list (plan?)" });
    await browser.pause(1500);
    const all = await rows();
    console.log(`providers: ${JSON.stringify(all)}`);
    const want = process.env.YOAI_CAPTURE_PROVIDER;
    const pick = want ? all.find((r) => r.text.startsWith(want)) : [...all].reverse().find((r) => r.on);
    if (!pick || !pick.on) throw new Error(`no provider to switch off (${want ?? "last on"})`);
    const sw = `[data-provider="${pick.id}"] [data-testid="auto-provider-switch"]`;
    // the list in the middle of the screen before the take starts
    await browser.execute(() => document.querySelector('[data-testid="auto-routing-models"]')!.scrollIntoView({ block: "center" }));
    await browser.pause(800);

    const take = await Take.start("providers");
    try {
      await take.hold(1200);
      await take.still("01-providers");
      await take.hover(`[data-provider="${pick.id}"] button[aria-expanded]`, 900);
      await take.click(sw);
      await take.hold(1800);
      await take.still("02-switched-off");
      await take.hold(1200);
    } finally {
      await take.stop();
    }
    // back as it was
    await $(sw).click();
    await browser.pause(800);
    console.log(`providers: ${pick.id} switched off in the take, back on: ${JSON.stringify((await rows()).find((r) => r.id === pick.id))}`);
  });
});
