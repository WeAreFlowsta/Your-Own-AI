// What memory adds to a request is built in one place for every path. On
// the built app: the memory text in the request the chat page sent equals
// what the builder gives for that turn; the same rules and today's date
// reach the local port as a persona and as an agent; and a project's rules
// are given on every call in that project and in no other.
import { mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { waitForChat, ask, waitForReplyDone, invoke } from "./_helpers";

const BASE = "http://127.0.0.1:11435/internal";
const stamp = Date.now().toString(36);
const RULE = `Prices are always in dollars, never rounded (one-builder ${stamp})`;
const PROJECT_RULE = `Bread only: no pastry in this project (one-builder ${stamp})`;
const PROJECT_COMMAND = `Print labels: labels --all (one-builder ${stamp})`;
// A plain question: it is not indexed as a turn and no fact is taken from
// it, so the stores are the same when the builder is asked again below.
const QUESTION = "What is the name of my bakery?";

const row = (scope: string, kind: string, text: string) => ({
  id: `one-builder-${kind}-${scope.slice(0, 7)}-${stamp}`, scope, kind, text, structured: null, as_of: Date.now(), valid_to: null,
  saved_by: "person", provenance: "authored", pinned: true, topics: [], replaces: [], deleted: false, updated_at: Date.now(),
});

async function assemble(body: Record<string, unknown>): Promise<any> {
  const res = await fetch(`${BASE}/memory-assemble`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) });
  const answer = await res.json();
  if (answer?.error) throw new Error(`memory-assemble: ${JSON.stringify(answer.error)}`);
  return answer;
}

const systemText = (messages: { role: string; content: unknown }[]) =>
  messages.filter((m) => m.role === "system").map((m) => String(m.content)).join("\n");

function today(): string {
  const d = new Date();
  return `Today is ${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}.`;
}

let ai = { id: "", agent: "" };
let project = "";
let otherProject = "";

