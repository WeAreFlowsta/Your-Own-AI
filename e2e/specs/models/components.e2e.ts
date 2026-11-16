// The document reader's card: offered in Add-ons > Components with what it
// is for and its size; once its files are in the models folder it reads as
// installed there and is listed in Settings too. Which of the two this run
// sees follows the models folder (YOAI_E2E_MODELS_DIR).
import { shot, waitForChat, invoke } from "./_helpers";

const card = (id: string) => browser.execute((i) => {
  const c = document.querySelector(`[data-testid="component-card"][data-component="${i}"]`) as HTMLElement | null;
  return c ? { installed: c.getAttribute("data-installed"), text: (c.innerText ?? "").replace(/\s+/g, " ").trim() } : null;
}, id);
const cards = () => browser.execute(() => document.querySelectorAll('[data-testid="component-card"]').length);
// A card finds out whether its files are here when it comes into view
// (the pages are long): bring every card that has not asked yet into view.
const showCards = () => browser.execute(() => {
  for (const c of document.querySelectorAll('[data-testid="component-card"][data-installed="checking"]')) (c as HTMLElement).scrollIntoView({ block: "center" });
});
const checking = () => browser.execute(() => document.querySelectorAll('[data-testid="component-card"][data-installed="checking"]').length);

describe("the document reader's card", () => {
  after(async () => {
    await browser.url("tauri://localhost/chat/");
    await waitForChat();
  });

  it("is offered in Add-ons with what it is for, and says so when it is installed", async () => {
    await waitForChat();
    const installed = await invoke<boolean>("is_document_reader_ready");
    await browser.url("tauri://localhost/add-ons/components/");
    await browser.waitUntil(async () => { await showCards(); const c = await card("document-reader"); return !!c && c.installed !== "checking"; }, { timeout: 60_000, interval: 1_000, timeoutMsg: "the document reader's card never showed in Add-ons" });
    const c = (await card("document-reader"))!;
    console.log(`e2e: the card in Add-ons (${installed ? "installed" : "not installed"}): ${c.text.slice(0, 260)}`);
    await shot(`components-reader-${installed ? "installed" : "offered"}`);
    expect(c.text).toContain("Document reader");
    expect(c.installed).toBe(installed ? "yes" : "no");
    if (installed) {
      expect(c.text).toContain("Installed - scanned PDFs and pictures of documents are read with it, on your device.");
    } else {
      expect(c.text).toContain("photos of documents, tables and forms");
      expect(c.text).toContain("1.4");
      expect(c.text).toContain("Download");
    }
    // The built-in reader keeps its own card either way.
    expect(await card("ocrs-scanned-pdf")).not.toBeNull();
  });

  it("is listed in Settings only once it is installed", async () => {
    const installed = await invoke<boolean>("is_document_reader_ready");
    await browser.url("tauri://localhost/settings/");
    await browser.waitUntil(async () => (await cards()) > 0 || !installed, { timeout: 60_000, interval: 1_000, timeoutMsg: "Settings never listed an installed component" });
    // A card that has not asked yet is on the page either way: the answer is
    // read once every card has asked.
    await browser.waitUntil(async () => { await showCards(); return (await checking()) === 0; }, { timeout: 60_000, interval: 1_000, timeoutMsg: "a component card in Settings never finished checking" });
    const c = await card("document-reader");
    expect(c !== null).toBe(installed);
  });
});
