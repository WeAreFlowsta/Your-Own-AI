// A tool call a program requires, through the app's own OpenAI endpoint, on
// every chat model in the models folder: the caller names one tool with
// `tool_choice` and must get a call to it back. The engine's own forcing
// does not hold for every model's chat format (LFM2.5-8B answered in words
// 2 of 3 times on 11-09), so the app sees it through: the engine's call,
// else the call read out of the answer's text, else one firmer try
// (inference_server.rs `forced_tool_call`). Ten asks per model; the count
// and what the app did for each are printed. Long (each model is loaded):
// YOAI_E2E_LONG.
import { waitForChat, invoke, invokeLong } from "./_helpers";

const ASKS = 10;
const TOOL = {
  type: "function",
  function: {
    name: "emit",
    description: "Record a status line.",
    parameters: { type: "object", properties: { status: { type: "string" } }, required: ["status"] },
  },
};

describe("a tool call a program requires", () => {
  it("comes back as a call to the named tool, on every chat model here", async function () {
    if (!process.env.YOAI_E2E_LONG) this.skip();
    this.timeout(3_600_000);
    await waitForChat();
    await browser.setTimeout({ script: 180_000 });
    const port = await invoke<number>("inference_port");
    const models = (await invoke<{ name: string }[]>("list_local_models"))
      .map((m) => m.name)
      .filter((n) => !/embed|bge-|mmproj/i.test(n));
    expect(models.length).toBeGreaterThan(0);
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
    const short: string[] = [];
    for (const model of models) {
      const ai = await setAiModel(model);
      await invokeLong("load_model", { filename: model, withVision: false, reason: "e2e required tool call" }, 600_000);
      let called = 0;
      const other: string[] = [];
      for (let i = 0; i < ASKS; i++) {
        const r = await fetch(`http://127.0.0.1:${port}/v1/chat/completions`, {
          method: "POST",
          headers: { "Content-Type": "application/json", Authorization: "Bearer local" },
          body: JSON.stringify({
            model: ai,
            messages: [{ role: "user", content: `Record that build number ${100 + i} passed.` }],
            tools: [TOOL],
            tool_choice: { type: "function", function: { name: "emit" } },
            max_tokens: 512,
            stream: false,
          }),
        });
        const j = (await r.json().catch(() => null)) as any;
        const call = j?.choices?.[0]?.message?.tool_calls?.[0]?.function;
        let args: any = null;
        try { args = typeof call?.arguments === "string" ? JSON.parse(call.arguments) : call?.arguments; } catch { /* not JSON */ }
        if (r.status === 200 && call?.name === "emit" && typeof args?.status === "string") called++;
        else other.push(`${r.status}: ${JSON.stringify(j?.choices?.[0]?.message ?? j?.error ?? null).slice(0, 140)}`);
      }
      console.log(`e2e: required call | ${model}: ${called} of ${ASKS}${other.length ? ` | not a call: ${other.join(" || ")}` : ""}`);
      if (called < ASKS) short.push(`${model}: ${called} of ${ASKS}`);
    }
    await setAiModel("auto:offline");
    expect(short).toEqual([]);
  });
});
