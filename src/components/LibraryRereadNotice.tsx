import { component$, useSignal, $, type QRL } from '@builder.io/qwik';
import { LuFolderOpen, LuLoader2 } from '@qwikest/icons/lucide';
import type { KnowledgeDocument } from '../utils/transcriptMemory';

/**
 * After a restore the library's records are back without their text (the
 * backup carries names, cards, tags and grants, never the passages). This
 * notice shows while any listed document is waiting, and lets the person
 * point at the folder their files live in; matching files are read again
 * into the same records.
 */
export const LibraryRereadNotice = component$<{
  docs: KnowledgeDocument[];
  onDone$: QRL<() => void>;
}>((props) => {
  const busy = useSignal(false);
  const result = useSignal('');
  const waiting = props.docs.filter((d) => d.chunkCount === 0).length;
  if (waiting === 0 && !result.value) return null;

  const reread = $(async () => {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({ directory: true, multiple: true, title: 'Where do these files live?' });
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
      result.value = parts.join(' · ') || 'No matching files in that folder.';
    } catch (e) {
      console.error('[Library] re-read failed:', e);
      result.value = typeof e === 'string' ? e : 'Could not read that folder.';
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
          {waiting === 1 ? 'its' : 'their'} text. Point at the folder the files live in and{' '}
          {waiting === 1 ? 'it' : 'they'} will be read again.
        </p>
      )}
      {result.value && <p class="text-[var(--text-muted)] mt-1">{result.value}</p>}
      {waiting > 0 && (
        <button
          type="button"
          disabled={busy.value}
          onClick$={reread}
          class="mt-1.5 inline-flex items-center gap-1.5 text-[var(--text-link)] hover:underline disabled:opacity-60"
        >
          {busy.value ? <LuLoader2 class="w-3.5 h-3.5 animate-spin" /> : <LuFolderOpen class="w-3.5 h-3.5" />}
          {busy.value ? 'Reading...' : 'Re-read from folder'}
        </button>
      )}
    </div>
  );
});
