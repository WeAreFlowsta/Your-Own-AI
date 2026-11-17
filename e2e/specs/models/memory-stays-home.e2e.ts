// Memory work stays on this computer whatever the AI's own model. A call to
// the app's OpenAI endpoint marked `X-Your-Own-AI-Task: memory-prose` (how
// the app's own memory writer marks a session digest) is answered by a
// local model even when the AI is set to an online model, and never reaches
// the online service. The spec stands in for that service: it listens on
// the address the app was launched to use (YOAI_PROXY_URL) and counts what
// arrives. Run with YOAI_PROXY_URL=http://127.0.0.1:18475.
import { createServer, type Server } from "node:http";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { waitForChat, invoke } from "./_helpers";

const PROXY = process.env.YOAI_PROXY_URL ?? "";
const LOG = resolve(import.meta.dirname ?? ".", "../../profile/.local/share/com.solar.yourowai/logs/Your Own AI.log");
const logText = () => { try { return readFileSync(LOG, "utf8").replace(/\x1b\[[0-9;]*m/g, ""); } catch { return ""; } };

describe("memory work for an AI set to an online model", () => {
  let server: Server | undefined;
  const hits: string[] = [];
  before(async () => {
    const m = PROXY.match(/^http:\/\/127\.0\.0\.1:(\d+)$/);
    if (!m) return;
    server = createServer((req, res) => {
      hits.push(`${req.method} ${req.url}`);
      req.resume();
      res.writeHead(503, { "content-type": "application/json" });
      res.end(JSON.stringify({ error: "the test's stand-in for the online service" }));
    });
    await new Promise<void>((ok) => server!.listen(Number(m[1]), "127.0.0.1", ok));
  });
  after(() => { server?.close(); });

  it("is answered on this computer and never reaches the online service", async function () {
    if (!server) this.skip();
    this.timeout(600_000);
    await waitForChat();
    await browser.setTimeout({ script: 180_000 });
    const loaded = await invoke<string | null>("get_current_model");
    const port = await invoke<number>("inference_port");
    const setAiModel = (model: string) =>
      browser.execute(async (m: string) => {
        const t = (window as any).__TAURI__;
        const path = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
        const store = await t.store.load(path);
        const ais = ((await store.get("custom-ais")) || []) as { name: string; model?: string }[];
        ais[0].model = m;
        await store.set("custom-ais", ais);
        await store.save();
        return ais[0].name;
      }, model);
    const call = async (ai: string, headers: Record<string, string>) => {
      const r = await fetch(`http://127.0.0.1:${port}/v1/chat/completions`, {
        method: "POST",
        headers: { "Content-Type": "application/json", Authorization: "Bearer local", ...headers },
        body: JSON.stringify({ model: ai, messages: [{ role: "user", content: "In one short sentence: the session moved the release notes into the changelog." }], max_tokens: 48, stream: false }),
      });
      const j = (await r.json().catch(() => null)) as any;
      return { status: r.status, text: String(j?.choices?.[0]?.message?.content ?? ""), error: j?.error ?? null };
    };
    const MEMORY = { "X-Your-Own-AI-Task": "memory-prose", "X-Your-Own-AI-Online-Share": "local" };

    // The AI answers with an online model...
    const ai = await setAiModel("online:test/stand-in-model");
    console.log(`e2e: loaded ${loaded}, port ${port}, AI ${ai} set to an online model; the stand-in service is at ${PROXY}`);

    // ...and its memory work does not: a local model answers it.
    const before = hits.length;
    const memory = await call(ai, MEMORY);
    console.log(`e2e: memory call ${JSON.stringify({ status: memory.status, text: memory.text.slice(0, 80), error: memory.error })}; requests at the stand-in service: ${hits.length - before}`);
    expect(memory.status).toBe(200);
    expect(memory.text.trim().length).toBeGreaterThan(0);
    expect(hits.length - before).toBe(0);
    expect(logText()).toContain(`memory work for ${ai} stays on this computer`);

    // The control: the same call without the mark is the AI's own turn, and
    // goes where its model is. It is not answered here; whether it reaches
    // the service depends on being signed in, so only "not local" is held.
    const ordinary = await call(ai, {});
    console.log(`e2e: the same call unmarked ${JSON.stringify({ status: ordinary.status, text: ordinary.text.slice(0, 80), error: ordinary.error })}; requests at the stand-in service since: ${JSON.stringify(hits.slice(before))}`);
    expect(ordinary.status !== 200 || hits.length > before).toBe(true);

    // An offline AI's memory work is local as well, with nothing sent out.
    const at = hits.length;
    await setAiModel("auto:offline");
    const offline = await call(ai, MEMORY);
    console.log(`e2e: an offline AI's memory call ${JSON.stringify({ status: offline.status, text: offline.text.slice(0, 80) })}`);
    expect(offline.status).toBe(200);
    expect(hits.length).toBe(at);
  });
});
