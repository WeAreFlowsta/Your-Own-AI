/**
 * Online-usage display preferences + the shared fetch.
 *
 * The header ticker is ON by default once a plan exists (the fetch returns
 * nothing for the free tier or signed out, so free users never see it) -
 * the person paying watches their spend from the first online turn. The
 * switch in Settings -> Your Flowsta Account turns it off. `usagePrefsChanged`
 * fires on toggle so the header reacts without a reload. (Eric, 09-03:
 * on by default once a plan exists.)
 */
import { invoke } from '@tauri-apps/api/core';

const TICKER_KEY = 'usageTickerEnabled';

export interface UsageSummary {
  month: string;
  tier: string;
  cost_usd: number;
  allowance_usd: number;
  /** The plan's monthly price - the allowance's origin. 0 on older proxies. */
  plan_usd?: number;
  overage_usd: number;
  overage_opt_in: boolean;
  requests: number;
  /** An overage invoice the card could not pay - online models pause past
   *  the allowance until it is paid. */
  overage_hold?: { invoice: string; amount_usd: number; month: string; hosted_invoice_url?: string | null } | null;
}

/** Header ticker switch. Defaults to ON (only an explicit "false" disables). */
export function usageTickerEnabled(): boolean {
  return localStorage.getItem(TICKER_KEY) !== 'false';
}

export function setUsageTickerEnabled(enabled: boolean): void {
  localStorage.setItem(TICKER_KEY, String(enabled));
  window.dispatchEvent(new CustomEvent('usagePrefsChanged'));
}

/** This month's usage, or null when signed out / offline / free tier. */
export async function fetchUsage(): Promise<UsageSummary | null> {
  try {
    const u = await invoke<UsageSummary | null>('flowsta_usage');
    if (!u || u.tier === 'free') return null;
    return u;
  } catch {
    return null;
  }
}

/** The one usage line every surface shows: used · included · over.
 *  "$102.13 used · $66 included · $36.13 over" - the last part only past
 *  the allowance ("$102 of $66" once read as a hundred dollars of overage). */
export function formatUsage(u: UsageSummary): string {
  const base = `$${u.cost_usd.toFixed(2)} used · $${Math.round(u.allowance_usd)} included`;
  const over = u.cost_usd - u.allowance_usd;
  const line = over > 0.005 ? `${base} · $${over.toFixed(2)} over` : base;
  return u.overage_hold ? `${line} · invoice unpaid` : line;
}

/** Where the included amount comes from, stated once beside it:
 *  "$66 included each month (your $60 plan plus 10%)". */
export function includedLine(u: UsageSummary): string {
  const inc = Math.round(u.allowance_usd);
  const plan = u.plan_usd ?? 0;
  if (plan <= 0) return `$${inc} included each month`;
  const bonus = Math.round(((u.allowance_usd - plan) / plan) * 100);
  return bonus > 0
    ? `$${inc} included each month (your $${Math.round(plan)} plan plus ${bonus}%)`
    : `$${inc} included each month (your $${Math.round(plan)} plan)`;
}
