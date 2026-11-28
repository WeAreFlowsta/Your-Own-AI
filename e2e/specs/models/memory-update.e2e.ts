// The update from the older memory, on the built app: the older stores
// are made the way the older builds wrote them (facts an AI learned, a
// name, the short description, a saved note, a project's memory text,
// learning paused), then the update's pass runs. What the person saved is
// carried over and used; what an AI guessed is set aside and given to no
// AI until Keep; the name keeps working; the project's lines are items of
// that project; tidying starts off; nothing waits for an answer.
import * as fs from "node:fs";
import * as path from "node:path";
import { fileURLToPath } from "node:url";
import { waitForChat, ask, waitForReplyDone, invoke, shot } from "./_helpers";

const here = path.dirname(fileURLToPath(import.meta.url));
const BASE = "http://127.0.0.1:11435/internal";
const T = (Date.now() - 300 * 86_400_000) * 1000; // about ten months ago, in microseconds

async function assemble(aiId: string, question: string): Promise<{ systemPart: string }> {
  const res = await fetch(`${BASE}/memory-assemble`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ ai: aiId, question, surface: "chat", windowTokens: 16384 }) });
  const answer = await res.json();
  if (!res.ok || answer?.error) throw new Error(`memory-assemble: ${JSON.stringify(answer?.error ?? res.status)}`);
  return answer.memory;
}

const fact = (id: string, predicate: string, value: string, kind: string, provenance: string, ai: string | null) => ({
  id, subject: "user", predicate, value, confidence: 0.9, valid_from: T, valid_to: null, source_action_hash: null, source_ai_id: ai,
  owner_scope: "personal", key_scope: "personal", entry_kind: kind, provenance, author_identity: null, created_at: T, updated_at: T,
});

type Item = { id: string; text: string; savedBy: string; item: { state: string; confirmed?: unknown; subject?: string | null; audience?: { kind: string; project?: string }; source?: { kind: string; why?: string } | null } };
const items = () => invoke<Item[]>("memory_items");
type Report = { keptByYou: number; setAside: number; name: boolean; projectLines: number; projects: number; portraitRetired: boolean; written: number; tidyingStartsOff: boolean };

let ai = { id: "", name: "", agent: "" };
const folder = path.resolve(here, "../../profile/update-project");
const LINES = ["Deploy with make ship", "The menu lives in data/menu.json", "Prices include tax", "Photos are 1600 pixels wide", "The owner prefers short commit messages"];

