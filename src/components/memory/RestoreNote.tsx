/**
 * Shown once at the top of Memory after a restore from the Vault backup:
 * what came back and what this computer still has to do. It can be read
 * and put away, or left alone; nothing waits for it.
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { Callout } from "../Callout";
import { RESTORE_NOTE_EVENT, afterBackup, restoreNote, restoreNoteSeen, restoredFrom, type MemoryBack } from "../../utils/memoryRestore";

export const RestoreNote = component$<{ class?: string }>((props) => {
  const note = useSignal<MemoryBack | null>(null);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    const check = async () => {
      note.value = await restoreNote();
    };
    void check();
    window.addEventListener(RESTORE_NOTE_EVENT, check);
    cleanup(() => window.removeEventListener(RESTORE_NOTE_EVENT, check));
  });

  const seen = $(() => restoreNoteSeen());

  if (!note.value) return null;
  return (
    <div data-testid="memory-restore-note" class={props.class ?? ""}>
      <Callout intent="info">
        <p class="font-semibold text-[var(--text-primary)]">Your memory is back.</p>
        <p class="mt-1" data-testid="memory-restore-from">
          {restoredFrom(note.value)}
        </p>
        <ul class="mt-2 list-disc pl-5 space-y-1">
          <li>Back, as it was: what replaced what, what you confirmed and corrected, your subjects, who can use what, the "Not used" list, and private conversations, still private.</li>
          <li>Being read again: your conversations, so your AIs can search them. Kept items can be used right away.</li>
          <li data-testid="memory-restore-after">{afterBackup(note.value)}</li>
        </ul>
        <div class="mt-2">
          <button type="button" data-testid="memory-restore-seen" onClick$={seen} class="text-xs font-medium text-[var(--text-link)] hover:underline">
            Got it
          </button>
        </div>
      </Callout>
    </div>
  );
});
