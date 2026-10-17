// Which model each kind of project helper runs on. The harness's own
// helpers: `plan` ("software architect for planning implementation
// strategies", read-only) takes the Planning model; `general-purpose` - the
// helper the agent hands every file change, build and test to - inherits the
// agent's own model; `explore` goes to this device when a capable model
// runs here. Before 10-17 general-purpose took the Planning model and the
// planner the agent's: the wrong way round.
//
// Two checks, no model reply needed: the harness config the app writes when
// a project opens, and the router's pick for an agent call and a plan call
// with a different model chosen for each.
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import { resolve } from "node:path";
import { invoke, waitForChat, resetAiModels } from "../models/_helpers";
import { PROFILE, haveIdentity, setAiModel, setRoutingPrefs, signInAsTestIdentity } from "./_online";

const HELPER = resolve(homedir(), ".local/share/com.solar.yourowai/build/your-own-ai-build");
const FOLDER = resolve(PROFILE, `helpers-project-${Date.now().toString(36)}`);
const AGENT_MODEL = "online:gpt-6-luna";
const PLAN_MODEL = "online:gpt-6.1-sol";

/** One TOML table's lines, as written (the app writes it, so it is simple). */
function table(toml: string, header: string): string[] {
  const at = toml.indexOf(header);
  if (at < 0) return [];
  const rest = toml.slice(at + header.length);
  const end = rest.search(/\n\[/);
  return (end < 0 ? rest : rest.slice(0, end)).split("\n").map((l) => l.trim()).filter(Boolean);
}

describe("project helpers run on the right model", () => {
  let ready = false;
  let key = "";
  let slug = "";

  before(async function () {
    this.timeout(300_000);
    if (!haveIdentity() || !existsSync(HELPER)) {
      console.log(`e2e: skipping - identity ${haveIdentity()}, Build helper ${existsSync(HELPER)}`);
      return;
    }
    await waitForChat();
    const session = await signInAsTestIdentity();
    expect(session.tier).toBe("pro");
    const ai = (await browser.execute(async () => {
      const T = (window as any).__TAURI__;
      const store = await T.store.load(await T.core.invoke("profile_store_path", { name: "ai-data.json" }));
      const a = (((await store.get("custom-ais")) || []) as { agentPubKey?: string; name?: string }[]).find((x) => x.agentPubKey);
      return { key: a?.agentPubKey ?? "", name: a?.name ?? "" };
    })) as { key: string; name: string };
    key = ai.key;
    slug = ai.name;
    await setAiModel(key, "auto:online-offline");
    await setRoutingPrefs({ routingOnlineAgent: AGENT_MODEL, routingOnlinePlanning: PLAN_MODEL, routingOnlineShare: "frontier", routingProjectThrifty: "0" });
    mkdirSync(FOLDER, { recursive: true });
    writeFileSync(resolve(FOLDER, "README.md"), "# Helpers test\n");
    await browser.execute((p: string) => localStorage.setItem("build-binary-path", p), HELPER);
    await browser.url("tauri://localhost/chat/");
    await (await $('[data-testid="chat-input"]')).waitForDisplayed({ timeout: 60_000 });
    ready = true;
  });

  after(async () => {
    if (!ready) return;
    await browser.execute(async () => { await (window as any).__yoaiCloseFolder?.(); }).catch(() => {});
    await invoke("stop_build_agent").catch(() => {});
    await browser.execute(() => localStorage.removeItem("build-binary-path")).catch(() => {});
    await setRoutingPrefs({ routingOnlineAgent: null, routingOnlinePlanning: null, routingOnlineShare: null, routingProjectThrifty: null });
    await resetAiModels();
  });

  it("writes the helper map: the planner on Planning, the working helpers on the agent's model", async function () {
    if (!ready) this.skip();
    this.timeout(300_000);
    await browser.waitUntil(async () => browser.execute(() => typeof (window as any).__yoaiOpenFolder === "function"), { timeout: 30_000 });
    await browser.execute((p: string) => (window as any).__yoaiOpenFolder(p), FOLDER);
    await browser.waitUntil(async () => {
      const s = await invoke<{ running: boolean; sessionId: string | null }>("build_agent_status");
      return s.running && !!s.sessionId;
    }, { timeout: 120_000, interval: 1_000, timeoutMsg: "the agent session never came up" });
    const toml = readFileSync(resolve(PROFILE, ".your-own-ai-build", "config.toml"), "utf8");
    const helpers = table(toml, "[subagents.models]");
    const planning = table(toml, "[model.planning]");
    console.log(`e2e: [subagents.models] ${JSON.stringify(helpers)}`);
    console.log(`e2e: [model.planning] ${JSON.stringify(planning)}`);
    expect(helpers).toContain('"plan" = "planning"');
    expect(helpers.some((l) => l.startsWith('"general-purpose"'))).toBe(false);
    // The model entry names the AI's slug (lower case) with ":plan".
    expect(planning).toContain(`model = "${slug.toLowerCase()}:plan"`);
  });

  it("routes a plan call to the Planning model and an agent call to the Agent model", async function () {
    if (!ready) this.skip();
    this.timeout(120_000);
    const port = await invoke<number>("inference_port");
    const pick = async (plan: boolean) => {
      const r = await fetch(`http://127.0.0.1:${port}/internal/route-preview?mode=online-offline&agent=1&plan=${plan ? 1 : 0}&q=work`, {
        headers: { Authorization: "Bearer local" },
      });
      return (await r.json()) as { model?: string; reason?: string; error?: string };
    };
    const planCall = await pick(true);
    const agentCall = await pick(false);
    console.log(`e2e: plan call -> ${JSON.stringify(planCall)}; agent call -> ${JSON.stringify(agentCall)}`);
    expect(planCall.model).toBe(PLAN_MODEL);
    expect(agentCall.model).toBe(AGENT_MODEL);
  });
});
