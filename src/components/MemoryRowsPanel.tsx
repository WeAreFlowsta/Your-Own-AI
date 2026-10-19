/**
 * The standing rows of one memory scope (rules, commands, places,
 * decisions - the person's own words, re-stated every turn) and what
 * dreaming changed there, with Undo. One component for every scope: the
 * person's on Your Memory, an AI's on its Memory page section
 * (MEMORY_REDESIGN.md §9).
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { LuPencil, LuTrash2, LuChevronDown, LuChevronRight } from "@qwikest/icons/lucide";
import {
  listMemoryRows,
  listMemoryRowsCached,
  listMemoryRevisions,
  describeDream,
  undoDream,
  saveMemoryRow,
  revise,
  deletion,
  aiScope,
  type MemoryRow,
  type MemoryRevision,
} from "../utils/memoryRows";

const LAYER1_KINDS: { kind: string; heading: string }[] = [
  { kind: "rule", heading: "Rules your AIs follow" },
  { kind: "command", heading: "Commands" },
  { kind: "location", heading: "Where things are" },
  { kind: "decision", heading: "Decisions" },
];
const isLayer1 = (r: MemoryRow) => LAYER1_KINDS.some((k) => k.kind === r.kind);

interface Props {
  /** The scope to show: `person`, or an AI's by founding key. When `aiId`
   *  is given instead, the AI's founding key is looked up and the scope
   *  is `ai:<founding>`. */
  scope?: string;
  aiId?: string;
  /** Rows are written through this agent (an AI's own for its scope). */
  agentKey?: string;
  readOnly?: boolean;
  /** Bump to reload (a parent added a row). */
  refresh?: number;
  /** Heading for the rules group ("Rules your AIs follow" / "Rules this AI follows"). */
  rulesHeading?: string;
}

