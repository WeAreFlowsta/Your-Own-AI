/**
 * Open: what is still waiting, oldest first. An open thing that later work
 * may have finished is listed first, with what suggests it, and the person
 * answers: nothing is closed for them. One set of words everywhere: Done,
 * Still waiting, Not needed. Each can be taken back from Recently.
 */
import { component$, useSignal, useVisibleTask$, $ } from '@builder.io/qwik';
import { useNavigate } from '@builder.io/qwik-city';
import LiquidMetalButton from '../LiquidMetalButton';
import type { KeptItem } from '../../utils/memoryKeep';
import { dayOf } from '../../utils/memoryKeep';

/** Sent when the number of open items may have changed. */
export const OPEN_CHANGED_EVENT = 'yoai-memory-changed';

export async function openItems(): Promise<KeptItem[]> {
  const { invoke } = await import('@tauri-apps/api/core');
  const all = await invoke<KeptItem[]>('memory_items').catch(() => [] as KeptItem[]);
  return all.filter((i) => i.item.state === 'open').sort((a, b) => a.asOf - b.asOf);
}

interface MayBeDone {
  open: KeptItem;
  later: KeptItem;
}

interface Ai {
  founding: string;
  name: string;
}

/** The AI an open item was left with, when one is known: the one that kept it. */
function aiOf(i: KeptItem, ais: Ai[]): Ai | undefined {
  const by = i.savedBy.startsWith('ai:') ? i.savedBy.slice(3) : ((i.item.source as { for_ai?: string | null } | null | undefined)?.for_ai ?? '');
  return ais.find((a) => a.founding === by);
}

