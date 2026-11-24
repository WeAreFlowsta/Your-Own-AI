/**
 * Open: what is still waiting, on the person or on an AI. Each can be
 * closed as done or as not needed; both can be taken back from Recently.
 */
import { component$, useSignal, useVisibleTask$, $ } from '@builder.io/qwik';
import LiquidMetalButton from '../LiquidMetalButton';
import type { KeptItem } from '../../utils/memoryKeep';
import { dayOf } from '../../utils/memoryKeep';

/** Sent when the number of open items may have changed. */
export const OPEN_CHANGED_EVENT = 'yoai-memory-changed';

export async function openItems(): Promise<KeptItem[]> {
  const { invoke } = await import('@tauri-apps/api/core');
  const all = await invoke<KeptItem[]>('memory_items').catch(() => [] as KeptItem[]);
  return all.filter((i) => i.item.state === 'open').sort((a, b) => b.asOf - a.asOf);
}

export const OpenBoard = component$(() => {
  const items = useSignal<KeptItem[] | null>(null);
  const busy = useSignal('');

  const load = $(async () => {
    items.value = await openItems();
  });
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    await load();
    const again = () => void load();
    window.addEventListener(OPEN_CHANGED_EVENT, again);
    cleanup(() => window.removeEventListener(OPEN_CHANGED_EVENT, again));
  });

  const close = $(async (id: string, done: boolean) => {
    busy.value = id;
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('memory_item_change', { id, edit: { do: 'close', done } }).catch(() => undefined);
    busy.value = '';
    window.dispatchEvent(new CustomEvent(OPEN_CHANGED_EVENT));
  });

  if (items.value === null) return <div data-testid="open-board" />;
  if (items.value.length === 0) {
    return (
      <section class="bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]" data-testid="open-board">
        <h3 class="text-lg font-semibold text-[var(--text-primary)]">Nothing is waiting</h3>
        <p class="mt-1 text-sm text-[var(--text-secondary)]">When something is left to do or to decide, it is listed here until it is closed.</p>
      </section>
    );
  }
  return (
    <section class="bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]" data-testid="open-board">
      <ul class="divide-y divide-[var(--border-subtle)]">
        {items.value.map((i) => (
          <li key={i.id} class="py-3 flex flex-wrap items-start justify-between gap-3" data-testid="open-row">
            <div class="min-w-0 flex-1">
              <div class="text-sm text-[var(--text-primary)] break-words">{i.text}</div>
              <div class="text-xs text-[var(--text-muted)]">Open since {dayOf(i.asOf)}</div>
            </div>
            <div class="flex items-center gap-2">
              <LiquidMetalButton variant="secondary" class="px-3 py-1.5 text-xs" testId="open-not-needed" disabled={busy.value === i.id} onClick$={() => close(i.id, false)}>
                Not needed
              </LiquidMetalButton>
              <LiquidMetalButton class="px-3 py-1.5 text-xs" testId="open-done" disabled={busy.value === i.id} onClick$={() => close(i.id, true)}>
                Done
              </LiquidMetalButton>
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
});
