// Flow 4: a synced folder of notes is read into the AI's documents, and a
// question about it is answered from them, with the passages shown.
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { shot, waitForChat, ask, waitForReplyDone, invoke } from "./_helpers";

describe("documents", () => {
  it("answers from a synced notes folder", async () => {
    await waitForChat();
    // A scratch notes folder with one fact nothing else in the model knows.
    const dir = resolve(process.cwd(), "e2e", "profile", "notes");
    mkdirSync(dir, { recursive: true });
    writeFileSync(resolve(dir, "harbour.md"), "# Harbour notes\n\nThe harbour master's cat is called Pumpernickel. It sleeps on the north pier.\n");
    // The selected AI's id, from the app's own store.
    const aiId = await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const path = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(path);
      const ais = (await store.get("custom-ais")) || [];
      return ais[0]?.id || null;
    });
    expect(aiId).toBeTruthy();
    await invoke("corpus_folder_add", { path: dir, aiId, kind: null });
    // The folder is read and embedded in the background: wait until the note
    // is in the AI's documents with its passages ready (chunk_count > 0),
    // then ask. Asking earlier gets an honest "no notes" - seen on the first run.
    await browser.waitUntil(
      async () => {
        const docs = await invoke<{ meta: unknown; chunk_count: number }[]>("corpus_documents", { aiId });
        return docs.some((d) => JSON.stringify(d.meta).toLowerCase().includes("harbour") && d.chunk_count > 0);
      },
      { timeout: 180_000, timeoutMsg: "the note was not read into the AI's documents within 3 minutes" },
    );
    await waitForChat();
    await ask("What is the harbour master's cat called, according to my notes?");
    const r = await waitForReplyDone();
    await shot("40-documents-reply");
    expect(r.text.toLowerCase()).toContain("pumpernickel");
    const sources = await $('[data-testid="reply-sources"]');
    if (await sources.isExisting()) {
      await sources.click();
      await (await $('[data-testid="reply-library"]')).waitForDisplayed({ timeout: 10_000 });
      await shot("41-documents-shown");
    }
  });
});
