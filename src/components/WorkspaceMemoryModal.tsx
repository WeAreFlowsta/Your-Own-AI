import { component$, useSignal, useTask$, $, type Signal } from "@builder.io/qwik";
import { LuX, LuPencil, LuTrash2, LuPlus, LuCheck } from "@qwikest/icons/lucide";
import LiquidMetalButton from "./LiquidMetalButton";
import {
  getWorkspaceMemory,
  memoryRows,
  saveWorkspaceMemory,
  MEMORY_MAX_LINES,
} from "../utils/workspaceMemory";
import { getLocalCustomAis } from "../utils/localAiStorage";

/**
 * The workspace's memory - what this folder's work has taught the AIs -
 * viewed and edited as ROWS (each line is a row; click the pencil to edit
 * its text inline, delete what's wrong, add what's missing). Saving writes
 * a new revision to the chain, so every human correction is part of the
 * memory's auditable history.
 */
export const WorkspaceMemoryModal = component$<{
  folderPath: Signal<string | null>;
}>(({ folderPath }) => {
  const rows = useSignal<string[]>([]);
  const loading = useSignal(false);
  const updatedAt = useSignal(0);
  const revisions = useSignal(0);
  const editingIdx = useSignal<number | null>(null);
  const draft = useSignal("");
  const adding = useSignal(false);
  const dirty = useSignal(false);
  const saving = useSignal(false);

  useTask$(async ({ track }) => {
    const folder = track(() => folderPath.value);
    if (!folder) return;
    loading.value = true;
    dirty.value = false;
    editingIdx.value = null;
    adding.value = false;
    try {
      const m = await getWorkspaceMemory(folder);
      rows.value = memoryRows(m.content);
      updatedAt.value = m.updatedAt;
      revisions.value = m.revisions;
    } finally {
      loading.value = false;
    }
  });

  const commitEdit = $(() => {
    const idx = editingIdx.value;
    if (idx === null) return;
    const text = draft.value.trim();
    const next = [...rows.value];
    if (text) next[idx] = text;
    else next.splice(idx, 1);
    rows.value = next;
    editingIdx.value = null;
    dirty.value = true;
  });

  const save = $(async () => {
    const folder = folderPath.value;
    if (!folder || saving.value) return;
    saving.value = true;
    try {
      const ais = await getLocalCustomAis();
      const writer = ais.find((a) => a.agentPubKey);
      if (!writer) return;
      const ok = await saveWorkspaceMemory(
        { agentPubKey: writer.agentPubKey!, label: writer.name },
        folder,
        rows.value.join("\n"),
      );
      if (ok) {
        dirty.value = false;
        revisions.value += 1;
      }
    } finally {
      saving.value = false;
    }
  });

  if (!folderPath.value) return null;
  const folderLeaf = folderPath.value.split("/").filter(Boolean).pop();

  return (
    <div class="fixed inset-0 z-[70]">
      <div class="absolute inset-0 bg-black/40" onClick$={() => (folderPath.value = null)} />
      <div class="absolute left-1/2 top-1/2 -translate-x-1/2 -translate-y-1/2 w-[560px] max-w-[92vw] max-h-[80vh] flex flex-col rounded-xl border border-[var(--border-primary)] bg-[var(--bg-card)] shadow-2xl">
        <div class="flex items-center justify-between px-5 py-3 border-b border-[var(--border-subtle)]">
          <div class="min-w-0">
            <h2 class="text-base font-semibold text-[var(--text-primary)] font-varela truncate">
              Project memory - {folderLeaf}
            </h2>
            <p class="text-xs text-[var(--text-muted)] truncate">
              {revisions.value > 0
                ? `${rows.value.length} note${rows.value.length === 1 ? "" : "s"}${
                    rows.value.length >= MEMORY_MAX_LINES * 0.8
                      ? ` (of ${MEMORY_MAX_LINES} - older notes make room for new ones)`
                      : ""
                  } - ${revisions.value} revision${revisions.value === 1 ? "" : "s"} in your records, shared by all your AIs`
                : "Nothing remembered yet - it grows as your AIs work here"}
            </p>
          </div>
          <button
            onClick$={() => (folderPath.value = null)}
            class="shrink-0 ml-3 text-[var(--text-muted)] hover:text-[var(--text-primary)] bg-transparent border-none cursor-pointer"
          >
            <LuX class="h-4 w-4" />
          </button>
        </div>

        <div class="flex-1 overflow-y-auto px-5 py-3">
          {loading.value && (
            <p class="text-sm text-[var(--text-muted)] py-2">Loading from your records..</p>
          )}
          {!loading.value &&
            rows.value.map((row, idx) => (
              <div
                key={`${idx}-${row}`}
                class="group flex items-start gap-2 py-1 border-b border-[var(--border-subtle)]/40 last:border-b-0"
              >
                {editingIdx.value === idx ? (
                  <>
                    <textarea
                      // eslint-disable-next-line qwik/valid-lexical-scope
                      onInput$={(_, el) => (draft.value = el.value)}
                      onKeyDown$={(e) => {
                        if (e.key === "Enter" && !e.shiftKey) commitEdit();
                        if (e.key === "Escape") (editingIdx.value = null);
                      }}
                      value={draft.value}
                      rows={2}
                      class="flex-1 text-sm rounded-lg bg-[var(--bg-input)] border border-[var(--border-subtle)] p-2 font-mono text-[var(--text-primary)] resize-y"
                    />
                    <button
                      onClick$={commitEdit}
                      title="Done"
                      class="shrink-0 mt-1 text-green-600 dark:text-green-400 bg-transparent border-none cursor-pointer"
                    >
                      <LuCheck class="h-4 w-4" />
                    </button>
                  </>
                ) : (
                  <>
                    <span class="flex-1 text-sm font-mono text-[var(--text-secondary)] break-words min-w-0 py-0.5">
                      {row}
                    </span>
                    <button
                      onClick$={() => {
                        draft.value = row;
                        editingIdx.value = idx;
                      }}
                      title="Edit this note"
                      class="shrink-0 opacity-0 group-hover:opacity-100 text-[var(--text-muted)] hover:text-[var(--text-primary)] bg-transparent border-none cursor-pointer"
                    >
                      <LuPencil class="h-3.5 w-3.5" />
                    </button>
                    <button
                      onClick$={() => {
                        rows.value = rows.value.filter((_, i) => i !== idx);
                        dirty.value = true;
                      }}
                      title="Forget this note"
                      class="shrink-0 opacity-0 group-hover:opacity-100 text-red-500 dark:text-red-400 bg-transparent border-none cursor-pointer"
                    >
                      <LuTrash2 class="h-3.5 w-3.5" />
                    </button>
                  </>
                )}
              </div>
            ))}

          {!loading.value && adding.value && (
            <div class="flex items-start gap-2 py-1">
              <textarea
                // eslint-disable-next-line qwik/valid-lexical-scope
                onInput$={(_, el) => (draft.value = el.value)}
                onKeyDown$={(e) => {
                  if (e.key === "Enter" && !e.shiftKey) {
                    const text = draft.value.trim();
                    if (text && rows.value.length < MEMORY_MAX_LINES) {
                      rows.value = [...rows.value, text];
                      dirty.value = true;
                    }
                    draft.value = "";
                    adding.value = false;
                  }
                  if (e.key === "Escape") (adding.value = false);
                }}
                rows={2}
                placeholder="A durable note about this project (Enter to add)"
                class="flex-1 text-sm rounded-lg bg-[var(--bg-input)] border border-[var(--border-subtle)] p-2 font-mono text-[var(--text-primary)] resize-y"
              />
            </div>
          )}
          {!loading.value && !adding.value && (
            <button
              onClick$={() => {
                draft.value = "";
                adding.value = true;
              }}
              class="flex items-center gap-1.5 mt-2 text-xs text-[var(--text-link)] hover:underline bg-transparent border-none cursor-pointer"
            >
              <LuPlus class="h-3.5 w-3.5" /> Add a note
            </button>
          )}
        </div>

        <div class="flex flex-col-reverse sm:flex-row sm:justify-end gap-2 px-5 py-3 border-t border-[var(--border-subtle)]">
          <LiquidMetalButton
            variant="secondary"
            class="px-4 py-2 text-sm"
            onClick$={() => (folderPath.value = null)}
          >
            Close
          </LiquidMetalButton>
          <LiquidMetalButton
            class="px-4 py-2 text-sm"
            disabled={!dirty.value || saving.value}
            onClick$={save}
          >
            {saving.value ? "Saving.." : "Save changes"}
          </LiquidMetalButton>
        </div>
      </div>
    </div>
  );
});