export default component$<Props>((props) => {
  const rows = useSignal<MemoryRow[]>([]);
  const dreams = useSignal<MemoryRevision[]>([]);
  const revisions = useSignal<MemoryRevision[]>([]);
  const dreamsOpen = useSignal(false);
  // What each session left behind (one `episode` row per session, raw, in
  // the digest's own words - MEMORY_REDESIGN.md §5.5): shown so the person
  // sees what the next session will carry and can drop any of it.
  const episodes = useSignal<MemoryRow[]>([]);
  const sessionsOpen = useSignal(false);
  const openEpisode = useSignal<string | null>(null);
  const editingId = useSignal<string | null>(null);
  const editValue = useSignal("");
  const scope = useSignal("");

  const load$ = $(async () => {
    if (!scope.value) return;
    try {
      // The saved copy answers at once; the live read below can take a
      // while when the records are busy, and until it did, a project's
      // Sessions did not show at all (10-19).
      const cached = await listMemoryRowsCached(scope.value);
      rows.value = cached.filter(isLayer1);
      episodes.value = cached.filter((r) => r.kind === "episode" && !r.deleted).sort((a, b) => b.as_of - a.as_of);
    } catch { /* older backend */ }
    try {
      const all = await listMemoryRows(scope.value, props.agentKey);
      rows.value = all.filter(isLayer1);
      episodes.value = all.filter((r) => r.kind === "episode" && !r.deleted).sort((a, b) => b.as_of - a.as_of);
    } catch (e) {
      console.warn("[MemoryRows] rows did not answer (cached shown):", e);
    }
    try {
      const all = await listMemoryRevisions(scope.value, props.agentKey);
      revisions.value = all;
      dreams.value = all.filter((r) => r.provenance.startsWith("dream:")).slice(0, 20);
    } catch { /* records not up yet */ }
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ track }) => {
    track(() => props.refresh);
    track(() => props.scope);
    track(() => props.aiId);
    if (props.scope) {
      scope.value = props.scope;
    } else if (props.aiId) {
      try {
        const { getLocalCustomAis } = await import("../utils/localAiStorage");
        const ai = (await getLocalCustomAis()).find((a) => a.id === props.aiId || a.agentPubKey === props.aiId);
        scope.value = aiScope(ai?.foundingKey || ai?.agentPubKey || props.aiId);
      } catch {
        scope.value = aiScope(props.aiId);
      }
    }
    await load$();
  });

  return (
    <>
      {dreams.value.length > 0 && (
        <div class="mb-5 p-4 rounded-xl bg-[var(--bg-main)] border border-[var(--border-subtle)]" data-testid="memory-dreams">
          <div class="flex items-center justify-between gap-3">
            <p class="text-xs font-semibold text-[var(--text-muted)] uppercase tracking-wide">Tidied while you were away</p>
            <button type="button" class="text-xs text-[var(--text-link)] hover:underline" onClick$={() => (dreamsOpen.value = !dreamsOpen.value)}>
              {dreamsOpen.value ? "Hide" : `Show ${dreams.value.length}`}
            </button>
          </div>
          {!dreamsOpen.value && (
            <p class="mt-1 text-[11px] text-[var(--text-muted)]">
              Duplicates merged, notes filed, old entries moved to history - on this computer, by itself. Each change can be undone.
            </p>
          )}
          {dreamsOpen.value && (
            <div class="mt-2 flex flex-col gap-1.5">
              {dreams.value.map((d) => (
                <div key={d.action_hash ?? d.id + d.updated_at} class="flex items-start gap-2 text-sm" data-testid="memory-dream">
                  <span class="min-w-0 flex-1 text-[var(--text-secondary)]">
                    {describeDream(d, revisions.value)}
                    <span class="text-[var(--text-muted)]"> · {new Date(d.updated_at).toLocaleDateString()}</span>
                  </span>
                  {!props.readOnly && !d.superseded && (
                    <button
                      type="button"
                      class="text-xs text-[var(--text-link)] hover:underline shrink-0"
                      onClick$={async () => {
                        try {
                          await undoDream(d, revisions.value, props.agentKey);
                          await load$();
                        } catch (e) {
                          console.warn("[MemoryRows] undo failed:", e);
                        }
                      }}
                    >
                      Undo
                    </button>
                  )}
                </div>
              ))}
            </div>
          )}
        </div>
      )}
      {rows.value.length > 0 && (
        <div class="mb-5 p-4 rounded-xl bg-[var(--bg-main)] border border-[var(--border-subtle)]" data-testid="memory-rules">
          {LAYER1_KINDS.filter((k) => rows.value.some((r) => r.kind === k.kind)).map((k) => (
            <div key={k.kind} class="mb-3 last:mb-0">
              <p class="text-xs font-semibold text-[var(--text-muted)] uppercase tracking-wide">
                {k.kind === "rule" && props.rulesHeading ? props.rulesHeading : k.heading}
              </p>
              <div class="mt-2 flex flex-col gap-1.5">
                {rows.value.filter((r) => r.kind === k.kind).map((r) => (
                  <div key={r.id} class="group flex items-start gap-2" data-testid="memory-rule" data-kind={r.kind}>
                    {!props.readOnly && editingId.value === r.id ? (
                      <input
                        type="text"
                        value={editValue.value}
                        autoFocus
                        onInput$={(_, el) => (editValue.value = el.value)}
                        onKeyDown$={async (e) => {
                          if (e.key === "Escape") editingId.value = null;
                          if (e.key === "Enter") {
                            const text = editValue.value.trim();
                            editingId.value = null;
                            if (text && text !== r.text) {
                              await saveMemoryRow(revise(r, { text }), props.agentKey);
                              await load$();
                            }
                          }
                        }}
                        class="flex-1 px-2.5 py-1.5 rounded-lg bg-[var(--bg-card)] border border-[var(--border-subtle)] text-sm text-[var(--text-primary)] focus:outline-none focus:border-[var(--bg-button-primary)]"
                      />
                    ) : (
                      <p class="min-w-0 flex-1 text-sm text-[var(--text-primary)]">{r.text}</p>
                    )}
                    {!props.readOnly && editingId.value !== r.id && (
                      <div class="flex items-center gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                        <button
                          type="button"
                          class="p-1 rounded text-[var(--text-muted)] hover:text-[var(--text-primary)]"
                          title="Edit"
                          onClick$={() => {
                            editingId.value = r.id;
                            editValue.value = r.text;
                          }}
                        >
                          <LuPencil class="w-3.5 h-3.5" />
                        </button>
                        <button
                          type="button"
                          class="p-1 rounded text-[var(--text-muted)] hover:text-red-400"
                          title="Remove rule"
                          onClick$={async () => {
                            await saveMemoryRow(deletion(r), props.agentKey);
                            await load$();
                          }}
                        >
                          <LuTrash2 class="w-3.5 h-3.5" />
                        </button>
                      </div>
                    )}
                  </div>
                ))}
              </div>
            </div>
          ))}
          <p class="mt-2 text-[11px] text-[var(--text-muted)]">
            Kept in your own records, so they follow you to your other devices. Every AI is reminded of them when it answers.
          </p>
        </div>
      )}
      {episodes.value.length > 0 && (
        <div class="mb-5 p-4 rounded-xl bg-[var(--bg-main)] border border-[var(--border-subtle)]" data-testid="memory-sessions">
          <button
            type="button"
            class="flex items-center gap-1.5 text-xs font-semibold text-[var(--text-muted)] uppercase tracking-wide bg-transparent border-none cursor-pointer p-0"
            onClick$={() => (sessionsOpen.value = !sessionsOpen.value)}
            data-testid="memory-sessions-toggle"
          >
            {sessionsOpen.value ? <LuChevronDown class="w-3.5 h-3.5" /> : <LuChevronRight class="w-3.5 h-3.5" />}
            Sessions ({episodes.value.length})
          </button>
          {sessionsOpen.value && (
            <div class="mt-2 flex flex-col gap-1.5">
              {episodes.value.map((r) => {
                const firstLine = r.text.split("\n").find((l) => l.trim()) ?? "";
                const open = openEpisode.value === r.id;
                return (
                  <div key={r.id} class="group flex items-start gap-2" data-testid="memory-episode">
                    <button
                      type="button"
                      class="min-w-0 flex-1 text-left bg-transparent border-none cursor-pointer p-0"
                      onClick$={() => (openEpisode.value = open ? null : r.id)}
                    >
                      <span class="text-[11px] text-[var(--text-muted)] mr-2">{new Date(r.as_of).toLocaleDateString(undefined, { day: "numeric", month: "short" })}</span>
                      <span class={`text-sm text-[var(--text-primary)] ${open ? "whitespace-pre-wrap" : ""}`}>
                        {open ? r.text : firstLine.length > 140 ? firstLine.slice(0, 140) + "…" : firstLine}
                      </span>
                    </button>
                    {!props.readOnly && (
                      <button
                        type="button"
                        class="p-1 rounded text-[var(--text-muted)] hover:text-red-400 opacity-0 group-hover:opacity-100 transition-opacity"
                        title="Forget this session"
                        onClick$={async () => {
                          await saveMemoryRow(deletion(r), props.agentKey);
                          await load$();
                        }}
                      >
                        <LuTrash2 class="w-3.5 h-3.5" />
                      </button>
                    )}
                  </div>
                );
              })}
              <p class="mt-1 text-[11px] text-[var(--text-muted)]">
                What each session left behind, in its own words. The next session starts from these; dreaming folds them into the notes above over time.
              </p>
            </div>
          )}
        </div>
      )}
    </>
  );
});
