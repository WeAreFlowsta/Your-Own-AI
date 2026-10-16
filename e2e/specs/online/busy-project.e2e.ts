// A busy online agent in a project on a big AI - the 10-16 evening session,
// reproduced: an online agent took a step every few seconds (about 20 tool
// calls and a helper agent in four and a half minutes) on an AI with 1,071
// conversations, and the app flooded its own records with list reads while
// a project note took five minutes to save.
//
// Runs with `npm run e2e:online`: the staging proxy, signed in as the e2e
// test identity (~/.config/yoai-e2e/staging-identity.json, a test plan in
// staging's subscriptions_test - never a person's plan), on GPT-6 Luna, the
// cheapest tool-capable model (a run costs a few cents). Needs the Build
// helper on this machine. Timings go to e2e/shots/online-timings.json.
import { createPrivateKey, sign } from "node:crypto";
import { homedir } from "node:os";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { resolve } from "node:path";
import { SHOTS } from "../../wdio.conf";
import { shot, invoke, waitForChat, ask, lastReply, resetAiModels, seedConversations } from "../models/_helpers";

const RUN = Date.now().toString(36);
const PROXY = process.env.YOAI_PROXY_URL ?? "https://yoai-model-proxy-staging-386500392150.us-central1.run.app";
const IDENTITY = process.env.YOAI_E2E_STAGING_IDENTITY ?? resolve(homedir(), ".config/yoai-e2e/staging-identity.json");
const HELPER = resolve(homedir(), ".local/share/com.solar.yourowai/build/your-own-ai-build");
const MODEL = process.env.YOAI_E2E_ONLINE_MODEL ?? "gpt-6-luna";
const BIG = Number(process.env.YOAI_E2E_BIG_N ?? 1000);
const PROFILE = resolve(import.meta.dirname ?? ".", "../../profile");
const FOLDER = resolve(PROFILE, `online-project-${RUN}`);
const LOG = resolve(PROFILE, ".local/share/com.solar.yourowai/logs/Your Own AI.log");

const timings: Record<string, unknown> = {};
const save = () => writeFileSync(resolve(SHOTS, "online-timings.json"), JSON.stringify({ run: RUN, model: MODEL, big: BIG, at: new Date().toISOString(), timings }, null, 2));
async function timed<T>(name: string, budget: number, step: () => Promise<T>): Promise<T> {
  const t0 = Date.now();
  const out = await step();
  const ms = Date.now() - t0;
  timings[name] = { ms, budget };
  console.log(`e2e: ${name} ${ms} ms (budget ${budget})`);
  expect(ms).toBeLessThan(budget);
  return out;
}

/** The test identity signs the proxy's challenge, as the Vault would. */
async function stagingTokens() {
  const id = JSON.parse(readFileSync(IDENTITY, "utf8")) as { agent_pub_key: string; did: string; seed_b64: string };
  const der = Buffer.concat([Buffer.from("302e020100300506032b657004220420", "hex"), Buffer.from(id.seed_b64, "base64")]);
  const key = createPrivateKey({ key: der, format: "der", type: "pkcs8" });
  const post = (path: string, body: unknown) =>
    fetch(`${PROXY}${path}`, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify(body) }).then((r) => r.json());
  const { challenge } = await post("/auth/vault/challenge", {});
  const signature = sign(null, Buffer.from(challenge, "base64"), key).toString("base64");
  const tokens = await post("/auth/vault/token", { challenge, agent_pub_key: id.agent_pub_key, did: id.did, signature });
  if (!tokens.access_token) throw new Error(`staging sign-in failed: ${JSON.stringify(tokens).slice(0, 200)}`);
  return { ...tokens, agent_pub_key: id.agent_pub_key, did: id.did };
}

type Writer = { key: string; name: string };
const writers = () =>
  browser.execute(async () => {
    const t = (window as any).__TAURI__;
    const store = await t.store.load(await t.core.invoke("profile_store_path", { name: "ai-data.json" }));
    const ais = ((await store.get("custom-ais")) || []) as { agentPubKey?: string; name: string }[];
    return ais.filter((a) => a.agentPubKey).map((a) => ({ key: a.agentPubKey!, name: a.name }));
  }) as Promise<Writer[]>;

/** Live list reads in the app log between two instants (the log is UTC). */
function listReads(fromIso: string, toIso: string) {
  const re = /^\[(\d{4}-\d\d-\d\d)\]\[(\d\d:\d\d:\d\d)\].*\[records\] conversation list (\w+) live for ([^:]+): (\d+) conversation\(s\) in (\d+) ms/;
  const pages = /^\[(\d{4}-\d\d-\d\d)\]\[(\d\d:\d\d:\d\d)\].*get_conversations_page .*?(\d+) ms$/;
  const rows: { at: string; ai: string; caller: string; n: number; ms: number }[] = [];
  let pageCount = 0;
  let pageMs = 0;
  for (const line of readFileSync(LOG, "utf8").split("\n")) {
    const m = re.exec(line);
    const at = m ? `${m[1]}T${m[2]}` : "";
    if (m && at >= fromIso.slice(0, 19) && at <= toIso.slice(0, 19)) rows.push({ at, ai: m[3], caller: m[4].trim(), n: Number(m[5]), ms: Number(m[6]) });
    const p = pages.exec(line);
    const pat = p ? `${p[1]}T${p[2]}` : "";
    if (p && pat >= fromIso.slice(0, 19) && pat <= toIso.slice(0, 19)) {
      pageCount++;
      pageMs += Number(p[3]);
    }
  }
  return { rows, pageCount, pageMs };
}

