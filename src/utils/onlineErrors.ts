/**
 * Billing/auth codes the chat renders as action cards (sign in, link plan,
 * manage plan). Any surface that receives a failed online-model call - the
 * normal chat path, agent session updates, or the agent RPC response - must
 * raise the SAME card, never print raw provider JSON at the user.
 */
const ONLINE_ERROR_CODES = [
  "auth_required",
  "entitlement_required",
  "allowance_exceeded",
  "overage_settlement_failed",
] as const;

/** Fish a known online-error code (and its message) out of an error string.
 *  The proxy's JSON arrives in many wrappers - bare, {"error":{...}},
 *  embedded in prose, or re-encoded with escaped quotes - so match the code
 *  as a bare word (these words don't occur in legitimate error prose). */
export function extractOnlineError(
  raw: string,
): { code: string; message?: string; invoice_url?: string } | null {
  for (const code of ONLINE_ERROR_CODES) {
    if (!raw.includes(code)) continue;
    const m = raw.match(/"message\\?"\s*:\s*\\?"([^"\\]+)/);
    // An unpaid overage invoice travels with its hosted page: the card can
    // offer to pay it directly.
    const inv = raw.match(/"invoice_url\\?"\s*:\s*\\?"(https:[^"\\]+)/);
    return { code, message: m?.[1], ...(inv?.[1] ? { invoice_url: inv[1] } : {}) };
  }
  return null;
}
