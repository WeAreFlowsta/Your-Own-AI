import { component$, useComputed$, useSignal } from "@builder.io/qwik";
import {
  LuTerminal,
  LuPencil,
  LuFileText,
  LuGlobe,
  LuTrash2,
  LuSearch,
  LuWrench,
  LuCheck,
  LuX,
  LuChevronRight,
  LuChevronDown,
} from "@qwikest/icons/lucide";
import type { AgentRun } from "../types";

/**
 * One quiet collapsible card per contiguous run of agent tool calls.
 * Ticks line-by-line while the agent works; collapses to a one-line
 * summary ("Worked in N files - M actions") when the turn ends. Agent
 * text renders as normal bubbles around it, so the explain -> act ->
 * explain rhythm of a turn stays readable.
 */
export const AgentActivityCard = component$<{ run: AgentRun }>(({ run }) => {
  // User toggle wins; otherwise open while working, collapsed when done.
  const userExpanded = useSignal<boolean | undefined>(undefined);
  const expanded = useComputed$(() => userExpanded.value ?? !run.done);

  const files = useComputed$(() => {
    const set = new Set<string>();
    for (const a of run.actions) for (const p of a.locations ?? []) set.add(p);
    return set.size;
  });

  const latest = run.actions[run.actions.length - 1];
  const summary = run.done
    ? files.value > 0
      ? `Worked in ${files.value} file${files.value === 1 ? "" : "s"} - ${run.actions.length} action${run.actions.length === 1 ? "" : "s"}`
      : `Did ${run.actions.length} action${run.actions.length === 1 ? "" : "s"}`
    : latest?.title || "Working...";

  return (
    <div class="max-w-4xl mx-auto w-full">
      <div class="rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-card)] text-sm">
        <button
          onClick$={() => (userExpanded.value = !expanded.value)}
          class="flex w-full items-center gap-2 px-4 py-2 text-left text-[var(--text-muted)] hover:text-[var(--text-secondary)]"
        >
          {expanded.value ? (
            <LuChevronDown class="h-3.5 w-3.5 shrink-0" />
          ) : (
            <LuChevronRight class="h-3.5 w-3.5 shrink-0" />
          )}
          {!run.done && (
            <span class="inline-block w-1.5 h-1.5 rounded-full bg-[var(--text-link)] animate-pulse shrink-0" />
          )}
          <span class="truncate">{summary}</span>
        </button>

        {expanded.value && (
          <ul class="px-4 pb-3 space-y-1">
            {run.actions.map((a) => {
              const Icon =
                a.kind === "execute"
                  ? LuTerminal
                  : a.kind === "edit"
                    ? LuPencil
                    : a.kind === "delete"
                      ? LuTrash2
                      : a.kind === "fetch"
                        ? LuGlobe
                        : a.kind === "search"
                          ? LuSearch
                          : a.kind === "read"
                            ? LuFileText
                            : LuWrench;
              return (
                <li
                  key={a.toolCallId}
                  class="flex items-center gap-2 text-[var(--text-muted)]"
                >
                  {a.status === "completed" ? (
                    <LuCheck class="h-3.5 w-3.5 shrink-0 text-green-600 dark:text-green-400" />
                  ) : a.status === "failed" ? (
                    <LuX class="h-3.5 w-3.5 shrink-0 text-red-500 dark:text-red-400" />
                  ) : (
                    <span class="inline-block w-1.5 h-1.5 mx-1 rounded-full bg-[var(--text-link)] animate-pulse shrink-0" />
                  )}
                  <Icon class="h-3.5 w-3.5 shrink-0 opacity-70" />
                  <span class="truncate font-mono text-xs">{a.title}</span>
                </li>
              );
            })}
          </ul>
        )}
      </div>
    </div>
  );
});
