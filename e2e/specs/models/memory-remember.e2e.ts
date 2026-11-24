// The one Remember action, on the built app: pressed under a reply, or on
// selected words in a reply or in the person's own message, it keeps the
// exchange as a
// memory item that points at where it was said, and the Kept line shows
// what was kept, whom it is for, the other audience and Undo. A second
// press says it is already kept.
import { waitForChat, ask, waitForReplyDone, invoke, shot } from "./_helpers";

type Item = { id: string; text: string; item: { state: string; audience?: { kind: string; ai?: string }; source?: { kind: string; ptr?: { c: number; s: number } | null } | null } };
const items = () => invoke<Item[]>("memory_items");
const current = async () => (await items()).filter((i) => i.item.state === "current");

const lastOf = (role: "user" | "assistant") => `(//*[@data-testid="chat-message"][@data-role="${role}"])[last()]`;
const within = (role: "user" | "assistant", testid: string) => $(`${lastOf(role)}//*[@data-testid="${testid}"]`);
const press = async (el: WebdriverIO.Element) => {
  await el.waitForExist({ timeout: 20_000 });
  await browser.execute((e) => (e as HTMLElement).click(), el);
};
const textOf = async (el: WebdriverIO.Element) => (await browser.execute((e) => (e as HTMLElement).innerText, el)) as string;

