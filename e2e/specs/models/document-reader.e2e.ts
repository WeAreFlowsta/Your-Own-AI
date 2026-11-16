// The document reader (an optional component): a scanned PDF and a picture
// of a document are read into text before the AI answers, in the chat and
// through the local endpoint; an ordinary picture stays a picture. The
// documents are made up (e2e/fixtures/reader), so every answer is a figure
// the page holds.
// Needs a models folder with a chat model and the reader's three files
// (YOAI_E2E_MODELS_DIR: MiniCPM-OCR-1_0-Q4_K_M.gguf, its -mmproj-f16.gguf,
// document-check.rten). Slow on a processor: a page can take over a minute.
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { deflateSync, crc32 } from "node:zlib";
import { shot, waitForChat, ask, waitForReplyDone, lastReply, invoke } from "./_helpers";

const FIX = resolve(process.cwd(), "e2e", "fixtures", "reader");
const files = () => browser.execute(() => [...document.querySelectorAll('[data-testid="attached-file"]')].map((e) => e.getAttribute("data-name") ?? ""));
const pictures = () => browser.execute(() => document.querySelectorAll('[data-testid="attached-image"]').length);
const attach = (paths: string[]) => browser.execute((p) => (window as any).__yoaiAttach(p), paths);
const clear = () => browser.execute(() => { for (const b of document.querySelectorAll('[data-testid="attached-file"] button, [data-testid="attached-image"] button')) (b as HTMLElement).click(); });

/** A solid square as a PNG file: a picture with no words in it. */
function square(file: string, rgb: [number, number, number], side = 256) {
  const chunk = (type: string, data: Buffer) => {
    const body = Buffer.concat([Buffer.from(type), data]);
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(body) >>> 0);
    return Buffer.concat([len, body, crc]);
  };
  const head = Buffer.alloc(13);
  head.writeUInt32BE(side, 0); head.writeUInt32BE(side, 4); head[8] = 8; head[9] = 2;
  const row = Buffer.concat([Buffer.from([0]), Buffer.concat(Array.from({ length: side }, () => Buffer.from(rgb)))]);
  const raw = Buffer.concat(Array.from({ length: side }, () => row));
  writeFileSync(file, Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", head), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]));
}

describe("the document reader", () => {
  before(async function () {
    await waitForChat();
    const ready = await invoke<boolean>("is_document_reader_ready").catch(() => false);
    console.log(`e2e: the document reader is ${ready ? "installed" : "not installed"}`);
    if (!ready) {
      console.log("e2e: the document reader spec needs YOAI_E2E_MODELS_DIR with the reader's three files");
      this.skip();
    }
  });

  it("reads a scanned PDF and the AI answers with a figure from it", async function () {
    this.timeout(900_000);
    await attach([resolve(FIX, "1-letter.pdf")]);
    await browser.waitUntil(async () => (await files()).includes("1-letter.pdf"), { timeout: 600_000, interval: 3_000, timeoutMsg: "the scanned letter was never read into an attachment" });
    await shot("reader-pdf-attached");
    await ask("What is the estimated cost of the treatment in the attached letter? Answer with the amount only.");
    await waitForReplyDone(300_000);
    const reply = await lastReply();
    console.log(`e2e: about the letter: ${reply.slice(0, 200)}`);
    expect(reply).toContain("186.50");
  });

  it("reads a photo of a document into text, and leaves an ordinary picture a picture", async function () {
    this.timeout(900_000);
    await clear();
    const dir = resolve(process.cwd(), "e2e", "profile", "pictures");
    mkdirSync(dir, { recursive: true });
    const plain = resolve(dir, "plain.png");
    square(plain, [40, 120, 200]);
    await attach([plain]);
    await browser.waitUntil(async () => (await pictures()) === 1, { timeout: 60_000, timeoutMsg: "the ordinary picture never showed" });
    await browser.pause(4_000);
    expect(await files()).toEqual([]);
    await clear();
    await attach([resolve(FIX, "4-photo.jpg")]);
    await browser.waitUntil(async () => (await files()).includes("4-photo.jpg (text)"), { timeout: 600_000, interval: 3_000, timeoutMsg: `the photo of a notice was never read as a document: ${JSON.stringify(await files())}` });
    expect(await pictures()).toBe(1);
    await shot("reader-photo-attached");
    await ask("On what date is the annual meeting in the attached notice? Answer with the date only.");
    await waitForReplyDone(300_000);
    const reply = await lastReply();
    console.log(`e2e: about the notice: ${reply.slice(0, 200)}`);
    expect(reply).toMatch(/18(th)? (of )?June/i);
  });

  it("gives a picture of a document sent to the local endpoint as its text", async function () {
    this.timeout(900_000);
    const port = await invoke<number>("inference_port");
    const ai = await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const path = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(path);
      const ais = ((await store.get("custom-ais")) || []) as { name: string }[];
      return ais[0]?.name ?? null;
    });
    expect(ai).toBeTruthy();
    const picture = `data:image/png;base64,${readFileSync(resolve(FIX, "3-form.png")).toString("base64")}`;
    // From the test's own process: the webview's origin may not fetch the local server.
    const r = await fetch(`http://127.0.0.1:${port}/v1/chat/completions`, {
      method: "POST",
      headers: { "Content-Type": "application/json", Authorization: "Bearer local" },
      body: JSON.stringify({
        model: ai,
        messages: [{ role: "user", content: [{ type: "image_url", image_url: { url: picture } }, { type: "text", text: "What is the postcode on this form? Answer with the postcode only." }] }],
        max_tokens: 40,
        stream: false,
      }),
    });
    const j = (await r.json().catch(() => null)) as any;
    const text = String(j?.choices?.[0]?.message?.content ?? "");
    console.log(`e2e: the endpoint answered ${r.status}: ${text.slice(0, 200)} ${j?.error ? JSON.stringify(j.error).slice(0, 200) : ""}`);
    expect(r.status).toBe(200);
    expect(text.replace(/\s+/g, " ")).toContain("GL5 2HT");
  });
});
