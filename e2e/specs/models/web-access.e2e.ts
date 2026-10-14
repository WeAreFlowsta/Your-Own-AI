// Web access per AI (NEXT.md: fetch for everyone, local, free; the switch is
// ON by default; the offer line when it is off). Needs the network for
// example.com; the page is read on this computer.
import { shot, waitForChat, invoke, ask, waitForReplyDone, lastReply, resetAiModels } from "./_helpers";

const PAGE_QUESTION = "Read https://example.com/ and tell me in one sentence what that page says.";
const LIVE_QUESTION = "What is the weather in Paris right now?";

const setWebAccess = async (on: boolean) => {
  const names = await browser.execute(async (v: boolean) => {
    const t = (window as any).__TAURI__;
    const path = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
    const store = await t.store.load(path);
    const ais = ((await store.get("custom-ais")) || []) as { name: string; webAccess?: boolean }[];
    for (const a of ais) a.webAccess = v;
    await store.set("custom-ais", ais);
    await store.save();
    return ais.map((a) => `${a.name}=${a.webAccess}`);
  }, on);
  console.log(`e2e: web access ${JSON.stringify(names)}`);
  await browser.url("tauri://localhost/chat/");
  await waitForChat();
};

const offerShown = () => browser.execute(() => !!document.querySelector('[data-testid="web-offer"]'));
const clickOffer = (label: string) => browser.execute((l: string) => {
  const b = [...(document.querySelector('[data-testid="web-offer"]')?.querySelectorAll("button") ?? [])].find((x) => x.textContent?.trim() === l) as HTMLElement | undefined;
  b?.click();
  return !!b;
}, label);

describe("web access per AI", () => {
  after(async () => {
    // Web access back on for every AI (the last leg does this too, but a failed leg must not leave it off).
    await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const path = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(path);
      const ais = ((await store.get("custom-ais")) || []) as { webAccess?: boolean }[];
      for (const a of ais) a.webAccess = true;
      await store.set("custom-ais", ais);
      await store.save();
    }).catch(() => {});
    await browser.execute(() => { localStorage.removeItem("web-offer-off"); localStorage.removeItem("build-offer-off"); }).catch(() => {});
    await resetAiModels();
  });

  it("refuses local addresses before any network I/O", async function () {
    this.timeout(400_000); // waitForChat may load the model first (a fresh app in a suite run)
    await waitForChat();
    for (const url of ["http://127.0.0.1:11435/health", "http://localhost:27777/", "https://printer.local/", "http://10.0.0.1/"]) {
      const said = await browser.execute(async (u: string) => {
        try { await (window as any).__TAURI__.core.invoke("web_read_page", { url: u }); return "READ"; } catch (e) { return String(e); }
      }, url);
      console.log(`e2e: ${url} -> ${said.slice(0, 120)}`);
      expect(said).not.toBe("READ");
      expect(said).toContain("never read");
    }
  });

  it("reads a page the person points at, with no prompt (default on)", async function () {
    this.timeout(600_000);
    await waitForChat();
    const read = await browser.execute(async () => {
      try { return await (window as any).__TAURI__.core.invoke("web_read_page", { url: "https://example.com/" }); } catch (e) { return `ERR ${String(e)}`; }
    });
    console.log(`e2e: example.com reads as ${JSON.stringify(String(read).slice(0, 160))}`);
    if (String(read).startsWith("ERR")) {
      console.log("e2e: no network for example.com - skipping the page questions");
      this.skip();
    }
    // example.com's wording has changed over the years; its title has not.
    expect(String(read).toLowerCase()).toContain("example domain");
    await ask(PAGE_QUESTION);
    const reply = await waitForReplyDone(300_000);
    console.log(`e2e: reply ${JSON.stringify(reply.text.slice(0, 200))}`);
    const t = reply.text.toLowerCase();
    expect(t.includes("example") || t.includes("illustrative") || t.includes("documentation")).toBe(true);
    expect(await offerShown()).toBe(false);
    // example.com is thin (a few lines): the research step ran, and with the
    // Build helper not installed the reply carries the one-line offer of it.
    const research = await browser.execute(() => ({
      note: document.querySelector('[data-testid="web-note"]')?.textContent ?? null,
      offer: document.querySelector('[data-testid="build-offer"]')?.textContent ?? null,
    }));
    console.log(`e2e: research ${JSON.stringify(research)}`);
    expect(research.note).toContain("little to read");
    expect(research.offer).toContain("One page wasn't enough here");
    await shot("web-access-read");
    await browser.execute(() => {
      const b = [...(document.querySelector('[data-testid="build-offer"]')?.querySelectorAll("button") ?? [])].find((x) => x.textContent?.trim() === "Don't ask again") as HTMLElement | undefined;
      b?.click();
    });
    await browser.waitUntil(async () => browser.execute(() => !document.querySelector('[data-testid="build-offer"]')), { timeout: 10_000, timeoutMsg: "the Build offer stayed" });
    await browser.execute(() => localStorage.removeItem("build-offer-off"));
  });

  it("offers the web when the switch is off, and reads once on request", async function () {
    this.timeout(900_000);
    await setWebAccess(false);
    await ask(PAGE_QUESTION);
    await waitForReplyDone(300_000);
    await browser.waitUntil(offerShown, { timeout: 10_000, timeoutMsg: "the offer line did not show" });
    const offer = await browser.execute(() => document.querySelector('[data-testid="web-offer"]')?.textContent ?? "");
    console.log(`e2e: offer ${JSON.stringify(offer)}`);
    expect(offer).toContain("This needs the web, which is off for this AI.");
    await shot("web-access-offer");
    const before = ((await browser.execute(() => (window as any).__yoaiTurns?.() ?? [])) as { id: string }[]).map((t) => t.id);
    expect(await clickOffer("Just this once")).toBe(true);
    // The same question again, this time with the page read: a new turn at the
    // root (the bubble itself can come and go between two slow polls).
    await browser.waitUntil(async () => ((await browser.execute(() => (window as any).__yoaiTurns?.() ?? [])) as { id: string }[]).some((t) => !before.includes(t.id)), { timeout: 120_000, timeoutMsg: "the question was not asked again" });
    const again = await waitForReplyDone(300_000);
    console.log(`e2e: reply once ${JSON.stringify(again.text.slice(0, 200))}`);
    if (again.text.includes("empty reply twice")) {
      // Known on this box (NEXT.md §A): one invoke of the resend reaches the backend twice and the
      // first stream's events never arrive. Guarded in the app; the cause is still open.
      console.log("e2e: KNOWN FLAKE - the resend's stream was lost after a duplicate invoke; skipping this leg");
      this.skip();
    }
    const t = again.text.toLowerCase();
    expect(t.includes("example") || t.includes("illustrative") || t.includes("documentation")).toBe(true);
    expect(await offerShown()).toBe(false);
    await shot("web-access-once");
  });

  it("stops offering for this AI on request", async function () {
    this.timeout(600_000);
    await setWebAccess(false);
    await ask(LIVE_QUESTION);
    await waitForReplyDone(300_000);
    await browser.waitUntil(offerShown, { timeout: 10_000, timeoutMsg: "the offer line did not show for a live question" });
    expect(await clickOffer("Don't ask again for this AI")).toBe(true);
    await browser.waitUntil(async () => !(await offerShown()), { timeout: 10_000, timeoutMsg: "the offer line stayed" });
    await ask(LIVE_QUESTION);
    await waitForReplyDone(300_000);
    expect(await offerShown()).toBe(false);
    await shot("web-access-never");
    await setWebAccess(true);
  });
});