describe("Remember keeps the exchange", () => {
  it("under a reply: the Kept line, for all the person's AIs", async () => {
    await waitForChat();
    await ask("Our flour supplier is Mill and Sons and they deliver on Tuesdays. Reply with one short sentence.");
    await waitForReplyDone();
    expect((await current()).length).toBe(0);
    await press(await within("assistant", "remember-reply"));
    const line = await within("assistant", "kept-line");
    await line.waitForExist({ timeout: 60_000 });
    const words = await textOf(line);
    console.log(`e2e: kept line: ${JSON.stringify(words)}`);
    expect(words).toContain("Kept.");
    expect(words).toContain("This exchange, your message and the reply together.");
    expect(words).toContain("For all your AIs.");
    const kept = await current();
    expect(kept.length).toBe(1);
    expect(kept[0].item.source?.kind).toBe("exchange");
    expect(kept[0].item.source?.ptr?.c).toBeGreaterThan(0);
    expect(kept[0].item.audience?.kind ?? "all").toBe("all");
    await shot("remember-kept-line");
  });

  it("the other audience is one press away, and back", async () => {
    await press(await within("assistant", "kept-only-ai"));
    await browser.waitUntil(async () => (await current())[0]?.item.audience?.kind === "ai", { timeout: 60_000, timeoutMsg: "the item's audience did not become the one AI" });
    expect(await textOf(await within("assistant", "kept-line"))).toContain("Only for ");
    await shot("remember-only-this-ai");
    await press(await within("assistant", "kept-for-all"));
    await browser.waitUntil(async () => ((await current())[0]?.item.audience?.kind ?? "all") === "all", { timeout: 60_000, timeoutMsg: "the item's audience did not go back to all AIs" });
  });

  it("a second press says it is already kept, and writes nothing", async () => {
    await press(await within("assistant", "remember-reply"));
    const already = await within("assistant", "kept-already");
    await already.waitForExist({ timeout: 60_000 });
    expect(await textOf(already)).toMatch(/^Already kept on \w+ \d+\.$/);
    expect((await current()).length).toBe(1);
    await shot("remember-already-kept");
  });

  it("Remember on a later reply keeps that exchange, and Undo takes it back", async () => {
    await ask("The oven service is booked for the first Monday of each month. Reply with one short sentence.");
    await waitForReplyDone();
    // The person's own message carries no Remember of its own: the one under
    // the reply keeps the exchange, their words and the answer together.
    expect(await (await $('[data-testid="remember-own"]')).isExisting()).toBe(false);
    await press(await within("assistant", "remember-reply"));
    const line = await within("assistant", "kept-line");
    await line.waitForExist({ timeout: 60_000 });
    expect(await textOf(line)).toContain("Kept.");
    const kept = await current();
    expect(kept.length).toBe(2);
    expect(kept.some((i) => i.text.includes("oven service"))).toBe(true);
    await shot("remember-later-reply");
    await press(await within("assistant", "kept-undo"));
    await browser.waitUntil(async () => (await current()).length === 1, { timeout: 60_000, timeoutMsg: "Undo did not take the item back" });
  });

  it("on selected words: the selection is the item's words", async () => {
    const picked = (await browser.execute((xpath) => {
      const host = document.evaluate(xpath, document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null).singleNodeValue as HTMLElement | null;
      if (!host) return "";
      const walker = document.createTreeWalker(host, NodeFilter.SHOW_TEXT);
      let node: Node | null = null;
      while ((node = walker.nextNode())) {
        const t = node.textContent ?? "";
        const inBar = (node.parentElement as HTMLElement | null)?.closest("button, [data-testid='kept-line']");
        if (!inBar && t.trim().length >= 12) break;
      }
      if (!node) return "";
      const t = node.textContent ?? "";
      const start = t.length - t.trimStart().length;
      const range = document.createRange();
      range.setStart(node, start);
      range.setEnd(node, Math.min(t.length, start + 24));
      const sel = window.getSelection()!;
      sel.removeAllRanges();
      sel.addRange(range);
      document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
      return sel.toString().trim();
    }, lastOf("assistant"))) as string;
    expect(picked.length).toBeGreaterThanOrEqual(8);
    const button = await $('[data-testid="remember-selection"]');
    await press(button);
    const line = await within("assistant", "kept-line");
    await line.waitForExist({ timeout: 60_000 });
    expect(await textOf(line)).toContain("The words you selected.");
    const kept = await current();
    console.log(`e2e: selection ${JSON.stringify(picked)} kept as ${JSON.stringify(kept.map((i) => i.text))}`);
    expect(kept.some((i) => i.item.source?.kind === "selection" && i.text === picked)).toBe(true);
    await shot("remember-selection");
  });

  it("on selected words in the person's own message: the same, under that message", async () => {
    const picked = (await browser.execute((xpath) => {
      const host = document.evaluate(xpath, document, null, XPathResult.FIRST_ORDERED_NODE_TYPE, null).singleNodeValue as HTMLElement | null;
      const node = host?.querySelector('[data-testid="own-message-text"]')?.firstChild;
      if (!node) return "";
      const range = document.createRange();
      range.setStart(node, 0);
      range.setEnd(node, Math.min((node.textContent ?? "").length, 24));
      const sel = window.getSelection()!;
      sel.removeAllRanges();
      sel.addRange(range);
      document.dispatchEvent(new MouseEvent("mouseup", { bubbles: true }));
      return sel.toString().trim();
    }, lastOf("user"))) as string;
    expect(picked.length).toBeGreaterThanOrEqual(8);
    await press(await $('[data-testid="remember-selection"]'));
    const line = await within("user", "kept-line");
    await line.waitForExist({ timeout: 60_000 });
    expect(await textOf(line)).toContain("The words you selected.");
    const kept = await current();
    console.log(`e2e: own selection ${JSON.stringify(picked)} kept as ${JSON.stringify(kept.map((i) => i.text))}`);
    expect(kept.some((i) => i.item.source?.kind === "selection" && i.text === picked)).toBe(true);
    await shot("remember-own-selection");
  });

  it("said in the chat: 'remember that' with words keeps this message, alone it keeps the exchange before", async () => {
    const before = (await current()).length;
    await ask("Remember that the shop closes at 3 pm on Sundays.");
    await waitForReplyDone();
    const own = await within("user", "kept-line");
    await own.waitForExist({ timeout: 90_000 });
    expect(await textOf(own)).toContain("Kept.");
    let kept = await current();
    expect(kept.length).toBe(before + 1);
    expect(kept.some((i) => i.text.includes("closes at 3 pm on Sundays"))).toBe(true);
    console.log(`e2e: the reply to "remember that ...": ${JSON.stringify((await textOf(await $(lastOf("assistant")))).slice(0, 300))}`);
    await shot("remember-said-this");

    await ask("What is a good name for a sourdough loaf? One short suggestion.");
    await waitForReplyDone();
    const suggestion = `(//*[@data-testid="chat-message"][@data-role="assistant"])[last()]`;
    const suggested = await textOf(await $(suggestion));
    await ask("remember that");
    await waitForReplyDone();
    // The line stands under the reply before, which is now the last but one.
    const line = await $(`(//*[@data-testid="chat-message"][@data-role="assistant"])[last()-1]//*[@data-testid="kept-line"]`);
    await line.waitForExist({ timeout: 90_000 });
    kept = await current();
    expect(kept.length).toBe(before + 2);
    console.log(`e2e: "remember that" kept ${JSON.stringify(kept.map((i) => i.text.slice(0, 80)))} (the reply before began ${JSON.stringify(suggested.slice(0, 80))})`);
    await shot("remember-said-that");
  });

  it("Recently lists every change, with the tidying line at its top, and Undo works there", async () => {
    await browser.url("tauri://localhost/your-memory/#recently");
    const board = await $('[data-testid="recently-board"]');
    await board.waitForExist({ timeout: 60_000 });
    // The list fills when it comes into view.
    await browser.execute((e) => (e as HTMLElement).scrollIntoView({ block: "start" }), board);
    await browser.waitUntil(async () => (await $$('[data-testid="recently-row"]').length) >= 4, { timeout: 60_000, timeoutMsg: "Recently did not list the changes" });
    const rows = (await browser.execute(() => Array.from(document.querySelectorAll('[data-testid="recently-row"]')).map((r) => `${r.getAttribute("data-what")}: ${(r as HTMLElement).innerText.split(/\s+/).join(" ")}`))) as string[];
    console.log(`e2e: recently: ${JSON.stringify(rows)}`);
    // Kept: the reply, the later reply (undone), the selection in a reply, the
    // selection in the person's own message, and the two said in the chat.
    expect(rows.filter((r) => r.startsWith("kept:")).length).toBe(6);
    expect(rows.some((r) => r.startsWith("undone:"))).toBe(true);
    expect(rows.some((r) => r.includes("Kept by you"))).toBe(true);
    const line = await $('[data-testid="tidying-line"]');
    await line.waitForExist({ timeout: 30_000 });
    console.log(`e2e: tidying line (${await line.getAttribute("data-state")}): ${JSON.stringify(await textOf(line))}`);
    await shot("recently-board");
    const before = (await current()).length;
    await press(await $('[data-testid="recently-row"][data-what="kept"] [data-testid="recently-undo"]'));
    await browser.waitUntil(async () => (await current()).length === before - 1, { timeout: 60_000, timeoutMsg: "Undo on Recently did not take the item back" });
    await shot("recently-after-undo");
  });
});
