/**
 * Where the app's big downloads live - chat models, vision projectors,
 * speed-up drafts, engine backend packs, OCR models, the Build agent - and
 * the control to move them to another drive. Embedded on the offline models
 * page and in Settings > Components so the setting is findable from both.
 *
 * The backend command does the careful part: refuses mid-download, stops a
 * loaded model first, moves with rollback, emits "models-move" progress.
 */
import { component$, useStore, useVisibleTask$, $ } from '@builder.io/qwik';
import LiquidMetalButton from './LiquidMetalButton';

interface Props {
  /** Also show the folder path itself (the models page already shows it). */
  showPath?: boolean;
}

export default component$<Props>((props) => {
  const store = useStore({
    dir: '',
    freeBytes: 0,
    modelsBytes: 0,
    moving: null as null | { done: number; total: number; bytesDone: number; bytesTotal: number },
    error: '',
  });

  const refresh = $(async () => {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const info = await invoke<{ dir: string; free_bytes: number; models_bytes: number }>(
        'models_disk_info'
      );
      store.dir = info.dir;
      store.freeBytes = info.free_bytes;
      store.modelsBytes = info.models_bytes;
    } catch (e) {
      // The path must still show even when the disk numbers don't.
      store.error = `Couldn't read the drive numbers: ${String(e)}`;
      try {
        const { invoke } = await import('@tauri-apps/api/core');
        store.dir = await invoke<string>('get_models_directory');
      } catch {
        /* nothing else to fall back to */
      }
    }
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    await refresh();
    // A finished download changes the numbers; keep them honest.
    const { listen } = await import('@tauri-apps/api/event');
    const un = await listen('model-download-complete', () => refresh());
    cleanup(() => un());
  });

  const change$ = $(async () => {
    store.error = '';
    try {
      const { open } = await import('@tauri-apps/plugin-dialog');
      const picked = await open({
        directory: true,
        multiple: false,
        title: 'Where should models be stored?',
      });
      if (typeof picked !== 'string' || !picked) return;
      const { invoke } = await import('@tauri-apps/api/core');
      const { listen } = await import('@tauri-apps/api/event');
      store.moving = { done: 0, total: 0, bytesDone: 0, bytesTotal: 0 };
      const un = await listen<{ done: number; total: number; bytes_done: number; bytes_total: number }>(
        'models-move',
        (e) => {
          store.moving = {
            done: e.payload.done,
            total: e.payload.total,
            bytesDone: e.payload.bytes_done,
            bytesTotal: e.payload.bytes_total,
          };
        }
      );
      try {
        await invoke('set_models_directory', { path: picked });
      } finally {
        un();
        store.moving = null;
        await refresh();
      }
    } catch (error) {
      store.error = String(error);
      store.moving = null;
    }
  });

  return (
    <div class="flex flex-col gap-1">
      {props.showPath && store.dir && (
        <code class="text-xs bg-[var(--bg-dropdown)] px-2 py-1 rounded border border-[var(--border-subtle)] break-all">
          {store.dir}
        </code>
      )}
      <div class="flex items-center justify-between gap-2 pt-1">
        <span class="text-xs text-[var(--text-muted)]">
          {(store.modelsBytes / 1e9).toFixed(1)} GB stored · {(store.freeBytes / 1e9).toFixed(1)} GB
          free on this drive
        </span>
        {store.moving ? (
          <span class="text-xs text-[var(--text-secondary)]">
            Moving - {Math.min(store.moving.done + 1, Math.max(store.moving.total, 1))} of{' '}
            {store.moving.total}
            {store.moving.bytesTotal > 0
              ? ` (${(store.moving.bytesDone / 1e9).toFixed(1)} of ${(store.moving.bytesTotal / 1e9).toFixed(1)} GB)`
              : ''}
            ...
          </span>
        ) : (
          <LiquidMetalButton
            variant="secondary"
            class="px-3 py-1.5 text-xs shrink-0"
            onClick$={change$}
            title="Pick a folder on any drive - models and other large components move there and future downloads follow. They live in a 'Your Own AI models' folder inside it."
          >
            Change...
          </LiquidMetalButton>
        )}
      </div>
      {store.error && <p class="text-xs text-red-400 break-all">{store.error}</p>}
    </div>
  );
});
