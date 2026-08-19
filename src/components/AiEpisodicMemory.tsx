/**
 * The per-AI episodic memory view (Knows tab): what THIS AI remembers from
 * your past conversations together — distinct from the shared profile below it.
 * Read-only list + a "forget" control. The data is the same per-AI vector store
 * the chat recall uses (transcriptMemory.ts).
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { readThroughWarmup } from "../utils/recordsWarmup";
import { LuBrain, LuTrash2, LuDownload } from "@qwikest/icons/lucide";
import {
  getAiMemories,
  clearAiMemory,
  deleteAiMemory,
  type TranscriptEmbedding,
} from "../utils/transcriptMemory";
import { isEmbeddingModelReady } from "../utils/embeddings";
import ConfirmModal from "./ConfirmModal";
import LiquidMetalButton from "./LiquidMetalButton";

const DISPLAY_CAP = 30;

function relTime(micros: number): string {
  const ms = micros / 1000;
  const diff = Date.now() - ms;
  const m = Math.floor(diff / 60000);
  if (m < 1) return "just now";
  if (m < 60) return `${m}m ago`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h}h ago`;
  const d = Math.floor(h / 24);
  if (d < 7) return `${d}d ago`;
  return new Date(ms).toLocaleDateString();
}

interface Props {
  aiId: string;
  aiName?: string;
}

export default component$<Props>(({ aiId, aiName }) => {
  const items = useSignal<TranscriptEmbedding[]>([]);
  const loading = useSignal(true);
  const modelReady = useSignal(true);
  const showClear = useSignal(false);

  const warming = useSignal(false);
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ track, cleanup }) => {
    const id = track(() => aiId);
    if (!id) {
      loading.value = false;
      return;
    }
    let alive = true;
    cleanup(() => (alive = false));
    loading.value = true;
    modelReady.value = await isEmbeddingModelReady();
    // Memories decrypt with a key the conductor holds - reads silently
    // come back empty while it starts, and "no memories" against a
    // history the user knows exists is the one message this surface
    // must never send by accident.
    items.value = await readThroughWarmup(
      () => getAiMemories(id),
      (w) => (warming.value = w),
      () => alive,
    );
    loading.value = false;
  });

  const doClear = $(async () => {
    await clearAiMemory(aiId);
    items.value = [];
    showClear.value = false;
  });

  const doDeleteOne = $(async (id: string) => {
    await deleteAiMemory(aiId, id);
    items.value = items.value.filter((m) => m.id !== id);
  });

  const name = aiName || "This AI";

  return (
    <>
      <div>
        <div class="flex flex-wrap items-center justify-between gap-3 mb-5">
          <p class="text-sm text-[var(--text-secondary)] flex items-center gap-1.5">
            <LuBrain class="w-3.5 h-3.5 text-[var(--text-muted)] flex-shrink-0" />
            What {name} remembers from your conversations —{" "}
            <span class="text-[var(--text-muted)]">only this AI</span>
          </p>
          {items.value.length > 0 && (
            <LiquidMetalButton
              variant="danger"
              onClick$={() => (showClear.value = true)}
              class="flex items-center gap-1.5 px-3 py-1.5 text-xs"
            >
              <LuTrash2 class="w-3.5 h-3.5" /> Forget
            </LiquidMetalButton>
          )}
        </div>

        {loading.value ? (
          <div class="text-center py-10">
            <div class="inline-block w-6 h-6 border-4 border-[var(--border-subtle)] border-t-[var(--bg-button-primary)] rounded-full animate-spin"></div>
            {warming.value && (
              <p class="mt-3 text-sm text-[var(--text-muted)] max-w-md mx-auto">
                Your records are warming up - just after launch, its memories take a moment to be ready.
              </p>
            )}
          </div>
        ) : items.value.length === 0 ? (
          <div class="text-center py-12 rounded-2xl border border-dashed border-[var(--border-subtle)]">
            <LuBrain class="w-10 h-10 mx-auto mb-3 text-[var(--text-muted)]" />
            {modelReady.value ? (
              <p class="text-sm text-[var(--text-secondary)] max-w-md mx-auto">
                As you chat, {name} quietly remembers key moments from your
                conversations and brings them back when they're relevant.
              </p>
            ) : (
              <p class="text-sm text-[var(--text-secondary)] max-w-md mx-auto flex flex-col items-center gap-2">
                <span>
                  {name} can't remember conversations yet — that needs the memory
                  recall model.
                </span>
                <a
                  href="/settings"
                  class="inline-flex items-center gap-1.5 text-[var(--text-link)] hover:underline"
                >
                  <LuDownload class="w-3.5 h-3.5" /> Get it in Settings → Components
                </a>
              </p>
            )}
          </div>
        ) : (
          <div class="space-y-2">
            {items.value.slice(0, DISPLAY_CAP).map((m) => (
              <div
                key={m.id}
                class="group flex items-start gap-3 p-3 rounded-xl bg-[var(--bg-card)] border border-[var(--border-subtle)]"
              >
                <span class="text-[11px] font-medium px-1.5 py-0.5 rounded flex-shrink-0 mt-0.5 bg-[var(--bg-dropdown)] text-[var(--text-secondary)] min-w-[2.5rem] text-center">
                  {m.role === "user" ? "You" : name}
                </span>
                <p class="min-w-0 flex-1 text-sm text-[var(--text-primary)] break-words">
                  {m.text}
                </p>
                <span class="text-[11px] text-[var(--text-muted)] flex-shrink-0 mt-0.5">
                  {relTime(m.created_at)}
                </span>
                <LiquidMetalButton
                  variant="danger"
                  onClick$={() => doDeleteOne(m.id)}
                  title="Forget this"
                  class="p-1 flex-shrink-0 opacity-0 group-hover:opacity-100 transition-opacity"
                >
                  <LuTrash2 class="w-3.5 h-3.5" />
                </LiquidMetalButton>
              </div>
            ))}
            {items.value.length > DISPLAY_CAP && (
              <p class="text-center text-[11px] text-[var(--text-muted)] pt-1">
                Showing the {DISPLAY_CAP} most recent of {items.value.length}{" "}
                remembered moments.
              </p>
            )}
          </div>
        )}
      </div>

      <ConfirmModal
        isOpen={showClear.value}
        variant="danger"
        title={`Forget ${name}'s conversation memory?`}
        message={`This clears what ${name} remembers from your past conversations. Your conversation history itself is kept. This can't be undone.`}
        confirmLabel="Forget"
        onConfirm$={doClear}
        onCancel$={() => (showClear.value = false)}
      />
    </>
  );
});
