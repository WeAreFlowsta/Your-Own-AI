// "Show more memory controls" (Settings, Memory), on the built app. Off,
// which is how a new profile starts: a kept item offers Correct and
// Forget, and "That's right" when an AI wrote it; the tidying line stays
// away. On: Move, "Who can use this" and "See where this came from" are
// there too. "That's right" confirms the item and then goes.
import { waitForChat, invoke, shot } from "./_helpers";

type Seen = { id: string; item: { confirmed?: unknown; subject?: string | null } };
const MINE = "Controls test: the shop opens at nine.";
const THEIRS = "Controls test: the delivery van is blue.";

async function keep(text: string, c: number, why: string): Promise<Seen> {
  const kept = await invoke<{ item: Seen }>("memory_keep", {
    input: { text, itemType: "detail", ptr: { c, s: 1, t: c, a: 0, b: 0 }, asOf: Math.round(c / 1000), kind: "exchange", why },
  });
  return kept.item;
}

async function setMore(on: boolean) {
  await browser.execute((v: string) => {
    localStorage.setItem("yoai-memory-more", v);
    window.dispatchEvent(new CustomEvent("yoai-memory-more"));
  }, on ? "1" : "0");
}

async function openItem(seen: Seen) {
  await browser.url("tauri://localhost/your-memory/");
  await browser.waitUntil(async () => browser.execute(() => typeof (window as any).__yoaiMap?.select === "function"), { timeout: 60_000, timeoutMsg: "the map never came up" });
  await browser.execute((s: string) => (window as any).__yoaiMap.select(s), seen.item.subject || "sub-about-you");
  const row = await $(`[data-testid="map-panel-item"][data-id="${seen.id}"]`);
  await row.waitForExist({ timeout: 60_000, timeoutMsg: "the kept item is not in its subject's panel" });
  return row;
}

const has = async (row: WebdriverIO.Element, testid: string) => (await row.$(`[data-testid="${testid}"]`)).isExisting();

describe("show more memory controls", () => {
  let mine: Seen;
  let theirs: Seen;

  before(async function () {
    this.timeout(900_000);
    await waitForChat();
    await browser.waitUntil(async () => invoke<unknown[]>("memory_items").then(() => true, () => false), { timeout: 600_000, interval: 5_000, timeoutMsg: "memory did not answer within ten minutes" });
    const now = Date.now() * 1000;
    mine = await keep(MINE, now - 2_000_000, "pressed");
    theirs = await keep(THEIRS, now - 1_000_000, "settled");
  });

  it("off: an item offers Correct and Forget, and That's right only when an AI wrote it", async function () {
    this.timeout(300_000);
    await setMore(false);
    const own = await openItem(mine);
    expect(await has(own, "item-correct")).toBe(true);
    expect(await has(own, "item-forget")).toBe(true);
    expect(await has(own, "item-confirm")).toBe(false);
    for (const hidden of ["item-move", "item-audience", "item-source", "item-make-rule"]) expect(await has(own, hidden)).toBe(false);
    const ai = await openItem(theirs);
    expect(await has(ai, "item-confirm")).toBe(true);
    expect(await has(ai, "item-audience")).toBe(false);
    expect(await ai.getText()).toContain("written by an AI, not confirmed");
    // The Recent board carries no tidying line at rest.
    await (await $('[data-testid="memory-tab"][data-tab="recently"]')).click();
    await browser.pause(2_000);
    const line = await $('[data-testid="tidying-line"]');
    if (await line.isExisting()) expect(await line.getAttribute("data-state")).toBe("paused");
    await shot("memory-more-off");
  });

  it("That's right confirms the item, and the offer goes", async function () {
    this.timeout(300_000);
    const ai = await openItem(theirs);
    await browser.execute((e) => (e as HTMLElement).click(), await ai.$('[data-testid="item-confirm"]'));
    await browser.waitUntil(async () => !!(await invoke<Seen[]>("memory_items")).find((i) => i.id === theirs.id)?.item.confirmed, { timeout: 60_000, timeoutMsg: "the item was not confirmed" });
    const again = await openItem(theirs);
    expect(await has(again, "item-confirm")).toBe(false);
    expect(await again.getText()).toContain("confirmed by you");
  });

  it("on: the further controls are on the item", async function () {
    this.timeout(300_000);
    await setMore(true);
    const own = await openItem(mine);
    expect(await has(own, "item-audience")).toBe(true);
    expect(await has(own, "item-correct")).toBe(true);
    expect(await has(own, "item-forget")).toBe(true);
    await shot("memory-more-on");
  });

  it("the switch in Settings turns it", async function () {
    this.timeout(300_000);
    await setMore(false);
    await browser.url("tauri://localhost/settings/");
    const toggle = await $('[data-testid="settings-memory-more"]');
    await toggle.waitForExist({ timeout: 60_000 });
    await browser.execute((e) => (e as HTMLElement).click(), toggle);
    await browser.waitUntil(async () => (await browser.execute(() => localStorage.getItem("yoai-memory-more"))) === "1", { timeout: 15_000, timeoutMsg: "the switch did not turn the choice on" });
    await shot("memory-more-setting");
  });

  after(async () => {
    // The other specs of a run press the further controls.
    await setMore(true);
  });
});