describe("the update from the older memory", () => {
  it("starts from stores as the older builds wrote them", async function () {
    this.timeout(900_000);
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
    await invoke("save_memory_facts", {
      facts: [
        fact("f-name", "name_is", "Marta", "fact", "extracted", ai.id),
        fact("f-biz", "runs_business", "Crumb and Kettle, a bakery in Port Albury", "fact", "extracted", ai.id),
        fact("f-portrait", "synthesis", "Marta runs a small bakery and likes plain answers.", "synthesis", "extracted", null),
        fact("f-note", "note", "The flour supplier is Mill and Sons and they deliver on Tuesdays.", "note", "authored", null),
      ],
    });
    fs.mkdirSync(folder, { recursive: true });
    await invoke("project_memory_save", { writerKey: ai.agent, writerLabel: ai.name, folder, content: LINES.map((l) => `- ${l}`).join("\n") });
    await browser.execute(() => localStorage.setItem("yoai-memory-paused", "1"));
    // The app's own pass may have run already on this new, empty profile:
    // the profile is put back to before it, as an updated one starts.
    await fetch(`${BASE}/memory-update-forget`, { method: "POST" });
    expect(await invoke("memory_update_report")).toBe(null);
    // Before the update the older stores are still given.
    const before = await assemble(ai.id, "What do you know about me?");
    expect(before.systemPart).toContain("Their name is Marta.");
    expect(before.systemPart).toContain("Crumb and Kettle");
  });

  it("the pass carries over what was saved, sets aside what was guessed, and reports each thing once", async function () {
    this.timeout(900_000);
    const report = await invoke<Report>("memory_update_run", { learningWasPaused: true });
    console.log(`e2e: update report: ${JSON.stringify(report)}`);
    expect({ name: report.name, setAside: report.setAside, keptByYou: report.keptByYou, projectLines: report.projectLines, projects: report.projects, portrait: report.portraitRetired, off: report.tidyingStartsOff }).toEqual({
      name: true, setAside: 1, keptByYou: 1, projectLines: 5, projects: 1, portrait: true, off: true,
    });
    const all = await items();
    const note = all.find((i) => i.text.includes("Mill and Sons"))!;
    expect(note.item.state).toBe("current");
    expect(note.item.confirmed).toBeTruthy();
    expect(note.item.audience?.kind ?? "all").toBe("all");
    const aside = all.find((i) => i.text.includes("Crumb and Kettle"))!;
    expect(aside.item.state).toBe("set_aside");
    const lines = all.filter((i) => i.item.audience?.kind === "project");
    expect(lines.map((i) => i.text).sort()).toEqual([...LINES].sort());
    expect(lines.every((i) => !!i.item.confirmed && i.item.source?.kind === "legacy")).toBe(true);
    // A second pass writes nothing, and the report stands.
    const again = await invoke<Report>("memory_update_run", { learningWasPaused: true });
    expect(again.written).toBe(report.written);
    expect((await items()).length).toBe(all.length);
    const tidy = await invoke<{ state: string }>("memory_tidy_status");
    expect(tidy.state).toBe("off");
  });

  it("a first message after the update carries the name, and none of the old facts or the description", async () => {
    const after = await assemble(ai.id, "What do you know about me?");
    console.log(`e2e: system part after the update: ${JSON.stringify(after.systemPart.slice(0, 500))}`);
    expect(after.systemPart).toContain("Their name is Marta.");
    expect(after.systemPart).not.toContain("Crumb and Kettle");
    expect(after.systemPart).not.toContain("small bakery");
  });

  it("the line shows once, the chat works with it showing, and Dismiss is for good", async function () {
    this.timeout(600_000);
    await browser.url("tauri://localhost/chat/");
    await waitForChat();
    const line = await $('[data-testid="memory-update-line"]');
    await line.waitForExist({ timeout: 60_000 });
    console.log(`e2e: update line: ${JSON.stringify(await line.getText())}`);
    await shot("update-line");
    // Nothing waits for it: a message is sent and answered with the line untouched.
    await ask("Reply with the one word: ready");
    await waitForReplyDone();
    await browser.url("tauri://localhost/chat/");
    await waitForChat();
    const again = await $('[data-testid="memory-update-line"]');
    await again.waitForExist({ timeout: 60_000 });
    await browser.execute((e) => (e as HTMLElement).click(), await $('[data-testid="memory-update-dismiss"]'));
    await again.waitForExist({ reverse: true, timeout: 20_000 });
    await browser.url("tauri://localhost/chat/");
    await waitForChat();
    await browser.pause(3000);
    expect(await (await $('[data-testid="memory-update-line"]')).isExisting()).toBe(false);
  });

  it("'What changed in the update' says where each thing went", async () => {
    await browser.url("tauri://localhost/memory-update/");
    const page = await $('[data-testid="memory-update-page"]');
    await page.waitForExist({ timeout: 60_000 });
    await (await $('[data-testid="update-row-set-aside"]')).waitForExist({ timeout: 30_000 });
    const words = (await browser.execute((e) => (e as HTMLElement).innerText, page)) as string;
    console.log(`e2e: what changed: ${JSON.stringify(words.split(/\n+/).filter((l) => l.trim()))}`);
    for (const id of ["conversations", "kept", "set-aside", "portrait", "recalled", "rules", "projects", "tidying", "old-conversations"]) {
      expect(await (await $(`[data-testid="update-row-${id}"]`)).isExisting()).toBe(true);
    }
    expect(words).toContain("1 thing you saved with Remember");
    expect(words).toContain("1 fact your AIs had learned about you");
    expect(words).toContain("5 notes from 1 project");
    await shot("update-what-changed");
  });

  it("Keep on a set-aside fact makes it the person's, and the next message carries it", async function () {
    this.timeout(300_000);
    await browser.url("tauri://localhost/your-memory/");
    const list = await $('[data-testid="set-aside-list"]');
    await list.waitForExist({ timeout: 60_000 });
    console.log(`e2e: set aside: ${JSON.stringify((await browser.execute((e) => (e as HTMLElement).innerText, list)) as string)}`);
    await shot("update-set-aside");
    await browser.execute((e) => (e as HTMLElement).click(), await $('[data-testid="set-aside-keep"]'));
    await browser.waitUntil(async () => (await items()).find((i) => i.text.includes("Crumb and Kettle"))?.item.state === "current", { timeout: 60_000, timeoutMsg: "Keep did not make the fact current" });
    const kept = (await items()).find((i) => i.text.includes("Crumb and Kettle"))!;
    expect(kept.item.confirmed).toBeTruthy();
    const next = await assemble(ai.id, "What do you know about me?");
    expect(next.systemPart).toContain("Crumb and Kettle");
    await list.waitForExist({ reverse: true, timeout: 20_000 });
  });
});
