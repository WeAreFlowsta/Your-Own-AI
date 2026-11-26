import { component$, useSignal, $, type Signal } from '@builder.io/qwik';
import type { Kept } from '../utils/memoryKeep';

/**
 * The line under a message once it is kept: "Kept", and Undo (or "Marked
 * done: ..." when the message closed something that was open). Who it is
 * for and what it says are in Memory, where it can be changed. A second
 * press on Remember shows "Already kept on [day]." in its place. Setting
 * the signal to null takes the line away (after an undo).
 */
export const KeptLine = component$<{ kept: Signal<Kept | null> }>((props) => {
  const busy = useSignal(false);
  const failed = useSignal('');
  const kept = props.kept;

  const undo = $(async () => {
    const k = kept.value;
    if (!k?.change || busy.value) return;
    busy.value = true;
    failed.value = '';
    try {
      const { undoKeep } = await import('../utils/memoryKeep');
      await undoKeep(k.change);
      kept.value = null;
    } catch {
      failed.value = 'Could not undo it just now.';
    }
    busy.value = false;
  });

  const k = kept.value;
  if (!k) return null;
  const day = new Date(k.item.asOf).toLocaleDateString('en-US', { month: 'long', day: 'numeric', ...(new Date(k.item.asOf).getFullYear() === new Date().getFullYear() ? {} : { year: 'numeric' }) });

  return (
    <div class="mt-1 text-[11px] leading-relaxed text-[var(--text-muted)]" data-testid="kept-line">
      {k.closed ? (
        <span class="text-[var(--text-secondary)]" data-testid="kept-closed">
          Marked done: {k.item.text}
        </span>
      ) : k.already ? (
        <span data-testid="kept-already">Already kept on {day}.</span>
      ) : (
        <span class="text-[var(--text-secondary)]">Kept</span>
      )}
      {!k.already && k.change && (
        <button type="button" class="ml-2 underline hover:text-[var(--text-primary)] disabled:opacity-50" disabled={busy.value} onClick$={undo} data-testid="kept-undo">
          Undo
        </button>
      )}
      {failed.value && <span class="ml-2">{failed.value}</span>}
    </div>
  );
});
