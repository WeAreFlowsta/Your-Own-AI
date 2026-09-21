import { component$, useSignal, useVisibleTask$, $, type QRL } from '@builder.io/qwik';
import { LuFolderSearch, LuFolderOpen, LuLoader2 } from '@qwikest/icons/lucide';
import type { KnowledgeDocument } from '../utils/transcriptMemory';

function dirOf(path: string): string {
  const cut = Math.max(path.lastIndexOf('/'), path.lastIndexOf('\\'));
  return cut > 0 ? path.slice(0, cut) : path;
}

function plural(n: number, one: string, many: string): string {
  return `${n} ${n === 1 ? one : many}`;
}

/** Below this many, the rows are the whole interface: each one already says
 *  what is wrong and offers Relink and Remove. */
const BULK_FROM = 3;

/**
 * BULK help for documents whose file the app cannot see - and only that.
 * Every such document says so on its own row, with Relink and Remove; this
 * block appears when doing it row by row would be tedious:
 *  - a location that lost 3+ documents gets one "Locate" (a folder that
 *    moved is found again in one step, the way a video editor relinks a bin
 *    of offline clips);
 *  - with 3+ documents needing files: "Find them in a folder" (one pick
 *    fills every record that matches) and "Remove all".
 * With one or two, nothing shows here.
 */
