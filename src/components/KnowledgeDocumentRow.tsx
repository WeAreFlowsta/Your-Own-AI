import { component$, type QRL } from '@builder.io/qwik';
import { LuFileText, LuTrash2 } from '@qwikest/icons/lucide';
import type { KnowledgeDocument } from '../utils/transcriptMemory';

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/**
 * One document in an AI's knowledge list - shared by the edit-AI dialog's
 * Knowledge tab and the memory page, so the "Mine" tag and the summary are
 * maintained once. "Mine" = written by the person: guessed from metadata
 * when there is any, flippable here, and what the summary about them keys
 * on (your own writing vs what you keep).
 */
export const KnowledgeDocumentRow = component$<{
  doc: KnowledgeDocument;
  onToggleMine$: QRL<(docId: string, mine: boolean) => void>;
  onRemove$: QRL<(docId: string) => void>;
}>((props) => {
  const doc = props.doc;
  return (
    <li class="rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-card)] px-3 py-2 group">
      <div class="flex items-center gap-2.5">
        <LuFileText class="w-4 h-4 text-[var(--text-muted)] shrink-0" />
        <span class="text-sm text-[var(--text-primary)] truncate flex-1" title={doc.title ? `${doc.title} (${doc.filename})` : doc.filename}>
          {doc.filename}
          {doc.author && <span class="text-[var(--text-muted)]"> · by {doc.author}</span>}
        </span>
        <button
          type="button"
          onClick$={() => props.onToggleMine$(doc.docId, !doc.mine)}
          title={doc.mine ? 'Written by you. Click to change.' : 'Something you keep. Click if you wrote it.'}
          class={`text-[10px] px-1.5 py-0.5 rounded-full border shrink-0 transition-colors ${
            doc.mine
              ? 'border-[var(--bg-button-primary)] text-[var(--text-primary)]'
              : 'border-[var(--border-subtle)] text-[var(--text-muted)] opacity-0 group-hover:opacity-100'
          }`}
        >
          Mine
        </button>
        <span class="text-[10px] text-[var(--text-muted)] shrink-0">
          {doc.chunkCount === 0
            ? 'waiting for its file'
            : `${formatSize(doc.sizeBytes)} · ${doc.chunkCount} ${doc.chunkCount === 1 ? 'piece' : 'pieces'}`}
        </span>
        <button
          type="button"
          onClick$={() => props.onRemove$(doc.docId)}
          title="Take this document away from this AI"
          class="text-[var(--text-muted)] hover:text-red-500 transition-colors opacity-0 group-hover:opacity-100 shrink-0"
        >
          <LuTrash2 class="w-3.5 h-3.5" />
        </button>
      </div>
      {doc.summary && (
        <p class="mt-1 text-xs text-[var(--text-muted)] line-clamp-2">{doc.summary}</p>
      )}
    </li>
  );
});
