// Memory and an AI that answers online, on the built app. The spec stands
// in for the online service: it listens on the address the app was launched
// to use (YOAI_PROXY_URL), answers like the service and keeps every request
// it is sent, so what left this computer can be read back word for word.
// Run with YOAI_PROXY_URL=http://127.0.0.1:18475.
//
// What is held: the first message to an online AI is sent with nothing to
// answer first; what memory keeps about the person rides with it and the
// line under the reply says so; with sharing turned off nothing from memory
// is in the next request; a message that looks like it holds a key is
// asked about before anything is sent.
import { createServer, type Server } from "node:http";
import { waitForChat, ask, waitForReplyDone, invoke, shot } from "./_helpers";

const PROXY = process.env.YOAI_PROXY_URL ?? "";
const MODEL = "stand-in-model";
const KEPT = "My oven is a Rofco B40.";

type Sent = { url: string; body: string };

describe("memory and an AI that answers online", () => {
  let server: Server | undefined;
  const chats: Sent[] = [];
  const other: string[] = [];

  before(async () => {
    const m = PROXY.match(/^http:\/\/127\.0\.0\.1:(\d+)$/);
    if (!m) return;
    server = createServer((req, res) => {
      let body = "";
      req.on("data", (c) => (body += c));
      req.on("end", () => {
        const url = req.url ?? "";
        const json = (v: unknown) => {
          res.writeHead(200, { "content-type": "application/json" });
          res.end(JSON.stringify(v));
        };
        if (req.method === "POST" && url.startsWith("/v1/chat/completions")) {
          chats.push({ url, body });
          let wantsStream = false;
          try { wantsStream = !!JSON.parse(body).stream; } catch { /* answered as plain JSON */ }
          const words = "The stand-in service answers.";
          if (!wantsStream) return json({ id: "c1", object: "chat.completion", model: MODEL, choices: [{ index: 0, message: { role: "assistant", content: words }, finish_reason: "stop" }], usage: { prompt_tokens: 1, completion_tokens: 5, total_tokens: 6 } });
          res.writeHead(200, { "content-type": "text/event-stream", "cache-control": "no-cache" });
          const chunk = (delta: unknown, finish: string | null) => res.write(`data: ${JSON.stringify({ id: "c1", object: "chat.completion.chunk", model: MODEL, choices: [{ index: 0, delta, finish_reason: finish }] })}\n\n`);
          chunk({ role: "assistant", content: "" }, null);
          chunk({ content: words }, null);
          chunk({}, "stop");
          res.write("data: [DONE]\n\n");
          return res.end();
        }
        other.push(`${req.method} ${url}`);
        if (url.startsWith("/v1/models")) return json({ object: "list", data: [{ id: MODEL, display_name: "Stand-in", description: "The test's stand-in.", owned_by: "standin", context_window: 128000, category: "chat", categories: ["chat"], vision: false }], health: {} });
        if (url.startsWith("/billing/entitlement")) return json({ tier: "plus", linked: true });
        if (url.startsWith("/auth/vault/refresh")) return json({ access_token: "stand-in-access", refresh_token: "stand-in-refresh", expires_in: 86000 });
        return json({});
      });
    });
    await new Promise<void>((ok) => server!.listen(Number(m[1]), "127.0.0.1", ok));
  });
  after(() => { server?.close(); });

  const lastUserText = (s: Sent) => {
    const messages = (JSON.parse(s.body).messages ?? []) as { role: string; content: unknown }[];
    const text = (c: unknown) => (typeof c === "string" ? c : Array.isArray(c) ? c.map((p: { text?: string }) => p.text ?? "").join("\n") : "");
    return { all: messages.map((x) => text(x.content)).join("\n\n"), shape: messages.map((x) => `${x.role}:${text(x.content).length}`).join(" ") };
  };
  const press = async (sel: string) => {
    const el = await $(sel);
    await el.waitForExist({ timeout: 30_000 });
    await browser.execute((e) => (e as HTMLElement).click(), el);
  };
  const under = (testid: string) => `(//*[@data-testid="chat-message"][@data-role="assistant"])[last()]//*[@data-testid="${testid}"]`;
  const textOf = async (sel: string) => {
    const el = await $(sel);
    await el.waitForExist({ timeout: 60_000 });
    return (await browser.execute((e) => (e as HTMLElement).innerText, el)) as string;
  };

  let aiName = "";

  it("the first message to an online AI is sent with nothing to answer first, with what memory keeps about the person", async function () {
    if (!server) this.skip();
    this.timeout(600_000);
    await waitForChat();
    // A new profile's first reply gives the AI its place in the records, and
    // the update's pass ends: from then on what was kept is what is given.
    await ask("Name one primary color and nothing else.");
    await waitForReplyDone();
    let why = "";
    await browser.waitUntil(
      async () => {
        if (await invoke("memory_update_report").catch(() => null)) return true;
        // The app runs the pass by itself a little after it starts; here it is asked for at once.
        why = String(await invoke("memory_update_run", { learningWasPaused: false }).then(() => "", (e) => e));
        return !why;
      },
      { timeout: 180_000, interval: 5_000, timeoutMsg: "the update's pass never finished" },
    ).catch((e) => {
      throw new Error(`${String(e)}: ${why}`);
    });
    // Something the person keeps about themselves, kept on this computer.
    await invoke("memory_keep_words", { text: KEPT });
    // Signed in, as a Vault sign-in would leave it, and the AI set to answer online.
    aiName = (await browser.execute(async (model: string) => {
      const T = (window as any).__TAURI__;
      const auth = await T.store.load(await T.core.invoke("profile_store_path", { name: "flowsta-auth.json" }));
      await auth.set("access_token", "stand-in-access");
      await auth.set("refresh_token", "stand-in-refresh");
      await auth.set("agent_pub_key", "uhCAkStandInStandInStandInStandInStandInStandInStandIn");
      await auth.set("did", "did:key:stand-in");
      await auth.set("expires_at", Math.floor(Date.now() / 1000) + 86_000);
      await auth.save();
      const store = await T.store.load(await T.core.invoke("profile_store_path", { name: "ai-data.json" }));
      const ais = ((await store.get("custom-ais")) || []) as { name: string; model?: string; memoryOnline?: boolean; memoryOnlineLineSeen?: boolean }[];
      ais[0].model = `online:${model}`;
      delete ais[0].memoryOnline;
      delete ais[0].memoryOnlineLineSeen;
      await store.set("custom-ais", ais);
      await store.save();
      return ais[0].name;
    }, MODEL)) as string;
    await browser.url("tauri://localhost/your-ais/");
    await browser.url("tauri://localhost/chat/");
    await (await $('[data-testid="chat-input"]')).waitForDisplayed({ timeout: 60_000 });

    const before = chats.length;
    await ask("What oven do I have? One short sentence.");
    await waitForReplyDone();
    console.log(`e2e: requests at the stand-in service: ${chats.length - before} chat, others ${JSON.stringify([...new Set(other)])}`);
    expect(chats.length).toBeGreaterThan(before);
    const sent = lastUserText(chats[chats.length - 1]);
    console.log(`e2e: the request as it arrived (role:characters): ${sent.shape}`);
    // What the person keeps about themselves went with the question.
    expect(sent.all).toContain(KEPT);
    // The reply is the service's, and the line under it says what was sent.
    const line = await textOf(under("given-sent"));
    console.log(`e2e: the line under the reply: ${JSON.stringify(line)}`);
    expect(line).toContain("Sent to");
    expect(line).toContain("1 thing you keep about yourself");
    // The first online reply of an AI carries the one-time line.
    const first = await textOf(under("sent-first"));
    console.log(`e2e: the one-time line: ${JSON.stringify(first)}`);
    expect(first.length).toBeGreaterThan(20);
    await shot("memory-online-sent");
  });

  it("with sharing turned off, nothing from memory is in the next request, and the line says so", async function () {
    if (!server) this.skip();
    this.timeout(600_000);
    await press(under("sent-turn-off"));
    await (await $(under("sent-changed"))).waitForExist({ timeout: 30_000 });
    const before = chats.length;
    await ask("And what oven is that again? One short sentence.");
    await waitForReplyDone();
    expect(chats.length).toBeGreaterThan(before);
    const sent = lastUserText(chats[chats.length - 1]);
    console.log(`e2e: with sharing off, the request (role:characters): ${sent.shape}`);
    expect(sent.all).not.toContain(KEPT);
    const line = await textOf(under("given-sent"));
    console.log(`e2e: the line under the reply: ${JSON.stringify(line)}`);
    expect(line).toContain("Memory is not shared with");
    // The one-time line does not come a second time.
    expect(await (await $(under("sent-first"))).isExisting()).toBe(false);
    await shot("memory-online-not-shared");
    // The page that says what is sent shows this AI, online, with sharing off.
    await browser.url("tauri://localhost/memory-sent/");
    const row = await $('[data-testid="sent-ai"][data-online="1"]');
    await row.waitForExist({ timeout: 60_000 });
    console.log(`e2e: on the sent page: ${JSON.stringify(await textOf('[data-testid="sent-ai"][data-online="1"]'))}`);
    expect(await row.getAttribute("data-shared")).toBe("0");
    await shot("memory-online-sent-page");
    await press('[data-testid="sent-ai"][data-online="1"] [data-testid="sent-ai-switch"]');
    await browser.waitUntil(async () => (await (await $('[data-testid="sent-ai"][data-online="1"]')).getAttribute("data-shared")) === "1", { timeout: 30_000, timeoutMsg: "the switch on the sent page did not turn sharing on" });
  });

  it("a message that looks like it holds a key is asked about before anything is sent", async function () {
    if (!server) this.skip();
    this.timeout(600_000);
    await browser.url("tauri://localhost/chat/");
    await (await $('[data-testid="chat-input"]')).waitForDisplayed({ timeout: 60_000 });
    const before = chats.length;
    const KEY = "sk-proj-9fQ2xLmT7vRb3KpZ8wYc1NdE5hUa6JgS4oVi0BtX";
    await ask(`My key is ${KEY} - why would a request with it be refused?`);
    const box = await $('[data-testid="key-ask"]');
    await box.waitForDisplayed({ timeout: 60_000 });
    console.log(`e2e: the ask: ${JSON.stringify(await textOf('[data-testid="key-ask"]'))}`);
    expect(chats.length).toBe(before);
    await shot("memory-online-key-ask");
    // Sent without it: the request arrives, and the key is not in it.
    await press('[data-testid="key-ask-without"]');
    await browser.waitUntil(async () => chats.length > before, { timeout: 120_000, timeoutMsg: "nothing was sent after choosing to send without the key" });
    await waitForReplyDone();
    for (const c of chats.slice(before)) expect(c.body).not.toContain(KEY);
    console.log(`e2e: sent without the key: ${chats.length - before} request(s), none holds it`);
  });

  after(async () => {
    if (!server) return;
    // The AI answers on this computer again for whatever runs next.
    await browser.execute(async () => {
      const T = (window as any).__TAURI__;
      const store = await T.store.load(await T.core.invoke("profile_store_path", { name: "ai-data.json" }));
      const ais = ((await store.get("custom-ais")) || []) as { model?: string }[];
      ais[0].model = "auto:offline";
      await store.set("custom-ais", ais);
      await store.save();
      const auth = await T.store.load(await T.core.invoke("profile_store_path", { name: "flowsta-auth.json" }));
      for (const k of ["access_token", "refresh_token", "agent_pub_key", "did", "expires_at"]) await auth.delete(k);
      await auth.save();
    }).catch(() => undefined);
    void aiName;
  });
});
