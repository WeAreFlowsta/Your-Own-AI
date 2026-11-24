/**
 * Rules: what the person's AIs always follow. A rule is written here in
 * the person's own words, listed with the rest, and given to every AI
 * with every answer. Below: the offers the person declined.
 */
import { component$, useSignal, $ } from '@builder.io/qwik';
import LiquidMetalButton from '../LiquidMetalButton';
import MemoryRowsPanel from '../MemoryRowsPanel';
import { DeclinedOffers } from './DeclinedOffers';
import { saveMemoryRow, newRow, personScope } from '../../utils/memoryRows';

export const RulesBoard = component$(() => {
  const draft = useSignal('');
  const refresh = useSignal(0);
  const failed = useSignal('');
  const saving = useSignal(false);

  const add = $(async () => {
    const text = draft.value.trim();
    if (!text || saving.value) return;
    saving.value = true;
    try {
      await saveMemoryRow(newRow(personScope, 'rule', text, { pinned: true }));
      draft.value = '';
      failed.value = '';
      refresh.value++;
    } catch (e) {
      failed.value = `The rule could not be saved just now. ${String(e)}`;
    }
    saving.value = false;
  });

  return (
    <div class="flex flex-col gap-6" data-testid="rules-board">
      <section class="bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]">
        <p class="text-sm text-[var(--text-secondary)] mb-3">A rule is something your AIs always follow. Write it in your own words.</p>
        <div class="flex flex-wrap items-center gap-2 mb-5">
          <input
            type="text"
            class="flex-1 min-w-[220px] px-3 py-2 rounded-lg bg-[var(--bg-input)] border border-[var(--border-input)] text-[var(--text-primary)] text-sm"
            placeholder="For example: Always ask before sending anything"
            data-testid="rule-add"
            value={draft.value}
            onInput$={(_, el) => (draft.value = el.value)}
            onKeyDown$={(e) => e.key === 'Enter' && add()}
          />
          <LiquidMetalButton class="px-4 py-2 text-sm" testId="rule-add-save" onClick$={add} disabled={saving.value}>
            Add rule
          </LiquidMetalButton>
        </div>
        {failed.value && <p class="text-sm text-[var(--text-secondary)] mb-3">{failed.value}</p>}
        <MemoryRowsPanel scope={personScope} refresh={refresh.value} />
      </section>
      <DeclinedOffers />
    </div>
  );
});
