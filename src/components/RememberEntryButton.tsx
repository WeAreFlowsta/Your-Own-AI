import { component$, useSignal, $ } from '@builder.io/qwik';
import { LuBookmark, LuCheck } from '@qwikest/icons/lucide';

/**
 * "Remember" a single transcript entry (a reply, or your own message) into the
 * AI's memory, from the memory-page transcript. Small self-contained state so
 * it works inside the entries .map(). Saving routes through the same
 * rememberText util the chat page uses.
 */
export const RememberEntryButton = component$<{ aiId: string; text: string }>((props) => {
  const state = useSignal(''); // '' | 'saving' | 'saved' | 'error'

  const remember = $(async () => {
    if (state.value === 'saving' || state.value === 'saved') return;
    state.value = 'saving';
    try {
      const { rememberText } = await import('../utils/rememberText');
      const ok = await rememberText(props.aiId, props.text);
      state.value = ok ? 'saved' : 'error';
    } catch {
      state.value = 'error';
    }
    if (state.value === 'error') setTimeout(() => (state.value = ''), 2500);
  });

  return (
    <button
      type="button"
      onClick$={remember}
      disabled={state.value === 'saving' || state.value === 'saved'}
      title="Remember this - this AI will draw on it in future conversations"
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
