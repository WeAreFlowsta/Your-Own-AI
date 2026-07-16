import { component$, useSignal, useVisibleTask$, $ } from '@builder.io/qwik';
import { LuBookmark, LuCheck } from '@qwikest/icons/lucide';
import type { RememberHandle } from '../utils/rememberText';

/**
 * "Remember" a single transcript entry (a reply, or your own message) into
 * memory, from the memory-page transcript. Saving routes through the same
 * rememberText util the chat page uses (destination = the Settings → Memory
 * choice for remembered replies), and it's a toggle: click a "Remembered"
 * entry to forget the save again. Small self-contained state so it works
 * inside the entries .map().
 */
export const RememberEntryButton = component$<{ aiId: string; text: string }>((props) => {
  const state = useSignal(''); // '' | 'saving' | 'saved' | 'error'
  const handle = useSignal<RememberHandle | null>(null);

  // Reflect an already-saved entry across reloads (served from a shared
  // per-store index, so a long transcript costs one store read total).
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    try {
      const { findRememberedCached } = await import('../utils/rememberText');
      const h = await findRememberedCached(props.aiId, props.text, 'reply');
      if (h && state.value === '') {
        handle.value = h;
        state.value = 'saved';
      }
    } catch {
      /* display-only check */
    }
  });

  const remember = $(async () => {
    if (state.value === 'saving') return;
    if (state.value === 'saved' && handle.value) {
      state.value = 'saving';
      try {
        const { forgetRemembered } = await import('../utils/rememberText');
        await forgetRemembered(props.aiId, handle.value);
        handle.value = null;
        state.value = '';
      } catch {
        state.value = 'saved';
      }
      return;
    }
    state.value = 'saving';
    try {
      const { rememberText } = await import('../utils/rememberText');
      const h = await rememberText(props.aiId, props.text, 'reply');
      handle.value = h;
      state.value = h ? 'saved' : 'error';
    } catch {
      state.value = 'error';
    }
    if (state.value === 'error') setTimeout(() => (state.value = ''), 2500);
  });

  return (
    <button
      type="button"
      onClick$={remember}
      disabled={state.value === 'saving'}
      title={
        state.value === 'saved'
          ? 'Remembered - click to forget it again'
          : 'Remember this - your AI will draw on it in future conversations'
      }
      class="inline-flex items-center gap-1 text-[11px] text-[var(--text-muted)] hover:text-[var(--text-primary)] transition-colors disabled:opacity-100"
    >
      {state.value === 'saved' ? (
        <>
          <LuCheck class="w-3 h-3 text-emerald-500" /> Remembered
        </>
      ) : (
        <>
          <LuBookmark class="w-3 h-3" />
          {state.value === 'saving' ? 'Saving…' : state.value === 'error' ? 'Try again' : 'Remember'}
        </>
      )}
    </button>
  );
});
