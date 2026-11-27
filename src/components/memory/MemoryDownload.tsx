import { component$, $, useSignal } from '@builder.io/qwik';
import { EMBEDDING_MODEL } from '../../data/recommended-models';

/**
 * Where a reply says memory did not look because the part of the app that
 * searches is not on this computer: the one thing that fixes it, in place.
 * The download normally starts by itself; it does not on a connection
 * that is metered, or when it failed, and this is then the way to get it.
 */
export const MemoryDownload = component$(() => {
  const state = useSignal<'' | 'downloading' | 'ready' | 'failed'>('');
  const percent = useSignal(0);

  const fetchIt = $(async () => {
    if (state.value === 'downloading') return;
    state.value = 'downloading';
    percent.value = 0;
    try {
      const { modelManager } = await import('../../utils/modelManager');
      const { isEmbeddingModelReady } = await import('../../utils/embeddings');
      await modelManager.downloadModel(EMBEDDING_MODEL.downloadUrl, EMBEDDING_MODEL.filename, (p) => {
        percent.value = Math.round(p.percent);
      });
      state.value = (await isEmbeddingModelReady()) ? 'ready' : 'failed';
    } catch {
      state.value = 'failed';
    }
  });

  const mb = Math.round(EMBEDDING_MODEL.size * 1000);
  return (
    <div class="flex flex-wrap items-center gap-x-3 gap-y-1" data-testid="memory-download" data-state={state.value}>
      {state.value === 'ready' ? (
        <span>It is on this computer now. Ask again and memory will look.</span>
      ) : state.value === 'downloading' ? (
        <span>Downloading: {percent.value}%. You can keep chatting.</span>
      ) : (
        <>
          {state.value === 'failed' && <span>The download failed.</span>}
          <button type="button" class="underline" data-testid="memory-download-start" onClick$={fetchIt}>
            {state.value === 'failed' ? 'Download again' : `Download it (${mb} MB)`}
          </button>
        </>
      )}
    </div>
  );
});
