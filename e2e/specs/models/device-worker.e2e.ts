// A Build helper's model call marked for this device (":agent-device") is
// answered on the LOADED model only when that model is fit for agent work and
// the device has room; otherwise it takes the leader's route. Whether a model
// spawns helpers is its own choice, so this spec makes the call itself, the
// way the harness would, and reads the app's decision from its route events
// and its log. No Build install needed.
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { waitForChat, invoke } from "./_helpers";

type Route = { role?: string; where?: string; model?: string };

describe("a helper's call on this device", () => {
  it("is answered on the loaded model when it fits, and the device queue holds its cap", async function () {
    this.timeout(600_000);
    await waitForChat();
    await browser.setTimeout({ script: 180_000 });
    const loaded = await invoke<string | null>("get_current_model");
    const port = await invoke<number>("inference_port");
    const ai = await browser.execute(async () => {
      const t = (window as any).__TAURI__;
      const path = await t.core.invoke("profile_store_path", { name: "ai-data.json" });
      const store = await t.store.load(path);
      const ais = ((await store.get("custom-ais")) || []) as { name: string }[];
      return ais[0]?.name ?? null;
    });
    console.log(`e2e: loaded ${loaded}, port ${port}, AI ${ai}`);
    expect(ai).toBeTruthy();
    await browser.execute(async () => {
      const w = window as any;
      w.__yoaiRoutes = [];
      await w.__TAURI__.event.listen("agent-route", (e: any) => w.__yoaiRoutes.push(e.payload));
    });
    const routes = async () => (await browser.execute(() => (window as any).__yoaiRoutes ?? [])) as Route[];
    // One call, as a helper would make it: the device-worker model name on
    // the app's own OpenAI endpoint. From the test's own process - the
    // webview's origin may not fetch the local server.
    const call = async (n: number) => {
      const r = await fetch(`http://127.0.0.1:${port}/v1/chat/completions`, {
        method: "POST",
        headers: { "Content-Type": "application/json", Authorization: "Bearer local" },
        body: JSON.stringify({ model: `${ai}:agent-device`, messages: [{ role: "user", content: `Reply with the number ${n} and nothing else.` }], max_tokens: 8, stream: false }),
      });
      const j = (await r.json().catch(() => null)) as any;
      return { status: r.status, text: j?.choices?.[0]?.message?.content ?? null, error: j?.error ?? null };
    };
    const one = await call(1);
    console.log(`e2e: one call ${JSON.stringify(one)}`);
    expect(one.status).toBe(200);
    let seen = await routes();
    console.log(`e2e: routes after one ${JSON.stringify(seen)}`);
    const helpers = seen.filter((r) => r.role === "device-worker");
    expect(helpers.length).toBe(1);
    expect(["this computer", "the leader's route", "this computer (the AI is offline only)"]).toContain(helpers[0].where ?? "");
    if (helpers[0].where === "this computer") expect(helpers[0].model).toBe(loaded);
    // Two at once: on a small card the cap is one, so the second decision
    // sees the first's seat and either waits as an offline-only AI's worker
    // or takes the leader's route - never a third place.
    const pair = (await Promise.all([call(2), call(3)])).map((r) => r.status);
    console.log(`e2e: pair ${JSON.stringify(pair)}`);
    expect(pair).toEqual([200, 200]);
    seen = await routes();
    const pairRoutes = seen.filter((r) => r.role === "device-worker").slice(1);
    console.log(`e2e: pair routes ${JSON.stringify(pairRoutes)}`);
    expect(pairRoutes.length).toBe(2);
    for (const r of pairRoutes) expect(["this computer", "the leader's route", "this computer (the AI is offline only)"]).toContain(r.where ?? "");
    // The app's own account of each decision.
    const appLog = resolve(import.meta.dirname ?? ".", "../../profile/.local/share/com.solar.yourowai/logs/Your Own AI.log");
    const lines = readFileSync(appLog, "utf8").split("\n").filter((l) => l.includes("device worker for"));
    console.log(`e2e: decisions\n${lines.slice(-3).map((l) => l.replace(/^.*\[inference\] /, "  ")).join("\n")}`);
    expect(lines.length).toBeGreaterThanOrEqual(3);
  });
});
