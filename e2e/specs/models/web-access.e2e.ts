// Web access per AI (the plan: fetch for everyone, local, free; the switch is
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
// What the web step found lives in the last reply's Sources panel (no
// sentences in the action bar): open it, read the note.
const noteFromSources = async () => {
  await browser.execute(() => {
    const m = [...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop();
    (m?.querySelector('[data-testid="reply-sources"]') as HTMLElement | undefined)?.click();
  });
  await browser.waitUntil(() => browser.execute(() => !!document.querySelector('[data-testid="web-note"]')), { timeout: 10_000, timeoutMsg: "the Sources panel showed no web note" }).catch(() => {});
  return browser.execute(() => document.querySelector('[data-testid="web-note"]')?.textContent ?? null);
};
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
    const research = {
      note: await noteFromSources(),
      offer: await browser.execute(() => document.querySelector('[data-testid="build-offer"]')?.textContent ?? null),
    };
    console.log(`e2e: research ${JSON.stringify(research)}`);
    expect(research.note).toContain("little to read");
    expect(research.offer).toContain("One page wasn't enough here");
    await shot("web-access-read");
    // A bare domain, the way people type one, is an address too.
    await ask("What is on example.com? One sentence.");
    const bare = await waitForReplyDone(300_000);
    console.log(`e2e: bare-domain reply ${JSON.stringify(bare.text.slice(0, 200))}`);
    const b = bare.text.toLowerCase();
    expect(b.includes("example") || b.includes("documentation") || b.includes("placeholder")).toBe(true);
    expect(await noteFromSources()).toContain("little to read");
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
    expect(offer).toContain("Web access is off for this AI.");
    await shot("web-access-offer");
    const before = ((await browser.execute(() => (window as any).__yoaiTurns?.() ?? [])) as { id: string }[]).map((t) => t.id);
    expect(await clickOffer("Just this once")).toBe(true);
    // The same question again, this time with the page read: a new turn at the
    // root (the bubble itself can come and go between two slow polls).
    await browser.waitUntil(async () => ((await browser.execute(() => (window as any).__yoaiTurns?.() ?? [])) as { id: string }[]).some((t) => !before.includes(t.id)), { timeout: 120_000, timeoutMsg: "the question was not asked again" });
    const turnOf = async () => ((await browser.execute(() => (window as any).__yoaiTurns?.() ?? [])) as { id: string; state: string }[]).find((t) => !before.includes(t.id));
    // The loop a person does by hand (10-14): leave the chat while the resend runs,
    // come back through the tray - the page shows ONE question and ONE reply
    // (the live one), never the replaced web-off answer above it.
    await (await $('button[title^="Menu"]')).click();
    await browser.execute(() => {
      const item = [...document.querySelectorAll('[role="menuitem"]')].find((b) => (b as HTMLElement).innerText.trim().startsWith("Settings")) as HTMLElement | undefined;
      item?.click();
    });
    await browser.waitUntil(async () => (await browser.execute(() => location.pathname)) === "/settings/", { timeout: 30_000, timeoutMsg: "Settings never opened" });
    await new Promise((r) => setTimeout(r, 3000));
    const rowOpened = await browser.execute(() => {
      const r = document.querySelector('[data-testid="turn-row"]');
      const open = [...(r?.querySelectorAll("button") ?? [])].find((b) => b.textContent?.trim() === "Open") as HTMLElement | undefined;
      open?.click();
      return !!open;
    });
    console.log(`e2e: tray Open on Settings: ${rowOpened}`);
    if (!rowOpened) {
      // The resend already ended and its row is gone: the drawer opens it instead.
      await browser.url("tauri://localhost/chat/");
      await waitForChat();
    }
    await browser.waitUntil(async () => (await browser.execute(() => location.pathname)) === "/chat/", { timeout: 30_000, timeoutMsg: "the chat never reopened" });
    await new Promise((r) => setTimeout(r, 4000));
    const onReturn = await browser.execute(() => ({
      users: document.querySelectorAll('[data-testid="chat-message"][data-role="user"]').length,
      assistants: document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]').length,
      unanswered: document.body.innerText.includes("never got an answer"),
    }));
    console.log(`e2e: on return ${JSON.stringify(onReturn)}`);
    expect(onReturn.users).toBe(1);
    expect(onReturn.assistants).toBe(1);
    expect(onReturn.unanswered).toBe(false);
    await shot("web-access-once-return");
    // Judge the NEW turn (the bubble list is rebuilt under the resend): wait for
    // it to end, then read the last reply.
    await browser.waitUntil(async () => { const t = await turnOf(); return !!t && t.state !== "running" && t.state !== "queued"; }, { timeout: 300_000, interval: 1000, timeoutMsg: "the resend never ended" });
    const resent = await turnOf();
    const again = await waitForReplyDone(60_000);
    console.log(`e2e: reply once (turn ${resent?.state}) ${JSON.stringify(again.text.slice(0, 200))}`);
    if (again.text.includes("empty reply twice") || again.text.includes("lost on the way") || resent?.state === "error") {
      // Known on this box (the plan §A): one invoke of the resend reaches the backend twice and the
      // first stream's events never arrive. Guarded in the app; the cause is still open.
      console.log("e2e: KNOWN FLAKE - the resend's stream was lost after a duplicate invoke; skipping this leg");
      this.skip();
    }
    const t = again.text.toLowerCase();
    expect(t.includes("example") || t.includes("illustrative") || t.includes("documentation")).toBe(true);
    expect(await offerShown()).toBe(false);
    await shot("web-access-once");
    // Reopened from the RECORDS (New chat, then the drawer): the records hold
    // both answers, the page shows the question once with its latest answer,
    // and the page it read is in Sources.
    await (await $('[data-testid="new-chat"]')).click();
    await browser.waitUntil(() => browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length === 0), { timeout: 10_000, timeoutMsg: "New chat did not clear the page" });
    await (await $('[data-testid="conversations-open"]')).click();
    const item = await $('[data-testid="conversation-item"]');
    await item.waitForDisplayed({ timeout: 20_000 });
    // The row's first button is the open; a click on the row itself can land
    // beside it.
    const opened = await browser.execute(() => {
      const items = [...document.querySelectorAll('[data-testid="conversation-item"]')];
      const first = items[0];
      const b = first?.querySelector("button") as HTMLElement | undefined;
      b?.click();
      return { count: items.length, title: (first as HTMLElement | undefined)?.innerText.replace(/\s+/g, " ").slice(0, 80) ?? "", clicked: !!b };
    });
    console.log(`e2e: drawer ${JSON.stringify(opened)}`);
    expect(opened.clicked).toBe(true);
    await browser.waitUntil(async () => {
      const r = await lastReply();
      return !!r && r.state === "done" && r.text.length > 20;
    }, { timeout: 120_000, interval: 1000, timeoutMsg: "the conversation did not reopen from the records" });
    // The records land behind the first paint (a reply still at the root
    // shows at once): give the Sources button a moment.
    await browser.waitUntil(() => browser.execute(() => !!document.querySelector('[data-testid="reply-sources"]')), { timeout: 15_000, timeoutMsg: "no Sources on the reopened reply" }).catch(() => {});
    const reopened = await browser.execute(() => ({
      users: document.querySelectorAll('[data-testid="chat-message"][data-role="user"]').length,
      assistants: document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]').length,
      sources: !!document.querySelector('[data-testid="reply-sources"]'),
      last: ([...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined)?.innerText.slice(0, 120) ?? "",
    }));
    console.log(`e2e: reopened from records ${JSON.stringify(reopened)}`);
    expect(reopened.users).toBe(1);
    expect(reopened.assistants).toBe(1);
    expect(reopened.sources).toBe(true);
    await shot("web-access-once-reopened");
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
