import { component$, useStore, useSignal, useTask$, $, type QRL } from '@builder.io/qwik';
import { invoke } from '@tauri-apps/api/core';
import { LuX, LuCheck } from '@qwikest/icons/lucide';
import LiquidMetalButton from './LiquidMetalButton';
import type { ExportOptions } from '../utils/exportConversation';

const ALWAYS_INCLUDED = [
  'The full conversation and timestamps',
  "Each entry's hash and the AI's identity — the integrity anchors",
  'Grounded sources — claims tied to their quotes and document hashes',
  'Web sources and attachment hashes',
];

interface ExportConversationModalProps {
  isOpen: boolean;
  /** For context in the header. */
  conversationTitle: string;
  onClose$: QRL<() => void>;
  onExport$: QRL<(opts: ExportOptions, sign: boolean) => void>;
}

/**
 * "Export options" dialog shown when exporting a conversation. The receipt always
 * includes the conversation, timestamps, hashes, and grounded sources; these
 * extras can reveal more than you want to share, so they're opt-in (and reset to
 * off each time the dialog opens). The Sign It option publishes the exported
 * file's signature to the Sign It network via the user's Vault (approval
 * happens there), so anyone can verify the file - it needs the Vault unlocked
 * and uses one signature from the user's plan.
 */
export const ExportConversationModal = component$<ExportConversationModalProps>((props) => {
  const opts = useStore<ExportOptions>({
    includeSystemPrompt: false,
    includeAttachmentContent: false,
    includeThinking: false,
  });
  const signWithSignIt = useSignal(false);
  /** null = still checking; the checkbox enables only on a running, unlocked Vault. */
  const vaultReady = useSignal<boolean | null>(null);

  // Reset to the safe defaults whenever the dialog opens, and probe the Vault
  // (isOpen only turns true in the browser, so the invoke never runs in SSG).
  useTask$(({ track }) => {
    if (track(() => props.isOpen)) {
      opts.includeSystemPrompt = false;
      opts.includeAttachmentContent = false;
      opts.includeThinking = false;
      signWithSignIt.value = false;
      vaultReady.value = null;
      invoke<{ installed: boolean; unlocked: boolean }>('flowsta_vault_status')
        .then((s) => (vaultReady.value = !!s.unlocked))
        .catch(() => (vaultReady.value = false));
    }
  });

  const submit$ = $(() => {
    props.onExport$(
      {
        includeSystemPrompt: opts.includeSystemPrompt,
        includeAttachmentContent: opts.includeAttachmentContent,
        includeThinking: opts.includeThinking,
      },
      signWithSignIt.value,
    );
  });

  if (!props.isOpen) return null;

  return (
    <div
      class="fixed inset-0 bg-black/60 flex items-center justify-center p-4 z-50"
      onClick$={props.onClose$}
    >
      <div
        class="bg-[var(--bg-header-footer)] p-6 md:p-8 rounded-xl shadow-2xl w-full max-w-md relative"
        onClick$={(e: MouseEvent) => e.stopPropagation()}
      >
        <div class="flex justify-between items-center mb-4">
          <h2 class="text-2xl font-semibold text-[var(--text-primary)] font-varela">
            Export conversation
          </h2>
          <button
            onClick$={props.onClose$}
            class="p-2 rounded-full text-[var(--text-muted)] hover:bg-[var(--bg-dropdown-hover)] transition-colors"
            aria-label="Close"
          >
            <LuX class="w-6 h-6" />
          </button>
        </div>

        <p class="text-sm text-[var(--text-secondary)] mb-4">
          A shareable receipt — proof of what was said.
        </p>

        <p class="text-xs font-semibold uppercase tracking-wide text-[var(--text-muted)] mb-2">
          Always included
        </p>
        <ul class="space-y-1.5 mb-5">
          {ALWAYS_INCLUDED.map((item) => (
            <li key={item} class="flex items-start gap-2 text-sm text-[var(--text-secondary)]">
              <LuCheck class="w-4 h-4 mt-0.5 flex-shrink-0 text-emerald-500 dark:text-emerald-400" />
              <span>{item}</span>
            </li>
          ))}
        </ul>

        <p class="text-xs font-semibold uppercase tracking-wide text-[var(--text-muted)] mb-1">
          Also include
        </p>
        <p class="text-xs text-[var(--text-muted)] mb-3">
          Off by default — these can reveal more than you want to share.
        </p>
        <div class="space-y-3 mb-6">
          <label class="flex items-start gap-3 text-sm text-[var(--text-secondary)] cursor-pointer">
            <input
              type="checkbox"
              class="mt-0.5"
              checked={opts.includeSystemPrompt}
              onChange$={(_, el) => (opts.includeSystemPrompt = el.checked)}
            />
            <span>The AI's system prompt — its instructions and persona.</span>
          </label>
          <label class="flex items-start gap-3 text-sm text-[var(--text-secondary)] cursor-pointer">
            <input
              type="checkbox"
              class="mt-0.5"
              checked={opts.includeAttachmentContent}
              onChange$={(_, el) => (opts.includeAttachmentContent = el.checked)}
            />
            <span>Full attachment contents — otherwise only their hashes are included.</span>
          </label>
          <label class="flex items-start gap-3 text-sm text-[var(--text-secondary)] cursor-pointer">
            <input
              type="checkbox"
              class="mt-0.5"
              checked={opts.includeThinking}
              onChange$={(_, el) => (opts.includeThinking = el.checked)}
            />
            <span>The AI's reasoning.</span>
          </label>
        </div>

        <p class="text-xs font-semibold uppercase tracking-wide text-[var(--text-muted)] mb-1">
          Sign It
        </p>
        <div class="mb-6">
          <label
            class={`flex items-start gap-3 text-sm text-[var(--text-secondary)] ${
              vaultReady.value ? 'cursor-pointer' : 'opacity-60'
            }`}
          >
            <input
              type="checkbox"
              class="mt-0.5"
              checked={signWithSignIt.value}
              disabled={!vaultReady.value}
              onChange$={(_, el) => (signWithSignIt.value = el.checked)}
            />
            <span>
              Sign with your Flowsta identity. The signature is published to the Sign It
              network, so anyone can verify this exact file at flowsta.com. Uses 1
              signature from your plan.
            </span>
          </label>
          {vaultReady.value === false && (
            <p class="text-xs text-[var(--text-muted)] mt-1.5 ml-7">
              Needs your Flowsta Vault running and unlocked.
            </p>
          )}
        </div>

        <div class="flex justify-end items-center gap-3">
          <LiquidMetalButton
            variant="secondary"
            onClick$={props.onClose$}
            class="px-6 py-2 text-sm"
          >
            Cancel
          </LiquidMetalButton>
          <LiquidMetalButton onClick$={submit$} class="px-6 py-2 text-sm">
            {signWithSignIt.value ? 'Sign & export' : 'Export'}
          </LiquidMetalButton>
        </div>
      </div>
    </div>
  );
});