describe("one builder for what memory adds to a request", () => {
  it("has a rule, a fact and a project with rules of its own", async () => {
    await waitForChat();
    // The first reply of a new profile gives the AI its place in the records.
    await ask("What colour is the sky on a clear day?");
    await waitForReplyDone();
    const found = await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const p = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(p);
      const ais = ((await store.get("custom-ais")) || []) as { id?: string; agentPubKey?: string }[];
      const asked = (((window as any).__yoaiTurnsFull?.() ?? []) as { agentKey: string }[]).slice(-1)[0]?.agentKey ?? "";
      const a = ais.find((x) => x.agentPubKey === asked) ?? ais[0];
      return a ? { id: a.id ?? "", agent: a.agentPubKey ?? "" } : null;
    });
    expect(found?.agent).toBeTruthy();
    ai = found!;
    await invoke("save_memory_row", { agentKey: null, row: row("person", "rule", RULE) });
    const now = Date.now() * 1000;
    const facts = await invoke<unknown[]>("get_memory_facts");
    await invoke("save_memory_facts", { facts: [...facts, {
      id: `one-builder-fact-${stamp}`, subject: "user", predicate: "runs_business", value: `Crumb & Kettle (${stamp})`, confidence: 0.95,
      valid_from: now, owner_scope: "personal", key_scope: "personal", entry_kind: "fact", provenance: "authored", created_at: now, updated_at: now,
    }] });
    project = await invoke<string>("project_identity_for", { folder: mkdtempSync(join(tmpdir(), "yoai-one-builder-")) });
    otherProject = await invoke<string>("project_identity_for", { folder: mkdtempSync(join(tmpdir(), "yoai-one-builder-other-")) });
    await invoke("save_memory_row", { agentKey: null, row: row(`project:${project}`, "rule", PROJECT_RULE) });
    await invoke("save_memory_row", { agentKey: null, row: row(`project:${project}`, "command", PROJECT_COMMAND) });
    // Every turn reads the per-scope caches, which a read of the scope
    // fills (the page reads a scope after it saves to it).
    await invoke("list_memory_rows", { agentKey: null, scope: "person" });
    await invoke("list_memory_rows", { agentKey: null, scope: `project:${project}` });
    await browser.waitUntil(async () => {
      const person = await invoke<{ text: string }[]>("list_memory_rows_cached", { scope: "person" });
      const proj = await invoke<{ text: string }[]>("list_memory_rows_cached", { scope: `project:${project}` });
      return person.some((r) => r.text === RULE) && proj.length === 2;
    }, { timeout: 60_000, timeoutMsg: "the saved rules never reached the row caches" });
  });

  it("the chat page sends exactly what the builder gives for the turn", async () => {
    await waitForChat();
    await ask(QUESTION);
    await waitForReplyDone();
    const { request } = await (await fetch(`${BASE}/last-chat-request`)).json();
    const sent = systemText(request.messages);
    const { memory } = await assemble({ ai: ai.id, question: QUESTION, surface: "chat" });
    console.log(`e2e: the builder's part (${memory.systemPart.length} characters):\n${memory.systemPart}`);
    expect(memory.systemPart).toContain(RULE);
    // A fact an AI learned by the older way is set aside: it is not in the prompt.
    expect(memory.systemPart).not.toContain(`Crumb & Kettle (${stamp})`);
    expect(memory.systemPart.endsWith(today())).toBe(true);
    expect(memory.counts.rules).toBeGreaterThanOrEqual(1);
    expect(sent).toContain(memory.systemPart);
    // No project is open: none of a project's rules ride along.
    expect(sent).not.toContain("Rules for this project:");
  });

  it("the local port as a persona gets the same part", async () => {
    const chat = (await assemble({ ai: ai.id, question: QUESTION, surface: "chat" })).memory.systemPart;
    const port = await assemble({ ai: ai.id, question: QUESTION });
    expect(systemText(port.messages)).toContain(chat);
    const off = await assemble({ ai: ai.id, question: QUESTION, memory: false });
    expect(systemText(off.messages)).not.toContain(RULE);
  });

  it("an agent gets the rules on every call, and a project's rules only in that project", async () => {
    const plain = await assemble({ ai: ai.id, question: "List the files here.", mode: "agent", messages: [{ role: "system", content: "You are a coding agent." }] });
    const text = systemText(plain.messages);
    console.log(`e2e: an agent's system text (${text.length} characters) starts: ${JSON.stringify(text.slice(0, 200))}`);
    // The caller's own system prompt stays first.
    expect(text.indexOf("You are a coding agent.")).toBeGreaterThanOrEqual(0);
    expect(text.indexOf("You are a coding agent.")).toBeLessThan(text.indexOf(RULE));
    expect(text).toContain(RULE);
    expect(text).toContain(today());
    expect(text).not.toContain(PROJECT_RULE);

    const first = systemText((await assemble({ ai: ai.id, question: "List the files here.", mode: "agent", project })).messages);
    expect(first).toContain(RULE);
    expect(first).toContain(`Rules for this project:\nRules from them, in their words - follow them in every answer:\n- ${PROJECT_RULE}\nCommands they use here:\n- ${PROJECT_COMMAND}`);
    // A later call of the same session, with twenty prompts behind it.
    const history = Array.from({ length: 20 }, (_, i) => [{ role: "user", content: `Step ${i + 1}?` }, { role: "assistant", content: `Done ${i + 1}.` }]).flat();
    const later = systemText((await assemble({ ai: ai.id, question: "And now?", mode: "agent", project, messages: history })).messages);
    expect(later).toContain(PROJECT_RULE);
    expect(later).toContain(RULE);

    const elsewhere = systemText((await assemble({ ai: ai.id, question: "List the files here.", mode: "agent", project: otherProject })).messages);
    expect(elsewhere).toContain(RULE);
    expect(elsewhere).not.toContain(PROJECT_RULE);
    expect(elsewhere).not.toContain("Rules for this project:");
  });

  it("a project rule saved in the middle of a session is in the next call", async () => {
    const LATE = `Weigh the flour before the water (one-builder late ${stamp})`;
    const before = systemText((await assemble({ ai: ai.id, question: "Next step?", mode: "agent", project })).messages);
    expect(before).not.toContain(LATE);
    await invoke("save_memory_row", { agentKey: null, row: { ...row(`project:${project}`, "rule", LATE), id: `one-builder-late-${stamp}` } });
    await invoke("list_memory_rows", { agentKey: null, scope: `project:${project}` });
    await browser.waitUntil(async () => systemText((await assemble({ ai: ai.id, question: "Next step?", mode: "agent", project })).messages).includes(LATE), {
      timeout: 30_000, interval: 2_000, timeoutMsg: "the rule saved mid-session did not reach the next call",
    });
  });

  after(async () => {
    for (const scope of ["person", `project:${project}`]) {
      const rows = await invoke<any[]>("list_memory_rows", { agentKey: null, scope }).catch(() => []);
      for (const r of rows.filter((x) => String(x.text).includes(stamp))) {
        await invoke("save_memory_row", { agentKey: null, row: { ...r, deleted: true, updated_at: Date.now(), action_hash: null } }).catch(() => undefined);
      }
    }
    const facts = await invoke<any[]>("get_memory_facts").catch(() => []);
    await invoke("save_memory_facts", { facts: facts.filter((f) => f.id !== `one-builder-fact-${stamp}`) }).catch(() => undefined);
  });
});
