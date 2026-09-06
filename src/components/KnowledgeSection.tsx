import { component$, $, useSignal, useVisibleTask$, type QRL } from '@builder.io/qwik';
import { LuBookOpen, LuLoader2, LuPlus, LuUpload } from '@qwikest/icons/lucide';
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
  useVisibleTask$(async ({ cleanup }) => {
    ready.value = await isEmbeddingModelReady();
    props.store.knowledgeDocs = await listKnowledgeDocuments(props.aiId);
    // Cards are written on the device in the background; rows refresh as
    // each one lands.
    const lib = await import('../utils/documentSummaries');
    cleanup(lib.onDocumentSummary(async () => {
      props.store.knowledgeDocs = await listKnowledgeDocuments(props.aiId);
    }));
    void lib.summarizePendingDocuments().then(() => lib.refreshLibraryPortrait());
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
    if (picked.added) {
      const lib = await import('../utils/documentSummaries');
      void lib.summarizePendingDocuments().then(() => lib.refreshLibraryPortrait());
    }
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

      {/* The drop zone is the surface, not a footnote: icon, one line that
          says drop, the file types, and the picker button inside it. */}
      <div
        class={`rounded-lg border-2 border-dashed px-4 py-5 flex flex-col items-center text-center gap-1.5 transition-colors ${
          hovering.value
            ? 'border-[var(--bg-button-primary)] bg-[var(--bg-dropdown-hover)]'
            : 'border-[var(--border-subtle)] bg-[var(--bg-main)]'
        }`}
      >
        <LuUpload class={`w-5 h-5 ${hovering.value ? 'text-[var(--text-primary)]' : 'text-[var(--text-muted)]'}`} />
        <p class="text-sm text-[var(--text-secondary)]">
          {hovering.value ? 'Drop to add' : 'Drop files or folders here'}
        </p>
        <p class="text-[11px] text-[var(--text-muted)]">
          PDF, Word, EPUB, text, spreadsheets and code. A folder is read through.
        </p>
        <div class="flex items-center gap-3 mt-1.5">
          <LiquidMetalButton
            onClick$={addDocuments}
            disabled={props.store.knowledgeBusy || !ready.value}
            class="flex items-center gap-2 px-3 py-1.5 text-xs"
          >
            {props.store.knowledgeBusy ? <LuLoader2 class="w-4 h-4 animate-spin" /> : <LuPlus class="w-4 h-4" />}
            {props.store.knowledgeBusy ? 'Adding...' : 'Choose files'}
          </LiquidMetalButton>
          {props.store.knowledgeBusy && (
            <button type="button" class="text-xs text-[var(--text-link)] hover:underline" onClick$={cancel}>
              Stop
            </button>
          )}
        </div>
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
