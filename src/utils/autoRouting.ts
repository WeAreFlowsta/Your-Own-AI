/**
 * "Models automatic routing may use" - the person's switches, shared by
 * Settings > Routing and the Online Models page. settings.json
 * `routingAutoAllow` holds every switch the person has touched, whatever
 * its value: `{ providers: { id: bool }, models: { "<id>": bool } }`. A
 * default stays a default only until the person changes it; a switch they
 * set keeps its value when the catalog's default later changes, also when
 * the two were equal at the time. The rule
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

/** Pure: the switches after the person set a provider's. It is kept whatever
 *  its value, and it decides only for the models they never touched: a
 *  model's own switch stays as they set it. */
export function withProviderSwitch(v: AutoAllow, providerId: string, on: boolean): AutoAllow {
  return { providers: { ...v.providers, [providerId]: on }, models: { ...v.models } };
}

/** Pure: the switches after the person set one model's. Kept whatever its
 *  value, also when it matches what its provider says today. */
export function withModelSwitch(v: AutoAllow, modelId: string, on: boolean): AutoAllow {
  return { providers: { ...v.providers }, models: { ...v.models, [modelId]: on } };
}

/** A provider's switch. `_modelIds` is no longer used (the switches of its
 *  models were cleared here until 0.9.1). */
export async function setProviderAllowed(provider: OnlineProvider, _modelIds: string[], on: boolean): Promise<void> {
  await writeAutoAllow(withProviderSwitch(await readAutoAllow(), provider.id, on));
}

/** One model's switch. `_providerAllows` is no longer used (a switch equal
 *  to its provider's was not stored until 0.9.1). */
export async function setModelAllowed(modelId: string, _providerAllows: boolean, on: boolean): Promise<void> {
  await writeAutoAllow(withModelSwitch(await readAutoAllow(), modelId, on));
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
  { key: "plan", storageKey: "routingOnlinePlanning", label: "Planning", needs: "tools" },
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
