/**
 * A line from one of the person's notes, on a Subject page: dashed
 * outline, the file and when it was edited, why the line is here, and two
 * actions: show the note, or do not use this (the line, the note or its
 * folder). Never corrected or forgotten here: the note is changed where
 * it lives.
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { doNotUse, followNotes, noteHeading, noteReason, noteStateWords, NOTES_EVENT, type NoteView, type Reach } from "../../utils/memoryNotes";

interface NoteItem {
  id: string;
  text: string;
  item: { state_at?: number; source?: { doc?: { name?: string; changed?: number; asked?: string | null; state?: string | null } | null } | null; [k: string]: unknown };
}

export const NoteRow = component$<{ item: NoteItem }>(({ item }) => {
  const doc = item.item.source?.doc ?? {};
  const now = useSignal<NoteView | null>(null);
  const choosing = useSignal(false);
  const busy = useSignal(false);
  const problem = useSignal("");

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    const look = async () => {
      now.value = (await followNotes()).find((n) => n.id === item.id) ?? null;
    };
    await look();
    window.addEventListener(NOTES_EVENT, look);
    cleanup(() => window.removeEventListener(NOTES_EVENT, look));
  });

  const show = $(async () => {
    const path = now.value?.path;
    if (!path) return;
    try {
      const { revealItemInDir } = await import("@tauri-apps/plugin-opener");
      await revealItemInDir(path);
    } catch (e) {
      problem.value = `The note could not be shown: ${String((e as { message?: string })?.message ?? e)}`;
    }
  });

  const choose = $(async (reach: Reach) => {
    busy.value = true;
    try {
      await doNotUse(item.id, reach);
      choosing.value = false;
    } catch (e) {
      problem.value = String((e as { message?: string })?.message ?? e);
    } finally {
      busy.value = false;
    }
  });

  const state = now.value?.state ?? (doc.state as NoteView["state"] | undefined) ?? "here";
  if (state === "not-used") return null;
  const name = now.value?.name ?? doc.name ?? "a note";
  const changed = now.value?.changed ?? doc.changed ?? 0;
  const words = noteStateWords(state);
  const act = "text-xs text-[var(--text-link)] hover:underline disabled:opacity-50";
  return (
    <div class="rounded-lg border border-dashed border-[var(--border-input)] px-3 py-2" data-testid="note-row" data-id={item.id} data-state={state}>
      <div class="text-xs font-medium text-[var(--text-secondary)]" data-testid="note-heading">
        {noteHeading({ name, changed })}
      </div>
      <div class={["mt-1 text-sm break-words", state === "here" || state === "unreachable" ? "text-[var(--text-primary)]" : "text-[var(--text-muted)]"]}>{item.text}</div>
      <div class="mt-1 text-xs text-[var(--text-muted)]" data-testid="note-reason">
        {noteReason(doc.asked, item.item.state_at ?? Date.now())}
      </div>
      {words && (
        <div class="mt-1 text-xs text-[var(--text-secondary)]" data-testid="note-state">
          {words}
        </div>
      )}
      {problem.value && <div class="mt-1 text-xs text-[var(--text-secondary)]">{problem.value}</div>}
      {choosing.value ? (
        <div class="mt-1.5 flex flex-col items-start gap-1" data-testid="note-reach">
          <span class="text-xs text-[var(--text-secondary)]">Do not use</span>
          <button type="button" class={act} data-testid="note-reach-to" data-reach="line" disabled={busy.value} onClick$={() => choose("line")}>
            This line
          </button>
          <button type="button" class={act} data-testid="note-reach-to" data-reach="note" disabled={busy.value} onClick$={() => choose("note")}>
            This note ({name})
          </button>
          <button type="button" class={act} data-testid="note-reach-to" data-reach="folder" disabled={busy.value} onClick$={() => choose("folder")}>
            Every note in its folder
          </button>
          <button type="button" class="text-xs text-[var(--text-muted)] hover:underline" onClick$={() => (choosing.value = false)}>
            Cancel
          </button>
        </div>
      ) : (
        <div class="mt-1.5 flex flex-wrap gap-x-4 gap-y-1">
          {state !== "deleted" && now.value?.path && (
            <button type="button" class={act} data-testid="note-show" onClick$={show}>
              Show the note
            </button>
          )}
          <button type="button" class={act} data-testid="note-do-not-use" onClick$={() => (choosing.value = true)}>
            Do not use this
          </button>
        </div>
      )}
    </div>
  );
});
