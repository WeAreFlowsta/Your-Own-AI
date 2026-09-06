import { useSignal, useVisibleTask$, type Signal } from '@builder.io/qwik';
import type { CorpusProgress } from '../utils/corpus';

/** The library import's progress line, live while an import runs. */
export function useCorpusProgress(): Signal<CorpusProgress | null> {
  const progress = useSignal<CorpusProgress | null>(null);
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    let un: (() => void) | null = null;
    import('../utils/corpus').then(({ onCorpusProgress }) =>
      onCorpusProgress((p) => {
        progress.value = p.phase === 'done' ? null : p;
      }).then((fn) => {
        un = fn;
      }),
    );
    cleanup(() => {
      if (un) un();
    });
  });
  return progress;
}

export function progressText(p: CorpusProgress): string {
  const where = p.total > 1 ? `${p.done + 1} of ${p.total}: ` : '';
  return p.phase === 'reading' ? `Reading ${where}${p.file}` : `Remembering ${where}${p.file}`;
}
