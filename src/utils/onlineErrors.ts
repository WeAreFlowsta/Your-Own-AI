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

/** Provider and service failures (the proxy's coded errors, and the app's
 *  own for a stream that ended in an error), in plain words for the reply's
 *  error line - which keeps its Retry button. Before this every one of them
 *  printed as raw JSON ("Error: {"message":...}"). */
const PROVIDER_ERROR_TEXT: Record<string, string> = {
  provider_busy: "The model's provider is busy right now. Try again in a moment.",
  provider_timeout: "The model's provider didn't answer in time. Try again.",
  provider_error: "The model's provider had an error. Try again, or pick another model.",
  provider_model_unavailable: "This model isn't available from its provider right now. Pick another model, or try again later.",
  provider_not_configured: "This model isn't available right now. Pick another model.",
  unknown_model: "This model isn't in the online catalog any more. Pick another model.",
  provider_rejected: "The model's provider refused this request. Try rephrasing it, or pick another model.",
  provider_stream_failed: "The model stopped partway through the reply. Try again.",
  stream_error: "The reply stopped partway through. Try again.",
  rate_limited: "Too many requests in a minute. Wait a moment, then try again.",
  online_error: "The online model couldn't answer. Try again.",
};

/** The coded failure in an error string, if it is one we can say plainly. */
export function providerErrorCode(raw: string): string | null {
  const m = raw.match(/"code\\?"\s*:\s*\\?"([a-z_]+)/);
  return m && m[1] in PROVIDER_ERROR_TEXT ? m[1] : null;
}

/** Plain words for a coded failure; null when the error is not one. */
export function providerErrorText(raw: string): string | null {
  const code = providerErrorCode(raw);
  return code ? PROVIDER_ERROR_TEXT[code] : null;
}
