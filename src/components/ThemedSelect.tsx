import { component$, useSignal, type QRL } from '@builder.io/qwik';
import { LuChevronDown } from '@qwikest/icons/lucide';

export interface ThemedSelectOption {
  value: string;
  label: string;
}

export interface ThemedSelectProps {
  value: string;
  options: ThemedSelectOption[];
  onChange$: QRL<(value: string) => void>;
  /** Shown while no option matches `value`. */
  placeholder?: string;
  disabled?: boolean;
  id?: string;
  /** Classes for the outer box (width and margins); the look is fixed. */
  class?: string;
  /** Open the list to the right edge of the box (a control at a row's end). */
  alignRight?: boolean;
}

/**
 * THE dropdown. A native <select> popup is drawn by the system on webkit
 * (GTK on Linux) and ignores our light / dark colors, so every choice list
 * in the app is this instead. New dropdowns use it; never a bare <select>.
 */
export const ThemedSelect = component$<ThemedSelectProps>((props) => {
  const open = useSignal(false);
  const current = props.options.find((o) => o.value === props.value);
  return (
    <div class={`relative ${props.class ?? ''}`}>
      <button
        type="button"
        id={props.id}
        disabled={props.disabled}
        aria-haspopup="listbox"
        aria-expanded={open.value}
        onClick$={() => { open.value = !open.value; }}
        class="flex w-full items-center justify-between gap-2 rounded-full border border-[var(--border-subtle)] bg-[var(--bg-input)] px-4 py-2 text-left text-sm text-[var(--text-primary)] focus:outline-none disabled:opacity-60"
      >
        <span class="truncate">{current?.label ?? props.placeholder ?? ''}</span>
        <LuChevronDown class={`h-4 w-4 shrink-0 text-[var(--text-muted)] transition-transform ${open.value ? 'rotate-180' : ''}`} />
      </button>
      {open.value && (
        <>
          <div class="fixed inset-0 z-40" onClick$={() => { open.value = false; }} />
          <div
            role="listbox"
            class={`absolute top-full z-50 mt-1 max-h-72 min-w-full overflow-y-auto rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-dropdown)] py-1 shadow-xl ${props.alignRight ? 'right-0' : 'left-0'}`}
          >
            {props.options.map((o) => (
              <button
                key={o.value}
                type="button"
                role="option"
                aria-selected={o.value === props.value}
                onClick$={() => { open.value = false; props.onChange$(o.value); }}
                class={`block w-full px-3 py-1.5 text-left text-sm hover:bg-[var(--bg-card)] transition-colors ${
                  o.value === props.value ? 'font-medium text-[var(--text-primary)]' : 'text-[var(--text-secondary)]'
                }`}
              >
                {o.label}
              </button>
            ))}
          </div>
        </>
      )}
    </div>
  );
});

export default ThemedSelect;
