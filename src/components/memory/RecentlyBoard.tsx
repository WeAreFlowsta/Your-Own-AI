import { component$, useSignal, useVisibleTask$, $ } from '@builder.io/qwik';
import type { KeptItem } from '../../utils/memoryKeep';
import { dayOf } from '../../utils/memoryKeep';
import { TidyingLine } from './TidyingLine';
import { MemoryQuestions } from './MemoryQuestions';

/** One entry of memory's list of changes, as the backend keeps it. */
interface Change {
  id: string;
  at: number;
  what: 'kept' | 'replaced' | 'closed' | 'forgotten' | 'confirmed' | 'corrected' | 'audience_changed' | 'undone';
  items: string[];
  /** `person` | `ai:<key>` | `tidy`. */
  by: string;
  undo: { do: 'forget' | 'restore' | 'nothing'; text?: string };
  device?: string | null;
}

const RECENT_DAYS = 7;
const PAGE = 20;

/** What happened, in the row's first words. */
function happened(c: Change, who: string): string {
  const mine = c.by === 'person';
  switch (c.what) {
    case 'kept':
      return mine ? 'Kept by you' : `Kept by ${who}, not confirmed`;
    case 'replaced':
      return 'Replaced';
    case 'closed':
      return 'Closed';
    case 'forgotten':
      return 'Forgotten';
    case 'confirmed':
      return 'You confirmed this';
    case 'corrected':
      return 'Your note changed';
    case 'audience_changed':
      return 'Who can use this changed';
    default:
      return 'Undone';
  }
}

/**
 * Recently: the one list of what memory changed, newest first, with Undo
 * on every change that can be taken back. The tidying line stands at its
 * top. Older changes sit under "Earlier", a page at a time.
 */
