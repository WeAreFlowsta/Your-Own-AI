// A made-up notes folder and ten questions only it answers, on the built
// app: six details, three things that changed over time (an older note
// discusses the old value at length, the current one states the new value
// in a row), and one question no note answers. Each file is given the day
// it was last changed, so the passages carry dates that differ.
import { cpSync, readFileSync, rmSync, utimesSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { waitForChat, ask, waitForReplyDone, invoke } from "./_helpers";
import { askDocumentQuestions, count, rankDocumentQuestions, leaveConversationsOut, summary, type DocQuestion } from "./_documentQuestions";

const FIXTURES = resolve(process.cwd(), "e2e", "fixtures");
const OUT = process.env.YOAI_MEMORY_OUT ?? "";
let addedFolder: string | null = null;

describe("questions a notes folder answers", () => {
  after(async () => {
    if (addedFolder) await invoke("corpus_folder_remove", { folderId: addedFolder, removeDocuments: true }).catch(() => {});
  });

  it("gives the answering note's passage, with its date, and the current value", async function () {
    this.timeout(3_600_000);
    const set = JSON.parse(readFileSync(resolve(FIXTURES, "notes-questions.json"), "utf8")) as { changed: Record<string, string>; questions: DocQuestion[] };
    const dir = resolve(process.cwd(), "e2e", "profile", "notes-questions");
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
});
