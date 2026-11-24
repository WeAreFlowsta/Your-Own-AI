// The Map, on the built app: About you is there before anything is kept,
// with the line that says the map grows; what is kept from a conversation
// becomes a subject with its own circle; pressing a circle shows what the
// subject holds; a subject can be named; the List shows the same content.
import { waitForChat, ask, waitForReplyDone, invoke, shot } from "./_helpers";

type Subject = { id: string; ids: string[]; name: string; nameAccepted: boolean; kind: string; items: number };
type MapData = { subjects: Subject[]; links: unknown[]; projects: unknown[]; unplaced: number };
type Placed = { id: string; name: string; x: number; y: number };
type Item = { id: string; text: string; item: { state: string; subject?: string | null } };

const map = () => invoke<MapData>("memory_map", { view: null });
const placed = async () => ((await browser.execute(() => (window as any).__yoaiMap?.placed?.() ?? [])) as Placed[]);
const press = async (el: WebdriverIO.Element) => {
  await el.waitForExist({ timeout: 20_000 });
  await browser.execute((e) => (e as HTMLElement).click(), el);
};
const textOf = async (sel: string) => (await browser.execute((s) => (document.querySelector(s) as HTMLElement | null)?.innerText ?? "", sel)) as string;
const openMemory = async () => {
  await browser.url("tauri://localhost/your-memory/");
  await (await $('[data-testid="memory-map-stage"] canvas')).waitForExist({ timeout: 60_000 });
};

