// When an online model fails before its first word, the turn goes once to
// another provider's model of the same kind - on an AI that routes
// automatically. An AI pinned to the model is never switched: its error
// line offers the other model, one tap away. Automatic routing uses only
// the models the person allows (the catalog's default leaves some
// providers off); a model the person picks by name is used either way.
// The staging proxy fails one model on purpose (x-yoai-test-fail busy@id).
import { invoke, waitForChat, ask, lastReply, shot, resetAiModels } from "../models/_helpers";
import { cheapPicks, haveIdentity, setAiModel, setRoutingPrefs, signInAsTestIdentity, ONLINE_MODEL } from "./_online";

type Model = { id: string; provider?: string | null; display_name: string };
type Provider = { id: string; name: string; based_in?: string | null; auto_default: boolean };

const reply = () =>
  browser.execute(() => {
    const m = [...document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]')].pop() as HTMLElement | undefined;
    return m
      ? {
          state: m.dataset.state ?? "",
          failed: m.dataset.error === "1",
          served: m.dataset.served ?? "",
          alt: m.querySelector('[data-testid="reply-alt"]')?.textContent?.trim() ?? "",
          text: m.innerText,
        }
      : null;
  });

const counts = () =>
  browser.execute(() => ({
    users: document.querySelectorAll('[data-testid="chat-message"][data-role="user"]').length,
    assistants: document.querySelectorAll('[data-testid="chat-message"][data-role="assistant"]').length,
  }));

async function setAutoAllow(v: { providers?: Record<string, boolean>; models?: Record<string, boolean> } | null) {
  await browser.execute(async (val: unknown) => {
    const store = await (window as any).__TAURI__.store.load("settings.json");
    if (val === null) await store.delete("routingAutoAllow");
    else await store.set("routingAutoAllow", val);
    await store.save();
  }, v as unknown);
}

async function newChat() {
  await browser.url("tauri://localhost/chat/");
  await waitForChat();
}

