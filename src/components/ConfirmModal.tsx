/**
 * A styled in-app confirmation dialog — replaces the native browser confirm()
 * so destructive actions match the app's look instead of the OS chrome.
 */
import { component$, type QRL } from "@builder.io/qwik";
import { LuTrash2, LuInfo } from "@qwikest/icons/lucide";
import LiquidMetalButton from "./LiquidMetalButton";

interface ConfirmModalProps {
  isOpen: boolean;
  title: string;
  message: string;
  confirmLabel?: string;
  cancelLabel?: string;
  variant?: "danger" | "default";
  /** Action in flight — locks the buttons and shows a spinner. */
  busy?: boolean;
  onConfirm$: QRL<() => void>;
  onCancel$: QRL<() => void>;
}

export default component$<ConfirmModalProps>(
  ({
    isOpen,
    title,
    message,
    confirmLabel = "Confirm",
    cancelLabel = "Cancel",
    variant = "default",
    busy = false,
    onConfirm$,
    onCancel$,
  }) => {
    if (!isOpen) return null;
    const danger = variant === "danger";

    return (
      <div
        class="fixed inset-0 bg-black/60 flex items-center justify-center p-4 z-50"
        onClick$={() => !busy && onCancel$()}
      >
        <div
          class="bg-[var(--bg-header-footer)] p-6 md:p-7 rounded-xl shadow-2xl w-full max-w-md relative"
          onClick$={(e: MouseEvent) => e.stopPropagation()}
        >
          <div class="flex items-start gap-4 mb-6">
            <div
              class={`w-11 h-11 rounded-full flex items-center justify-center flex-shrink-0 ${
                danger
                  ? "bg-red-500/10 text-red-500 dark:text-red-400"
                  : "bg-[var(--bg-dropdown)] text-[var(--text-secondary)]"
              }`}
            >
              {danger ? <LuTrash2 class="w-5 h-5" /> : <LuInfo class="w-5 h-5" />}
            </div>
            <div class="min-w-0 flex-1 pt-0.5">
              <h2 class="text-lg font-semibold text-[var(--text-primary)] font-varela">
                {title}
              </h2>
              <p class="text-sm text-[var(--text-secondary)] mt-1">{message}</p>
            </div>
          </div>

          {/* secondary first in DOM; primary on the right (flex-col-reverse on mobile) */}
          <div class="flex flex-col-reverse sm:flex-row sm:justify-end gap-2">
            <LiquidMetalButton
              variant="secondary"
              onClick$={onCancel$}
              disabled={busy}
              class="px-4 py-2 text-sm"
            >
              {cancelLabel}
            </LiquidMetalButton>
            <LiquidMetalButton
              variant={danger ? "danger" : "primary"}
              onClick$={onConfirm$}
              disabled={busy}
              class="px-4 py-2 text-sm"
            >
              {busy && (
                <span class="w-4 h-4 border-2 border-current border-t-transparent rounded-full animate-spin" />
              )}
              {confirmLabel}
            </LiquidMetalButton>
          </div>
        </div>
      </div>
    );
  },
);
