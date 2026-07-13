/**
 * Callout — info-box notice surface, ported from the Flowsta dashboard design
 * system (Website / Website-dev / Vault) to match its look in Your Own AI.
 *
 * A tinted box with a vivid coloured LEFT ACCENT BAR + intent icon; colour is
 * carried by the bar + icon, body text stays neutral (theme-var tokens) for
 * legibility in both light and dark. No CTA/dismiss — informational only.
 */
import { component$, Slot, useSignal, useVisibleTask$, $ } from '@builder.io/qwik';
import { shouldShowHelp, dismissHelp } from '../utils/helpPrefs';

export type CalloutIntent = 'info' | 'success' | 'warning' | 'danger' | 'premium';

interface CalloutProps {
  intent: CalloutIntent;
  title?: string;
  /** Override the default per-intent icon with an outline-svg `d` path. */
  iconPath?: string;
  /**
   * When set, this Callout is a dismissible HELP TIP: it shows a "Got it"
   * button, stays hidden once dismissed, and respects the global "Show help
   * tips" switch. Without an id it's a plain, always-on notice.
   */
  id?: string;
  class?: string;
}

interface IntentCfg {
  wrap: string; // bg tint + all-side border
  bar: string;  // left accent-bar colour
  icon: string; // icon colour
  path: string; // default outline-svg `d` path
}

// Full static class strings per intent — Tailwind only emits classes it sees
// literally, so these must NOT be built by interpolation.
const INTENT: Record<CalloutIntent, IntentCfg> = {
  info: {
    wrap: 'bg-sky-500/10 border-sky-500/25',
    bar: 'border-l-sky-400',
    icon: 'text-sky-400',
    path: 'M11.25 11.25l.041-.02a.75.75 0 011.063.852l-.708 2.836a.75.75 0 001.063.853l.041-.021M21 12a9 9 0 11-18 0 9 9 0 0118 0zm-9-3.75h.008v.008H12V8.25z',
  },
  success: {
    wrap: 'bg-emerald-500/10 border-emerald-500/25',
    bar: 'border-l-emerald-400',
    icon: 'text-emerald-400',
    path: 'M9 12.75L11.25 15 15 9.75M21 12a9 9 0 11-18 0 9 9 0 0118 0z',
  },
  warning: {
    wrap: 'bg-amber-500/10 border-amber-500/25',
    bar: 'border-l-amber-400',
    icon: 'text-amber-400',
    path: 'M12 9v3.75m-9.303 3.376c-.866 1.5.217 3.374 1.948 3.374h14.71c1.73 0 2.813-1.874 1.948-3.374L13.949 3.378c-.866-1.5-3.032-1.5-3.898 0L2.697 16.126zM12 15.75h.007v.008H12v-.008z',
  },
  danger: {
    wrap: 'bg-red-500/10 border-red-500/25',
    bar: 'border-l-red-400',
    icon: 'text-red-400',
    path: 'M12 9v3.75m9-.75a9 9 0 11-18 0 9 9 0 0118 0zm-9 3.75h.008v.008H12v-.008z',
  },
  premium: {
    wrap: 'bg-indigo-500/10 border-indigo-500/25',
    bar: 'border-l-indigo-400',
    icon: 'text-indigo-400',
    path: 'M11.48 3.499a.562.562 0 011.04 0l2.125 5.111a.563.563 0 00.475.345l5.518.442c.499.04.701.663.321.988l-4.204 3.602a.563.563 0 00-.182.557l1.285 5.385a.562.562 0 01-.84.61l-4.725-2.885a.563.563 0 00-.586 0L6.982 20.54a.562.562 0 01-.84-.61l1.285-5.386a.563.563 0 00-.182-.557l-4.204-3.602a.563.563 0 01.321-.988l5.518-.442a.563.563 0 00.475-.345L11.48 3.5z',
  },
};

export const Callout = component$<CalloutProps>(({ intent, title, iconPath, id, class: className = '' }) => {
  const c = INTENT[intent];
  // Help tips (id set) start hidden until the client confirms they should show
  // — avoids a flash of a tip the user already dismissed. Plain callouts (no id)
  // show immediately.
  const hidden = useSignal(!!id);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    if (!id) return;
    const check = () => { hidden.value = !shouldShowHelp(id); };
    check();
    window.addEventListener('helpTipsChanged', check);
    cleanup(() => window.removeEventListener('helpTipsChanged', check));
  });

  const onGotIt = $(() => {
    if (id) dismissHelp(id);
  });

  return (
    <>
      {!hidden.value && (
        <div class={`rounded-lg border border-l-4 p-4 ${c.wrap} ${c.bar} ${className}`}>
          <div class="flex items-start gap-3">
            <svg class={`w-5 h-5 mt-0.5 shrink-0 ${c.icon}`} fill="none" stroke="currentColor" viewBox="0 0 24 24">
              <path stroke-linecap="round" stroke-linejoin="round" stroke-width="2" d={iconPath || c.path} />
            </svg>
            <div class="min-w-0 flex-1">
              {title && <h3 class="font-semibold text-[var(--text-primary)]">{title}</h3>}
              <div class={`text-sm text-[var(--text-secondary)] ${title ? 'mt-1' : ''}`}>
                <Slot />
              </div>
              {id && (
                <button
                  type="button"
                  onClick$={onGotIt}
                  class="mt-3 text-xs font-medium text-[var(--text-link)] hover:underline"
                >
                  Got it
                </button>
              )}
            </div>
          </div>
        </div>
      )}
    </>
  );
});

export default Callout;
