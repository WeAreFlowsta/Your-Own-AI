// Your own server with an access key: a server that asks for one refuses a
// connect without it and with a wrong one, says which, and takes the right
// one; the key is kept across a reload and travels on the chat path too.
// The server is a stand-in started here, answering like any
// OpenAI-compatible server started with an API key (and like this app's
// own External access): 401 without the right bearer.
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { resolve } from "node:path";
import { SHOTS } from "../../wdio.conf";

const shot = (name: string) => browser.saveScreenshot(resolve(SHOTS, `${name}.png`));

const KEY = "yoai-e2e-right-key";
const MODEL = "e2e-server-model";

/** What the stand-in saw: one line per request, with the bearer it carried. */
const seen: { path: string; bearer: string | null }[] = [];
let server: Server;
let base = "";

function startServer(): Promise<void> {
  server = createServer((req, res) => {
    const auth = req.headers.authorization ?? "";
    const bearer = auth.startsWith("Bearer ") ? auth.slice(7) : null;
    const path = (req.url ?? "").split("?")[0];
    seen.push({ path, bearer });
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      if (bearer !== KEY) {
        res.writeHead(401, { "content-type": "application/json" });
        res.end(JSON.stringify({ error: { message: "A valid access key is required.", code: "invalid_access_key" } }));
        return;
      }
      if (path === "/v1/models") {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ object: "list", data: [{ id: MODEL, object: "model" }] }));
      } else if (path === "/props") {
        res.writeHead(200, { "content-type": "application/json" });
        res.end(JSON.stringify({ default_generation_settings: { n_ctx: 8192 } }));
      } else if (path === "/v1/chat/completions") {
        const stream = (() => { try { return !!JSON.parse(body).stream; } catch { return false; } })();
        if (stream) {
          res.writeHead(200, { "content-type": "text/event-stream" });
          const chunk = { id: "e2e", object: "chat.completion.chunk", model: MODEL, choices: [{ index: 0, delta: { role: "assistant", content: "ready" }, finish_reason: null }] };
          res.write(`data: ${JSON.stringify(chunk)}\n\n`);
          res.write(`data: ${JSON.stringify({ ...chunk, choices: [{ index: 0, delta: {}, finish_reason: "stop" }] })}\n\n`);
          res.end("data: [DONE]\n\n");
        } else {
          res.writeHead(200, { "content-type": "application/json" });
          res.end(JSON.stringify({
            id: "e2e", object: "chat.completion", model: MODEL,
            choices: [{ index: 0, message: { role: "assistant", content: "ready" }, finish_reason: "stop" }],
            usage: { prompt_tokens: 5, completion_tokens: 1, total_tokens: 6 },
          }));
        }
      } else {
        res.writeHead(404);
        res.end();
      }
    });
  });
  return new Promise((ok) => server.listen(0, "127.0.0.1", () => {
    base = `http://127.0.0.1:${(server.address() as AddressInfo).port}`;
    ok();
  }));
}

/** Set an input the way typing would: Qwik's onInput$ reads the event. */
const fill = (testId: string, text: string) =>
  browser.execute((id: string, t: string) => {
    const el = document.querySelector(`[data-testid="${id}"]`) as HTMLInputElement;
    el.value = t;
    el.dispatchEvent(new Event("input", { bubbles: true }));
  }, testId, text);

async function openEngines() {
  await browser.url("tauri://localhost/settings/");
  await browser.waitUntil(async () => !(await $("#app-loading").isDisplayed()), { timeout: 60_000, timeoutMsg: "the startup loader never cleared" });
}

async function connectWith(key: string) {
  const url = await $('[data-testid="own-server-url"]');
  await url.waitForExist({ timeout: 60_000 });
  await url.scrollIntoView();
  await fill("own-server-url", base);
  await fill("own-server-key", key);
  await (await $('[data-testid="own-server-connect"]')).click();
}

const errorText = async () => {
  const e = await $('[data-testid="own-server-error"]');
  return (await e.isExisting()) ? await e.getText() : "";
};