export const OpenBoard = component$(() => {
  const items = useSignal<KeptItem[] | null>(null);
  const maybe = useSignal<MayBeDone[]>([]);
  const ais = useSignal<Ai[]>([]);
  const tidyOff = useSignal(false);
  const busy = useSignal('');
  const nav = useNavigate();

  const load = $(async () => {
    const { invoke } = await import('@tauri-apps/api/core');
    maybe.value = await invoke<MayBeDone[]>('memory_may_be_done').catch(() => []);
    items.value = await openItems();
  });
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    await load();
    const again = () => void load();
    window.addEventListener(OPEN_CHANGED_EVENT, again);
    cleanup(() => window.removeEventListener(OPEN_CHANGED_EVENT, again));
    const { invoke } = await import('@tauri-apps/api/core');
    tidyOff.value = await invoke<{ state: string }>('memory_tidy_status').then((s) => s.state === 'off', () => false);
    const { getLocalCustomAis } = await import('../../utils/localAiStorage');
    ais.value = (await getLocalCustomAis().catch(() => []))
      .filter((a) => a.status !== 'archived')
      .map((a) => ({ founding: a.foundingKey || a.agentPubKey || '', name: a.name }))
      .filter((a) => a.founding);
  });

  const close = $(async (id: string, done: boolean) => {
    busy.value = id;
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('memory_item_change', { id, edit: { do: 'close', done } }).catch(() => undefined);
    busy.value = '';
    window.dispatchEvent(new CustomEvent(OPEN_CHANGED_EVENT));
  });

  const stillWaiting = $(async (open: string, later: string) => {
    busy.value = open;
    const { invoke } = await import('@tauri-apps/api/core');
    await invoke('memory_still_waiting', { open, later }).catch(() => undefined);
    busy.value = '';
    await load();
  });

  // The request, ready to send, in the conversation the item came from
  // (so the AI it was left with has what was said), or in the chat as it is.
  const askNow = $(async (i: KeptItem) => {
    const { invoke } = await import('@tauri-apps/api/core');
    const startedAt = (i.item.source as { ptr?: { c: number } | null } | null | undefined)?.ptr?.c ?? 0;
    const place = startedAt > 0 ? await invoke<{ hash: string; agentKey: string } | null>('memory_conversation_at', { startedAt }).catch(() => null) : null;
    try {
      if (place) sessionStorage.setItem('resume-conversation', JSON.stringify({ hash: place.hash, agentKey: place.agentKey }));
      sessionStorage.setItem('chat-words-ready', `Please do this now: ${i.text}`);
    } catch {
      /* the chat opens without the words */
    }
    await nav('/chat/');
  });

  if (items.value === null) return <div data-testid="open-board" />;
  if (items.value.length === 0) {
    return (
      <section class="bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]" data-testid="open-board">
        <h3 class="text-lg font-semibold text-[var(--text-primary)]">Nothing is waiting</h3>
        <p class="mt-1 text-sm text-[var(--text-secondary)]" data-testid="open-empty">
          {tidyOff.value ? 'Things you mark as open appear here.' : 'When something is left to do or to decide, it is listed here until it is closed.'}
        </p>
      </section>
    );
  }
  const suggested = new Set(maybe.value.map((m) => m.open.id));
  const waiting = items.value.filter((i) => !suggested.has(i.id));
  const buttons = (i: KeptItem, later?: KeptItem) => (
    <div class="flex flex-wrap items-center gap-2">
      <LiquidMetalButton variant="secondary" class="px-3 py-1.5 text-xs" testId="open-not-needed" disabled={busy.value === i.id} onClick$={() => close(i.id, false)}>
        Not needed
      </LiquidMetalButton>
      {later && (
        <LiquidMetalButton variant="secondary" class="px-3 py-1.5 text-xs" testId="open-still-waiting" disabled={busy.value === i.id} onClick$={() => stillWaiting(i.id, later.id)}>
          Still waiting
        </LiquidMetalButton>
      )}
      <LiquidMetalButton class="px-3 py-1.5 text-xs" testId="open-done" disabled={busy.value === i.id} onClick$={() => close(i.id, true)}>
        Done
      </LiquidMetalButton>
    </div>
  );
  const heading = 'text-sm font-semibold text-[var(--text-primary)]';
  return (
    <section class="bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]" data-testid="open-board">
      <p class="text-sm text-[var(--text-secondary)]">Everything still waiting, oldest first.</p>
      {maybe.value.length > 0 && (
        <div class="mt-5" data-testid="open-maybe-done">
          <h3 class={heading}>May already be done</h3>
          <ul class="divide-y divide-[var(--border-subtle)]">
            {maybe.value.map((m) => (
              <li key={m.open.id} class="py-3 flex flex-wrap items-start justify-between gap-3" data-testid="open-maybe-row" data-id={m.open.id}>
                <div class="min-w-0 flex-1">
                  <div class="text-sm text-[var(--text-primary)] break-words">{m.open.text}</div>
                  <div class="text-xs text-[var(--text-muted)]">Open since {dayOf(m.open.asOf)}. It was never marked done.</div>
                  <div class="mt-1.5 border-l-2 border-[var(--border-subtle)] pl-3 text-sm text-[var(--text-secondary)] break-words" data-testid="open-maybe-later">
                    {m.later.text}
                    <span class="block text-xs text-[var(--text-muted)]">Kept {dayOf(m.later.asOf)}</span>
                  </div>
                </div>
                {buttons(m.open, m.later)}
              </li>
            ))}
          </ul>
        </div>
      )}
      {waiting.length > 0 && (
        <div class="mt-5" data-testid="open-waiting">
          {maybe.value.length > 0 && <h3 class={heading}>Still waiting</h3>}
          <ul class="divide-y divide-[var(--border-subtle)]">
            {waiting.map((i) => {
              const ai = aiOf(i, ais.value);
              return (
                <li key={i.id} class="py-3 flex flex-wrap items-start justify-between gap-3" data-testid="open-row" data-id={i.id}>
                  <div class="min-w-0 flex-1">
                    <div class="text-sm text-[var(--text-primary)] break-words">{i.text}</div>
                    <div class="text-xs text-[var(--text-muted)]">
                      Open since {dayOf(i.asOf)}.{' '}
                      <button type="button" class="text-[var(--text-link)] hover:underline" data-testid="open-ask-now" onClick$={() => askNow(i)}>
                        Ask {ai?.name ?? 'your AI'} to do it now
                      </button>
                    </div>
                  </div>
                  {buttons(i)}
                </li>
              );
            })}
          </ul>
        </div>
      )}
    </section>
  );
});
