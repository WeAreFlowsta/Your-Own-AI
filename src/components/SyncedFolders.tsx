import { component$, useSignal, useVisibleTask$, $, type QRL } from '@builder.io/qwik';
import { LuFolderSync, LuLoader2, LuRefreshCw } from '@qwikest/icons/lucide';
import { Callout } from './Callout';
import type { SyncedFolder, FolderSyncReport } from '../utils/corpus';
import { useLibraryChanged } from '../hooks/useCorpusProgress';

interface SyncedFoldersProps {
  aiId: string;
  /** The memory component is on this computer (embedding can run). */
  ready: boolean;
  /** Documents changed - the parent re-reads its list. */
  onChanged$: QRL<() => void>;
}

function leaf(path: string): string {
  return path.replace(/[\\/]+$/, '').split(/[\\/]/).pop() || path;
}

function when(secs: number | null): string {
  if (!secs) return 'not read yet';
  const mins = Math.max(0, Math.round((Date.now() / 1000 - secs) / 60));
  if (mins < 1) return 'checked just now';
  if (mins < 60) return `checked ${mins} min ago`;
  const hours = Math.round(mins / 60);
  return hours < 24 ? `checked ${hours} h ago` : `checked ${Math.round(hours / 24)} d ago`;
}

function said(r: FolderSyncReport): string {
  if (r.unreachable) return `${leaf(r.folder)} could not be read - is the drive connected? Nothing was changed.`;
  const parts = [
    r.added ? `${r.added} new` : '',
    r.updated ? `${r.updated} changed` : '',
    r.removed ? `${r.removed} gone` : '',
  ].filter(Boolean);
  const base = parts.length ? parts.join(', ') : 'nothing new';
  const failed = r.failed.length ? ` - ${r.failed.length} could not be read` : '';
  // Said, never silent: what was left alone and why.
  const cloud = r.online_only
    ? ` ${r.online_only} ${r.online_only === 1 ? 'file is' : 'files are'} online only (kept in a cloud drive) and ${r.online_only === 1 ? 'was' : 'were'} not read.`
    : '';
  // Neutral on purpose: a document from before origins were kept is also
  // held back, and nobody "added it themselves".
  const offline = r.offline
    ? ` ${r.offline} ${r.offline === 1 ? "document's file is" : "documents' files are"} gone - kept, and still answering. Relink or remove ${r.offline === 1 ? 'it' : 'them'} in the list below.`
    : '';
  return `${leaf(r.folder)}: ${base}${failed}${r.cancelled ? ' (stopped)' : ''}.${cloud}${offline}`;
}

/**
 * Folders this AI keeps in sync: a notes vault (Logseq, Obsidian) or any
 * folder of documents. New files are read, edited ones are read again,
 * deleted ones leave the library - checked a few minutes after launch, every
 * half hour, and on "Check now".
 */
export const SyncedFolders = component$<SyncedFoldersProps>((props) => {
  const folders = useSignal<SyncedFolder[]>([]);
  const busy = useSignal<string | null>(null);
  const note = useSignal('');

  const load = $(async () => {
    const { corpusFolders } = await import('../utils/corpus');
    try {
      folders.value = (await corpusFolders()).filter((f) => f.meta.ai_ids.includes(props.aiId));
    } catch {
      folders.value = [];
    }
  });

  // A folder dropped on the documents section registers itself there: follow
  // every library change, not only this block's own buttons.
  useLibraryChanged(load);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ track, cleanup }) => {
    track(() => props.aiId);
    await load();
    const { listen } = await import('@tauri-apps/api/event');
    const un = await listen('corpus-folders-synced', async () => {
      await load();
      await props.onChanged$();
    });
    cleanup(() => un());
  });

  const run = $(async (folderId: string) => {
    const { corpusFolderSync } = await import('../utils/corpus');
    busy.value = folderId;
    note.value = '';
    try {
      const reports = await corpusFolderSync(folderId);
      note.value = reports.length ? reports.map(said).join(' ') : 'A check is already running.';
      // New and changed documents get their cards, like an import's.
      if (reports.some((r) => r.added + r.updated > 0)) {
        const lib = await import('../utils/documentSummaries');
        void lib.summarizePendingDocuments().then(() => lib.refreshLibraryPortrait());
      }
    } catch (e) {
      note.value = `The folder could not be checked: ${String(e)}`;
    } finally {
      busy.value = null;
    }
    await load();
    await props.onChanged$();
  });

  const add = $(async () => {
    const { open } = await import('@tauri-apps/plugin-dialog');
    const picked = await open({ directory: true, multiple: false });
    if (!picked || Array.isArray(picked)) return;
    const { corpusFolderAdd } = await import('../utils/corpus');
    try {
      const id = await corpusFolderAdd(picked, props.aiId);
      await load();
      await run(id);
    } catch (e) {
      note.value = String(e);
    }
  });

  const stop = $(async (folderId: string) => {
    const { corpusFolderRemove } = await import('../utils/corpus');
    await corpusFolderRemove(folderId, false);
    note.value = 'No longer kept in sync. The documents already read stay in the library.';
    await load();
  });

  return (
    <div class="mb-3">
      <div class="flex items-center justify-between gap-3 mb-1.5">
        <p class="text-xs text-[var(--text-muted)]">
          Folders kept in sync - a notes folder (Logseq, Obsidian) or any folder: new files are read, edited ones read
          again, deleted ones leave.
        </p>
        <button
          type="button"
          onClick$={add}
          disabled={!props.ready || busy.value !== null}
          class="flex shrink-0 items-center gap-1.5 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)] disabled:opacity-50 cursor-pointer"
        >
          <LuFolderSync class="w-3.5 h-3.5" /> Keep a folder in sync
        </button>
      </div>
      {folders.value.map((f) => (
        <div
          key={f.folder_id}
          class="flex items-center gap-3 py-1.5 px-3 mb-1 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-card)] text-sm"
        >
          <LuFolderSync class="w-4 h-4 shrink-0 text-[var(--text-muted)]" />
          <div class="min-w-0 flex-1">
            <p class="truncate text-[var(--text-primary)]" title={f.meta.path}>{leaf(f.meta.path)}</p>
            <p class="text-xs text-[var(--text-muted)]">
              {f.reachable
                ? `${f.documents} document${f.documents === 1 ? '' : 's'} - ${when(f.last_scan_at)}`
                : 'Cannot be read right now - nothing is removed while it is away'}
            </p>
          </div>
          <button
            type="button"
            onClick$={() => run(f.folder_id)}
            disabled={busy.value !== null || !props.ready}
            class="flex shrink-0 items-center gap-1 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)] disabled:opacity-50 cursor-pointer"
          >
            {busy.value === f.folder_id ? (
              <LuLoader2 class="w-3.5 h-3.5 animate-spin" />
            ) : (
              <LuRefreshCw class="w-3.5 h-3.5" />
            )}
            Check now
          </button>
          <button
            type="button"
            onClick$={() => stop(f.folder_id)}
            disabled={busy.value !== null}
            class="shrink-0 text-xs text-[var(--text-muted)] hover:text-[var(--text-primary)] disabled:opacity-50 cursor-pointer"
          >
            Stop syncing
          </button>
        </div>
      ))}
      {note.value && (
        <Callout intent="info">
          <p class="text-xs">{note.value}</p>
        </Callout>
      )}
    </div>
  );
});
