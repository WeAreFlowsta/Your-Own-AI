// Images in a chat: two pictures attached to one message reach a model that
// can see, and the answer names what is in each, in order. The pictures are
// two solid squares (red, then blue), so nothing about the answer is taste.
// Needs a models folder with a vision model and its vision file beside it
// (YOAI_E2E_MODELS_DIR: Gemma 4 E2B + its mmproj + the memory components).
import { mkdirSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { deflateSync, crc32 } from "node:zlib";
import { shot, waitForChat, ask, waitForReplyDone, invoke } from "./_helpers";

/** A solid square as a PNG file. */
function square(file: string, rgb: [number, number, number], side = 96) {
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
const chips = () => browser.execute(() => document.querySelectorAll('[data-testid="attached-image"]').length);
const attach = (paths: string[]) => browser.execute((p) => (window as any).__yoaiAttach(p), paths);

describe("vision", () => {
  const dir = resolve(process.cwd(), "e2e", "profile", "pictures");
  const red = resolve(dir, "first.png");
  const blue = resolve(dir, "second.png");

  // Without a model that can see, the whole spec stands aside (a `before`
  // skip takes every test with it; a skip inside one test would not).
  before(async function () {
    await waitForChat();
    const models = await invoke<{ name: string }[]>("list_local_models").catch(() => []);
    const sees = await invoke<unknown>("find_vision_model", { query: null, queryVec: null }).catch(() => null);
    console.log(`e2e: models ${models.map((m) => m.name).join(", ")}; a model that can see: ${sees ? JSON.stringify(sees).slice(0, 120) : "none"}`);
    if (!sees) {
      console.log("e2e: no vision model in this models folder - the vision spec needs YOAI_E2E_MODELS_DIR with Gemma 4 E2B and its vision file");
      this.skip();
    }
  });

  it("holds several pictures on one message, the same one once", async () => {
    mkdirSync(dir, { recursive: true });
    square(red, [255, 0, 0]);
    square(blue, [0, 0, 255]);
    await attach([red]);
    await browser.waitUntil(async () => (await chips()) === 1, { timeout: 20_000, timeoutMsg: "the first picture never showed in the message box" });
    await attach([blue]);
    await browser.waitUntil(async () => (await chips()) === 2, { timeout: 20_000, timeoutMsg: "the second picture replaced the first instead of joining it" });
    await attach([red]);
    await browser.pause(1500);
    expect(await chips()).toBe(2);
    // Attaching the first again moved it to the end: put the order back.
    await browser.execute(() => (document.querySelectorAll('[data-testid="attached-image"] button')[0] as HTMLElement).click());
    await browser.waitUntil(async () => (await chips()) === 1, { timeout: 10_000 });
    await browser.execute(() => (document.querySelector('[data-testid="attached-image"] button') as HTMLElement).click());
    await browser.waitUntil(async () => (await chips()) === 0, { timeout: 10_000, timeoutMsg: "a picture could not be removed" });
    await attach([red]);
    await browser.waitUntil(async () => (await chips()) === 1, { timeout: 20_000 });
    await attach([blue]);
    await browser.waitUntil(async () => (await chips()) === 2, { timeout: 20_000 });
    await shot("vision-two-attached");
  });

  it("answers about both, in the order they were given", async () => {
    await ask("Name the color of the first image, then the color of the second. Answer in a few words.");
    // An AI whose model cannot see offers the one that can, for this message.
    const useVision = await $('[data-testid="vision-use"]');
    const offered = await useVision.waitForDisplayed({ timeout: 60_000 }).then(() => true).catch(() => false);
    if (offered) {
      console.log("e2e: the AI's model cannot see; taking the vision model for this message");
      await shot("vision-offer");
      await useVision.click();
    }
    const reply = await waitForReplyDone(600_000);
    const text = reply.text.toLowerCase();
    console.log(`e2e: vision reply: ${reply.text.replace(/\s+/g, " ").slice(0, 200)}`);
    await shot("vision-answer");
    const sent = await browser.execute(() => {
      const mine = [...document.querySelectorAll('[data-testid="chat-message"][data-role="user"]')].pop();
      return mine ? mine.querySelectorAll('img[alt="attached"]').length : 0;
    });
    expect(sent).toBe(2);
    expect(text).toContain("red");
    expect(text).toContain("blue");
    expect(text.indexOf("red")).toBeLessThan(text.lastIndexOf("blue"));
    // The pictures went with that message; the box is empty for the next one.
    expect(await chips()).toBe(0);
  });
});
