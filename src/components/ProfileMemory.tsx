/**
 * The shared user-level memory profile — "what your AIs know about you".
 *
 * One component, two surfaces (the data is user-level either way):
 *  - Full management on the global **Your Memory** page (`readOnly={false}`):
 *    edit / forget facts, pause remembering, forget all.
 *  - Read-only mirror on the per-AI **[AI]'s Memory → Knows** tab
 *    (`readOnly`): see what this AI is working with, with a "Manage →" link
 *    back here instead of duplicating the controls.
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { readThroughWarmup } from "../utils/recordsWarmup";
import { Link } from "@builder.io/qwik-city";
import {
  LuUser,
  LuBrain,
  LuPencil,
  LuTrash2,
  LuPause,
  LuPlay,
  LuCheck,
  LuX,
  LuShieldCheck,
  LuMessageSquare,
  LuArrowRight,
  LuPlus,
  LuChevronDown,
} from "@qwikest/icons/lucide";
import {
  getFacts,
  forgetFact,
  editFactValue,
  forgetAll,
  addAuthoredFact,
  addAuthoredNote,
  isMemoryPaused,
  setMemoryPaused,
  type Fact,
} from "../utils/memory";

/** Friendly labels for the relations a user can author by hand. Values are the
 *  predicate vocabulary the extraction grammar also uses, so authored and
 *  learned facts read and rank the same way. */
const RELATION_OPTIONS = [
  { predicate: "note", label: "Note (free text)", example: "Anything you'd like your AIs to know — in your own words" },
  { predicate: "name_is", label: "Name", example: "e.g. Alex" },
  { predicate: "lives_in", label: "Lives in", example: "e.g. Berlin" },
  { predicate: "works_on", label: "Works on", example: "e.g. a startup" },
  { predicate: "likes", label: "Likes", example: "e.g. jazz" },
  { predicate: "dislikes", label: "Dislikes", example: "e.g. spicy food" },
  { predicate: "prefers", label: "Prefers", example: "e.g. tea over coffee" },
  { predicate: "interested_in", label: "Interested in", example: "e.g. astronomy" },
  { predicate: "goal_is", label: "Goal", example: "e.g. run a marathon" },
  { predicate: "has", label: "Has", example: "e.g. a dog named Rusty" },
  { predicate: "is", label: "Is", example: "e.g. vegetarian" },
  { predicate: "born_in", label: "Born in", example: "e.g. Manchester" },
  { predicate: "age_is", label: "Age", example: "e.g. 34" },
];
import { memoryExtractionIdle } from "../utils/memoryExtraction";
import ConfirmModal from "./ConfirmModal";
import LiquidMetalButton from "./LiquidMetalButton";
import { LuDownload } from "@qwikest/icons/lucide";

/** Friendly name for an import tag ("import:claude-code" -> "Claude Code"). */
const IMPORT_SOURCE_NAMES: Record<string, string> = {
  chatgpt: "ChatGPT",
  claude: "Claude",
  perplexity: "Perplexity",
  "claude-code": "Claude Code",
  aider: "Aider",
  opencode: "OpenCode",
  codex: "Codex",
  cursor: "Cursor",
};

function importLabel(sourceAiId: string): string {
  const key = sourceAiId.slice("import:".length);
  return IMPORT_SOURCE_NAMES[key] ?? key;
}

interface ProfileMemoryProps {
  /** Read-only mirror (per-AI tab) vs full management (global page). */
  readOnly?: boolean;
  /** Where the "Manage →" link points in read-only mode. */
  manageHref?: string;
}

