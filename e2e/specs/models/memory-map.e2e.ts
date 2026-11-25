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
    // Not named by the person yet: shown by the conversation's title and the day, on this computer's clock.
    const shown = (await placed()).find((p) => p.id === subject.id)!.name;
    console.log(`e2e: shown as ${JSON.stringify(shown)}`);
    const today = new Date().toLocaleDateString("en-US", { month: "long", day: "numeric" });
    expect(shown.endsWith(`, ${today}`)).toBe(true);
    expect(await textOf('[data-testid="map-sentence"]')).toBe("What your AIs can use: 2 subjects, 1 item.");

    // A real press on the circle, where it is drawn.
    await browser.pause(4_000); // the circles settle
    const at = (await placed()).find((p) => p.id === subject.id)!;
    const opened = () =>
      browser
        .waitUntil(async () => (await (await $('[data-testid="map-panel"]')).getAttribute("data-subject")) === subject.id, { timeout: 8_000 })
        .then(() => true)
        .catch(() => false);
    await browser.action("pointer").move({ x: Math.round(at.x), y: Math.round(at.y) }).down().up().perform().catch(() => undefined);
    let pressed = await opened();
    console.log(`e2e: the driver's pointer press on the circle opened it: ${pressed}`);
    if (!pressed) {
      // The same press as the page's own pointer events, through the drawing's own hit test.
      await browser.execute((x: number, y: number) => {
        const c = document.querySelector('[data-testid="memory-map-stage"] canvas') as HTMLCanvasElement;
        const ev = (type: string) => c.dispatchEvent(new PointerEvent(type, { bubbles: true, cancelable: true, clientX: x, clientY: y, pointerId: 1, pointerType: "mouse", isPrimary: true, button: 0, buttons: type === "pointerdown" ? 1 : 0, view: window }));
        ev("pointermove");
        return new Promise<void>((done) =>
          setTimeout(() => {
            ev("pointerdown");
            ev("pointerup");
            c.dispatchEvent(new MouseEvent("click", { bubbles: true, cancelable: true, clientX: x, clientY: y, view: window }));
            done();
          }, 400),
        );
      }, at.x, at.y);
      pressed = await opened();
      console.log(`e2e: pointer events at the circle's place opened it: ${pressed}`);
    }
    if (!pressed) await browser.execute((id) => (window as any).__yoaiMap.select(id), subject.id);
    await browser.waitUntil(async () => (await textOf('[data-testid="map-panel-name"]')) === shown, { timeout: 20_000, timeoutMsg: "the panel did not open on the subject" });
    await (await $('[data-testid="map-panel-item"]')).waitForExist({ timeout: 20_000 });
    // The panel shows the item as it was kept (the reply's own words, whatever the model said).
    expect(await textOf('[data-testid="map-panel-item"]')).toContain((kept as Item & { text: string }).text.slice(0, 40));
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

  it("an item can be corrected, moved to another subject and forgotten, from the subject's panel", async function () {
    this.timeout(300_000);
    const all = () => invoke<Item[]>("memory_items");
    const panelItems = async () => ((await browser.execute(() => document.querySelectorAll('[data-testid="map-panel-item"]').length)) as number);
    const you = (await map()).subjects.find((s) => s.kind === "about_you")!;
    const other = (await map()).subjects.find((s) => s.kind === "conversation")!;
    await browser.execute((id) => (window as any).__yoaiMap.select(id), you.id);
    await (await $('[data-testid="map-panel-item"]')).waitForExist({ timeout: 20_000 });

    // Correct: the words change, and the item is still the person's.
    await press(await $('[data-testid="item-correct"]'));
    const box = await $('[data-testid="item-correct-text"]');
    await box.waitForExist({ timeout: 20_000 });
    await browser.execute((e) => {
      const el = e as HTMLTextAreaElement;
      el.focus();
      el.value = "I bake on Saturdays and Sundays.";
      el.dispatchEvent(new Event("input", { bubbles: true }));
    }, box);
    await press(await $('[data-testid="item-correct-save"]'));
    await browser.waitUntil(async () => (await all()).some((i) => i.text === "I bake on Saturdays and Sundays." && i.item.state === "current"), { timeout: 60_000, timeoutMsg: "the correction was not saved" });
    await browser.waitUntil(async () => (await textOf('[data-testid="map-panel-item"]')).includes("Saturdays and Sundays"), { timeout: 30_000, timeoutMsg: "the panel did not show the corrected words" });

    // Move: it leaves About you for the other subject.
    await press(await $('[data-testid="item-move"]'));
    await press(await $(`[data-testid="item-move-to"][data-subject="${other.id}"]`));
    await browser.waitUntil(async () => other.ids.includes((await all()).find((i) => i.text === "I bake on Saturdays and Sundays.")?.item.subject ?? ""), { timeout: 60_000, timeoutMsg: "the item did not move" });
    await (await $('[data-testid="map-panel-empty"]')).waitForExist({ timeout: 30_000 });
    const after = await map();
    console.log(`e2e: after the move: ${JSON.stringify(after.subjects.map((s) => [s.name, s.items]))}`);
    expect(after.subjects.find((s) => s.id === you.id)!.items).toBe(0);
    expect(after.subjects.find((s) => s.id === other.id)!.items).toBe(2);

    // Forget: one fewer in the subject, and the list of changes has all three.
    await browser.execute((id) => (window as any).__yoaiMap.select(id), other.id);
    await browser.waitUntil(async () => (await panelItems()) === 2, { timeout: 30_000, timeoutMsg: "the subject's two items were not listed" });
    await press(await $('[data-testid="item-forget"]'));
    await browser.waitUntil(async () => (await map()).subjects.find((s) => s.id === other.id)?.items === 1, { timeout: 60_000, timeoutMsg: "the item was not forgotten" });
    const whats = (await invoke<{ what: string }[]>("memory_changes")).map((c) => c.what);
    console.log(`e2e: changes: ${JSON.stringify(whats)}`);
    for (const what of ["corrected", "moved", "forgotten"]) expect(whats).toContain(what);
    await shot("memory-map-item-actions");
  });
});
