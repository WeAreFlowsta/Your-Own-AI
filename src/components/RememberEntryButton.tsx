import { component$, useSignal, $ } from '@builder.io/qwik';
import { LuBookmark } from '@qwikest/icons/lucide';
import { KeptLine } from './KeptLine';
import type { Kept } from '../utils/memoryKeep';

/**
 * Remember on one entry of a conversation shown on the Memory page (a
 * reply, or the person's own message): the same action as in the chat. The
 * exchange is kept as a memory item that points at it, and the Kept line
 * takes the button's place, with its audience and Undo. Small
 * self-contained state so it works inside the entries .map().
 */
export const RememberEntryButton = component$<{ aiId: string; aiName?: string; conversation: string; role: string; seq: number; text: string }>((props) => {
  const state = useSignal(''); // '' | 'saving' | what went wrong
  const kept = useSignal<Kept | null>(null);

  const remember = $(async () => {
    if (state.value === 'saving') return;
    state.value = 'saving';
    const { keepMessage, keepError } = await import('../utils/memoryKeep');
    try {
      kept.value = await keepMessage({ aiId: props.aiId, role: props.role === 'user' ? 'user' : 'assistant', text: props.text, seq: props.seq, conversation: props.conversation });
      state.value = '';
    } catch (e) {
      state.value = keepError(e);
      setTimeout(() => (state.value = ''), 4000);
    }
  });

  if (kept.value) return <KeptLine kept={kept} aiId={props.aiId} aiName={props.aiName ?? ''} own={props.role === 'user'} />;
  return (
    <button
      type="button"
      onClick$={remember}
      disabled={state.value === 'saving'}
      title="Keep this exchange in memory, for your AIs to use later"
      class="inline-flex items-center gap-1 text-[11px] text-[var(--text-muted)] hover:text-[var(--text-primary)] transition-colors disabled:opacity-100"
      data-testid="remember-entry"
    >
      <LuBookmark class="w-3 h-3" />
      {state.value === 'saving' ? 'Keeping…' : state.value ? state.value : 'Remember'}
    </button>
  );
});