export const DocumentsNeedingFiles = component$<{
  docs: KnowledgeDocument[];
  aiId: string;
  onDone$: QRL<() => void>;
}>((props) => {
  const busy = useSignal('');
  const result = useSignal('');
  const confirmRemove = useSignal(false);
  const needing = props.docs.filter((d) => d.chunkCount === 0 || !!d.offlineSince);

  // First, quietly look where restored records were last seen: the files may
  // simply be there (the restore tried once; this catches files put back).
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ track }) => {
    const waiting = track(() => props.docs.filter((d) => d.chunkCount === 0).length);
    if (waiting === 0) return;
    try {
      const { corpusRelink } = await import('../utils/corpus');
      const r = await corpusRelink();
      if (r.restored) {
        result.value = `Found ${plural(r.restored, 'document', 'documents')} where ${r.restored === 1 ? 'it was' : 'they were'}.`;
        await props.onDone$();
      }
    } catch (e) {
      console.error('[Library] relink failed:', e);
    }
  });

  const groups = new Map<string, KnowledgeDocument[]>();
  for (const d of needing) {
    if (!d.path) continue;
    const where = dirOf(d.path);
    groups.set(where, [...(groups.get(where) ?? []), d]);
  }
  const movedFolders = [...groups.entries()].filter(([, docs]) => docs.length >= BULK_FROM);
  const bulk = needing.length >= BULK_FROM;
  if (!bulk && movedFolders.length === 0 && !result.value) return null;

  const locate = $(async (oldDir: string) => {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({ directory: true, multiple: false, title: 'Where is that folder now?' });
    if (!picked || Array.isArray(picked)) return;
    busy.value = oldDir;
    result.value = '';
    try {
      const { corpusLocate } = await import('../utils/corpus');
      const r = await corpusLocate(oldDir, picked);
      const parts = [
        r.relinked ? `${plural(r.relinked, 'document', 'documents')} linked again` : '',
        r.still_missing ? `${r.still_missing} not in that folder` : '',
        r.failed.length ? `could not read ${r.failed.map((f) => f.file).join(', ')}` : '',
      ].filter(Boolean);
      result.value = parts.join(' · ') || 'Nothing in that folder matched.';
      if (r.relinked) {
        const lib = await import('../utils/documentSummaries');
        void lib.summarizePendingDocuments().then(() => lib.refreshLibraryPortrait());
      }
    } catch (e) {
      result.value = typeof e === 'string' ? e : 'Could not look in that folder.';
    } finally {
      busy.value = '';
      await props.onDone$();
    }
  });

  // One folder, walked: every record that needs its file and matches a file
  // in it (by name and size) is filled.
  const choose = $(async () => {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({ directory: true, multiple: true, title: 'Choose a folder the files may be in' });
    if (!picked) return;
    busy.value = 'choose';
    result.value = '';
    try {
      const { corpusReread } = await import('../utils/corpus');
      const r = await corpusReread(Array.isArray(picked) ? picked : [picked]);
      const parts = [
        r.restored ? `Read ${plural(r.restored, 'document', 'documents')} again` : '',
        r.remaining ? `${r.remaining} still need their file` : '',
        r.failed.length ? `could not read ${r.failed.map((f) => f.file).join(', ')}` : '',
      ].filter(Boolean);
      result.value = parts.join(' · ') || 'None of those matched a document that needs its file.';
    } catch (e) {
      result.value = typeof e === 'string' ? e : 'Could not read those files.';
    } finally {
      busy.value = '';
      await props.onDone$();
    }
  });

  const removeAll = $(async () => {
    busy.value = 'remove';
    try {
      const { removeKnowledgeDocument } = await import('../utils/transcriptMemory');
      const gone = props.docs.filter((d) => d.chunkCount === 0 || !!d.offlineSince);
      for (const d of gone) await removeKnowledgeDocument(props.aiId, d.docId);
      result.value = `Removed ${plural(gone.length, 'document', 'documents')} that had no file.`;
    } catch (e) {
      console.error('[Library] remove failed:', e);
      result.value = 'Could not remove them.';
    } finally {
      busy.value = '';
      confirmRemove.value = false;
      await props.onDone$();
    }
  });

  const link = 'inline-flex items-center gap-1.5 text-[var(--text-link)] hover:underline disabled:opacity-60 cursor-pointer';
  return (
    <div class="mb-3 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-main)] px-3 py-2 text-xs">
      {movedFolders.map(([where, docs]) => (
        <p key={where} class="flex flex-wrap items-center gap-x-3 gap-y-0.5 text-[var(--text-secondary)]">
          <span class="break-all">{docs.length} documents were in {where}</span>
          <button type="button" disabled={busy.value !== ''} onClick$={() => locate(where)} class={link}>
            {busy.value === where ? <LuLoader2 class="w-3.5 h-3.5 animate-spin" /> : <LuFolderSearch class="w-3.5 h-3.5" />}
            {busy.value === where ? 'Looking...' : 'Locate'}
          </button>
        </p>
      ))}
      {bulk && (
        <div class="flex flex-wrap items-center gap-x-3 gap-y-1 text-[var(--text-secondary)]">
          <span>{needing.length} documents need their files</span>
          <button type="button" disabled={busy.value !== ''} onClick$={choose} class={link}>
            {busy.value === 'choose' ? <LuLoader2 class="w-3.5 h-3.5 animate-spin" /> : <LuFolderOpen class="w-3.5 h-3.5" />}
            {busy.value === 'choose' ? 'Looking...' : 'Find them in a folder'}
          </button>
          {!confirmRemove.value ? (
            <button
              type="button"
              disabled={busy.value !== ''}
              onClick$={() => (confirmRemove.value = true)}
              class="text-[var(--text-muted)] hover:text-red-500 hover:underline cursor-pointer"
            >
              Remove all {needing.length}
            </button>
          ) : (
            <span class="inline-flex flex-wrap items-center gap-2">
              Remove these {needing.length} documents from this AI? Their cards go too.
              <button type="button" onClick$={removeAll} class="text-red-600 dark:text-red-400 hover:underline cursor-pointer">Remove</button>
              <button type="button" onClick$={() => (confirmRemove.value = false)} class="text-[var(--text-link)] hover:underline cursor-pointer">Keep</button>
            </span>
          )}
        </div>
      )}
      {result.value && <p class="text-[var(--text-muted)] mt-1">{result.value}</p>}
    </div>
  );
});
