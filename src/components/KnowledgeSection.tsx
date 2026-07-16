import { component$, $, useVisibleTask$, type QRL } from '@builder.io/qwik';
import { LuBookOpen, LuFileText, LuTrash2, LuLoader2, LuPlus } from '@qwikest/icons/lucide';
import {
  listKnowledgeDocuments,
  removeKnowledgeDocument,
} from '../utils/transcriptMemory';
import { pickAndIngestDocuments, ingestFailureMessage } from '../utils/knowledgeIngest';

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

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * "Knowledge" tab of the edit-AI dialog: documents this AI is given, ingested
 * as retrievable knowledge (chunked + embedded, same store as authored lore).
 * The AI pulls the relevant pieces into any conversation - no context blown,
 * and the source file can be deleted or moved afterward.
 */
export const KnowledgeSection = component$<KnowledgeSectionProps>((props) => {
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    props.store.knowledgeDocs = await listKnowledgeDocuments(props.aiId);
  });

  const addDocuments: QRL<() => void> = $(async () => {
    props.store.knowledgeError = '';
    let picked: { failures: string[] } | null = null;
    props.store.knowledgeBusy = true;
    try {
      picked = await pickAndIngestDocuments(props.aiId);
    } catch (err) {
      console.error('[Knowledge] add documents failed:', err);
    } finally {
      props.store.knowledgeBusy = false;
    }
    if (!picked) return; // cancelled
    props.store.knowledgeDocs = await listKnowledgeDocuments(props.aiId);
    if (picked.failures.length > 0) {
      props.store.knowledgeError = ingestFailureMessage(picked.failures);
    }
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
        Give this AI documents it can always draw on. It reads and remembers
        them, then uses the relevant parts in any conversation - so you can ask
        about them anytime, and the original files can be moved or deleted after.
      </p>

      <button
        type="button"
        onClick$={addDocuments}
        disabled={props.store.knowledgeBusy}
        class="flex items-center gap-2 px-3 py-2 rounded-lg text-sm border border-[var(--border-subtle)] bg-[var(--bg-input)] text-[var(--text-primary)] hover:bg-[var(--bg-dropdown-hover)] transition-colors disabled:opacity-60"
      >
        {props.store.knowledgeBusy ? (
          <LuLoader2 class="w-4 h-4 animate-spin" />
        ) : (
          <LuPlus class="w-4 h-4" />
        )}
        {props.store.knowledgeBusy ? 'Reading documents...' : 'Add documents'}
      </button>

      {props.store.knowledgeError && (
        <p class="text-xs text-red-600 dark:text-red-400 mt-2">{props.store.knowledgeError}</p>
      )}

      {docs.length > 0 && (
        <ul class="mt-3 space-y-1.5">
          {docs.map((doc) => (
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
