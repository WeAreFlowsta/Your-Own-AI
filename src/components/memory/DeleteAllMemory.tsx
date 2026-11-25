/**
 * Delete all memory: one quiet row at the bottom of Memory, and the dialog
 * it opens. The dialog says what goes and what stays, place by place, and
 * asks for the word DELETE, because this cannot be undone. Nothing here
 * happens unless the person opens it.
 */
import { component$, useSignal, $ } from '@builder.io/qwik';
import LiquidMetalButton from '../LiquidMetalButton';

interface Preview {
  items: number;
  subjects: number;
  notUsed: number;
}

const count = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;

export const DeleteAllMemory = component$(() => {
  const open = useSignal(false);
  const preview = useSignal<Preview | null>(null);
  const withConversations = useSignal(false);
  const typed = useSignal('');
  const busy = useSignal('');
  const said = useSignal('');

  const show = $(async () => {
    typed.value = '';
    said.value = '';
    withConversations.value = false;
    open.value = true;
    const { invoke } = await import('@tauri-apps/api/core');
    preview.value = await invoke<Preview>('memory_delete_all_preview').catch(() => null);
  });

  const backUp = $(async () => {
    busy.value = 'backup';
    said.value = '';
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const r = await invoke<{ records?: number; skipped?: string }>('vault_backup_now');
      said.value = r.skipped ? `No backup was made: ${r.skipped}.` : 'Backed up to your Vault.';
    } catch (e) {
      said.value = `The backup could not be made just now. ${String(e)}`;
    }
    busy.value = '';
  });

  const go = $(async () => {
    if (typed.value.trim() !== 'DELETE' || busy.value) return;
    busy.value = 'delete';
    said.value = '';
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      await invoke('memory_delete_all');
      if (withConversations.value) {
        // Every conversation of every AI, one by one, as deleting one does it.
        const [{ getLocalCustomAis }, { getConversationsCached, deleteConversation }] = await Promise.all([import('../../utils/localAiStorage'), import('../../utils/holochainTranscripts')]);
        const ais = await getLocalCustomAis().catch(() => []);
        let done = 0;
        for (const ai of ais) {
          if (!ai.agentPubKey) continue;
          for (const c of await getConversationsCached(ai.agentPubKey)) {
            await deleteConversation((c as { agent_key?: string }).agent_key || ai.agentPubKey, c.hash).catch(() => undefined);
            done++;
            said.value = `Deleting conversations: ${done} so far.`;
          }
        }
      }
      open.value = false;
      window.dispatchEvent(new CustomEvent('yoai-memory-changed'));
    } catch (e) {
      said.value = `It could not be deleted just now. Nothing more was removed. ${String(e)}`;
    }
    busy.value = '';
  });

  const p = preview.value;
  const choice = (on: boolean) =>
    `w-full text-left rounded-xl border px-4 py-3 ${on ? 'border-[var(--bg-button-primary)] bg-[var(--bg-main)]' : 'border-[var(--border-subtle)] hover:border-[var(--border-input)]'}`;
  const small = 'mt-1 text-sm text-[var(--text-secondary)]';

  return (
    <>
      <p class="mt-8 text-xs text-[var(--text-muted)]" data-testid="delete-all-row">
        Memory is in your Vault backup.{' '}
        <button type="button" class="underline hover:text-[var(--text-secondary)]" data-testid="delete-all-open" onClick$={show}>
          Delete all memory
        </button>
      </p>
      {open.value && (
        <div class="fixed inset-0 z-[70]" data-testid="delete-all-dialog">
          <div class="absolute inset-0 bg-black/50" onClick$={() => !busy.value && (open.value = false)} />
          <div class="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 w-[560px] max-w-[92vw] max-h-[86vh] overflow-y-auto rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-6">
            <h2 class="text-xl font-varela text-[var(--text-primary)]">Delete all memory?</h2>
            <p class={small}>This cannot be undone here. Choose what goes.</p>

            <div class="mt-4 flex flex-col gap-2">
              <button type="button" class={choice(!withConversations.value)} data-testid="delete-all-memory-only" aria-pressed={!withConversations.value} onClick$={() => (withConversations.value = false)}>
                <div class="font-medium text-[var(--text-primary)]">Memory only</div>
                <div class={small} data-testid="delete-all-counts">
                  {p ? `All ${count(p.items, 'kept item', 'kept items')}, all ${count(p.subjects, 'subject', 'subjects')}, and the "Not used" list.` : 'Everything memory has kept, and the "Not used" list.'} Your conversations and
                  your rules stay, and your AIs can still search your conversations.
                </div>
              </button>
              <button type="button" class={choice(withConversations.value)} data-testid="delete-all-with-conversations" aria-pressed={withConversations.value} onClick$={() => (withConversations.value = true)}>
                <div class="font-medium text-[var(--text-primary)]">Memory and every conversation</div>
                <div class={small}>Everything above, and every conversation with every AI. Your rules and your AIs stay.</div>
              </button>
            </div>

            <ul class="mt-4 list-disc pl-5 space-y-1 text-sm text-[var(--text-secondary)]">
              <li>On this computer: deleted now.</li>
              <li>On your other devices: deleted when they next connect.</li>
              <li>In Vault backups made before today: still there. Restoring one of those would bring it back.</li>
              <li>What was already sent to an online AI cannot be taken back.</li>
            </ul>

            <label class="mt-5 block text-sm text-[var(--text-primary)]" for="delete-all-word">
              Type DELETE to go on
            </label>
            <input
              id="delete-all-word"
              type="text"
              autocomplete="off"
              class="mt-1 w-full px-3 py-2 rounded-lg bg-[var(--bg-input)] border border-[var(--border-input)] text-[var(--text-primary)] text-sm"
              data-testid="delete-all-word"
              value={typed.value}
              onInput$={(_, el) => (typed.value = el.value)}
            />
            {said.value && (
              <p class={small} data-testid="delete-all-said">
                {said.value}
              </p>
            )}

            <div class="mt-5 flex flex-wrap items-center justify-between gap-2">
              <LiquidMetalButton variant="secondary" class="px-3 py-2 text-sm" testId="delete-all-backup" disabled={!!busy.value} onClick$={backUp}>
                {busy.value === 'backup' ? 'Backing up...' : 'Back up to the Vault first'}
              </LiquidMetalButton>
              <div class="flex items-center gap-2">
                <LiquidMetalButton variant="secondary" class="px-3 py-2 text-sm" testId="delete-all-cancel" disabled={!!busy.value} onClick$={() => (open.value = false)}>
                  Cancel
                </LiquidMetalButton>
                <LiquidMetalButton variant="danger" class="px-3 py-2 text-sm" testId="delete-all-go" disabled={typed.value.trim() !== 'DELETE' || !!busy.value} onClick$={go}>
                  {busy.value === 'delete' ? 'Deleting...' : 'Delete all memory'}
                </LiquidMetalButton>
              </div>
            </div>
          </div>
        </div>
      )}
    </>
  );
});
