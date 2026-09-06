import { component$, useSignal, $, type QRL } from '@builder.io/qwik';
import { LuFileText, LuFolderOpen, LuLoader2 } from '@qwikest/icons/lucide';
import type { KnowledgeDocument } from '../utils/transcriptMemory';

/**
 * After a restore the library's records are back without their text (the
 * backup carries names, cards, tags and grants, never the passages). This
 * notice shows while any listed document is waiting. Dropping the files
 * on the zone above (from anywhere) reads them back into their records -
 * the import path matches waiting records first; the buttons here open a
 * file or folder picker for the same thing.
 */
export const LibraryRereadNotice = component$<{
  docs: KnowledgeDocument[];
  onDone$: QRL<() => void>;
}>((props) => {
  const busy = useSignal(false);
  const result = useSignal('');
  const waiting = props.docs.filter((d) => d.chunkCount === 0).length;
  if (waiting === 0 && !result.value) return null;

  const reread = $(async (folder?: boolean) => {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open(
      folder
        ? { directory: true, multiple: true, title: 'Choose the folder the files live in' }
        : { directory: false, multiple: true, title: 'Choose the files to read again' }
    );
    if (!picked) return;
    busy.value = true;
    result.value = '';
    try {
      const { corpusReread } = await import('../utils/corpus');
      const r = await corpusReread(Array.isArray(picked) ? picked : [picked]);
      const parts: string[] = [];
      if (r.restored) parts.push(`Read ${r.restored} ${r.restored === 1 ? 'document' : 'documents'} again`);
      if (r.remaining) parts.push(`${r.remaining} still waiting`);
      if (r.failed.length) parts.push(`couldn't read ${r.failed.map((f) => f.file).join(', ')}`);
      if (r.cancelled) parts.push('stopped early');
      result.value = parts.join(' · ') || 'None of those matched a waiting document.';
    } catch (e) {
      console.error('[Library] re-read failed:', e);
      result.value = typeof e === 'string' ? e : 'Could not read those files.';
    } finally {
      busy.value = false;
      await props.onDone$();
    }
  });

  return (
    <div class="mb-3 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-main)] px-3 py-2 text-xs">
      {waiting > 0 && (
        <p class="text-[var(--text-secondary)]">
          {waiting === 1 ? 'One document' : `${waiting} documents`} came back from your backup without{' '}
          {waiting === 1 ? 'its' : 'their'} text. Drop the {waiting === 1 ? 'file' : 'files'} above again, from
          wherever {waiting === 1 ? 'it lives' : 'they live'}, or choose {waiting === 1 ? 'it' : 'them'} here, and{' '}
          {waiting === 1 ? 'it' : 'they'} will be read back into the same {waiting === 1 ? 'record' : 'records'}.
        </p>
      )}
      {result.value && <p class="text-[var(--text-muted)] mt-1">{result.value}</p>}
      {waiting > 0 && (
        <div class="mt-1.5 flex flex-wrap items-center gap-3">
          <button
            type="button"
            disabled={busy.value}
            onClick$={() => reread(false)}
            class="inline-flex items-center gap-1.5 text-[var(--text-link)] hover:underline disabled:opacity-60"
          >
            {busy.value ? <LuLoader2 class="w-3.5 h-3.5 animate-spin" /> : <LuFileText class="w-3.5 h-3.5" />}
            {busy.value ? 'Reading...' : 'Choose files'}
          </button>
          {!busy.value && (
            <button
              type="button"
              onClick$={() => reread(true)}
              class="inline-flex items-center gap-1.5 text-[var(--text-link)] hover:underline"
            >
              <LuFolderOpen class="w-3.5 h-3.5" />
              Choose a folder
            </button>
          )}
        </div>
      )}
    </div>
  );
});
