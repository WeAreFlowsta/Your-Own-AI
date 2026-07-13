import { component$ } from '@builder.io/qwik';
import { LuX, LuAlertTriangle } from '@qwikest/icons/lucide';
import LiquidMetalButton from './LiquidMetalButton';

interface LastActiveAiModalProps {
  isOpen: boolean;
  onClose: () => void;
  aiName: string;
  title?: string;
  message?: string;
  detail?: string;
}

export default component$<LastActiveAiModalProps>(({
  isOpen,
  onClose,
  aiName,
  title = 'Cannot Deactivate AI',
  message = 'You must have at least one active AI.',
  detail = 'Please activate another AI before deactivating this one.'
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
          <h2 class="text-2xl font-semibold text-[var(--text-primary)] font-varela">{title}</h2>
          <button
            onClick$={onClose}
            class="p-2 rounded-full text-[var(--text-muted)] hover:bg-[var(--bg-dropdown-hover)] transition-colors"
            aria-label="Close modal"
          >
            <LuX class="w-6 h-6" />
          </button>
        </div>

        <div class="mb-6">
          <div class="flex items-center justify-center mb-4">
            <div class="w-16 h-16 rounded-full bg-orange-100 dark:bg-orange-900/30 flex items-center justify-center">
              <LuAlertTriangle class="w-8 h-8 text-orange-600 dark:text-orange-400" />
            </div>
          </div>

          <p class="text-[var(--text-primary)] text-center mb-3">
            {message}
          </p>

          <p class="text-sm font-medium text-[var(--text-primary)] text-center bg-[var(--bg-dropdown)] px-4 py-2 rounded-lg border border-[var(--border-subtle)]">
            {aiName}
          </p>

          <p class="text-sm text-[var(--text-muted)] text-center mt-3">
            {detail}
          </p>
        </div>

        <div class="flex justify-center">
          <LiquidMetalButton
            onClick$={onClose}
            class="w-full sm:w-auto inline-flex justify-center px-6 py-2.5 text-base font-medium focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-[var(--focus-ring)] transition-colors"
          >
            Got it
          </LiquidMetalButton>
        </div>
      </div>
    </div>
  );
});
