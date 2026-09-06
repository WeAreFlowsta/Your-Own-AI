import { component$, $, useSignal, useVisibleTask$, type QRL } from '@builder.io/qwik';
import { LuLoader2 } from '@qwikest/icons/lucide';
import LiquidMetalButton from './LiquidMetalButton';
import { EMBEDDING_MODEL } from '../data/recommended-models';
import { isEmbeddingModelReady } from '../utils/embeddings';
import { modelManager } from '../utils/modelManager';

/**
 * Knowledge needs the memory component (the small embedding model): a
 * document is split into pieces and each piece is embedded so the AI can
 * pull the right ones into a later conversation. A chat attachment needs
 * none of that - its text rides into that one conversation. So this offer
 * appears exactly where knowledge is added, says in one line what the
 * component does, and downloads it in place. Renders nothing once it is on
 * disk. Field 09-05: the tab used to fail with "the knowledge model may
 * still be downloading" and point at a Settings page.
 */
export const MemoryComponentOffer = component$<{ onReady$?: QRL<() => void> }>((props) => {
  const missing = useSignal(false);
  const downloading = useSignal(false);
  const percent = useSignal(0);
  const error = useSignal('');

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    missing.value = !(await isEmbeddingModelReady());
  });

  const add = $(async () => {
    error.value = '';
    downloading.value = true;
    percent.value = 0;
    try {
      await modelManager.downloadModel(EMBEDDING_MODEL.downloadUrl, EMBEDDING_MODEL.filename, (p) => {
        percent.value = p.percent;
      });
      missing.value = !(await isEmbeddingModelReady());
      if (!missing.value && props.onReady$) await props.onReady$();
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    } finally {
      downloading.value = false;
    }
  });

  if (!missing.value) return null;
  const mb = Math.round(EMBEDDING_MODEL.size * 1000);
  return (
    <div class="rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-card)] p-3 mb-3">
      <p class="text-sm text-[var(--text-primary)]">Knowledge needs the memory component.</p>
      <p class="text-xs text-[var(--text-muted)] mt-1">
        A small file ({mb} MB) that runs on your device. It lets Your AIs find the
        right part of a document when you ask, instead of reading everything every
        time. One download serves all of them. Nothing leaves your machine.
      </p>
      {downloading.value ? (
        <div class="mt-2">
          <div class="w-full h-2 rounded-full bg-[var(--bg-main)] overflow-hidden">
            <div class="h-full bg-[var(--bg-button-primary)] transition-all duration-200" style={{ width: `${percent.value}%` }} />
          </div>
          <p class="text-xs text-[var(--text-muted)] mt-1 flex items-center gap-1.5">
            <LuLoader2 class="w-3.5 h-3.5 animate-spin" /> {percent.value}%
          </p>
        </div>
      ) : (
        <LiquidMetalButton onClick$={add} class="mt-2 px-3 py-1.5 text-xs">
          Add it
        </LiquidMetalButton>
      )}
      {error.value && <p class="text-xs text-red-600 dark:text-red-400 mt-2">{error.value}</p>}
    </div>
  );
});
