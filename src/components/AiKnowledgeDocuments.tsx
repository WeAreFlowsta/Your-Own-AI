import { component$, $, useSignal, useVisibleTask$ } from '@builder.io/qwik';
import { readThroughWarmup } from '../utils/recordsWarmup';
import { LuFileText, LuTrash2, LuPlus, LuLoader2 } from '@qwikest/icons/lucide';
import LiquidMetalButton from './LiquidMetalButton';
import {
  listKnowledgeDocuments,
  removeKnowledgeDocument,
  type KnowledgeDocument,
} from '../utils/transcriptMemory';
import { pickAndIngestDocuments, ingestDocumentPaths, ingestFailureMessage } from '../utils/knowledgeIngest';
import { isEmbeddingModelReady } from '../utils/embeddings';
import { MemoryComponentOffer } from './MemoryComponentOffer';
import { useFileDrop } from '../hooks/useFileDrop';

interface AiKnowledgeDocumentsProps {
  aiId: string;
  aiName: string;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * Documents given to this AI, on the memory page's Knows tab. Same data as
 * the edit-AI dialog's Knowledge tab (both read the one store), and documents
 * can be added from either place.
 */
export default component$<AiKnowledgeDocumentsProps>((props) => {
  const docs = useSignal<KnowledgeDocument[]>([]);
  const busy = useSignal(false);
  const error = useSignal('');
  const ready = useSignal(true);
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    ready.value = await isEmbeddingModelReady();
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  const warming = useSignal(false);
  useVisibleTask$(async ({ track, cleanup }) => {
    track(() => props.aiId);
    if (!props.aiId) return;
    let alive = true;
    cleanup(() => (alive = false));
    // Same conductor-held key as the rest of knowledge - an early empty
    // is "not yet", not "no documents".
    docs.value = await readThroughWarmup(
      () => listKnowledgeDocuments(props.aiId),
      (w) => (warming.value = w),
      () => alive,
    );
  });

  const finish = $(async (picked: { failures: string[] } | null) => {
    if (!picked) return; // cancelled
    docs.value = await listKnowledgeDocuments(props.aiId);
    if (picked.failures.length > 0) error.value = ingestFailureMessage(picked.failures);
  });

  const addDocuments = $(async () => {
    error.value = '';
    if (!ready.value) {
      error.value = 'Add the memory component above first.';
      return;
    }
    let picked: { failures: string[] } | null = null;
    busy.value = true;
    try {
      picked = await pickAndIngestDocuments(props.aiId);
    } catch (e) {
      console.error('[Knowledge] add documents failed:', e);
    } finally {
      busy.value = false;
    }
    await finish(picked);
  });

  const onDrop = $(async (paths: string[]) => {
    error.value = '';
    if (!ready.value) {
      error.value = 'Add the memory component above first.';
      return;
    }
    busy.value = true;
    let picked: { failures: string[] } | null = null;
    try {
      picked = await ingestDocumentPaths(props.aiId, paths);
    } catch (e) {
      console.error('[Knowledge] drop failed:', e);
    } finally {
      busy.value = false;
    }
    await finish(picked);
  });
  const hovering = useFileDrop('knowledge-page', onDrop);
  const onReady = $(() => {
    ready.value = true;
    error.value = '';
  });

  const removeDoc = $(async (docId: string) => {
    await removeKnowledgeDocument(props.aiId, docId);
    docs.value = await listKnowledgeDocuments(props.aiId);
  });

  const name = props.aiName || 'this AI';

  return (
    <div>
      {/* Section header — same shape as the sibling sections on this tab. */}
      <div class="flex items-center justify-between mb-3">
        <p class="text-sm text-[var(--text-secondary)] flex items-center gap-1.5">
          <LuFileText class="w-3.5 h-3.5 text-[var(--text-muted)] flex-shrink-0" />
          Documents you've given {name} —{' '}
          <span class="text-[var(--text-muted)]">only this AI</span>
        </p>
        <LiquidMetalButton
          onClick$={addDocuments}
          disabled={busy.value || !ready.value}
          class="flex items-center gap-1.5 px-3 py-1.5 text-xs"
        >
          {busy.value ? (
            <LuLoader2 class="w-3.5 h-3.5 animate-spin" />
          ) : (
            <LuPlus class="w-3.5 h-3.5" />
          )}
          {busy.value ? 'Reading...' : 'Add documents'}
        </LiquidMetalButton>
      </div>

      <MemoryComponentOffer aiName={name} onReady$={onReady} />

      {hovering.value && (
        <p class="text-xs text-[var(--text-secondary)] mb-2">Drop to add to {name}'s knowledge</p>
      )}

      {error.value && (
        <p class="text-xs text-red-600 dark:text-red-400 mb-2">{error.value}</p>
      )}

      {warming.value ? (
        <p class="text-sm text-[var(--text-muted)]">
          Your records are warming up - just after launch, its documents take a moment to be ready.
        </p>
      ) : docs.value.length === 0 ? (
        <p class="text-sm text-[var(--text-muted)]">
          No documents yet. Add files here (or in the Knowledge tab when editing{' '}
          {name}) and it will draw on them whenever they're relevant.
        </p>
      ) : (
        <ul class="space-y-1.5">
          {docs.value.map((doc) => (
            <li
              key={doc.docId}
              class="flex items-center gap-2.5 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-card)] px-3 py-2 group"
            >
              <LuFileText class="w-4 h-4 text-[var(--text-muted)] shrink-0" />
              <span class="text-sm text-[var(--text-primary)] truncate flex-1" title={doc.filename}>
                {doc.filename}
              </span>
              <span class="text-[10px] text-[var(--text-muted)] shrink-0">
                {formatSize(doc.sizeBytes)} · {doc.chunkCount} {doc.chunkCount === 1 ? 'piece' : 'pieces'}
              </span>
              <button
                type="button"
                onClick$={() => removeDoc(doc.docId)}
                title="Remove this document"
                class="text-[var(--text-muted)] hover:text-red-500 transition-colors opacity-0 group-hover:opacity-100 shrink-0"
              >
                <LuTrash2 class="w-3.5 h-3.5" />
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
});
