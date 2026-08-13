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
 */
import { invoke } from "@tauri-apps/api/core";

export interface OnlineEntitlement {
  signedIn: boolean;
  /** May online features be offered for NEW selections? */
  entitled: boolean;
}

export async function getOnlineEntitlement(): Promise<OnlineEntitlement> {
  try {
    const s = await invoke<{
      signed_in: boolean;
      linked: boolean | null;
      tier: string | null;
    }>("flowsta_session");
    return {
      signedIn: !!s.signed_in,
      // Linked is not entitled: devices link independently of paying now,
      // so an explicitly FREE tier gates even when linked. Unknown tier
      // (probe failed) keeps failing OPEN - paying users must never lose
      // controls to a slow check; the proxy enforces per request anyway.
      entitled: !!s.signed_in && s.linked !== false && s.tier !== "free",
    };
  } catch {
    return { signedIn: false, entitled: false };
  }
}