export default component$<ProfileMemoryProps>(
  ({ readOnly = false, manageHref = "/your-memory/" }) => {
    const facts = useSignal<Fact[]>([]);
    const factsLoading = useSignal(true);
    const memoryPaused = useSignal(false);
    const editingId = useSignal<string | null>(null);
    const editValue = useSignal("");
    const showForgetConfirm = useSignal(false);
    /** Review-an-import mode: an "import:<source>" tag, or null for all.
     *  Imports can flood the list (a coding history is mostly task talk),
     *  so auditing them needs their facts isolated and bulk-forgettable. */
    const sourceFilter = useSignal<string | null>(null);
    const showForgetSourceConfirm = useSignal(false);
    const showAdd = useSignal(false);
    const addPredicate = useSignal("likes");
    const addValue = useSignal("");
    const relOpen = useSignal(false);

    const factsWarming = useSignal(false);
    const activeFacts$ = $(async () => {
      facts.value = (await getFacts())
        .filter((f) => f.valid_to == null)
        .sort((a, b) => b.confidence - a.confidence);
    });

    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(async ({ cleanup }) => {
      let alive = true;
      cleanup(() => (alive = false));
      memoryPaused.value = isMemoryPaused();
      // Facts read through the conductor - hold the line through its
      // startup rather than declare "nothing learned yet" falsely.
      facts.value = await readThroughWarmup(
        async () =>
          (await getFacts())
            .filter((f) => f.valid_to == null)
            .sort((a, b) => b.confidence - a.confidence),
        (w) => (factsWarming.value = w),
        () => alive,
      );
      factsLoading.value = false;
      // A fact from the turn the user just sent may still be saving (extraction
      // is fire-and-forget). Wait for it, then refresh — so the list isn't
      // stale until a manual reload.
      memoryExtractionIdle().then(() => activeFacts$());
      const onFocus = () => activeFacts$();
      window.addEventListener("focus", onFocus);
      cleanup(() => window.removeEventListener("focus", onFocus));
    }, { strategy: "document-ready" });

    const handleForget = $(async (id: string) => {
      await forgetFact(id);
      await activeFacts$();
    });

    const handleStartEdit = $((f: Fact) => {
      editingId.value = f.id;
      editValue.value = f.value;
    });

    const handleSaveEdit = $(async () => {
      const id = editingId.value;
      const val = editValue.value.trim();
      editingId.value = null;
      if (id && val) {
        await editFactValue(id, val);
        await activeFacts$();
      }
    });

    const doForgetAll = $(async () => {
      await forgetAll();
      facts.value = [];
      showForgetConfirm.value = false;
    });

    const doForgetSource = $(async () => {
      const tag = sourceFilter.value;
      showForgetSourceConfirm.value = false;
      if (!tag) return;
      await forgetAll(tag);
      sourceFilter.value = null;
      await activeFacts$();
    });

    const doAddFact = $(async () => {
      const val = addValue.value.trim();
      if (!val) return;
      if (addPredicate.value === "note") {
        await addAuthoredNote(val);
      } else {
        await addAuthoredFact({ predicate: addPredicate.value, value: val });
      }
      addValue.value = "";
      showAdd.value = false;
      await activeFacts$();
    });

    const handleTogglePause = $(() => {
      memoryPaused.value = !memoryPaused.value;
      setMemoryPaused(memoryPaused.value);
    });

    return (
      <>
        <div>
        {/* Header + controls */}
        <div class="flex flex-wrap items-center justify-between gap-3 mb-5">
          <p class="text-sm text-[var(--text-secondary)] flex items-center gap-1.5">
            <LuUser class="w-3.5 h-3.5 text-[var(--text-muted)] flex-shrink-0" />
            What your AIs have learned about you —{" "}
            <span class="text-[var(--text-muted)]">shared across every AI</span>
          </p>
          {readOnly ? (
            // A <Link> (router nav) wearing the secondary liquid-metal pill.
            <Link
              href={manageHref}
              class="btn-liquid-metal btn-secondary-metal flex items-center gap-1.5 px-3 py-1.5 text-xs"
            >
              <div class="shader-border" />
              <div class="shader-inner-fill" />
              <span class="btn-content">
                Manage in Your Memory <LuArrowRight class="w-3.5 h-3.5" />
              </span>
            </Link>
          ) : (
            <div class="flex items-center gap-2">
              {/* Secondary/danger first, primary (Add) on the right. */}
              {/* Secondary metal pill; when paused, the content turns amber to flag
                  that AIs are not currently learning. */}
              <LiquidMetalButton
                variant="secondary"
                onClick$={handleTogglePause}
                title={
                  memoryPaused.value
                    ? "Resume remembering — your local AIs will learn from new conversations"
                    : "Pause remembering — your AIs stop learning new facts (existing memory is kept)"
                }
                class={`flex items-center gap-1.5 px-3 py-1.5 text-xs ${
                  memoryPaused.value ? "btn-amber-accent" : ""
                }`}
              >
                {memoryPaused.value ? (
                  <>
                    <LuPlay class="w-3.5 h-3.5" /> Paused
                  </>
                ) : (
                  <>
                    <LuPause class="w-3.5 h-3.5" /> Remembering
                  </>
                )}
              </LiquidMetalButton>
              {facts.value.length > 0 && (
                <LiquidMetalButton
                  variant="danger"
                  onClick$={() => (showForgetConfirm.value = true)}
                  class="flex items-center gap-1.5 px-3 py-1.5 text-xs"
                >
                  <LuTrash2 class="w-3.5 h-3.5" /> Forget all
                </LiquidMetalButton>
              )}
              <LiquidMetalButton
                onClick$={() => (showAdd.value = !showAdd.value)}
                title="Add something for your AIs to remember about you"
                class="flex items-center gap-1.5 px-3 py-1.5 text-xs"
              >
                <LuPlus class="w-3.5 h-3.5" /> Add
              </LiquidMetalButton>
            </div>
          )}
        </div>

        {!readOnly && showAdd.value && (
          <div class="mb-5 p-4 rounded-xl bg-[var(--bg-card)] border border-[var(--border-subtle)]">
            <div class="flex flex-col sm:flex-row gap-2">
              {/* Custom dropdown — a native <select> popup is GTK-themed on
                  Linux/webkit and ignores our option colours (unreadable in
                  dark theme), so we render our own theme-aware menu. */}
              <div class="relative sm:w-40 sm:flex-shrink-0">
                <button
                  type="button"
                  onClick$={() => (relOpen.value = !relOpen.value)}
                  class="w-full flex items-center justify-between gap-2 px-3 py-2 rounded-lg bg-[var(--bg-main)] border border-[var(--border-subtle)] text-sm text-[var(--text-primary)] focus:outline-none focus:border-[var(--bg-button-primary)]"
                >
                  <span>
                    {RELATION_OPTIONS.find((o) => o.predicate === addPredicate.value)?.label ?? "Select"}
                  </span>
                  <LuChevronDown class="w-4 h-4 text-[var(--text-muted)] flex-shrink-0" />
                </button>
                {relOpen.value && (
                  <>
                    <div
                      class="fixed inset-0 z-40"
                      onClick$={() => (relOpen.value = false)}
                    />
                    <div class="absolute left-0 top-full mt-1 w-full min-w-[150px] max-h-60 overflow-auto z-50 rounded-lg bg-[var(--bg-dropdown)] border border-[var(--border-subtle)] shadow-xl py-1">
                      {RELATION_OPTIONS.map((o) => (
                        <button
                          key={o.predicate}
                          type="button"
                          onClick$={() => {
                            addPredicate.value = o.predicate;
                            relOpen.value = false;
                          }}
                          class={`block w-full text-left px-3 py-1.5 text-sm hover:bg-[var(--bg-card)] transition-colors ${
                            o.predicate === addPredicate.value
                              ? "text-[var(--text-primary)] font-medium"
                              : "text-[var(--text-secondary)]"
                          }`}
                        >
                          {o.label}
                        </button>
                      ))}
                    </div>
                  </>
                )}
              </div>
              {addPredicate.value === "note" ? (
                <textarea
                  value={addValue.value}
                  rows={2}
                  placeholder={
                    RELATION_OPTIONS.find((o) => o.predicate === addPredicate.value)?.example ?? ""
                  }
                  onInput$={(_, el) => (addValue.value = el.value)}
                  onKeyDown$={(e) => {
                    if (e.key === "Escape") {
                      showAdd.value = false;
                      addValue.value = "";
                    }
                  }}
                  autoFocus
                  class="flex-1 px-3 py-2 rounded-lg bg-[var(--bg-main)] border border-[var(--border-subtle)] text-sm text-[var(--text-primary)] focus:outline-none focus:border-[var(--bg-button-primary)] resize-y"
                />
              ) : (
                <input
                  type="text"
                  value={addValue.value}
                  placeholder={
                    RELATION_OPTIONS.find((o) => o.predicate === addPredicate.value)?.example ?? ""
                  }
                  onInput$={(_, el) => (addValue.value = el.value)}
                  onKeyDown$={(e) => {
                    if (e.key === "Enter") doAddFact();
                    if (e.key === "Escape") {
                      showAdd.value = false;
                      addValue.value = "";
                    }
                  }}
                  autoFocus
                  class="flex-1 px-3 py-2 rounded-lg bg-[var(--bg-main)] border border-[var(--border-subtle)] text-sm text-[var(--text-primary)] focus:outline-none focus:border-[var(--bg-button-primary)]"
                />
              )}
              <div class="flex items-center gap-2 sm:flex-shrink-0">
                <LiquidMetalButton
                  variant="secondary"
                  onClick$={() => {
                    showAdd.value = false;
                    addValue.value = "";
                  }}
                  class="px-3 py-2 text-sm"
                >
                  Cancel
                </LiquidMetalButton>
                <LiquidMetalButton
                  onClick$={doAddFact}
                  disabled={!addValue.value.trim()}
                  class="px-4 py-2 text-sm"
                >
                  Add
                </LiquidMetalButton>
              </div>
            </div>
            <p class="mt-2 text-[11px] text-[var(--text-muted)]">
              Things you add are kept exactly as you write them, and won't be
              overwritten by what your AIs pick up in chat.
            </p>
          </div>
        )}

        {memoryPaused.value && (
          <div class="mb-5 p-3 rounded-lg bg-amber-500/10 border border-amber-500/30 text-xs text-amber-400 flex items-center gap-2">
            <LuPause class="w-3.5 h-3.5 flex-shrink-0" />
            Remembering is paused. Your AIs won't learn anything new until you resume.
          </div>
        )}

        {factsLoading.value ? (
          <div class="text-center py-16">
            <div class="inline-block w-8 h-8 border-4 border-[var(--border-subtle)] border-t-[var(--bg-button-primary)] rounded-full animate-spin"></div>
            <p class="mt-4 text-[var(--text-secondary)]">
              {factsWarming.value
                ? "Your records are warming up - just after launch, what it has learned takes a moment to be ready."
                : "Loading…"}
            </p>
          </div>
        ) : facts.value.length === 0 ? (
          <div class="text-center py-16 rounded-2xl border border-dashed border-[var(--border-subtle)]">
            <LuBrain class="w-12 h-12 mx-auto mb-3 text-[var(--text-muted)]" />
            <p class="text-lg text-[var(--text-primary)]">Nothing learned yet</p>
            <p class="text-sm mt-1 text-[var(--text-secondary)] max-w-md mx-auto">
              As you chat with your local AIs, they quietly remember durable
              things about you — your name, preferences, and ongoing projects —
              so you don't have to repeat yourself. It all stays encrypted on
              this device.
            </p>
          </div>
        ) : (
          <div class="space-y-2">
            {(() => {
              // Import filter chips - only when imported facts exist.
              const counts = new Map<string, number>();
              for (const f of facts.value) {
                if (f.source_ai_id?.startsWith("import:")) {
                  counts.set(
                    f.source_ai_id,
                    (counts.get(f.source_ai_id) ?? 0) + 1,
                  );
                }
              }
              if (counts.size === 0) return null;
              return (
                <div class="flex flex-wrap items-center gap-2 pb-1">
                  <LiquidMetalButton
                    variant="secondary"
                    onClick$={() => (sourceFilter.value = null)}
                    class={`px-2.5 py-1 text-xs ${
                      sourceFilter.value === null ? "btn-amber-accent" : ""
                    }`}
                  >
                    Everything ({facts.value.length})
                  </LiquidMetalButton>
                  {[...counts.entries()].map(([tag, n]) => (
                    <LiquidMetalButton
                      key={tag}
                      variant="secondary"
                      onClick$={() =>
                        (sourceFilter.value =
                          sourceFilter.value === tag ? null : tag)
                      }
                      class={`px-2.5 py-1 text-xs ${
                        sourceFilter.value === tag ? "btn-amber-accent" : ""
                      }`}
                    >
                      {importLabel(tag)} import ({n})
                    </LiquidMetalButton>
                  ))}
                  {!readOnly && sourceFilter.value && (
                    <LiquidMetalButton
                      variant="danger"
                      onClick$={() => (showForgetSourceConfirm.value = true)}
                      class="px-2.5 py-1 text-xs"
                    >
                      Forget all of these
                    </LiquidMetalButton>
                  )}
                </div>
              );
            })()}
            {facts.value
              .filter(
                (f) =>
                  sourceFilter.value === null ||
                  f.source_ai_id === sourceFilter.value,
              )
              .map((f) => (
              <div
                key={f.id}
                class="group flex items-start gap-3 p-3.5 rounded-xl bg-[var(--bg-card)] border border-[var(--border-subtle)]"
              >
                <div class="min-w-0 flex-1">
                  {!readOnly && editingId.value === f.id ? (
                    <div class="flex items-center gap-2">
                      <input
                        type="text"
                        value={editValue.value}
                        onInput$={(_, el) => (editValue.value = el.value)}
                        onKeyDown$={(e) => {
                          if (e.key === "Enter") handleSaveEdit();
                          if (e.key === "Escape") editingId.value = null;
                        }}
                        autoFocus
                        class="flex-1 px-2.5 py-1.5 rounded-lg bg-[var(--bg-main)] border border-[var(--border-subtle)] text-sm text-[var(--text-primary)] focus:outline-none focus:border-[var(--bg-button-primary)]"
                      />
                      <LiquidMetalButton
                        variant="secondary"
                        onClick$={handleSaveEdit}
                        title="Save"
                        class="p-1.5"
                      >
                        <LuCheck class="w-4 h-4" />
                      </LiquidMetalButton>
                      <LiquidMetalButton
                        variant="secondary"
                        onClick$={() => (editingId.value = null)}
                        title="Cancel"
                        class="p-1.5"
                      >
                        <LuX class="w-4 h-4" />
                      </LiquidMetalButton>
                    </div>
                  ) : (
                    <>
                      <p class="text-sm text-[var(--text-primary)] break-words">
                        {f.value}
                      </p>
                      <div class="mt-1 flex items-center gap-2 text-[11px] text-[var(--text-muted)]">
                        <span class="capitalize">
                          {f.predicate.replace(/_/g, " ")}
                        </span>
                        {f.provenance === "authored" ? (
                          <span class="flex items-center gap-1 text-sky-400/80">
                            <LuUser class="w-3 h-3" /> added by you
                          </span>
                        ) : f.source_action_hash ? (
                          <span class="flex items-center gap-1 text-emerald-400/80">
                            <LuShieldCheck class="w-3 h-3" /> from a signed
                            conversation
                          </span>
                        ) : f.source_ai_id?.startsWith("import:") ? (
                          <span class="flex items-center gap-1 text-violet-400/80">
                            <LuDownload class="w-3 h-3" /> from your{" "}
                            {importLabel(f.source_ai_id)} import
                          </span>
                        ) : (
                          <span class="flex items-center gap-1">
                            <LuMessageSquare class="w-3 h-3" /> learned from your chats
                          </span>
                        )}
                      </div>
                    </>
                  )}
                </div>
                {!readOnly && editingId.value !== f.id && (
                  <div class="flex items-center gap-1 flex-shrink-0 opacity-0 group-hover:opacity-100 transition-opacity">
                    <LiquidMetalButton
                      variant="secondary"
                      onClick$={() => handleStartEdit(f)}
                      title="Edit"
                      class="p-1.5"
                    >
                      <LuPencil class="w-3.5 h-3.5" />
                    </LiquidMetalButton>
                    <LiquidMetalButton
                      variant="danger"
                      onClick$={() => handleForget(f.id)}
                      title="Forget this"
                      class="p-1.5"
                    >
                      <LuTrash2 class="w-3.5 h-3.5" />
                    </LiquidMetalButton>
                  </div>
                )}
              </div>
            ))}
          </div>
        )}
        </div>

        <ConfirmModal
          isOpen={showForgetConfirm.value}
          variant="danger"
          title="Forget everything?"
          message="This clears everything your AIs have learned about you. Your conversation history is kept. This can't be undone."
          confirmLabel="Forget all"
          onConfirm$={doForgetAll}
          onCancel$={() => (showForgetConfirm.value = false)}
        />
        <ConfirmModal
          isOpen={showForgetSourceConfirm.value}
          variant="danger"
          title={`Forget the ${sourceFilter.value ? importLabel(sourceFilter.value) : ""} import's memories?`}
          message="This clears every memory learned from that import. The imported conversations themselves are kept, and anything learned from your own chats stays. This can't be undone."
          confirmLabel="Forget them"
          onConfirm$={doForgetSource}
          onCancel$={() => (showForgetSourceConfirm.value = false)}
        />
      </>
    );
  },
);
