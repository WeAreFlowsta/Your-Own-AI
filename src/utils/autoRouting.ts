/**
 * "Models automatic routing may use" - the person's switches, shared by
 * Settings > Routing and the Online Models page. settings.json
 * `routingAutoAllow` holds only what the person changed:
 * `{ providers: { id: bool }, models: { "online:<id>": bool } }`. The rule
 * (a model's own switch, then its provider's, then the catalog's default)
 * lives in the router (router.rs auto_allowed); `online_auto_allowed` hands
 * back each model's standing, so a screen never computes its own.
 */
import { invoke } from "@tauri-apps/api/core";

export type AutoAllow = { providers: Record<string, boolean>; models: Record<string, boolean> };
export type OnlineProvider = { id: string; name: string; based_in?: string | null; auto_default: boolean };

/** Fired on window after any change, so every screen showing it re-reads. */
export const AUTO_ROUTING_CHANGED = "yoai-auto-routing-changed";

async function store() {
  const { Store } = await import("@tauri-apps/plugin-store");
  return Store.load("settings.json");
}

export async function readAutoAllow(): Promise<AutoAllow> {
  try {
    const v = ((await (await store()).get("routingAutoAllow")) ?? {}) as Partial<AutoAllow>;
    return { providers: { ...(v.providers ?? {}) }, models: { ...(v.models ?? {}) } };
  } catch {
    return { providers: {}, models: {} };
  }
}

async function writeAutoAllow(v: AutoAllow): Promise<void> {
  const s = await store();
  if (!Object.keys(v.providers).length && !Object.keys(v.models).length) await s.delete("routingAutoAllow");
  else await s.set("routingAutoAllow", v);
  await s.save();
  window.dispatchEvent(new CustomEvent(AUTO_ROUTING_CHANGED));
}

/** A provider's switch governs all its models: the per-model switches under
 *  it are cleared. Back to the catalog's default = no entry at all. */
export async function setProviderAllowed(provider: OnlineProvider, modelIds: string[], on: boolean): Promise<void> {
  const v = await readAutoAllow();
  for (const id of modelIds) delete v.models[id];
  if (on === provider.auto_default) delete v.providers[provider.id];
  else v.providers[provider.id] = on;
  await writeAutoAllow(v);
}

/** One model's switch. Matching what its provider says = no entry. */
export async function setModelAllowed(modelId: string, providerAllows: boolean, on: boolean): Promise<void> {
  const v = await readAutoAllow();
  if (on === providerAllows) delete v.models[modelId];
  else v.models[modelId] = on;
  await writeAutoAllow(v);
}

/** Each model's standing ("online:<id>" -> allowed), from the router's rule. */
export async function autoAllowedMap(): Promise<Record<string, boolean>> {
  return invoke<Record<string, boolean>>("online_auto_allowed").catch(() => ({}));
}

/** Does the provider allow its models (its switch, else the catalog's default)? */
export function providerAllows(provider: OnlineProvider, v: AutoAllow): boolean {
  return v.providers[provider.id] ?? provider.auto_default;
}

/* ---- the person's model per kind of question (Settings > Routing's pickers) ---- */

/** The kinds of question with an online model each, as Settings names them.
 *  `needs`: chat = any model that is not a web-search one; search = a
 *  web-search model; tools = a model that can drive tools (project work). */
export const ROUTING_SLOTS = [
  { key: "everyday", storageKey: "routingOnlineEveryday", label: "Everyday questions", needs: "chat" },
  { key: "hard", storageKey: "routingOnlineHard", label: "Hard questions", needs: "chat" },
  { key: "fresh", storageKey: "routingOnlineFresh", label: "Needs current information", needs: "search" },
  { key: "agent", storageKey: "routingOnlineAgent", label: "Project work", needs: "tools" },
  { key: "plan", storageKey: "routingOnlinePlanning", label: "Planning and helper agents", needs: "tools" },
] as const;
export type RoutingSlot = (typeof ROUTING_SLOTS)[number];

/** The person's pick for a kind of question ("" = the recommended model). */
export function slotPick(storageKey: string): string {
  try {
    return localStorage.getItem(storageKey) || "";
  } catch {
    return "";
  }
}

/** Set or clear ("") a pick, where both paths read it: the chat from the
 *  window's storage, agent work and the local server from the store. */
export async function setSlotPick(storageKey: string, id: string): Promise<void> {
  try {
    if (id) localStorage.setItem(storageKey, id);
    else localStorage.removeItem(storageKey);
  } catch { /* storage */ }
  try {
    const s = await store();
    await s.set(storageKey, id);
    await s.save();
  } catch { /* the store mirror is best-effort, as in Settings */ }
  window.dispatchEvent(new CustomEvent(AUTO_ROUTING_CHANGED));
}

/** The models that take a kind of question now: the person's pick, else the
 *  router's recommended model(s) (routing_defaults). Hard has two defaults
 *  (technical and general) and the two older per-task picks. */
export function slotModels(slot: RoutingSlot, defaults: Record<string, string>): { ids: string[]; picked: boolean } {
  const pick = slotPick(slot.storageKey);
  if (pick) return { ids: [pick], picked: true };
  if (slot.key === "hard") {
    const code = slotPick("routingOnlineHardCode") || defaults.hard_code;
    const general = slotPick("routingOnlineHardGeneral") || defaults.hard_general;
    return { ids: [...new Set([code, general].filter(Boolean))], picked: false };
  }
  return { ids: defaults[slot.key] ? [defaults[slot.key]] : [], picked: false };
}
