import { component$, useSignal, $, type Signal } from '@builder.io/qwik';
import type { Kept } from '../utils/memoryKeep';

/**
 * The line under a message once it is kept: what was kept, whom it is
 * for, the other choice of audience, and Undo. A second press on Remember
 * shows "Already kept on [day]." in its place. Setting the signal to null
 * takes the line away (after an undo).
 */
export const KeptLine = component$<{
  kept: Signal<Kept | null>;
  /** The AI's id on this device, and its name for "Only Atlas". */
  aiId: string;
  aiName: string;
  /** The words kept were selected in the message. */
  selection?: boolean;
  /** Kept from the person's own message. */
  own?: boolean;
}>((props) => {
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

  const forAudience = $(async (only: boolean) => {
    const k = kept.value;
    if (!k || busy.value) return;
    busy.value = true;
    failed.value = '';
    try {
      const { setAudience, keysOf } = await import('../utils/memoryKeep');
      const { founding } = await keysOf(props.aiId);
      if (only && !founding) throw new Error('no key');
      const item = await setAudience(k.item.id, only ? { kind: 'ai', ai: founding } : { kind: 'all' });
      kept.value = { ...k, item };
    } catch {
      failed.value = 'Could not change it just now.';
    }
    busy.value = false;
  });

  const k = kept.value;
  if (!k) return null;
  const audience = k.item.item.audience;
  const onlyAi = audience?.kind === 'ai';
  const name = props.aiName || 'this AI';
  const day = new Date(k.item.asOf).toLocaleDateString('en-US', { month: 'long', day: 'numeric', ...(new Date(k.item.asOf).getFullYear() === new Date().getFullYear() ? {} : { year: 'numeric' }) });
  const what = props.selection ? 'The words you selected.' : 'This exchange, your message and the reply together.';
  const whom = onlyAi ? `Only for ${name}.` : audience?.kind === 'project' ? 'For this project.' : 'For all your AIs.';
  const link = 'underline hover:text-[var(--text-primary)] disabled:opacity-50';

  return (
    <div class="mt-1 text-[11px] leading-relaxed text-[var(--text-muted)]" data-testid="kept-line">
      {k.already ? (
        <span data-testid="kept-already">Already kept on {day}.</span>
      ) : (
        <span>
          <span class="text-[var(--text-secondary)]">Kept.</span> {what} {whom}
        </span>
      )}
      <span class="ml-2 inline-flex items-center gap-2">
        {!k.already &&
          (onlyAi ? (
            <button type="button" class={link} disabled={busy.value} onClick$={() => forAudience(false)} data-testid="kept-for-all">
              For all your AIs
            </button>
          ) : (
            <button type="button" class={link} disabled={busy.value} onClick$={() => forAudience(true)} data-testid="kept-only-ai">
              Only {name}
            </button>
          ))}
        {!k.already && k.change && (
          <button type="button" class={link} disabled={busy.value} onClick$={undo} data-testid="kept-undo">
            Undo
          </button>
        )}
      </span>
      {failed.value && <span class="ml-2">{failed.value}</span>}
      <div class="mt-0.5 line-clamp-2 italic" data-testid="kept-text">
        {k.item.text}
      </div>
    </div>
  );
});
