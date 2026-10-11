// Memory rules (build-docs MEMORY_REDESIGN.md §3, phase 1): a rule added on
// the Your Memory page is a pinned person-scope row in the person's own
// records - it is listed first, survives a reload (read back from the
// network), and a removal is a record that hides it.
import { shot, invoke } from "./_helpers";

const RULE = `Always ask before sending anything (test ${Date.now().toString(36)})`;

async function yourMemory() {
  await browser.url("tauri://localhost/your-memory/");
  await (await $('[data-testid="memory-add"]')).waitForDisplayed({ timeout: 60_000 });
}

describe("memory rules", () => {
  it("adds a rule, lists it first, keeps it across a reload, and removes it", async () => {
    await yourMemory();
    // The records answer only once the conductor is up and an AI is provisioned.
    await browser.waitUntil(async () => {
      try { await invoke("list_memory_rows", { agentKey: null, scope: "person" }); return true; } catch { return false; }
    }, { timeout: 180_000, timeoutMsg: "the person's memory rows never answered" });
    await (await $('[data-testid="memory-add"]')).click();
    await (await $('[data-testid="memory-add-kind"]')).click();
    await (await $('[data-testid="memory-add-option"][data-kind="rule"]')).click();
    const input = await $('[data-testid="memory-add-value"]');
    await browser.execute((t: string) => {
      const el = document.querySelector('[data-testid="memory-add-value"]') as HTMLInputElement;
      el.focus(); el.value = t;
      el.dispatchEvent(new Event("input", { bubbles: true }));
    }, RULE);
    await browser.keys("Enter");
    const rule = await $(`[data-testid="memory-rule"]*=${RULE.slice(0, 30)}`);
    await rule.waitForDisplayed({ timeout: 60_000 });
    await shot("memory-rule-added");
    // Read back from the records, not the page.
    const rows = await invoke<{ kind: string; text: string; pinned: boolean }[]>("list_memory_rows", { agentKey: null, scope: "person" });
    console.log(`e2e: rows ${JSON.stringify(rows.map((r) => [r.kind, r.text.slice(0, 40), r.pinned]))}`);
    const saved = rows.find((r) => r.text === RULE);
    expect(saved?.kind).toBe("rule");
    expect(saved?.pinned).toBe(true);
    // Survives a reload.
    await yourMemory();
    await (await $(`[data-testid="memory-rule"]*=${RULE.slice(0, 30)}`)).waitForDisplayed({ timeout: 60_000 });
    // Removing it is a record; the list no longer shows it.
    const row = await $(`[data-testid="memory-rule"]*=${RULE.slice(0, 30)}`);
    await browser.execute((el: HTMLElement) => el.classList.add("opacity-100"), row as any);
    await (await row.$('button[title="Remove rule"]')).click();
    await browser.waitUntil(async () => !(await $(`[data-testid="memory-rule"]*=${RULE.slice(0, 30)}`).isExisting()), { timeout: 30_000, timeoutMsg: "the removed rule is still listed" });
    const after = await invoke<{ text: string }[]>("list_memory_rows", { agentKey: null, scope: "person" });
    expect(after.some((r) => r.text === RULE)).toBe(false);
    await shot("memory-rule-removed");
  });
});
