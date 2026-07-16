import { component$, $, useSignal, useVisibleTask$ } from '@builder.io/qwik';
import { LuFileText, LuTrash2 } from '@qwikest/icons/lucide';
import {
  listKnowledgeDocuments,
  removeKnowledgeDocument,
  type KnowledgeDocument,
} from '../utils/transcriptMemory';

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
 * Documents given to this AI, on the memory page's Knows tab. Read-mostly:
 * lists what the AI has been given (added via the edit-AI dialog's Knowledge
 * section) and lets you remove one. Adding new documents lives in the dialog.
 */
export default component$<AiKnowledgeDocumentsProps>((props) => {
  const docs = useSignal<KnowledgeDocument[]>([]);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ track }) => {
    track(() => props.aiId);
    if (props.aiId) docs.value = await listKnowledgeDocuments(props.aiId);
  });

  const removeDoc = $(async (docId: string) => {
    await removeKnowledgeDocument(props.aiId, docId);
    docs.value = await listKnowledgeDocuments(props.aiId);
  });

  if (docs.value.length === 0) {
    return (
      <p class="text-sm text-[var(--text-muted)]">
        No documents yet. Add them from the "Knowledge" tab when you edit{' '}
        {props.aiName || 'this AI'}.
      </p>
    );
  }

  return (
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
  );
});
