// A made-up notes folder and ten questions only it answers, on the built
// app: six details, three things that changed over time (an older note
// discusses the old value at length, the current one states the new value
// in a row), and one question no note answers. Each file is given the day
// it was last changed, so the passages carry dates that differ.
import { cpSync, existsSync, readFileSync, renameSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { waitForChat, ask, waitForReplyDone, invoke, shot } from "./_helpers";
import { askDocumentQuestions, count, rankDocumentQuestions, leaveConversationsOut, summary, type DocQuestion } from "./_documentQuestions";

const FIXTURES = resolve(process.cwd(), "e2e", "fixtures");
const OUT = process.env.YOAI_MEMORY_OUT ?? "";
let addedFolder: string | null = null;
const DIR = resolve(process.cwd(), "e2e", "profile", "notes-questions");
let questions: DocQuestion[] = [];

type NoteItem = { id: string; text: string; item: { state: string; subject?: string | null; confirmed?: unknown; source: { kind: string; doc?: { name: string; state?: string | null } | null } } };
type NoteView = { id: string; state: string; name: string };
const noteItems = async () => (await invoke<NoteItem[]>("memory_items")).filter((i) => i.item.source?.kind === "document");
const textOf = async (sel: string) => (await browser.execute((q) => (document.querySelector(q) as HTMLElement | null)?.innerText ?? "", sel)) as string;
const press = async (sel: string) => {
  const el = await $(sel);
  await el.waitForExist({ timeout: 30_000 });
  await browser.execute((e) => (e as HTMLElement).click(), el);
};
async function syncFolder(): Promise<void> {
  for (let i = 0; i < 24; i++) {
    try {
      await invoke("corpus_folder_sync", { folderId: addedFolder });
      return;
    } catch (e) {
      if (!String(e).includes("busy") && !String(e).includes("timed out")) throw e;
      await new Promise((r) => setTimeout(r, 5000));
    }
  }
}
async function toChat(): Promise<void> {
  await browser.url("tauri://localhost/chat/");
  await waitForChat();
}

describe("questions a notes folder answers", () => {
  after(async () => {
    if (addedFolder) await invoke("corpus_folder_remove", { folderId: addedFolder, removeDocuments: true }).catch(() => {});
  });

  it("gives the answering note's passage, with its date, and the current value", async function () {
    this.timeout(3_600_000);
    const set = JSON.parse(readFileSync(resolve(FIXTURES, "notes-questions.json"), "utf8")) as { changed: Record<string, string>; questions: DocQuestion[] };
    const dir = DIR;
    questions = set.questions;
    rmSync(dir, { recursive: true, force: true });
    cpSync(resolve(FIXTURES, "notes"), dir, { recursive: true });
    for (const [name, day] of Object.entries(set.changed)) {
      const at = new Date(`${day}T10:00:00Z`);
      utimesSync(resolve(dir, name), at, at);
    }
    await waitForChat();
    await ask("Name one primary colour and nothing else.");
    await waitForReplyDone();
    const aiId = (await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const store = await t.store.load(await t.core.invoke("profile_store_path", { name: "ai-data.json" }));
      return (((await store.get("custom-ais")) || []) as { id?: string }[])[0]?.id ?? "";
    })) as string;
    expect(aiId).toBeTruthy();
    const folderId = await invoke<string>("corpus_folder_add", { path: dir, aiId, kind: null });
    addedFolder = folderId;
    let report: unknown = null;
    for (let i = 0; i < 24 && report === null; i++) {
      try {
        report = await invoke<unknown>("corpus_folder_sync", { folderId });
      } catch (e) {
        if (!String(e).includes("busy") && !String(e).includes("timed out")) throw e;
        await new Promise((r) => setTimeout(r, 5000));
      }
    }
    const files = Object.keys(set.changed).length;
    let ready = 0;
    await browser.waitUntil(
      async () => {
        const docs = await invoke<{ chunk_count: number }[]>("corpus_documents", { aiId });
        ready = docs.filter((d) => d.chunk_count > 0).length;
        return ready >= files;
      },
      { timeout: 600_000, interval: 5_000, timeoutMsg: () => `only ${ready} of ${files} notes were read in ten minutes` },
    );
    await leaveConversationsOut();
    if (OUT && process.env.YOAI_E2E_DOC_DUMP) writeFileSync(resolve(OUT, "notes-question-ranks.json"), JSON.stringify(await rankDocumentQuestions(set.questions, true)));
    const results = await askDocumentQuestions(set.questions);
    console.log(`e2e: NOTES QUESTIONS: ${summary(results)}`);
    if (OUT) writeFileSync(resolve(OUT, "notes-questions-results.json"), JSON.stringify(results, null, 1));
    // The search is the app's and does not vary: the note that answers is
    // among what was given, every time.
    const answerable = results.filter((r) => r.type !== "F");
    expect(answerable.filter((r) => !r.sourceGiven).map((r) => r.n)).toEqual([]);
    // The reply is the model's and varies a little from run to run.
    expect(count(results, (r) => r.type === "D" && r.right)).toBeGreaterThanOrEqual(5);
  });
  it("a line that answered sits on its Subject page as a row from the note", async function () {
    this.timeout(900_000);
    // The tyre supplier's account number: asked in the first step, answered from suppliers.md.
    let kept: NoteItem | undefined;
    await browser.waitUntil(async () => !!(kept = (await noteItems()).find((i) => i.text.includes("CR-20417"))), { timeout: 180_000, interval: 3_000, timeoutMsg: "no row was kept for the line that answered" });
    expect(kept!.item.source.doc?.name).toBe("suppliers.md");
    expect(kept!.item.confirmed ?? null).toBe(null);
    expect(kept!.item.subject).toBeTruthy();
    await browser.url("tauri://localhost/your-memory/");
    await press('[data-testid="map-mode-list"]');
    await press(`[data-testid="map-subject-row"][data-id="${kept!.item.subject}"]`);
    const row = `[data-testid="note-row"][data-id="${kept!.id}"]`;
    await (await $(row)).waitForExist({ timeout: 30_000 });
    await browser.waitUntil(async () => (await textOf(`${row} [data-testid="note-heading"]`)).includes("edited"), { timeout: 20_000 });
    expect(await textOf(`${row} [data-testid="note-heading"]`)).toBe("From your notes: suppliers.md, edited August 1, 2026");
    expect(await textOf(`${row} [data-testid="note-reason"]`)).toContain('You asked "What is our account number with the tyre supplier?"');
    expect(await textOf(row)).toContain("CR-20417");
    // The note is the person's: nothing here corrects or forgets it.
    expect(await textOf(row)).not.toContain("Correct");
    expect(await textOf(row)).not.toContain("Forget");
    expect(await (await $(`${row} [data-testid="note-do-not-use"]`)).isExisting()).toBe(true);
    await shot("notes-row-on-subject");
  });

  it("an edited note is read again: the old line is no longer used and the new one answers", async function () {
    this.timeout(900_000);
    const file = resolve(DIR, "suppliers.md");
    writeFileSync(file, readFileSync(file, "utf8").replace("CR-20417", "CR-55508"));
    await syncFolder();
    let now: NoteView | undefined;
    await browser.waitUntil(async () => (now = (await invoke<NoteView[]>("memory_notes_follow")).find((n) => n.name === "suppliers.md"))?.state === "line-gone", { timeout: 180_000, interval: 5_000, timeoutMsg: () => `the changed line was not noticed: ${JSON.stringify(now)}` });
    await toChat();
    const [again] = await askDocumentQuestions([{ ...questions[0], keys: [["CR-55508"]], stale: ["CR-20417"] }]);
    expect(again.right).toBe(true);
    expect(again.stale).toBe(false);
  });

  it("Do not use this asks its reach, the note is then passed over, and Use again brings it back", async function () {
    this.timeout(900_000);
    const item = (await noteItems()).find((i) => i.item.source.doc?.name === "suppliers.md")!;
    await browser.url("tauri://localhost/your-memory/");
    await press('[data-testid="map-mode-list"]');
    await press(`[data-testid="map-subject-row"][data-id="${item.item.subject}"]`);
    const row = `[data-testid="note-row"][data-id="${item.id}"]`;
    await press(`${row} [data-testid="note-do-not-use"]`);
    const reaches = (await browser.execute((r) => Array.from(document.querySelectorAll(`${r} [data-testid="note-reach-to"]`)).map((b) => b.getAttribute("data-reach")), row)) as string[];
    expect(reaches).toEqual(["line", "note", "folder"]);
    await shot("notes-do-not-use-reach");
    await press(`${row} [data-testid="note-reach-to"][data-reach="note"]`);
    await (await $(row)).waitForExist({ reverse: true, timeout: 30_000 });
    const notUsed = await invoke<{ id: string; reach: string; name: string }[]>("memory_notes_not_used");
    expect(notUsed.map((n) => [n.reach, n.name])).toEqual([["note", "suppliers.md"]]);
    // Asked again: the note is not among what was given.
    await toChat();
    const [without] = await askDocumentQuestions([{ ...questions[0], keys: [["CR-55508"]], stale: [] }]);
    console.log(`e2e: with the note not used: given ${JSON.stringify(without.given)}`);
    expect(without.sourceGiven).toBe(false);
    expect(without.right).toBe(false);
    // The Not used list has it, with the way back.
    await browser.url("tauri://localhost/your-memory/");
    await press('[data-testid="memory-tab"][data-tab="recently"]');
    const entry = '[data-testid="not-used-note"][data-reach="note"]';
    await (await $(entry)).waitForExist({ timeout: 30_000 });
    expect(await textOf(entry)).toContain("Note: suppliers.md");
    await shot("notes-not-used");
    await press(`${entry} [data-testid="not-used-note-again"]`);
    await browser.waitUntil(async () => (await invoke<unknown[]>("memory_notes_not_used")).length === 0, { timeout: 30_000, timeoutMsg: "Use again did not take the choice back" });
    await toChat();
    const [back] = await askDocumentQuestions([{ ...questions[0], keys: [["CR-55508"]], stale: [] }]);
    expect(back.sourceGiven).toBe(true);
  });

  it("a folder that cannot be reached is not a deleted note", async function () {
    this.timeout(600_000);
    const away = `${DIR}-away`;
    const before = (await noteItems()).length;
    expect(before).toBeGreaterThan(0);
    renameSync(DIR, away);
    try {
      const states = await invoke<NoteView[]>("memory_notes_follow");
      console.log(`e2e: with the folder away: ${JSON.stringify(states.map((n) => [n.name, n.state]))}`);
      expect(states.length).toBeGreaterThan(0);
      expect(states.every((n) => n.state === "unreachable")).toBe(true);
      expect((await noteItems()).length).toBe(before);
      expect((await noteItems()).some((i) => i.item.source.doc?.state === "deleted")).toBe(false);
    } finally {
      if (existsSync(away)) renameSync(away, DIR);
    }
    const states = await invoke<NoteView[]>("memory_notes_follow");
    expect(states.some((n) => n.state === "unreachable")).toBe(false);
  });
});
