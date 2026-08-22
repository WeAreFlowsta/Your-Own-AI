/**
 * Online-models entitlement — the single rule for whether online features may
 * be OFFERED in the UI (mirrors the Online Models page's gating: signed in
 * with Flowsta AND this device linked to a yourownai.net plan; `linked: null`
 * is unknown-but-probably-fine, so it fails open like that page does).
 *
 * Offering vs keeping: gates apply to NEW selections only. An AI whose model
 * is already online/auto-online keeps working from the UI's point of view —
 * entitlement is enforced per request by the proxy, and a billing problem
 * surfaces as the existing "fix your plan" flow rather than silently
 * stripping the user's settings.
 *
 * The moment a plan activates: every surface that learns the entitlement
 * feeds `noteEntitlement`, which keeps the last KNOWN state in localStorage
 * and turns a not-entitled -> entitled change into `entitlementChanged` plus
 * a pending "your AIs can now go online" tip (shown at the chat's Ask row).
 * Only known states are recorded - a failed tier probe fails open for the
 * controls but must never fake an activation.
 */
import { invoke } from "@tauri-apps/api/core";

export interface OnlineEntitlement {
  signedIn: boolean;
  /** May online features be offered for NEW selections? */
  entitled: boolean;
}

export interface EntitlementSession {
  signed_in: boolean;
  linked: boolean | null;
  tier: string | null;
}

/** The offering rule, shared by every caller that reads flowsta_session. */
export function entitledFromSession(s: EntitlementSession): boolean {
  // Linked is not entitled: devices link independently of paying now, so an
  // explicitly FREE tier gates even when linked. Unknown tier (probe failed)
  // keeps failing OPEN - paying users must never lose controls to a slow
  // check; the proxy enforces per request anyway.
  return !!s.signed_in && s.linked !== false && s.tier !== "free";
}

const KNOWN_KEY = "onlineEntitledKnown"; // 'yes' | 'no'
const UNLOCK_KEY = "onlineUnlockedAt"; // epoch ms of the last no -> yes change
const UNLOCK_TIP_TTL_MS = 14 * 24 * 60 * 60 * 1000;

/** The id the Callout dismisses under (Settings > "Show help tips" resets it). */
export const ONLINE_UNLOCK_TIP_ID = "online-unlocked";

/** Last recorded KNOWN entitlement, or null when nothing has been recorded. */
export function lastKnownEntitled(): "yes" | "no" | null {
  const v = localStorage.getItem(KNOWN_KEY);
  return v === "yes" || v === "no" ? v : null;
}

/**
 * Record what a fresh flowsta_session says. Signed out = not entitled;
 * signed in with a known tier = the rule above; signed in with the tier
 * unknown = skip (the probe failed - nothing to learn). Fires
 * `entitlementChanged` on any change and arms the unlock tip on no -> yes.
 */
export function noteEntitlement(s: EntitlementSession): void {
  let known: "yes" | "no";
  if (!s.signed_in) known = "no";
  else if (s.tier == null && s.linked !== false) return;
  else known = entitledFromSession(s) ? "yes" : "no";

  const prev = localStorage.getItem(KNOWN_KEY);
  if (prev === known) return;
  localStorage.setItem(KNOWN_KEY, known);
  if (prev === "no" && known === "yes") {
    localStorage.setItem(UNLOCK_KEY, String(Date.now()));
  }
  window.dispatchEvent(
    new CustomEvent("entitlementChanged", { detail: { entitled: known === "yes" } }),
  );
}

/** Should the "your AIs can now go online" tip be offered right now? */
export function onlineUnlockPending(): boolean {
  const at = Number(localStorage.getItem(UNLOCK_KEY));
  if (!at) return false;
  if (Date.now() - at > UNLOCK_TIP_TTL_MS) {
    localStorage.removeItem(UNLOCK_KEY);
    return false;
  }
  return lastKnownEntitled() === "yes";
}

export function clearOnlineUnlockPending(): void {
  localStorage.removeItem(UNLOCK_KEY);
  window.dispatchEvent(
    new CustomEvent("entitlementChanged", { detail: { entitled: lastKnownEntitled() === "yes" } }),
  );
}

export async function getOnlineEntitlement(): Promise<OnlineEntitlement> {
  try {
    const s = await invoke<EntitlementSession>("flowsta_session");
    noteEntitlement(s);
    return { signedIn: !!s.signed_in, entitled: entitledFromSession(s) };
  } catch {
    return { signedIn: false, entitled: false };
  }
}

/**
 * Re-check while the last known state is not-entitled, throttled - wired to
 * window focus / visibility by the layout so returning from a browser
 * checkout is the moment the app notices, without polling while signed in.
 */
let lastRecheck = 0;
export async function recheckEntitlementIfUnentitled(minGapMs = 15_000): Promise<void> {
  if (lastKnownEntitled() === "yes") return;
  const now = Date.now();
  if (now - lastRecheck < minGapMs) return;
  lastRecheck = now;
  await getOnlineEntitlement();
}