describe("fallback to another provider, and the models automatic routing may use", () => {
  let key = "";
  let models: Model[] = [];
  let providers: Provider[] = [];
  const providerOf = (id: string) => models.find((m) => m.id === id)?.provider ?? null;

  before(async function () {
    this.timeout(300_000);
    if (!haveIdentity()) return;
    await waitForChat();
    const session = await signInAsTestIdentity();
    expect(session.tier).toBe("pro");
    key = (await browser.execute(async () => {
      const T = (window as any).__TAURI__;
      const store = await T.store.load(await T.core.invoke("profile_store_path", { name: "ai-data.json" }));
      return (((await store.get("custom-ais")) || []) as { agentPubKey?: string }[]).find((a) => a.agentPubKey)?.agentPubKey ?? "";
    })) as string;
    models = await invoke<Model[]>("list_online_models");
    providers = await invoke<Provider[]>("list_online_providers");
    console.log(`e2e: ${models.length} online models, providers ${providers.map((p) => `${p.id}${p.auto_default ? "" : " (off)"}`).join(", ")}`);
  });
  after(async () => {
    if (!haveIdentity()) return;
    await invoke("e2e_set_online_failure", { mode: null }).catch(() => {});
    await setAutoAllow(null);
    await setRoutingPrefs({ routingOnlineShare: null });
    await resetAiModels();
  });

  it("leaves the providers that are off by default out of automatic picks, and an explicit pick still wins", async function () {
    if (!haveIdentity()) this.skip();
    this.timeout(120_000);
    expect(providers.length).toBeGreaterThan(0);
    const off = providers.filter((p) => !p.auto_default).map((p) => p.id);
    console.log(`e2e: off by default: ${off.join(", ")}`);
    await setAutoAllow(null);
    const defaults = await invoke<Record<string, string>>("routing_defaults");
    console.log(`e2e: routing defaults ${JSON.stringify(defaults)}`);
    for (const [slot, id] of Object.entries(defaults)) {
      expect({ slot, provider: providerOf(id), off: off.includes(providerOf(id) ?? "") }).toEqual({ slot, provider: providerOf(id), off: false });
    }
    // The person turns a provider off: its models leave the automatic picks.
    const everydayProvider = providerOf(defaults.everyday);
    await setAutoAllow({ providers: { [everydayProvider ?? ""]: false } });
    const without = await invoke<Record<string, string>>("routing_defaults");
    console.log(`e2e: with ${everydayProvider} off, everyday -> ${without.everyday}`);
    expect(providerOf(without.everyday)).not.toBe(everydayProvider);
    // ...but a model of that provider picked by name is still used.
    const r = await invoke<{ model: string; reason: string }>("route_model", {
      mode: "online-offline",
      query: "write a two-line poem about tea",
      task: "general",
      difficulty: "easy",
      onlineShare: "frontier",
      onlineEveryday: defaults.everyday,
    });
    console.log(`e2e: explicit pick of ${defaults.everyday} -> ${r.model} (${r.reason})`);
    expect(r.model).toBe(defaults.everyday);
    await setAutoAllow(null);
  });

  it("shows the list in Settings and the switch and Use for on each Online Models card", async function () {
    if (!haveIdentity()) this.skip();
    this.timeout(180_000);
    await browser.url("tauri://localhost/settings/");
    await (await $('[data-testid="auto-routing-models"]')).waitForDisplayed({ timeout: 60_000 });
    const rows = (await browser.execute(() =>
      [...document.querySelectorAll('[data-testid="auto-routing-models"] [data-provider]')].map((r) => ({
        id: (r as HTMLElement).dataset.provider,
        on: r.querySelector('[data-testid="auto-provider-switch"]')?.getAttribute("aria-checked"),
      })),
    )) as { id: string; on: string }[];
    console.log(`e2e: settings rows ${JSON.stringify(rows)}`);
    for (const p of providers) expect(rows.find((r) => r.id === p.id)?.on).toBe(String(p.auto_default));
    await browser.execute(() => {
      const list = document.querySelector('[data-testid="auto-routing-models"]') as HTMLElement;
      (list.querySelector("[aria-expanded]") as HTMLElement | null)?.click();
      list.scrollIntoView({ block: "center", behavior: "instant" as ScrollBehavior });
    });
    await browser.pause(600);
    await shot("routing-list-settings");
    await browser.url("tauri://localhost/online-models/");
    await browser.waitUntil(async () => browser.execute(() => document.querySelectorAll('[data-testid="model-auto-switch"]').length > 0), { timeout: 60_000, timeoutMsg: "no Auto switch on the cards" });
    await browser.pause(1_000);
    const badges = (await browser.execute(() => [...document.querySelectorAll('[data-testid="model-use-badge"]')].map((b) => b.textContent))) as string[];
    console.log(`e2e: use badges ${JSON.stringify(badges)}`);
    expect(badges.length).toBeGreaterThan(0);
    await browser.execute(() => {
      const b = document.querySelector('[data-testid="use-for"]') as HTMLElement | null;
      b?.scrollIntoView({ block: "center", behavior: "instant" as ScrollBehavior });
      b?.click();
    });
    await browser.pause(500);
    await shot("routing-online-models-use-for");
    await browser.keys("Escape");
  });

  it("answers from another provider when the online model is busy, and the reply says why", async function () {
    if (!haveIdentity()) this.skip();
    this.timeout(600_000);
    await setAiModel(key, "auto:online-offline");
    await setRoutingPrefs({ ...cheapPicks(), routingOnlineShare: "frontier" });
    await invoke("e2e_set_online_failure", { mode: `busy@${ONLINE_MODEL}` });
    await newChat();
    await ask("In one sentence: why do cats purr?");
    await browser.waitUntil(async () => {
      const r = await reply();
      return !!r && r.state === "done" && (r.failed || r.served.startsWith("online:"));
    }, { timeout: 240_000, interval: 1000, timeoutMsg: "the busy turn never finished" });
    const r = (await reply())!;
    console.log(`e2e: busy ${ONLINE_MODEL} -> served by ${r.served}: ${JSON.stringify(r.text.slice(0, 160))}`);
    await shot("fallback-auto");
    expect(r.failed).toBe(false);
    expect(r.served).not.toBe(`online:${ONLINE_MODEL}`);
    expect(r.served.startsWith("online:")).toBe(true);
    expect(providerOf(r.served)).not.toBe(providerOf(`online:${ONLINE_MODEL}`));
    expect(await counts()).toEqual({ users: 1, assistants: 1 });
    // The log is newest first.
    const last = (await invoke<{ model: string; reason: string }[]>("recent_routing_decisions"))[0];
    console.log(`e2e: decision log ${JSON.stringify(last)}`);
    expect(last.model).toBe(r.served);
    expect(last.reason).toContain("was busy, so");
  });

  it("never switches a pinned model, and offers the other one beside Retry", async function () {
    if (!haveIdentity()) this.skip();
    this.timeout(600_000);
    await setAiModel(key, `online:${ONLINE_MODEL}`);
    await invoke("e2e_set_online_failure", { mode: `busy@${ONLINE_MODEL}` });
    await newChat();
    await ask("In one sentence: why is the sea salty?");
    await browser.waitUntil(async () => (await reply())?.failed === true, { timeout: 120_000, timeoutMsg: "the pinned model's failure never showed" });
    const r = (await reply())!;
    console.log(`e2e: pinned error, offer ${JSON.stringify(r.alt)}`);
    await shot("fallback-pinned-offer");
    expect(r.alt).toMatch(/^Ask .+ instead$/);
    await (await $('[data-testid="reply-alt"]')).click();
    await browser.waitUntil(async () => {
      const x = await reply();
      return !!x && x.state === "done" && !x.failed && x.served.startsWith("online:");
    }, { timeout: 240_000, interval: 1000, timeoutMsg: "the offered model never answered" });
    const answered = (await reply())!;
    console.log(`e2e: offered model ${answered.served}: ${JSON.stringify(answered.text.slice(0, 160))}`);
    expect(answered.served).not.toBe(`online:${ONLINE_MODEL}`);
    expect(await counts()).toEqual({ users: 1, assistants: 1 });
    // The AI stays pinned.
    const model = await browser.execute(async (k: string) => {
      const T = (window as any).__TAURI__;
      const store = await T.store.load(await T.core.invoke("profile_store_path", { name: "ai-data.json" }));
      return (((await store.get("custom-ais")) || []) as { agentPubKey?: string; model?: string }[]).find((a) => a.agentPubKey === k)?.model ?? "";
    }, key);
    expect(model).toBe(`online:${ONLINE_MODEL}`);
  });
});