export const RecentlyBoard = component$(() => {
  const changes = useSignal<Change[] | null>(null);
  const items = useSignal<Record<string, KeptItem>>({});
  const names = useSignal<Record<string, string>>({});
  const earlierShown = useSignal(0);
  const busy = useSignal('');
  const failed = useSignal('');

  const load = $(async () => {
    const { invoke } = await import('@tauri-apps/api/core');
    const [log, all] = await Promise.all([invoke<Change[]>('memory_changes'), invoke<KeptItem[]>('memory_items')]);
    items.value = Object.fromEntries(all.map((i) => [i.id, i]));
    changes.value = log;
    try {
      const { getLocalCustomAis } = await import('../../utils/localAiStorage');
      const { foundingOf } = await import('../../utils/lookalikes');
      names.value = Object.fromEntries((await getLocalCustomAis()).map((a) => [foundingOf(a), a.name]));
    } catch {
      /* rows then say "an AI" */
    }
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    await load().catch(() => (changes.value = []));
    // Looking at the list is what makes its changes seen.
    void import('../../utils/memoryKeep').then(({ markChangesSeen }) => markChangesSeen());
    // A change made elsewhere on the page (a keep, an undo) shows at once.
    const again = () => void load().catch(() => {});
    window.addEventListener('focus', again);
    cleanup(() => window.removeEventListener('focus', again));
  });

  const undo = $(async (id: string) => {
    if (busy.value) return;
    busy.value = id;
    failed.value = '';
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('memory_undo', { changeId: id });
      await load();
    } catch {
      failed.value = id;
    }
    busy.value = '';
  });

  const log = changes.value;
  const cut = Date.now() - RECENT_DAYS * 86_400_000;
  // A change whose item was changed again by an undo afterwards is done with.
  const undoneAfter = (c: Change) => (log ?? []).some((u) => u.what === 'undone' && u.at > c.at && u.items.some((i) => c.items.includes(i)));
  const whoOf = (c: Change) => (c.by === 'tidy' ? 'tidying' : c.by.startsWith('ai:') ? names.value[c.by.slice(3)] || 'an AI' : 'you');

  const row = (c: Change) => {
    const item = items.value[c.items[0]];
    const newer = c.what === 'replaced' && c.items[1] ? items.value[c.items[1]] : undefined;
    const canUndo = c.undo.do !== 'nothing' && c.what !== 'undone' && !undoneAfter(c);
    return (
      <li key={c.id} class="py-2 border-b border-[var(--border-subtle)] last:border-b-0" data-testid="recently-row" data-what={c.what}>
        <div class="flex items-baseline justify-between gap-3">
          <div class="min-w-0 text-sm text-[var(--text-secondary)]">
            <span class="text-[var(--text-primary)]">{happened(c, whoOf(c))}</span>
            <span class="text-[var(--text-muted)]">
              {' '}
              on {dayOf(c.at)}
              {c.what !== 'kept' && c.by === 'tidy' ? ', by tidying' : c.what !== 'kept' && c.by.startsWith('ai:') ? `, by ${whoOf(c)}` : ''}
              {c.device ? ', on another of your devices' : ''}
            </span>
          </div>
          {canUndo && (
            <button type="button" class="flex-shrink-0 text-xs underline text-[var(--text-secondary)] hover:text-[var(--text-primary)] disabled:opacity-50" disabled={busy.value === c.id} onClick$={() => undo(c.id)} data-testid="recently-undo">
              {busy.value === c.id ? 'Undoing…' : 'Undo'}
            </button>
          )}
        </div>
        {c.what === 'replaced' && c.undo.do === 'restore' && newer ? (
          <div class="mt-1 text-sm text-[var(--text-secondary)]">
            <div>
              <span class="text-[var(--text-muted)]">Was:</span> {c.undo.text}
            </div>
            <div>
              <span class="text-[var(--text-muted)]">Now:</span> {newer.text}
            </div>
          </div>
        ) : (
          item && <div class="mt-1 text-sm text-[var(--text-secondary)] line-clamp-3">{item.text}</div>
        )}
        {failed.value === c.id && <div class="mt-1 text-xs text-[var(--text-muted)]">Could not undo it just now.</div>}
      </li>
    );
  };

  const recent = (log ?? []).filter((c) => c.at >= cut);
  const earlier = (log ?? []).filter((c) => c.at < cut);

  return (
    <section class="bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]" data-testid="recently-board">
      <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-2">Recently</h2>
      <p class="text-sm text-[var(--text-secondary)] mb-4">What memory changed lately. Changes memory made by itself can be undone.</p>
      <TidyingLine />
      <div class="mb-4">
        <MemoryQuestions />
      </div>
      {log === null ? (
        <p class="text-sm text-[var(--text-muted)]">Loading…</p>
      ) : log.length === 0 ? (
        <p class="text-sm text-[var(--text-muted)]" data-testid="recently-empty">
          Nothing has changed yet. Say "remember that" in any chat, or press Remember under a reply.
        </p>
      ) : (
        <>
          {recent.length > 0 && <ul>{recent.map(row)}</ul>}
          {earlier.length > 0 && (
            <div class="mt-4" data-testid="recently-earlier">
              <h3 class="text-sm font-semibold text-[var(--text-primary)] mb-1">Earlier</h3>
              {earlierShown.value === 0 ? (
                <button type="button" class="text-sm underline text-[var(--text-secondary)] hover:text-[var(--text-primary)]" onClick$={() => (earlierShown.value = PAGE)}>
                  Show {Math.min(PAGE, earlier.length)} earlier {earlier.length === 1 ? 'change' : 'changes'}
                </button>
              ) : (
                <>
                  <ul>{earlier.slice(0, earlierShown.value).map(row)}</ul>
                  {earlier.length > earlierShown.value && (
                    <button type="button" class="mt-2 text-sm underline text-[var(--text-secondary)] hover:text-[var(--text-primary)]" onClick$={() => (earlierShown.value += PAGE)}>
                      Show the next {Math.min(PAGE, earlier.length - earlierShown.value)}
                    </button>
                  )}
                </>
              )}
            </div>
          )}
        </>
      )}
    </section>
  );
});
