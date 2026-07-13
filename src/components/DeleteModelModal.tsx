import { component$ } from '@builder.io/qwik';
import { LuX, LuTrash2 } from '@qwikest/icons/lucide';
import LiquidMetalButton from './LiquidMetalButton';

interface DeleteModelModalProps {
  isOpen: boolean;
  onClose: () => void;
  onConfirm: () => void;
  modelName: string;
  isDeleting?: boolean;
}

export default component$<DeleteModelModalProps>(({
  isOpen,
  onClose,
  onConfirm,
  modelName,
  isDeleting = false
}) => {
  if (!isOpen) return null;

  return (
    <div
      class="fixed inset-0 bg-black/60 flex items-center justify-center p-4 z-50 transition-opacity duration-300 ease-in-out"
      onClick$={onClose}
    >
      <div
        class="bg-[var(--bg-header-footer)] p-6 md:p-8 rounded-xl shadow-2xl w-full max-w-md transform transition-all duration-300 ease-in-out scale-95 relative"
        onClick$={(e: MouseEvent) => e.stopPropagation()}
      >
        <div class="flex justify-between items-center mb-6">
          <h2 class="text-2xl font-semibold text-[var(--text-primary)] font-varela">Delete Model</h2>
          <button
            onClick$={onClose}
            class="p-2 rounded-full text-[var(--text-muted)] hover:bg-[var(--bg-dropdown-hover)] transition-colors"
            aria-label="Close modal"
            disabled={isDeleting}
          >
            <LuX class="w-6 h-6" />
          </button>
        </div>

        <div class="mb-6">
          <div class="flex items-center justify-center mb-4">
            <div class="w-16 h-16 rounded-full bg-red-100 dark:bg-red-900/30 flex items-center justify-center">
              <LuTrash2 class="w-8 h-8 text-red-600 dark:text-red-400" />
            </div>
          </div>

          <p class="text-[var(--text-primary)] text-center mb-2">
            Are you sure you want to delete this model?
          </p>

          <p class="text-sm font-medium text-[var(--text-primary)] text-center bg-[var(--bg-dropdown)] px-4 py-2 rounded-lg border border-[var(--border-subtle)]">
            {modelName}
          </p>
        </div>

        <div class="flex flex-col-reverse sm:flex-row sm:justify-end sm:space-x-3">
          <LiquidMetalButton
            variant="secondary"
            onClick$={onClose}
            class="mt-3 sm:mt-0 w-full sm:w-auto inline-flex justify-center px-6 py-2.5 text-base font-medium focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-[var(--focus-ring)] transition-colors disabled:opacity-70"
            disabled={isDeleting}
          >
            Cancel
          </LiquidMetalButton>
          <LiquidMetalButton
            variant="danger"
            onClick$={() => onConfirm()}
            class="w-full sm:w-auto inline-flex justify-center items-center px-6 py-2.5 text-base font-medium focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-red-500 transition-colors disabled:opacity-70"
            disabled={isDeleting}
          >
            {isDeleting ? (
              <>
                <div class="w-5 h-5 border-2 border-white border-t-transparent rounded-full animate-spin mr-2" />
                Deleting...
              </>
            ) : (
              <>
                <LuTrash2 class="w-[18px] h-[18px] mr-2" />
                Delete Model
              </>
            )}
          </LiquidMetalButton>
        </div>
      </div>
    </div>
  );
});
