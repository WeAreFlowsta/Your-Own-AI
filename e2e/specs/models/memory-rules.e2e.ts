// Memory rules (build-docs MEMORY_REDESIGN.md §3, phase 1): a rule added on
// the Your Memory page is a pinned person-scope row in the person's own
// records - it is listed first, survives a reload (read back from the
// network), and a removal is a record that hides it.
import { shot, invoke, waitForChat, ask, waitForReplyDone } from "./_helpers";

const RULE = `Always ask before sending anything (test ${Date.now().toString(36)})`;
const SAID_RULE = `From now on, answer in one short sentence (test ${Date.now().toString(36)})`;

async function mcpCall(name: string, args: Record<string, unknown>, headers: Record<string, string> = {}) {
  const r = await fetch("http://127.0.0.1:11435/mcp", {
    method: "POST",
    headers: { "content-type": "application/json", ...headers },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tools/call", params: { name, arguments: args } }),
  });
  const body = await r.json();
  return { status: r.status, text: String(body?.result?.content?.[0]?.text ?? ""), isError: !!body?.result?.isError };
}

async function yourMemory() {
  await browser.url("tauri://localhost/your-memory/");
  await (await $('[data-testid="memory-add"]')).waitForDisplayed({ timeout: 60_000 });
}

describe("memory rules", () => {
  it("adds a rule, lists it first, and the next answer is given it", async () => {
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
    // The per-scope cache is what every turn's memory block reads (layer 1).
    const cached = await invoke<{ text: string }[]>("list_memory_rows_cached", { scope: "person" });
    expect(cached.some((r) => r.text === RULE)).toBe(true);
    // The AI's read_memory tool (the in-app MCP server on :11435) answers
    // with the rule - asked from the test runner, no project open.
    const mcp = await fetch("http://127.0.0.1:11435/mcp", {
      method: "POST",
      headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tools/call", params: { name: "read_memory", arguments: { topic: "sending" } } }),
    });
    const mcpBody = await mcp.json();
    const toolText = String(mcpBody?.result?.content?.[0]?.text ?? "");
    console.log(`e2e: read_memory -> ${JSON.stringify(toolText.slice(0, 160))}`);
    expect(mcp.status).toBe(200);
    expect(toolText).toContain(RULE);
    expect(toolText).toContain("Rules (follow exactly)");
    // The next answer is given the rule, and the rail under it says so.
    await waitForChat();
    await ask("Rules test: name one colour and nothing else.");
    await waitForReplyDone();
    // The line sits in the reply's Model details (folded until opened).
    const lastReplyEl = (await $$('[data-testid="chat-message"][data-role="assistant"]')).slice(-1)[0];
    await (await lastReplyEl.$("button*=Model")).click();
    const line = await $('[data-testid="memory-line"]');
    await line.waitForDisplayed({ timeout: 15_000 });
    console.log(`e2e: memory line ${JSON.stringify(await line.getText())}`);
    expect(await line.getText()).toContain("1 rule");
    await shot("memory-rule-rail-line");
  });

  it("offers to keep an instruction said in chat as a rule, and the remember tool revises instead of duplicating", async () => {
    await waitForChat();
    // Said in chat, instruction-shaped: one line under the message offers.
    await ask(SAID_RULE);
    const offer = await $('[data-testid="rule-offer"]');
    await offer.waitForDisplayed({ timeout: 15_000 });
    await shot("memory-rule-offer");
    await (await $('[data-testid="rule-offer-keep"]')).click();
    await (await $('[data-testid="rule-kept"]')).waitForDisplayed({ timeout: 30_000 });
    await waitForReplyDone().catch(() => undefined);
    const rows = await invoke<{ kind: string; text: string }[]>("list_memory_rows", { agentKey: null, scope: "person" });
    expect(rows.some((r) => r.kind === "rule" && r.text === SAID_RULE)).toBe(true);
    // The remember tool: said twice with new words, one row with the newer text.
    const first = await mcpCall("remember", { text: "Run npm test before every commit (e2e)", kind: "command" });
    console.log(`e2e: remember 1 -> ${JSON.stringify(first.text)}`);
    expect(first.isError).toBe(false);
    const second = await mcpCall("remember", { text: "Always run npm test before each commit (e2e)", kind: "command" });
    console.log(`e2e: remember 2 -> ${JSON.stringify(second.text)}`);
    expect(second.text).toContain("replaces");
    const commands = (await invoke<{ kind: string; text: string }[]>("list_memory_rows", { agentKey: null, scope: "person" })).filter((r) => r.kind === "command" && r.text.includes("(e2e)"));
    console.log(`e2e: commands ${JSON.stringify(commands.map((c) => c.text))}`);
    expect(commands.length).toBe(1);
    expect(commands[0].text).toContain("Always run");
    // A project scope: a temp folder stands in for an open project; the
    // id file is made on first use, and the rows meet under project:<id>.
    const { mkdtempSync } = await import("node:fs");
    const { tmpdir } = await import("node:os");
    const { join } = await import("node:path");
    const folder = mkdtempSync(join(tmpdir(), "yoai-proj-"));
    const pid = await invoke<string>("project_identity_for", { folder });
    console.log(`e2e: project id ${pid}`);
    expect(pid.startsWith("id:")).toBe(true);
    const proj = await mcpCall("remember", { text: "Deploy with firebase deploy --only hosting (e2e)", kind: "command", scope: "project" }, { "x-project": folder });
    console.log(`e2e: remember project -> ${JSON.stringify(proj.text)}`);
    expect(proj.text).toContain("this project");
    const readBack = await mcpCall("read_memory", { topic: "firebase" }, { "x-project": folder });
    expect(readBack.text).toContain("firebase deploy --only hosting (e2e)");
    const noProject = await mcpCall("read_memory", { topic: "firebase" });
    expect(noProject.text).not.toContain("firebase deploy");
    const projRows = await invoke<any[]>("list_memory_rows", { agentKey: null, scope: `project:${pid.slice(0)}` });
    const projCmd = projRows.find((r) => r.kind === "command" && r.text.includes("(e2e)"));
    expect(projCmd).toBeDefined();
    await invoke("save_memory_row", { agentKey: null, row: { ...projCmd, deleted: true, updated_at: Date.now(), action_hash: null } });
    // Tidy: the command goes (a deletion is a record).
    const cmd = (await invoke<any[]>("list_memory_rows", { agentKey: null, scope: "person" })).find((r) => r.kind === "command" && r.text.includes("(e2e)"));
    await invoke("save_memory_row", { agentKey: null, row: { ...cmd, deleted: true, updated_at: Date.now(), action_hash: null } });
  });

  it("dreaming merges two commands that say the same, shows it in the history, and Undo brings both back", async () => {
    const stamp = Date.now().toString(36);
    const mk = (id: string, text: string, at: number) => ({
      id, scope: "person", kind: "command", text, structured: null, as_of: at, valid_to: null, saved_by: "person",
      provenance: "authored", pinned: false, topics: [], replaces: [], deleted: false, updated_at: at,
    });
    const A = `Run cargo test before every release (dream ${stamp})`;
    const B = `Always run cargo test before each release (dream ${stamp})`;
    await invoke("save_memory_row", { agentKey: null, row: mk(`dream-a-${stamp}`, A, Date.now() - 2000) });
    await invoke("save_memory_row", { agentKey: null, row: mk(`dream-b-${stamp}`, B, Date.now() - 1000) });
    // A note with a relative date and two statements: the model-assisted
    // pass (through the memory slot) dates it, splits it or tags it - never
    // rewords it. Which of the three a small model manages may vary; one
    // assisted revision is the proof.
    const noteText = `Met Alex at the market yesterday. The office opens at nine (dream ${stamp})`;
    await invoke("save_memory_row", { agentKey: null, row: { ...mk(`dream-n-${stamp}`, noteText, Date.now() - 3000), kind: "note" } });
    const n = await invoke<number>("dream_now", { scope: "person", agentKey: null });
    console.log(`e2e: dream wrote ${n} revision(s)`);
    expect(n).toBeGreaterThan(0);
    const assisted = (await invoke<any[]>("list_memory_revisions", { agentKey: null, scope: "person" }))
      .filter((r) => (r.id === `dream-n-${stamp}` || r.id.startsWith(`dream-n-${stamp}-`)) && r.provenance.startsWith("dream:"));
    console.log(`e2e: assisted ${JSON.stringify(assisted.map((r) => [r.id, r.text.slice(0, 40), r.as_of, r.topics]))}`);
    expect(assisted.length).toBeGreaterThan(0);
    for (const r of assisted) expect(noteText.toLowerCase()).toContain(r.text.toLowerCase().replace(/ \(dream.*$/, "").slice(0, 12));
    const live = (await invoke<any[]>("list_memory_rows", { agentKey: null, scope: "person" })).filter((r) => r.text.includes(`(dream ${stamp})`));
    expect(live.length).toBe(1);
    expect(live[0].text).toBe(B);
    expect(live[0].provenance.startsWith("dream:")).toBe(true);
    expect(live[0].provenance).toContain("authored");
    // The history shows it; Undo restores both.
    await yourMemory();
    const section = await $('[data-testid="memory-dreams"]');
    await section.waitForDisplayed({ timeout: 60_000 });
    await (await section.$("button*=Show")).click();
    const entry = await $(`[data-testid="memory-dream"]*=${B.slice(0, 24)}`);
    await entry.waitForDisplayed({ timeout: 15_000 });
    console.log(`e2e: dream line ${JSON.stringify(await entry.getText())}`);
    expect(await entry.getText()).toContain("Merged");
    await shot("memory-dream-history");
    await (await entry.$("button=Undo")).click();
    await browser.waitUntil(async () => {
      const rows = (await invoke<any[]>("list_memory_rows", { agentKey: null, scope: "person" })).filter((r) => r.text.includes(`(dream ${stamp})`));
      return rows.length === 2;
    }, { timeout: 30_000, timeoutMsg: "Undo did not bring both commands back" });
    // Tidy (the note and anything the dream made of it too).
    for (const r of (await invoke<any[]>("list_memory_rows", { agentKey: null, scope: "person" })).filter((x) => x.text.includes(`(dream ${stamp})`) || x.id.startsWith(`dream-n-${stamp}`))) {
      await invoke("save_memory_row", { agentKey: null, row: { ...r, deleted: true, updated_at: Date.now(), action_hash: null } });
    }
  });

  it("keeps the rule across a reload and removes it as a record", async () => {
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
    // Tidy the rule kept from chat the same way.
    const said = (await invoke<any[]>("list_memory_rows", { agentKey: null, scope: "person" })).find((r) => r.text === SAID_RULE);
    if (said) await invoke("save_memory_row", { agentKey: null, row: { ...said, deleted: true, updated_at: Date.now(), action_hash: null } });
  });
});
