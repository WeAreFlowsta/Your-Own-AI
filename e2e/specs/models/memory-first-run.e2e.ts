// Memory on a new profile: nothing to set up and nothing to explain away.
// The first reply to a plain hello carries no line about memory (nothing
// was looked for, and nothing is said about that), and each board of the
// Memory page has a short empty state: one or two sentences, never an
// empty drawing or a blank list.
import { waitForChat, ask, waitForReplyDone, shot } from "./_helpers";

const sentences = (text: string) => text.split(/[.!?](?:\s|$)/).filter((s) => s.trim().length > 0).length;

async function openTab(tab: string): Promise<void> {
  const button = await $(`[data-testid="memory-tab"][data-tab="${tab}"]`);
  await button.waitForDisplayed({ timeout: 60_000 });
  await button.click();
}

async function emptyState(selector: string): Promise<string> {
  const el = await $(selector);
  await el.waitForDisplayed({ timeout: 60_000, timeoutMsg: `no empty state at ${selector}` });
  return (await el.getText()).split(/\s+/).join(" ").trim();
}

describe("memory on a new profile", () => {
  it("a first hello gets an ordinary reply with no line about memory", async function () {
    this.timeout(600_000);
    await waitForChat();
    await ask("hi, what can you help me with?");
    const reply = await waitForReplyDone(300_000);
    expect(reply.text.trim().length).toBeGreaterThan(0);
    const said: string[] = [];
    for (const line of await $$('[data-testid="given-line"]').getElements()) said.push(await line.getText());
    console.log(`e2e: lines under the first reply: ${JSON.stringify(said)}`);
    expect(said.length).toBe(0);
    expect(await $$('[data-testid="memory-line"]').length).toBe(0);
    await shot("memory-first-run-reply");
  });

  it("every Memory board says in a sentence or two what will be there", async function () {
    this.timeout(300_000);
    await browser.url("tauri://localhost/your-memory/");
    await (await $('[data-testid="memory-tabs"]')).waitForExist({ timeout: 60_000 });
    // The Map: one circle, "About you", and the line that it grows.
    const grows = await emptyState('[data-testid="map-grow-line"]');
    console.log(`e2e: Map: ${grows}`);
    expect(sentences(grows)).toBeLessThanOrEqual(2);
    await shot("memory-first-run-map");

    await openTab("recently");
    const recently = await emptyState('[data-testid="recently-empty"]');
    console.log(`e2e: Recently: ${recently}`);
    expect(sentences(recently)).toBeLessThanOrEqual(2);
    expect(recently).toContain("Remember");

    await openTab("open");
    const open = await emptyState('[data-testid="open-empty"]');
    console.log(`e2e: Open: ${open}`);
    expect(sentences(open)).toBeLessThanOrEqual(2);

    // Rules: nothing listed, and the one action is the box to write one.
    await openTab("rules");
    await (await $('[data-testid="rules-board"]')).waitForDisplayed({ timeout: 60_000 });
    await (await $('[data-testid="rule-add"]')).waitForDisplayed({ timeout: 30_000, timeoutMsg: "the Rules board offered no way to add a rule" });
    await shot("memory-first-run-rules");

    // No restore note, no update line: a new profile has nothing to be told.
    expect(await $('[data-testid="memory-restore-note"]').isExisting()).toBe(false);
    expect(await $('[data-testid="memory-update-line"]').isExisting()).toBe(false);
  });
});
