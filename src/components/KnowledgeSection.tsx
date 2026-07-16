import { component$, $, useVisibleTask$, type QRL } from '@builder.io/qwik';
import { LuBookOpen, LuFileText, LuTrash2, LuLoader2, LuPlus } from '@qwikest/icons/lucide';
import {
  addDocumentKnowledge,
  listKnowledgeDocuments,
  removeKnowledgeDocument,
} from '../utils/transcriptMemory';

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
    let paths: string[] = [];
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const selected = await open({
        multiple: true,
        filters: [{
          name: 'Documents',
          extensions: [
            'txt','md','csv','json','xml','yaml','yml','toml','log','ini','cfg','conf',
            'pdf','docx','doc','xlsx','xls','ods','odt','rtf','html','htm','sql',
            'py','js','ts','tsx','jsx','rs','go','java','c','cpp','h','cs','rb','php',
          ],
        }],
      });
      if (!selected) return;
      paths = Array.isArray(selected) ? selected : [selected];
    } catch (err) {
      console.error('[Knowledge] File picker error:', err);
      return;
    }

    props.store.knowledgeBusy = true;
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const failures: string[] = [];
      for (const filePath of paths) {
        try {
          const doc = await invoke<{
            filename: string;
            size_bytes: number;
            content: string;
          }>('read_file_for_context', { filePath });
          const result = await addDocumentKnowledge(
            props.aiId,
            doc.filename,
            doc.size_bytes,
            doc.content,
          );
          if (!result) failures.push(doc.filename);
        } catch (e) {
          console.warn('[Knowledge] failed to ingest', filePath, e);
          failures.push(filePath.split(/[/\\]/).pop() || filePath);
        }
      }
      props.store.knowledgeDocs = await listKnowledgeDocuments(props.aiId);
      if (failures.length > 0) {
        props.store.knowledgeError =
          `Couldn't add ${failures.join(', ')}. If you just installed, the knowledge model may still be downloading (Settings - Components).`;
      }
    } finally {
      props.store.knowledgeBusy = false;
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
