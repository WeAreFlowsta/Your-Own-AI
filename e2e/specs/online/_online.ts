// Shared by the online specs: the staging test identity's sign-in, the
// routing settings a spec runs under, and what the app decided.
import { createPrivateKey, sign } from "node:crypto";
import { homedir } from "node:os";
import { existsSync, readFileSync } from "node:fs";
import { resolve } from "node:path";
import { invoke } from "../models/_helpers";

export const PROXY = process.env.YOAI_PROXY_URL ?? "https://yoai-model-proxy-staging-386500392150.us-central1.run.app";
export const IDENTITY = process.env.YOAI_E2E_STAGING_IDENTITY ?? resolve(homedir(), ".config/yoai-e2e/staging-identity.json");
export const ONLINE_MODEL = process.env.YOAI_E2E_ONLINE_MODEL ?? "gpt-6-luna";
export const PROFILE = resolve(import.meta.dirname ?? ".", "../../profile");
export const APP_LOG = resolve(PROFILE, ".local/share/com.solar.yourowai/logs/Your Own AI.log");
export const haveIdentity = () => existsSync(IDENTITY);

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

/** Signed in as the test identity: the tokens a Vault sign-in would store.
 *  Returns the session the app reports (signed in, tier). */
export async function signInAsTestIdentity() {
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
  return invoke<{ signed_in: boolean; tier: string | null }>("flowsta_session");
}

/** Routing settings, where both paths read them: the chat from the
 *  window's storage, agent work and the local server from the store. */
export async function setRoutingPrefs(prefs: Record<string, string | null>) {
  await browser.execute(async (p: Record<string, string | null>) => {
    const T = (window as any).__TAURI__;
    const store = await T.store.load("settings.json");
    for (const [k, v] of Object.entries(p)) {
      if (v === null) {
        localStorage.removeItem(k);
        await store.delete(k);
      } else {
        localStorage.setItem(k, v);
        await store.set(k, v);
      }
    }
    await store.save();
  }, prefs);
}

/** Every online slot on one cheap model; the news slot keeps its search model
 *  (a fresh question needs live web). */
export const cheapPicks = (model = ONLINE_MODEL) => ({
  routingOnlineEveryday: `online:${model}`,
  routingOnlineHard: `online:${model}`,
  routingOnlineHardCode: `online:${model}`,
  routingOnlineHardGeneral: `online:${model}`,
  routingOnlineAgent: `online:${model}`,
  routingOnlinePlanning: `online:${model}`,
});

export type Decision = { at_ms: number; model: string; reason: string; mode: string; share: string; task: string; difficulty: string; side: string };
export const decisions = () => invoke<Decision[]>("recent_routing_decisions");

/** This month's online requests for the signed-in plan (null when unknown). */
export async function onlineRequests(): Promise<number | null> {
  const u = await invoke<{ requests: number } | null>("flowsta_usage").catch(() => null);
  return u ? u.requests : null;
}

/** Set one AI's model setting ("auto:offline", "auto:online-offline", a file,
 *  or "online:<id>"). */
export async function setAiModel(agentKey: string, model: string) {
  await browser.execute(async (key: string, m: string) => {
    const T = (window as any).__TAURI__;
    const store = await T.store.load(await T.core.invoke("profile_store_path", { name: "ai-data.json" }));
    const ais = ((await store.get("custom-ais")) || []) as { agentPubKey?: string; model?: string }[];
    for (const a of ais) if (a.agentPubKey === key) a.model = m;
    await store.set("custom-ais", ais);
    await store.save();
  }, agentKey, model);
}

/** Lines of the app's log between two instants (the log is in UTC). */
export function logBetween(fromIso: string, toIso: string): string[] {
  const from = fromIso.slice(0, 19);
  const to = toIso.slice(0, 19);
  return readFileSync(APP_LOG, "utf8")
    .split("\n")
    .filter((l) => {
      const m = /^\[(\d{4}-\d\d-\d\d)\]\[(\d\d:\d\d:\d\d)\]/.exec(l);
      if (!m) return false;
      const at = `${m[1]}T${m[2]}`;
      return at >= from && at <= to;
    });
}