describe("your own server with an access key", () => {
  before(async () => {
    await startServer();
  });

  after(async () => {
    // Leave the scratch profile with no server connected for the next spec.
    await browser.execute(() => (window as any).__TAURI__.core.invoke("remove_external_engine")).catch(() => {});
    await new Promise((ok) => server.close(ok));
  });

  it("says the server asks for a key when none is given", async () => {
    await openEngines();
    await connectWith("");
    await browser.waitUntil(async () => (await errorText()).includes("asks for an access key"), {
      timeout: 30_000,
      timeoutMsg: async () => `error shown: "${await errorText()}"`,
    });
    await shot("60-own-server-no-key");
    expect(seen.some((r) => r.path === "/v1/models" && r.bearer === null)).toBe(true);
  });

  it("says the key was not accepted when it is wrong", async () => {
    await connectWith("yoai-e2e-wrong-key");
    await browser.waitUntil(async () => (await errorText()).includes("did not accept the access key"), {
      timeout: 30_000,
      timeoutMsg: async () => `error shown: "${await errorText()}"`,
    });
    await shot("61-own-server-wrong-key");
    expect(seen.some((r) => r.path === "/v1/models" && r.bearer === "yoai-e2e-wrong-key")).toBe(true);
  });

  it("connects with the right key and lists the server's model", async () => {
    seen.length = 0;
    await connectWith(KEY);
    const status = await $('[data-testid="own-server-status"]');
    await status.waitForExist({ timeout: 90_000 });
    await browser.waitUntil(async () => (await status.getAttribute("data-healthy")) === "1", { timeout: 30_000, timeoutMsg: "the server never showed as reachable" });
    const text = await status.getText();
    expect(text).toContain(base);
    expect(text).toContain("1 model");
    expect(text).toContain("access key saved");
    await shot("62-own-server-connected");
    // The connect probe, the bench and the window probe all carried the key.
    for (const path of ["/v1/models", "/v1/chat/completions", "/props"]) {
      expect(seen.filter((r) => r.path === path).every((r) => r.bearer === KEY)).toBe(true);
      expect(seen.some((r) => r.path === path)).toBe(true);
    }
  });

  it("keeps the key across a reload, and the window never receives it", async () => {
    seen.length = 0;
    await openEngines();
    // The section reads its saved server when it scrolls INTO VIEW (a Qwik
    // visible task): until then it shows the empty connect form. Scroll to
    // the section first, then wait for the saved server to replace the form.
    const card = await $('[data-testid="own-server-url"], [data-testid="own-server-status"]');
    await card.waitForExist({ timeout: 60_000 });
    await card.scrollIntoView();
    const status = await $('[data-testid="own-server-status"]');
    await status.waitForExist({ timeout: 30_000 });
    await browser.waitUntil(async () => (await status.getAttribute("data-healthy")) === "1", { timeout: 30_000, timeoutMsg: "the server did not answer after the reload" });
    expect(await status.getText()).toContain("access key saved");
    expect(seen.some((r) => r.path === "/v1/models" && r.bearer === KEY)).toBe(true);
    const info = await browser.execute(() => (window as any).__TAURI__.core.invoke("external_engine_info"));
    expect(JSON.stringify(info)).not.toContain(KEY);
    expect((info as { has_key: boolean }).has_key).toBe(true);
  });

  it("sends the key with a chat to the server's model", async () => {
    seen.length = 0;
    const reply = await browser.executeAsync((model: string, done: (r: unknown) => void) => {
      const t = (window as any).__TAURI__;
      const requestId = `e2e-own-server-${Date.now()}`;
      let text = "";
      const finish = (r: unknown) => { done(r); };
      t.event.listen(`chat-stream-${requestId}`, (e: { payload: any }) => {
        const p = e.payload;
        if (typeof p?.content === "string") text += p.content;
        if (typeof p?.delta === "string") text += p.delta;
      }).then(() =>
        t.core.invoke("stream_chat_completion", {
          requestId,
          messages: [{ role: "user", content: "Reply with the single word: ready" }],
          model: `external:${model}`,
          maxTokens: 8,
        }),
      ).then(() => finish({ ok: true, text }), (err: unknown) => finish({ ok: false, err: String(err) }));
    }, MODEL);
    await shot("63-own-server-chat");
    expect((reply as { ok: boolean }).ok).toBe(true);
    const chats = seen.filter((r) => r.path === "/v1/chat/completions");
    expect(chats.length).toBeGreaterThan(0);
    expect(chats.every((r) => r.bearer === KEY)).toBe(true);
  });
});
