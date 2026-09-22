import { component$, useSignal, $, type QRL } from '@builder.io/qwik';
import { LuFileText, LuTrash2, LuLink, LuRefreshCw, LuCloud, LuFileQuestion, LuLoader2 } from '@qwikest/icons/lucide';
import type { KnowledgeDocument } from '../utils/transcriptMemory';

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function day(secs: number): string {
  return new Date(secs * 1000).toLocaleDateString(undefined, { day: 'numeric', month: 'short' });
}

/** What the document's link to its file needs the person to know - or null
 *  when it is simply linked. One place, so both lists say the same thing. */
export function linkState(doc: KnowledgeDocument): { kind: 'waiting' | 'offline' | 'online'; text: string } | null {
  if (doc.chunkCount === 0) {
    return { kind: 'waiting', text: 'Needs its file - it came back from a backup without its text, so there is nothing to quote yet.' };
  }
  if (doc.offlineSince) {
    return { kind: 'offline', text: `File not found since ${day(doc.offlineSince)} - still answering from what was read.` };
  }
  if (doc.onlineOnly) {
    return { kind: 'online', text: 'Its file is online only (kept in a cloud drive) - still answering from what was read; changes are not seen until it is on this computer.' };
  }
  return null;
}

/**
 * One document in an AI's knowledge list - shared by the edit-AI dialog's
 * Knowledge tab and the memory page, so the "Mine" tag, the card and the
 * state of its link to the file are maintained once. "Mine" = written by the
 * person: guessed from metadata when there is any, flippable here, and what
 * the summary about them keys on (your own writing vs what you keep).
 */
