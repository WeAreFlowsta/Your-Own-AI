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
import { randomBytes } from "node:crypto";
import { shot, waitForChat, ask, lastReply, invoke } from "./_helpers";

const FIX = resolve(process.cwd(), "e2e", "fixtures", "reader");
const files = () => browser.execute(() => [...document.querySelectorAll('[data-testid="attached-file"]')].map((e) => e.getAttribute("data-name") ?? ""));
const pictures = () => browser.execute(() => document.querySelectorAll('[data-testid="attached-image"]').length);
// Started, not waited for: a script that returns the attach's promise is
// held until the read ends, and a page read on the processor takes longer
// than a script may (the call was then sent again, and the page read twice).
const attach = (paths: string[]) => browser.execute((p) => { void (window as any).__yoaiAttach(p); }, paths);
const clear = () => browser.execute(() => { for (const b of document.querySelectorAll('[data-testid="attached-file"] button, [data-testid="attached-image"] button')) (b as HTMLElement).click(); });

/** Ask, and wait for THIS question's answer. The last answer on the page
 *  is the previous question's, already done, until the new one starts: a
 *  wait for "done" alone can return on it (2026-11-17: the reply read was
 *  "Reeves is thinking.."). */
async function answer(question: string): Promise<string> {
  const answers = () => browser.execute(() => document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]').length);
  const before = await answers();
  await ask(question);
  await browser.waitUntil(async () => (await answers()) > before, { timeout: 120_000, interval: 1_000, timeoutMsg: "no answer started" });
  await browser.waitUntil(async () => (await lastReply())?.state === "done", { timeout: 300_000, interval: 1_000, timeoutMsg: "the assistant never finished" });
  return (await lastReply())!.text;
}

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

/** A square of noise as a PNG file: it does not compress, so its size is real. */
function noise(file: string, side: number) {
  const chunk = (type: string, data: Buffer) => {
    const body = Buffer.concat([Buffer.from(type), data]);
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    const crc = Buffer.alloc(4); crc.writeUInt32BE(crc32(body) >>> 0);
    return Buffer.concat([len, body, crc]);
  };
  const head = Buffer.alloc(13);
  head.writeUInt32BE(side, 0); head.writeUInt32BE(side, 4); head[8] = 8; head[9] = 2;
  const raw = randomBytes(side * (side * 3 + 1));
  for (let y = 0; y < side; y++) raw[y * (side * 3 + 1)] = 0;
  writeFileSync(file, Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", head), chunk("IDAT", deflateSync(raw, { level: 0 })), chunk("IEND", Buffer.alloc(0))]));
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
    const reply = await answer("What is the estimated cost of the treatment in the attached letter? Answer with the amount only.");
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
    const reply = await answer("On what date is the annual meeting in the attached notice? Answer with the date only.");
    console.log(`e2e: about the notice: ${reply.slice(0, 200)}`);
    expect(reply).toMatch(/18(th)? (of )?June/i);
  });

  it("reads every page of a scanned PDF: a figure on the second page is answered", async function () {
    this.timeout(1_500_000);
    await clear();
    await attach([resolve(FIX, "5-two-pages.pdf")]);
    await browser.waitUntil(async () => (await files()).includes("5-two-pages.pdf"), { timeout: 1_200_000, interval: 3_000, timeoutMsg: "the two-page statement was never read into an attachment" });
    const reply = await answer("In the attached statement, what is the balance carried forward? Answer with the amount only.");
    console.log(`e2e: about the second page: ${reply.slice(0, 200)}`);
    // 312.75 is on the second page and nowhere on the first.
    expect(reply).toContain("312.75");
    await clear();
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

  it("reads a document sent to the local endpoint's read route, and never takes a path", async function () {
    this.timeout(900_000);
    const port = await invoke<number>("inference_port");
    const read = async (body: unknown) => {
      const r = await fetch(`http://127.0.0.1:${port}/v1/documents/read`, { method: "POST", headers: { "Content-Type": "application/json", Authorization: "Bearer local" }, body: JSON.stringify(body) });
      return { status: r.status, json: (await r.json().catch(() => null)) as any };
    };
    const letter = await read({ data: readFileSync(resolve(FIX, "1-letter.pdf")).toString("base64") });
    console.log(`e2e: the read route answered ${letter.status}: ${JSON.stringify(letter.json).slice(0, 200)}`);
    expect(letter.status).toBe(200);
    expect(letter.json.pages).toBe(1);
    expect(letter.json.reader).toBe("document-reader");
    expect(String(letter.json.text)).toContain("186.50");
    // A path is not a document: the route opens nothing of the person's.
    const path = await read({ data: resolve(FIX, "1-letter.pdf") });
    expect(path.status).toBe(400);
    const nothing = await read({ path: resolve(FIX, "1-letter.pdf") });
    expect(nothing.status).toBe(400);
  });

  it("takes a chat request with a large picture, and refuses one over the limit by its size", async function () {
    this.timeout(900_000);
    const port = await invoke<number>("inference_port");
    const ai = await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const path = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(path);
      const ais = ((await store.get("custom-ais")) || []) as { name: string }[];
      return ais[0]?.name ?? null;
    });
    const send = async (content: unknown) => {
      const body = JSON.stringify({ model: ai, messages: [{ role: "user", content }], max_tokens: 20, stream: false });
      const r = await fetch(`http://127.0.0.1:${port}/v1/chat/completions`, { method: "POST", headers: { "Content-Type": "application/json", Authorization: "Bearer local" }, body });
      return { status: r.status, sent: body.length, text: (await r.text().catch(() => "")).slice(0, 200) };
    };
    // A picture of noise does not shrink: about 6 MB as a file, 8 MB in the
    // request. The server library's own limit of 2 MB refused this before
    // anything read it.
    mkdirSync(resolve(process.cwd(), "e2e", "profile"), { recursive: true });
    const big = resolve(process.cwd(), "e2e", "profile", "reader-noise.png");
    noise(big, 1400);
    const picture = `data:image/png;base64,${readFileSync(big).toString("base64")}`;
    const large = await send([{ type: "image_url", image_url: { url: picture } }, { type: "text", text: "Describe this picture in five words." }]);
    console.log(`e2e: a ${Math.round(large.sent / 1048576)} MB chat request answered ${large.status}: ${large.text}`);
    expect(large.sent).toBeGreaterThan(4 * 1048576);
    expect(large.status).not.toBe(413);
    // Over 48 MB: refused for its size.
    const over = await send("x".repeat(49 * 1048576));
    console.log(`e2e: a ${Math.round(over.sent / 1048576)} MB chat request answered ${over.status}`);
    expect(over.status).toBe(413);
  });
});
