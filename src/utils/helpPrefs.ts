/**
 * Help-tips preferences (localStorage).
 *
 * Two layers:
 *  - a GLOBAL on/off ("Show help tips", default ON) toggled from Settings, and
 *  - a per-tip dismissed set ("Got it") so a single tip stays gone once read.
 *
 * A tip shows only when help is globally on AND that tip isn't dismissed.
 * `helpTipsChanged` fires on any change so open Callouts update live.
 */

const ENABLED_KEY = 'helpTipsEnabled';
const DISMISSED_KEY = 'dismissedHelpTips';

function notify() {
  window.dispatchEvent(new CustomEvent('helpTipsChanged'));
}

/** Global switch. Defaults to ON (only an explicit "false" disables it). */
export function helpTipsEnabled(): boolean {
  return localStorage.getItem(ENABLED_KEY) !== 'false';
}

export function setHelpTipsEnabled(enabled: boolean): void {
  localStorage.setItem(ENABLED_KEY, String(enabled));
  notify();
}

function getDismissed(): Set<string> {
  try {
    const raw = localStorage.getItem(DISMISSED_KEY);
    return new Set<string>(raw ? JSON.parse(raw) : []);
  } catch {
    return new Set<string>();
  }
}

export function isHelpDismissed(id: string): boolean {
  return getDismissed().has(id);
}

export function dismissHelp(id: string): void {
  const s = getDismissed();
  s.add(id);
  localStorage.setItem(DISMISSED_KEY, JSON.stringify([...s]));
  notify();
}

/** "Show tips again" — clear all per-tip dismissals. */
export function resetHelpDismissals(): void {
  localStorage.removeItem(DISMISSED_KEY);
  notify();
}

/** Should this tip render right now? (global on AND not dismissed) */
export function shouldShowHelp(id: string): boolean {
  return helpTipsEnabled() && !isHelpDismissed(id);
}