export const KnowledgeDocumentRow = component$<{
  doc: KnowledgeDocument;
  onToggleMine$: QRL<(docId: string, mine: boolean) => void>;
  onRemove$: QRL<(docId: string) => void>;
  /** The document changed (relinked, read again): the list reloads. */
  onChanged$?: QRL<() => void>;
  /** A tick box for choosing several rows at once (the list owns the choice). */
  selected?: boolean;
  onSelect$?: QRL<(docId: string, on: boolean) => void>;
}>((props) => {
  const doc = props.doc;
  const state = linkState(doc);
  const busy = useSignal(false);
  const note = useSignal('');

  const relink = $(async () => {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({ directory: false, multiple: false, title: `Where is ${doc.filename} now?` });
    if (!picked || Array.isArray(picked)) return;
    busy.value = true;
    note.value = '';
    try {
      const { corpusRelinkOne } = await import('../utils/corpus');
      const readIn = await corpusRelinkOne(doc.docId, picked);
      note.value = readIn ? 'Linked - that file has different words, so it was read in.' : 'Linked - the same file, found again.';
      const lib = await import('../utils/documentSummaries');
      if (readIn) void lib.summarizePendingDocuments().then(() => lib.refreshLibraryPortrait());
    } catch (e) {
      note.value = `${doc.filename} ${typeof e === 'string' ? e : 'could not be linked.'}`;
    } finally {
      busy.value = false;
      await props.onChanged$?.();
    }
  });

  // The file may be back where it was (moved out and back, a drive
  // plugged in): look, without a picker.
  const checkAgain = $(async () => {
    busy.value = true;
    note.value = '';
    try {
      const { corpusCheckOne } = await import('../utils/corpus');
      const readIn = await corpusCheckOne(doc.docId);
      note.value = readIn ? 'Found and read again.' : 'Found - the same words, linked again.';
      if (readIn) {
        const lib = await import('../utils/documentSummaries');
        void lib.summarizePendingDocuments().then(() => lib.refreshLibraryPortrait());
      }
    } catch (e) {
      note.value = `${doc.filename} ${typeof e === 'string' ? e : 'could not be checked.'}`;
    } finally {
      busy.value = false;
      await props.onChanged$?.();
    }
  });

  const readAgain = $(async () => {
    busy.value = true;
    note.value = '';
    try {
      const { corpusReadAgain } = await import('../utils/corpus');
      await corpusReadAgain(doc.docId);
      note.value = 'Read again.';
      const lib = await import('../utils/documentSummaries');
      void lib.summarizePendingDocuments().then(() => lib.refreshLibraryPortrait());
    } catch (e) {
      note.value = `${doc.filename} ${typeof e === 'string' ? e : 'could not be read again.'}`;
    } finally {
      busy.value = false;
      await props.onChanged$?.();
    }
  });

  const action = 'inline-flex items-center gap-1 text-[var(--text-link)] hover:underline disabled:opacity-60 cursor-pointer';
  return (
    <li class={`rounded-lg border bg-[var(--bg-card)] px-3 py-2 group ${props.selected ? 'border-[var(--text-link)]' : 'border-[var(--border-subtle)]'}`}>
      <div class="flex items-center gap-2.5">
        {props.onSelect$ && (
          <input
            type="checkbox"
            checked={!!props.selected}
            onChange$={(_, el) => props.onSelect$!(doc.docId, el.checked)}
            class={`shrink-0 cursor-pointer ${props.selected ? '' : 'opacity-40 group-hover:opacity-100'}`}
            title="Choose this document"
          />
        )}
        {state?.kind === 'online' ? (
          <LuCloud class="w-4 h-4 text-[var(--text-muted)] shrink-0" />
        ) : state ? (
          <LuFileQuestion class="w-4 h-4 text-amber-500 shrink-0" />
        ) : (
          <LuFileText class="w-4 h-4 text-[var(--text-muted)] shrink-0" />
        )}
        <span class="text-sm text-[var(--text-primary)] truncate flex-1" title={doc.path ?? (doc.title ? `${doc.title} (${doc.filename})` : doc.filename)}>
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
            ? formatSize(doc.sizeBytes)
            : `${formatSize(doc.sizeBytes)} · ${doc.chunkCount} ${doc.chunkCount === 1 ? 'piece' : 'pieces'}`}
          {doc.rereadAt && !state ? ` · read again ${day(doc.rereadAt)}` : ''}
        </span>
        {!state && doc.path && (
          <button
            type="button"
            onClick$={readAgain}
            disabled={busy.value}
            title="Read this document again from its file"
            class="text-[var(--text-muted)] hover:text-[var(--text-primary)] transition-colors opacity-0 group-hover:opacity-100 shrink-0 disabled:opacity-60"
          >
            {busy.value ? <LuLoader2 class="w-3.5 h-3.5 animate-spin" /> : <LuRefreshCw class="w-3.5 h-3.5" />}
          </button>
        )}
        <button
          type="button"
          onClick$={() => props.onRemove$(doc.docId)}
          title="Take this document away from this AI"
          class="text-[var(--text-muted)] hover:text-red-500 transition-colors opacity-0 group-hover:opacity-100 shrink-0"
        >
          <LuTrash2 class="w-3.5 h-3.5" />
        </button>
      </div>
      {state && (
        <div class="mt-1 flex flex-wrap items-center gap-x-3 gap-y-1 text-xs">
          <span class={state.kind === 'online' ? 'text-[var(--text-muted)]' : 'text-amber-600 dark:text-amber-400'}>{state.text}</span>
          {state.kind !== 'online' && (
            <>
              <button type="button" onClick$={checkAgain} disabled={busy.value} class={action} title="Look for the file where it was">
                {busy.value ? <LuLoader2 class="w-3.5 h-3.5 animate-spin" /> : <LuRefreshCw class="w-3.5 h-3.5" />}
                Check again
              </button>
              <button type="button" onClick$={relink} disabled={busy.value} class={action}>
                {busy.value ? <LuLoader2 class="w-3.5 h-3.5 animate-spin" /> : <LuLink class="w-3.5 h-3.5" />}
                {busy.value ? 'Reading...' : 'Relink'}
              </button>
              <button type="button" onClick$={() => props.onRemove$(doc.docId)} disabled={busy.value} class="text-[var(--text-muted)] hover:text-red-500 hover:underline cursor-pointer">
                Remove
              </button>
            </>
          )}
        </div>
      )}
      {note.value && <p class="mt-1 text-xs text-[var(--text-secondary)]">{note.value}</p>}
      {doc.summary && (
        <p class="mt-1 text-xs text-[var(--text-muted)] line-clamp-2">{doc.summary}</p>
      )}
    </li>
  );
});
