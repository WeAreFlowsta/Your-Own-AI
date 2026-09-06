import { component$, $, useSignal, useVisibleTask$, type QRL } from '@builder.io/qwik';
import { LuBookOpen, LuLoader2, LuPlus } from '@qwikest/icons/lucide';
import LiquidMetalButton from './LiquidMetalButton';
import { MemoryComponentOffer } from './MemoryComponentOffer';
import { KnowledgeDocumentRow } from './KnowledgeDocumentRow';
import { useFileDrop } from '../hooks/useFileDrop';
import { useCorpusProgress, progressText } from '../hooks/useCorpusProgress';
import { listKnowledgeDocuments, removeKnowledgeDocument } from '../utils/transcriptMemory';
import { isEmbeddingModelReady } from '../utils/embeddings';
import { pickAndIngestDocuments, ingestDocumentPaths, ingestFailureMessage } from '../utils/knowledgeIngest';

/** Store slice this section reads/writes (a subset of AiFormModal's store). */
interface KnowledgeStore {
  knowledgeDocs: import('../utils/transcriptMemory').KnowledgeDocument[];
  knowledgeBusy: boolean;
  knowledgeError: string;
}

interface KnowledgeSectionProps {
  aiId: string;
  store: KnowledgeStore;
}

type Outcome = { failures: string[]; added: number; already: number; cancelled: boolean } | null;

/**
 * "Knowledge" tab of the edit-AI dialog: the documents this AI may draw on,
 * from the person's library (src/utils/corpus.ts). Adding here means "add to
 * my library and give this AI access"; files or whole folders, from the
 * picker or dropped on the window. Needs the memory component, offered in
 * place when missing.
 */
export const KnowledgeSection = component$<KnowledgeSectionProps>((props) => {
  const ready = useSignal(true);
  const notice = useSignal('');
  const progress = useCorpusProgress();

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    ready.value = await isEmbeddingModelReady();
    props.store.knowledgeDocs = await listKnowledgeDocuments(props.aiId);
  });

  const finish = $(async (picked: Outcome) => {
    if (!picked) return; // cancelled the picker
    props.store.knowledgeDocs = await listKnowledgeDocuments(props.aiId);
    const parts: string[] = [];
    if (picked.added) parts.push(`Added ${picked.added} ${picked.added === 1 ? 'document' : 'documents'}`);
    if (picked.already) parts.push(`${picked.already} already here`);
    if (picked.cancelled) parts.push('stopped early');
    notice.value = parts.join(' · ');
    if (picked.failures.length > 0) props.store.knowledgeError = ingestFailureMessage(picked.failures);
  });

  const run = $(async (work: () => Promise<Outcome>) => {
    props.store.knowledgeError = '';
    notice.value = '';
    if (!ready.value) {
      props.store.knowledgeError = 'Add the memory component above first.';
      return;
    }
    props.store.knowledgeBusy = true;
    let picked: Outcome = null;
    try {
      picked = await work();
    } catch (err) {
      console.error('[Knowledge] add documents failed:', err);
      props.store.knowledgeError = typeof err === 'string' ? err : 'Could not add those documents.';
    } finally {
      props.store.knowledgeBusy = false;
    }
    await finish(picked);
  });

  const addDocuments: QRL<() => void> = $(() => run(() => pickAndIngestDocuments(props.aiId)));
  const onDrop = $((paths: string[]) => run(() => ingestDocumentPaths(props.aiId, paths)));
  const hovering = useFileDrop('knowledge', onDrop);

  const cancel = $(async () => {
    const { corpusCancel } = await import('../utils/corpus');
    await corpusCancel();
  });
  const onReady = $(() => {
    ready.value = true;
    props.store.knowledgeError = '';
  });
  const toggleMine = $(async (docId: string, mine: boolean) => {
    const { corpusSetMine } = await import('../utils/corpus');
    await corpusSetMine(docId, mine);
    props.store.knowledgeDocs = await listKnowledgeDocuments(props.aiId);
  });
  const removeDoc = $(async (docId: string) => {
    await removeKnowledgeDocument(props.aiId, docId);
    props.store.knowledgeDocs = await listKnowledgeDocuments(props.aiId);
  });

  const docs = props.store.knowledgeDocs;

  return (
    <div>
      <div class="flex items-center gap-2 mb-1">
        <LuBookOpen class="w-4 h-4 text-[var(--text-secondary)]" />
        <h3 class="text-sm font-medium text-[var(--text-secondary)]">Knowledge</h3>
      </div>
      <p class="text-xs text-[var(--text-muted)] mb-3">
        Give this AI documents it can always draw on: files, or whole folders
        of them. It reads and remembers them, then uses the relevant parts in
        any conversation. The originals can be moved or deleted after.
      </p>

      <MemoryComponentOffer onReady$={onReady} />

      <div
        class={`rounded-lg border border-dashed px-3 py-3 flex flex-wrap items-center gap-3 transition-colors ${
          hovering.value
            ? 'border-[var(--bg-button-primary)] bg-[var(--bg-dropdown-hover)]'
            : 'border-[var(--border-subtle)]'
        }`}
      >
        <LiquidMetalButton
          onClick$={addDocuments}
          disabled={props.store.knowledgeBusy || !ready.value}
          class="flex items-center gap-2 px-3 py-1.5 text-xs"
        >
          {props.store.knowledgeBusy ? <LuLoader2 class="w-4 h-4 animate-spin" /> : <LuPlus class="w-4 h-4" />}
          {props.store.knowledgeBusy ? 'Adding...' : 'Add documents'}
        </LiquidMetalButton>
        <span class="text-xs text-[var(--text-muted)]">
          {hovering.value ? 'Drop to add' : 'or drop files or folders here'}
        </span>
        {props.store.knowledgeBusy && (
          <button type="button" class="text-xs text-[var(--text-link)] hover:underline ml-auto" onClick$={cancel}>
            Stop
          </button>
        )}
      </div>

      {props.store.knowledgeBusy && progress.value && (
        <p class="text-xs text-[var(--text-muted)] mt-2 truncate">{progressText(progress.value)}</p>
      )}
      {notice.value && <p class="text-xs text-[var(--text-secondary)] mt-2">{notice.value}</p>}
      {props.store.knowledgeError && (
        <p class="text-xs text-red-600 dark:text-red-400 mt-2">{props.store.knowledgeError}</p>
      )}

      {docs.length > 0 && (
        <ul class="mt-3 space-y-1.5">
          {docs.map((doc) => (
            <KnowledgeDocumentRow key={doc.docId} doc={doc} onToggleMine$={toggleMine} onRemove$={removeDoc} />
          ))}
        </ul>
      )}
    </div>
  );
});