describe("the Map", () => {
  it("has About you from the first day, and says it will grow", async function () {
    this.timeout(300_000);
    await waitForChat();
    await openMemory();
    await browser.waitUntil(async () => (await placed()).length === 1, { timeout: 60_000, timeoutMsg: "the first circle was not drawn" });
    const only = (await placed())[0];
    expect(only.name).toBe("About you");
    expect((await map()).subjects.map((s) => [s.name, s.kind, s.items])).toEqual([["About you", "about_you", 0]]);
    expect(await textOf('[data-testid="map-grow-line"]')).toBe("Your map grows as memory learns more.");
    expect(await textOf('[data-testid="map-sentence"]')).toBe("What your AIs can use: 1 subject, 0 items.");
    // The canvas is drawn on: its middle is not the page's ground.
    const drawn = (await browser.execute(() => {
      const c = document.querySelector('[data-testid="memory-map-stage"] canvas') as HTMLCanvasElement | null;
      if (!c) return 0;
      const px = c.getContext("2d")!.getImageData(0, 0, c.width, c.height).data;
      const seen = new Set<string>();
      for (let i = 0; i < px.length; i += 4 * 97) seen.add(`${px[i]},${px[i + 1]},${px[i + 2]}`);
      return seen.size;
    })) as number;
    console.log(`e2e: the map's canvas holds ${drawn} different colors`);
    expect(drawn).toBeGreaterThan(8);
    await shot("memory-map-first-day");
  });

  it("what is kept from a conversation is a subject with its own circle, and pressing it shows what it holds", async function () {
    this.timeout(600_000);
    await browser.url("tauri://localhost/chat/");
    await waitForChat();
    await ask("Our flour supplier is Mill and Sons and they deliver on Tuesdays. Reply with one short sentence.");
    await waitForReplyDone();
    await press(await $('(//*[@data-testid="chat-message"][@data-role="assistant"])[last()]//*[@data-testid="remember-reply"]'));
    await browser.waitUntil(async () => (await invoke<Item[]>("memory_items")).some((i) => i.item.state === "current"), { timeout: 60_000, timeoutMsg: "nothing was kept" });
    const kept = (await invoke<Item[]>("memory_items")).find((i) => i.item.state === "current")!;
    console.log(`e2e: kept in subject ${kept.item.subject}`);
    expect(kept.item.subject ?? "").toMatch(/^sub-conv-[0-9a-f]{16}$/);

    await openMemory();
    await browser.waitUntil(async () => (await placed()).length === 2, { timeout: 60_000, timeoutMsg: "the new subject has no circle" });
    const data = await map();
    const subject = data.subjects.find((s) => s.kind === "conversation")!;
    console.log(`e2e: the map: ${JSON.stringify(data.subjects.map((s) => [s.name, s.items]))}`);
    expect(subject.items).toBe(1);
    expect(subject.name).toMatch(/, (January|February|March|April|May|June|July|August|September|October|November|December) \d+$/);
    expect(await textOf('[data-testid="map-sentence"]')).toBe("What your AIs can use: 2 subjects, 1 item.");

    // A real press on the circle, where it is drawn.
    await browser.pause(4_000); // the circles settle
    const at = (await placed()).find((p) => p.id === subject.id)!;
    await browser.action("pointer").move({ x: Math.round(at.x), y: Math.round(at.y) }).down().up().perform();
    const pressed = await browser
      .waitUntil(async () => (await (await $('[data-testid="map-panel"]')).getAttribute("data-subject")) === subject.id, { timeout: 10_000 })
      .then(() => true)
      .catch(() => false);
    console.log(`e2e: a pointer press on the circle opened it: ${pressed}`);
    if (!pressed) await browser.execute((id) => (window as any).__yoaiMap.select(id), subject.id);
    await browser.waitUntil(async () => (await textOf('[data-testid="map-panel-name"]')) === subject.name, { timeout: 20_000, timeoutMsg: "the panel did not open on the subject" });
    await (await $('[data-testid="map-panel-item"]')).waitForExist({ timeout: 20_000 });
    expect(await textOf('[data-testid="map-panel-item"]')).toContain("Mill and Sons");
    expect(await textOf('[data-testid="map-panel-count"]')).toBe("1 item");
    await shot("memory-map-subject");
    expect(pressed).toBe(true);
  });

  it("a subject can be named, and the name is the person's from then on", async function () {
    this.timeout(300_000);
    await press(await $('[data-testid="map-rename"]'));
    const input = await $('[data-testid="map-name-input"]');
    await input.waitForExist({ timeout: 20_000 });
    await browser.execute((e) => {
      const el = e as HTMLInputElement;
      el.focus();
      el.value = "Flour supplier";
      el.dispatchEvent(new Event("input", { bubbles: true }));
    }, input);
    await press(await $('[data-testid="map-name-save"]'));
    await browser.waitUntil(async () => (await textOf('[data-testid="map-panel-name"]')) === "Flour supplier", { timeout: 30_000, timeoutMsg: "the new name did not show" });
    const named = (await map()).subjects.find((s) => s.kind === "conversation")!;
    expect([named.name, named.nameAccepted]).toEqual(["Flour supplier", true]);
    expect((await placed()).some((p) => p.name === "Flour supplier")).toBe(true);
  });

  it("the List holds the same subjects, and what the person writes about themselves lands in About you", async function () {
    this.timeout(300_000);
    await invoke("memory_keep_words", { text: "I bake on Saturdays." });
    await browser.execute(() => window.dispatchEvent(new CustomEvent("yoai-memory-changed")));
    await browser.waitUntil(async () => (await textOf('[data-testid="map-sentence"]')) === "What your AIs can use: 2 subjects, 2 items.", { timeout: 30_000, timeoutMsg: "the map did not follow what was kept" });
    await press(await $('[data-testid="map-mode-list"]'));
    await (await $('[data-testid="map-list"]')).waitForExist({ timeout: 20_000 });
    const rows = (await browser.execute(() => Array.from(document.querySelectorAll('[data-testid="map-subject-row"]')).map((r) => (r as HTMLElement).innerText.replace(/\s+/g, " ")))) as string[];
    console.log(`e2e: the list: ${JSON.stringify(rows)}`);
    expect(rows.length).toBe(2);
    expect(rows[0]).toContain("About you");
    expect(rows[0]).toContain("1 item");
    expect(rows[1]).toContain("Flour supplier");
    await press(await $('[data-testid="map-subject-row"]'));
    await browser.waitUntil(async () => (await textOf('[data-testid="map-panel-name"]')) === "About you", { timeout: 20_000 });
    expect(await textOf('[data-testid="map-panel-item"]')).toContain("I bake on Saturdays.");
    await shot("memory-map-list");
    await press(await $('[data-testid="map-mode-map"]'));
    await browser.pause(1_500);
    await shot("memory-map-two-subjects");
  });
});
