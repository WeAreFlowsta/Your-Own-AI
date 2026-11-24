// What memory holds to be true now, on the built app with the made-up
// bakery history: two kept items about the same matter; the note after a
// question; a tidying pass that marks the older one replaced; the older
// exchange left out of what the AI is given and shown as left out; "Not
// replaced" puts it back and the pair is never judged again; an item the
// person kept themselves is never replaced without asking.
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { waitForChat, ask, waitForReplyDone, invoke, shot } from "./_helpers";

const here = path.dirname(fileURLToPath(import.meta.url));
const fixture = path.resolve(here, "../../fixtures/memory");
const BASE = "http://127.0.0.1:11435/internal";
const history = JSON.parse(fs.readFileSync(path.join(fixture, "history.json"), "utf8")) as { conversations: { id: string; title: string; started_at: string | number; messages: unknown[] }[] };
const micros = (when: string | number) => (typeof when === "number" ? when : Date.parse(when) * 1000);
const startOf = (title: string) => micros(history.conversations.find((c) => c.title === title)!.started_at);

async function dev(method: "GET" | "POST", route: string, query: Record<string, string> = {}, body?: unknown): Promise<any> {
  const q = new URLSearchParams(query).toString();
  const res = await fetch(`${BASE}/${route}${q ? `?${q}` : ""}`, {
    method,
    headers: body === undefined ? undefined : { "content-type": "application/json" },
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  const answer = await res.json().catch(() => ({ error: `HTTP ${res.status}` }));
  if (!res.ok || answer?.error) throw new Error(`${route}: ${JSON.stringify(answer?.error ?? res.status)}`);
  return answer;
}

const lastUserText = (request: { messages: { role: string; content: unknown }[] }) => {
  const last = [...request.messages].reverse().find((m) => m.role === "user");
  return typeof last?.content === "string" ? last.content : JSON.stringify(last?.content ?? "");
};

type Item = { id: string; text: string; item: { state: string; replaced_by?: string | null; now?: string | null; confirmed?: unknown } };
const items = () => invoke<Item[]>("memory_items");
const item = async (id: string) => (await items()).find((i) => i.id === id)!;

/** Keep the exchange at `seq` of the conversation titled `title` as an item. */
async function keep(title: string, seq: number, text: string, by?: string): Promise<string> {
  const c = startOf(title);
  const kept = await invoke<{ item: Item; already: boolean }>("memory_keep", {
    input: { text, itemType: "decision", ptr: { c, s: seq, t: c, a: 0, b: 0 }, asOf: Math.round(c / 1000), kind: "exchange", why: by ? "settled" : "pressed", ...(by ? { by } : {}) },
  });
  expect(kept.already).toBe(false);
  return kept.item.id;
}

const assemble = async (question: string) => (await dev("POST", "memory-assemble", {}, { ai: ai.id, question, surface: "chat", windowTokens: 16384 })).memory as { beforeQuestion: string | null; afterQuestion: string | null; given: any };

let ai = { id: "", name: "", agent: "" };
let old40 = "";
let new60 = "";
const DELIVERY = "What is the free delivery threshold?";

describe("what memory holds now, on the made-up history", () => {
  it("has an AI that has answered, and the history read into the index", async () => {
    await waitForChat();
    await ask("What colour is the sky on a clear day?");
    await waitForReplyDone();
    const found = await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const p = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(p);
      const ais = ((await store.get("custom-ais")) || []) as { id?: string; name?: string; agentPubKey?: string }[];
      const asked = (((window as any).__yoaiTurnsFull?.() ?? []) as { agentKey: string }[]).slice(-1)[0]?.agentKey ?? "";
      const a = ais.find((x) => x.agentPubKey === asked) ?? ais[0];
      return a ? { id: a.id ?? "", name: a.name ?? "", agent: a.agentPubKey ?? "" } : null;
    });
    expect(found?.agent).toBeTruthy();
    ai = found!;
    const model = (await invoke<string | null>("get_current_model")) ?? "fixture";
    const loaded = await dev("POST", "memory-fixture-load", { path: path.join(fixture, "history.json"), all: ai.id, model });
    expect(loaded.messages).toBe(66);
    await invoke("memory_read_now", { on: true });
    await browser.waitUntil(async () => {
      const status: any = await invoke("memory_reading_status");
      return status.total >= 66 && status.read >= status.total;
    }, { timeout: 900_000, interval: 5_000, timeoutMsg: "the history was not read into the index within 15 minutes" });
    // With nothing kept there is no note: the question is the last thing sent.
    const before = await assemble(DELIVERY);
    expect(before.afterQuestion).toBeNull();
    expect(before.given.left_out ?? []).toEqual([]);
  });

  it("puts what is kept after the question, as a note", async () => {
    // The January decision, kept by an AI (not confirmed by the person);
    // the March one, kept by the person.
    old40 = await keep("Delivery pricing", 2, "Free delivery applies to orders over $40.", "ai:made-up-founding-key");
    new60 = await keep("Delivery threshold", 0, "Free delivery starts at $60 from Monday March 9. Orders under $60 keep the $6 fee.");
    let note: string | null = null;
    await browser.waitUntil(async () => {
      note = (await assemble(DELIVERY)).afterQuestion;
      return !!note && note.includes("$60") && note.includes("$40");
    }, { timeout: 120_000, interval: 3_000, timeoutMsg: "the two kept items did not both reach the note" }).catch(async (e) => {
      console.log(`e2e: likeness of each item to the question: ${JSON.stringify(await dev("GET", "memory-items-near", { q: DELIVERY }))}`);
      throw e;
    });
    console.log(`e2e: likeness of each item to the question: ${JSON.stringify((await dev("GET", "memory-items-near", { q: DELIVERY })).near)}`);
    console.log(`e2e: the note before tidying: ${note}`);
    expect(note!.startsWith("[From memory, the state record for this, current as of 2026-03-03: ")).toBe(true);
    expect(note!.endsWith(" Anything older that disagrees is history.]")).toBe(true);
    // The newer first; the AI's own note says it is not confirmed.
    expect(note!.indexOf("$60")).toBeLessThan(note!.indexOf("$40"));
    expect(note).toContain("over $40. (an AI's note, not confirmed by them)");
    // A question about something else gets no note.
    expect((await assemble("When do we feed the sourdough starter?")).afterQuestion).toBeNull();
  });

  it("a tidying pass marks the older item replaced, with an undo, and judges nothing twice", async () => {
    const { report } = await dev("POST", "memory-tidy", { force: "1" });
    console.log(`e2e: tidying: ${JSON.stringify(report)}`);
    expect(report.ran).toBe(true);
    expect(report.judged).toBeGreaterThanOrEqual(1);
    const pair = (report.pairs as { earlier: string; later: string; verdict: string }[]).find((p) => p.earlier === old40 && p.later === new60);
    expect(pair?.verdict).toBe("REPLACES");
    expect(report.replaced).toBe(1);
    expect(report.leftInQueue).toBe(0);
    const older = await item(old40);
    expect(older.item.state).toBe("replaced");
    expect(older.item.replaced_by).toBe(new60);
    expect(older.text).toBe("Free delivery applies to orders over $40.");
    const newer = await item(new60);
    expect(newer.item.state).toBe("current");
    console.log(`e2e: what is true now, as the judgment wrote it: ${newer.item.now}`);
    expect(newer.item.now).toContain("60");
    const changes = await invoke<{ what: string; by: string; items: string[] }[]>("memory_changes");
    expect(changes[0]).toMatchObject({ what: "replaced", by: "tidy", items: [old40, new60] });
    // Nothing waits, so a second pass has nothing to do.
    const again = (await dev("POST", "memory-tidy", { force: "1" })).report;
    expect(again.judged).toBe(0);
  });

  it("gives the AI what is true now, and leaves the replaced exchange out", async () => {
    let memory = await assemble(DELIVERY);
    await browser.waitUntil(async () => {
      memory = await assemble(DELIVERY);
      return !!memory.afterQuestion && !memory.afterQuestion.includes("$40");
    }, { timeout: 60_000, interval: 3_000, timeoutMsg: "the note still carries the replaced item" });
    console.log(`e2e: the note after tidying: ${memory.afterQuestion}`);
    console.log(`e2e: left out: ${JSON.stringify(memory.given.left_out)}`);
    expect(memory.afterQuestion).toContain("60");
    expect(memory.given.note_items).toEqual([new60]);
    expect(memory.given.note).toMatch(/^[0-9a-f]{64}$/);
    // The January exchange that set $40 is not given; the report says so.
    expect(memory.beforeQuestion).toContain("older exchanges that memory has marked as replaced were left out");
    expect(memory.beforeQuestion).not.toContain("Free delivery now applies to orders over $40");
    expect(memory.beforeQuestion).toContain("free delivery starts at $60");
    const left = memory.given.left_out as { hit: { c: number; s: number }; item: string }[];
    expect(left.length).toBeGreaterThanOrEqual(1);
    expect(left.every((l) => l.item === old40 && l.hit.c === startOf("Delivery pricing"))).toBe(true);
    // The record holds pointers, never the words.
    expect(JSON.stringify(memory.given)).not.toContain("delivery");
  });

  it("a typed question is sent as block, question, note, and the panel shows what was left out", async () => {
    await waitForChat();
    await (await $('[data-testid="new-chat"]')).click();
    await browser.waitUntil(async () => browser.execute(() => document.querySelectorAll('[data-testid="chat-message"]').length === 0), { timeout: 15_000 });
    await ask(DELIVERY);
    const reply = await waitForReplyDone(600_000);
    console.log(`e2e: the reply: ${JSON.stringify(reply.text.slice(0, 240))}`);
    const { request } = await dev("GET", "last-chat-request");
    const sent = lastUserText(request);
    expect(sent.startsWith("[From memory. A search of all their conversations was run for this question.")).toBe(true);
    expect(sent).toContain(`]\n\n${DELIVERY}\n\n[From memory, the state record for this, current as of 2026-03-03: `);
    expect(sent.endsWith(" Anything older that disagrees is history.]")).toBe(true);
    expect(reply.text).toContain("60");
    // The bubble keeps the question as typed.
    const bubble = await browser.execute(() => ([...document.querySelectorAll('[data-testid="chat-message"][data-role="user"]')].pop() as HTMLElement).innerText);
    expect(bubble).not.toContain("From memory");
    // The panel: what was left out, and why.
    const line = await $('[data-testid="given-line"]');
    if ((await line.isExisting()) && (await $('[data-testid="given-show"]').isExisting())) {
      await (await $('[data-testid="given-show"]')).click();
    } else {
      const lastReplyEl = (await $$('[data-testid="chat-message"][data-role="assistant"]')).slice(-1)[0];
      await (await lastReplyEl.$("button*=Model")).click();
      await (await $('[data-testid="given-open"]')).click();
    }
    const row = await $('[data-testid="given-left-out-row"]');
    await row.waitForDisplayed({ timeout: 30_000 });
    await shot("memory-state-left-out");
    const text = await row.getText();
    console.log(`e2e: the left-out row: ${JSON.stringify(text)}`);
    expect(text).toContain("January 12");
    expect(text).toContain(`so it was left out of what ${ai.name} was given.`);
    expect(await (await $('[data-testid="given-checked"]')).getAttribute("data-checked")).toBe("exact");
  });

  it("'Not replaced' puts it back, and tidying never judges that pair again", async () => {
    await (await $('[data-testid="given-not-replaced"]')).click();
    await browser.waitUntil(async () => (await item(old40)).item.state === "current", { timeout: 30_000, timeoutMsg: "the item did not become current again" });
    await shot("memory-state-not-replaced");
    expect(await (await $('[data-testid="given-left-out-row"]')).getText()).toContain("This is current again");
    const memory = await assemble(DELIVERY);
    expect(memory.given.left_out ?? []).toEqual([]);
    expect(memory.afterQuestion).toContain("$40");
    // The undo is in the ledger every device reads.
    const rows = await invoke<{ id: string; structured?: { sys?: string; judged?: { outcome: string; of?: string } } }[]>("list_memory_rows", { agentKey: null, scope: "memory" });
    const ledger = rows.filter((r) => r.structured?.sys === "undo").map((r) => r.structured!.judged!);
    console.log(`e2e: the ledger: ${JSON.stringify(ledger)}`);
    expect(ledger.some((j) => j.outcome === "replaced")).toBe(true);
    expect(ledger.some((j) => j.outcome === "undone" && j.of === "replaced")).toBe(true);
  });

  it("what the person kept themselves is never replaced without asking", async () => {
    const eight = await keep("Opening hours", 2, "Saturday we open at 8 am. Closed Sunday. We close at 3 every open day.");
    const half7 = await keep("Saturday hours", 0, "Saturday opening moves to 7:30 am from April 18.");
    // The March delivery item waits again too (as a newly kept one would):
    // its pair with the January one is settled, so it is not judged.
    const { report } = await dev("POST", "memory-tidy", { force: "1", queue: new60 });
    console.log(`e2e: tidying, both kept by the person: ${JSON.stringify(report)}`);
    const pair = (report.pairs as { earlier: string; later: string; verdict: string }[]).find((p) => p.earlier === eight && p.later === half7);
    expect(pair).toBeTruthy();
    // Whatever the model said, the older item stands: a "replaces" becomes a question.
    expect((await item(eight)).item.state).toBe("current");
    expect((await item(half7)).item.state).toBe("current");
    if (pair!.verdict === "REPLACES") expect(report.asked).toBeGreaterThanOrEqual(1);
    expect(report.replaced).toBe(0);
    // And the delivery pair, undone before, was not judged again.
    expect((report.pairs as { earlier: string; later: string }[]).some((p) => p.earlier === old40 && p.later === new60)).toBe(false);
  });
});
