import { component$, type QRL } from '@builder.io/qwik';
import { LuX, LuArchive, LuTrash2 } from '@qwikest/icons/lucide';

interface DeleteAiModalProps {
  isOpen: boolean;
  onClose$: QRL<() => void>;
  onArchive$: QRL<() => void>;
  onPurge$: QRL<() => void>;
  aiName: string;
  /** Which action is in flight, so we can show a spinner + lock both. */
  busyAction?: 'archive' | 'purge' | null;
}

export default component$<DeleteAiModalProps>(({
  isOpen,
  onClose$,
  onArchive$,
  onPurge$,
  aiName,
  busyAction = null,
}) => {
  if (!isOpen) return null;

  const busy = busyAction !== null;

  return (
    <div
      class="fixed inset-0 bg-black/60 flex items-center justify-center p-4 z-50 transition-opacity duration-300 ease-in-out"
      onClick$={onClose$}
    >
      <div
        class="bg-[var(--bg-header-footer)] p-6 md:p-8 rounded-xl shadow-2xl w-full max-w-md transform transition-all duration-300 ease-in-out scale-95 relative"
        onClick$={(e: MouseEvent) => e.stopPropagation()}
      >
        <div class="flex justify-between items-center mb-5">
          <h2 class="text-2xl font-semibold text-[var(--text-primary)] font-varela">Remove {aiName}?</h2>
          <button
            onClick$={onClose$}
            class="p-2 rounded-full text-[var(--text-muted)] hover:bg-[var(--bg-dropdown-hover)] transition-colors disabled:opacity-50"
            aria-label="Close modal"
            disabled={busy}
          >
            <LuX class="w-6 h-6" />
          </button>
        </div>

        <p class="text-sm text-[var(--text-secondary)] mb-5">
          Choose how to remove this AI. Archiving is reversible; deleting is not.
        </p>

        <div class="space-y-3">
          {/* Archive — the safe, recommended choice */}
          <button
            onClick$={() => onArchive$()}
            disabled={busy}
            class="w-full text-left p-4 rounded-xl border border-[var(--border-subtle)] hover:border-[var(--bg-button-primary)] hover:bg-[var(--bg-dropdown-hover)] transition-colors disabled:opacity-60 disabled:cursor-not-allowed flex items-start gap-3"
          >
            <div class="w-9 h-9 rounded-lg bg-[var(--bg-dropdown)] flex items-center justify-center flex-shrink-0">
              {busyAction === 'archive' ? (
                <div class="w-4 h-4 border-2 border-[var(--text-secondary)] border-t-transparent rounded-full animate-spin" />
              ) : (
                <LuArchive class="w-5 h-5 text-[var(--text-secondary)]" />
              )}
            </div>
            <div class="min-w-0">
              <p class="text-sm font-semibold text-[var(--text-primary)]">Archive</p>
              <p class="text-xs text-[var(--text-muted)] mt-0.5">
                Hide it from your list but keep its conversation history and
                what it learned. Restore it anytime.
              </p>
            </div>
          </button>

          {/* Purge — irreversible */}
          <button
            onClick$={() => onPurge$()}
            disabled={busy}
            class="w-full text-left p-4 rounded-xl border border-[var(--border-subtle)] hover:border-red-500/50 hover:bg-red-500/5 transition-colors disabled:opacity-60 disabled:cursor-not-allowed flex items-start gap-3"
          >
            <div class="w-9 h-9 rounded-lg bg-red-500/10 flex items-center justify-center flex-shrink-0">
              {busyAction === 'purge' ? (
                <div class="w-4 h-4 border-2 border-red-400 border-t-transparent rounded-full animate-spin" />
              ) : (
                <LuTrash2 class="w-5 h-5 text-red-500 dark:text-red-400" />
              )}
            </div>
            <div class="min-w-0">
              <p class="text-sm font-semibold text-red-600 dark:text-red-400">Delete permanently</p>
              <p class="text-xs text-[var(--text-muted)] mt-0.5">
                Erase {aiName}, its signed conversations, and everything it
                learned about you. This can't be undone.
              </p>
            </div>
          </button>
        </div>

        <div class="mt-5 flex justify-center">
          <button
            onClick$={onClose$}
            disabled={busy}
            class="px-6 py-2 text-sm font-medium text-[var(--text-secondary)] hover:text-[var(--text-primary)] transition-colors disabled:opacity-50"
          >
            Cancel
          </button>
        </div>
      </div>
    </div>
  );
});