describe(`online: a busy ${MODEL} agent in a project on an AI with ${BIG} conversations`, () => {
  let w: Writer;
  let ready = false;

  before(async function () {
    this.timeout(1_800_000);
    if (!existsSync(IDENTITY)) {
      console.log(`e2e: no staging test identity at ${IDENTITY} - skipping`);
      return;
    }
    await waitForChat();
    if (!(await invoke<boolean>("path_is_file", { path: HELPER }))) {
      console.log(`e2e: no Build helper at ${HELPER} - skipping`);
      return;
    }
    // Signed in as the test identity: the tokens the Vault sign-in would store.
    const tokens = await stagingTokens();
    await browser.execute(async (t: Record<string, unknown>) => {
      const T = (window as any).__TAURI__;
      const store = await T.store.load(await T.core.invoke("profile_store_path", { name: "flowsta-auth.json" }));
      await store.set("access_token", t.access_token);
      await store.set("refresh_token", t.refresh_token);
      await store.set("agent_pub_key", t.agent_pub_key);
      await store.set("did", t.did);
      await store.set("expires_at", Math.floor(Date.now() / 1000) + 86_000);
      await store.save();
    }, tokens);
    const session = await invoke<{ signed_in: boolean; tier: string | null }>("flowsta_session");
    console.log(`e2e: session ${JSON.stringify(session)}`);
    expect(session.signed_in).toBe(true);
    expect(session.tier).toBe("pro");

    // The project: a few files to read, a test command to remember.
    mkdirSync(resolve(FOLDER, "src"), { recursive: true });
    writeFileSync(resolve(FOLDER, "README.md"), "# Shop API\n\nA small HTTP API for an online shop.\n");
    writeFileSync(resolve(FOLDER, "package.json"), JSON.stringify({ name: "shop-api", scripts: { [`check-${RUN}`]: "tsc --noEmit && vitest run" } }, null, 2));
    const files: Record<string, string> = {
      "auth.ts": "export function signIn(user: string, password: string) { return user.length > 0 && password.length > 7; }\n",
      "db.ts": "export const db = new Map<string, unknown>();\nexport function save(id: string, value: unknown) { db.set(id, value); }\n",
      "cart.ts": "export function total(prices: number[]) { return prices.reduce((a, b) => a + b, 0); }\n",
      "orders.ts": "import { save } from './db';\nexport function placeOrder(id: string, items: string[]) { save(id, { items, at: Date.now() }); }\n",
      "server.ts": "import { signIn } from './auth';\nexport function handle(path: string) { return path === '/login' ? signIn('a', 'password1') : null; }\n",
    };
    for (const [name, body] of Object.entries(files)) writeFileSync(resolve(FOLDER, "src", name), body);

    w = (await writers())[0];
    // The helper model is not needed for an online agent; the run's 8 GB cap
    // has no room for it beside the chat model (which device workers use).
    await invoke("stop_utility_server").catch(() => {});
    const t0 = Date.now();
    const made = await seedConversations(w.key, w.name, BIG);
    timings[`seed: ${BIG} conversations`] = { ms: Date.now() - t0 };
    console.log(`e2e: seeded ${made} conversations on ${w.name} in ${Date.now() - t0} ms`);

    // The AI works on the online model.
    await browser.execute((p: string) => localStorage.setItem("build-binary-path", p), HELPER);
    await browser.execute(async (model: string, key: string) => {
      const T = (window as any).__TAURI__;
      const store = await T.store.load(await T.core.invoke("profile_store_path", { name: "ai-data.json" }));
      const ais = ((await store.get("custom-ais")) || []) as { agentPubKey?: string; model?: string }[];
      for (const a of ais) if (a.agentPubKey === key) a.model = `online:${model}`;
      await store.set("custom-ais", ais);
      await store.save();
    }, MODEL, w.key);
    await browser.url("tauri://localhost/chat/");
    await (await $('[data-testid="chat-input"]')).waitForDisplayed({ timeout: 60_000 });
    ready = true;
  });

  after(async () => {
    await browser.execute(() => localStorage.removeItem("build-binary-path")).catch(() => {});
    await browser.execute(async () => { await (window as any).__yoaiCloseFolder?.(); }).catch(() => {});
    await invoke("stop_build_agent").catch(() => {});
    await resetAiModels();
    save();
  });

  it("works through a busy turn without flooding the records, and memory stays quick meanwhile", async function () {
    if (!ready) this.skip();
    this.timeout(1_800_000);
    await browser.waitUntil(async () => browser.execute(() => typeof (window as any).__yoaiOpenFolder === "function"), { timeout: 30_000 });
    await timed("open project: session up", 120_000, async () => {
      await browser.execute((p: string) => (window as any).__yoaiOpenFolder(p), FOLDER);
      await browser.waitUntil(async () => {
        const s = await invoke<{ running: boolean; sessionId: string | null }>("build_agent_status");
        return s.running && !!s.sessionId;
      }, { timeout: 120_000, interval: 1_000, timeoutMsg: "the agent session never came up" });
    });

    const turnFrom = new Date().toISOString();
    await ask(
      "Use a helper agent to read every file in src/ and report one line per file on what it does, while you read README.md and package.json yourself. " +
      "Then save the project's check command (from package.json) with your remember_for_project tool, and reply with the per-file list.",
    );
    // While the agent works: the person's own note and a memory read, every
    // ten seconds, timed; any permission card is allowed (a scratch folder).
    const noteMs: number[] = [];
    const readMs: number[] = [];
    let allowed = 0;
    let n = 0;
    let lastCheck = 0;
    await browser.waitUntil(async () => {
      const allow = await $('[data-testid="permission-allow"]');
      if (await allow.isExisting().catch(() => false)) {
        await allow.click().catch(() => {});
        allowed++;
      }
      if (Date.now() - lastCheck >= 10_000) {
        lastCheck = Date.now();
        n++;
        let t = Date.now();
        await invoke("project_memory_note", { writerKey: w.key, writerLabel: w.name, folder: FOLDER, note: `Note ${n} saved while the agent worked.` });
        noteMs.push(Date.now() - t);
        t = Date.now();
        await invoke("project_memory_get", { folder: FOLDER, fresh: false });
        readMs.push(Date.now() - t);
      }
      return (await lastReply())?.state === "done";
    }, { timeout: 1_200_000, interval: 1_000, timeoutMsg: "the agent never finished" });
    const turnTo = new Date().toISOString();
    const turnMs = Date.parse(turnTo) - Date.parse(turnFrom);
    const reply = (await lastReply())!;
    await shot("online-busy-reply");
    console.log(`e2e: turn ${turnMs} ms; allowed ${allowed} request(s); reply ${reply.text.length} chars`);
    timings["turn"] = { ms: turnMs, allowed };
    timings["note while working"] = { max: Math.max(0, ...noteMs), all: noteMs, budget: 3_000 };
    timings["memory read while working"] = { max: Math.max(0, ...readMs), all: readMs, budget: 500 };
    console.log(`e2e: notes while working ${JSON.stringify(noteMs)}; reads ${JSON.stringify(readMs)}`);
    for (const ms of noteMs) expect(ms).toBeLessThan(3_000);
    for (const ms of readMs) expect(ms).toBeLessThan(500);

    // The records: live list reads per AI per minute stay bounded.
    const { rows, pageCount, pageMs } = listReads(turnFrom, turnTo);
    const minutes = Math.max(1, turnMs / 60_000);
    const perAi: Record<string, number> = {};
    const callers: Record<string, number> = {};
    for (const r of rows) {
      perAi[r.ai] = (perAi[r.ai] ?? 0) + 1;
      callers[r.caller] = (callers[r.caller] ?? 0) + 1;
    }
    timings["list reads during the turn"] = { perAi, callers, pageCount, pageMs, minutes: +minutes.toFixed(1) };
    console.log(`e2e: live list reads ${JSON.stringify(perAi)} by ${JSON.stringify(callers)}; ${pageCount} page read(s), ${pageMs} ms`);
    // One live read per AI per 15 s at most (conversation_cache::LIVE_REUSE), plus slack.
    for (const count of Object.values(perAi)) expect(count).toBeLessThanOrEqual(Math.ceil(minutes * 4) + 2);

    // What the agent was asked to remember is in the project's memory.
    const memory = await invoke<{ content: string }>("project_memory_get", { folder: FOLDER, fresh: true });
    timings["agent remembered the check command"] = memory.content.includes(`check-${RUN}`);
    console.log(`e2e: memory has the check command: ${memory.content.includes(`check-${RUN}`)}`);
  });

  it("opens the conversations drawer and the Projects tab quickly with that many conversations", async function () {
    if (!ready) this.skip();
    this.timeout(300_000);
    await timed("drawer: first conversations shown", 3_000, async () => {
      await (await $('[data-testid="conversations-open"]')).click();
      await (await $('[data-testid="conversation-item"]')).waitForExist({ timeout: 3_000 });
    });
    await shot("online-drawer");
    await browser.keys("Escape");
    await browser.url("tauri://localhost/memory/");
    const tab = await $('[data-testid="memory-tab-projects"]');
    await tab.waitForDisplayed({ timeout: 60_000 });
    await timed("Projects tab: this project listed", 10_000, async () => {
      await tab.click();
      await (await $(`[data-testid="project-item"][data-folder="${FOLDER}"]`)).waitForExist({ timeout: 10_000 });
    });
    await shot("online-projects-tab");
  });
});
