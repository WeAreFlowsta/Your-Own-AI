import { component$ } from "@builder.io/qwik";
import { useVisionDownload } from "../contexts/VisionDownloadContext";
import { modelManager } from "../utils/modelManager";

/**
 * App-wide chip showing the vision-model download. Rendered in the root layout so
 * it stays visible on every page while a multi-GB download runs in the background.
 */
export const VisionDownloadIndicator = component$(() => {
  const dl = useVisionDownload();
  const a = dl.state.active;
  if (!a) return null;

  const file = a.files[a.currentIndex];

  return (
    <div class="fixed bottom-4 right-4 z-[60] w-72 max-w-[calc(100vw-2rem)] rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] shadow-lg p-3">
      {a.status === "downloading" && (
        <>
          <div class="flex items-center gap-2 mb-2">
            <span class="inline-block w-2 h-2 rounded-full bg-[var(--bg-button-primary)] animate-pulse" />
            <span class="text-sm font-medium text-[var(--text-primary)]">
              Downloading vision model
            </span>
          </div>
          <p class="text-xs text-[var(--text-muted)] mb-2 truncate">
            {file?.label}
            {a.files.length > 1 ? ` (${a.currentIndex + 1} of ${a.files.length})` : ""}
          </p>
          <div class="w-full h-2 rounded-full bg-[var(--bg-main)] overflow-hidden">
            <div
              class="h-full bg-[var(--bg-button-primary)] transition-all duration-200"
              style={{ width: `${a.percent}%` }}
            />
          </div>
          <p class="text-xs text-[var(--text-muted)] mt-1">
            {a.percent}%
            {a.total
              ? ` · ${modelManager.formatModelSize(a.downloaded)} / ${modelManager.formatModelSize(a.total)}`
              : ""}
          </p>
        </>
      )}

      {a.status === "done" && (
        <div class="flex items-center justify-between gap-2">
          <span class="text-sm text-[var(--text-primary)]">
            ✓ Vision model ready
          </span>
          <button
            onClick$={dl.dismiss$}
            class="text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)]"
          >
            Dismiss
          </button>
        </div>
      )}

      {a.status === "error" && (
        <>
          <p class="text-sm text-red-600 dark:text-red-400 mb-2">
            Vision download failed
          </p>
          <p class="text-xs text-[var(--text-muted)] mb-2 break-words">{a.error}</p>
          <div class="flex items-center gap-3">
            <button
              onClick$={dl.retry$}
              class="text-sm text-[var(--text-link)] hover:underline"
            >
              Try again
            </button>
            <button
              onClick$={dl.dismiss$}
              class="text-sm text-[var(--text-muted)] hover:text-[var(--text-secondary)]"
            >
              Dismiss
            </button>
          </div>
        </>
      )}
    </div>
  );
});
