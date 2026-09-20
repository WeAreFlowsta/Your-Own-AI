import { useSignal, useVisibleTask$, type QRL, type Signal } from '@builder.io/qwik';
import type { CorpusProgress } from '../utils/corpus';

/** The library import's progress line, live while an import runs. */
export function useCorpusProgress(): Signal<CorpusProgress | null> {
  const progress = useSignal<CorpusProgress | null>(null);
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    let un: (() => void) | null = null;
    let gone = false;
    import('../utils/corpus').then(({ onCorpusProgress }) =>
      onCorpusProgress((p) => {
        progress.value = p.phase === 'done' ? null : p;
      }).then((fn) => {
        if (gone) fn();
        else un = fn;
      }),
    );
    cleanup(() => {
      gone = true;
      if (un) un();
    });
  });
  return progress;
}

/**
 * Runs `reload` whenever the library has just changed - an import or a folder
 * check finished, whichever part of the app started it. A list that reloads
 * only after ITS OWN import misses a drop handled elsewhere.
 */
export function useLibraryChanged(reload$: QRL<() => void>): void {
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    const uns: (() => void)[] = [];
    let gone = false;
    const keep = (fn: () => void) => {
      if (gone) fn();
      else uns.push(fn);
    };
    import('../utils/corpus').then(({ onCorpusProgress }) =>
      onCorpusProgress((p) => {
        if (p.phase === 'done' && !gone) void reload$();
      }).then(keep),
    );
    import('@tauri-apps/api/event').then(({ listen }) =>
      listen('corpus-folders-synced', () => {
        if (!gone) void reload$();
      }).then(keep),
    );
    cleanup(() => {
      gone = true;
      uns.forEach((fn) => fn());
    });
  });
}

export function progressText(p: CorpusProgress): string {
  const where = p.total > 1 ? `${p.done + 1} of ${p.total}: ` : '';
  if (p.phase === 'reading') return `Reading ${where}${p.file}`;
  const pieces = p.pieces_total
    ? ` (${p.pieces_done?.toLocaleString() ?? 0} of ${p.pieces_total.toLocaleString()} pieces)`
    : '';
  return `Remembering ${where}${p.file}${pieces}`;
}
