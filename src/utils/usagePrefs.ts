/**
 * Online-usage display preferences + the shared fetch.
 *
 * The header ticker is OFF by default - the interface stays clean unless
 * the user opts in from Settings -> Your Flowsta Account. `usagePrefsChanged`
 * fires on toggle so the header reacts without a reload.
 */
import { invoke } from '@tauri-apps/api/core';

const TICKER_KEY = 'usageTickerEnabled';

export interface UsageSummary {
  month: string;
  tier: string;
  cost_usd: number;
  allowance_usd: number;
  overage_usd: number;
  overage_opt_in: boolean;
  requests: number;
}

/** Header ticker switch. Defaults to OFF (only an explicit "true" enables). */
export function usageTickerEnabled(): boolean {
  return localStorage.getItem(TICKER_KEY) === 'true';
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

/** "$4.31 of $20" - the one formatting used everywhere. */
export function formatUsage(u: UsageSummary): string {
  return `$${u.cost_usd.toFixed(2)} of $${Math.round(u.allowance_usd)}`;
}
