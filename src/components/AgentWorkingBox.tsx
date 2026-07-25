import {
  component$,
  useComputed$,
  useSignal,
  useTask$,
  useVisibleTask$,
  type QRL,
  type Signal,
} from "@builder.io/qwik";
import {
  LuCheck,
  LuChevronRight,
  LuChevronDown,
  LuBrain,
} from "@qwikest/icons/lucide";
import { AgentPermissionCard } from "./AgentPermissionCard";
import { AgentDiffBlock } from "./AgentDiffBlock";
import { renderMarkdown } from "../utils/renderMarkdown";
import type {
  AgentAction,
  AgentLogItem,
  AgentPermission,
  AgentPlanEntry,
} from "../types";

interface AgentWorkingBoxProps {
  log: AgentLogItem[];
  /** True while the turn is streaming - the rail is open with a live pearl. */
  working: boolean;
  /** Collapsed-stub expansion, owned by ChatMessage so the action bar's
   *  Steps button and the stub itself toggle the same state. */
  railOpen: Signal<boolean>;
  /** apiDurationMs from the turn's usage - shown on the stub. */
  durationMs?: number;
  /** Live retry text ("Retrying (7/15) - context size exceeded..") - wins
   *  over everything on the pearl: a retry loop must never look like a
   *  hang behind a stale action label. */
  retryStatus?: string;
  onPermissionRespond$?: QRL<
    (requestId: number, decision: "allow" | "reject", always: boolean) => void
  >;
  onPermissionOffscreen$?: QRL<(offscreen: boolean) => void>;
}

const SHOW_THOUGHTS_KEY = "agent-show-thoughts";
const METAL = "linear-gradient(180deg, #cfd8dc 0%, #59c9ff 45%, #7e99a6 100%)";
const METAL_H = "linear-gradient(90deg, #cfd8dc 0%, #59c9ff 45%, #7e99a6 100%)";

/**
 * THE WORK RAIL - how an agent turn shows its work.
 *
 * Two registers: the AI SPEAKS (full-size, unboxed text) and the AI WORKS
 * (a thin liquid-metal thread down an indented column; steps are nodes
 * with humanized labels and expandable real results; thoughts grow on the
 * same thread when the thinking dial is on). One point of life: the aqua
 * pearl at the thread's tip with the current action. Permission cards are
 * the only loud machinery and interrupt the thread at full width.
 *
 * When the turn ends the whole story folds into one stub line - check,
 * thread, "6 steps - 5 files - 40s" - leaving only the final answer full
 * size. The stub (and the action bar's Steps button) reopens the flow.
 */
type GroupItem =
  | { kind: "action"; id: string; action: AgentAction }
  | { kind: "thought"; id: string; text: string }
  | { kind: "plan"; id: string; entries: AgentPlanEntry[] };

type FlowElement =
  | { kind: "text"; id: string; text: string }
  | { kind: "group"; id: string; items: GroupItem[] }
  | { kind: "permission"; id: string; permission: AgentPermission };

function formatDuration(ms?: number): string | null {
  if (!ms || ms <= 0) return null;
  const s = Math.round(ms / 1000);
  if (s < 60) return `${s}s`;
  return `${Math.floor(s / 60)}m ${s % 60}s`;
}

export const AgentWorkingBox = component$<AgentWorkingBoxProps>(
  ({ log, working, railOpen, durationMs, retryStatus, onPermissionRespond$, onPermissionOffscreen$ }) => {
    const showThoughts = useSignal(true);
    const openOutputs = useSignal<Record<string, boolean>>({});

    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(() => {
      try {
        // Full detail is the DEFAULT (absent = on): the living rail is the
        // product. "0" = the simple view - same liveness, less verbosity.
        showThoughts.value = localStorage.getItem(SHOW_THOUGHTS_KEY) !== "0";
      } catch {
        showThoughts.value = true;
      }
    });

    // While working, the trailing narration streams in a stable element at
    // the END of the flow (streaming innerHTML inside the keyed list
    // duplicates neighbors) - identical styling to a settled text block.
    const trailingNarration = useComputed$(() => {
      const last = log[log.length - 1];
      return working && last?.type === "narration" ? last : null;
    });

    const flow = useComputed$<FlowElement[]>(() => {
      const items = trailingNarration.value ? log.slice(0, -1) : log;
      const out: FlowElement[] = [];
      for (const item of items) {
        if (item.type === "narration") {
          out.push({ kind: "text", id: item.id, text: item.text });
        } else if (item.type === "permission") {
          out.push({ kind: "permission", id: item.id, permission: item.permission });
        } else {
          const row: GroupItem =
            item.type === "thought"
              ? { kind: "thought", id: item.id, text: item.text }
              : item.type === "plan"
                ? { kind: "plan", id: item.id, entries: item.entries }
                : { kind: "action", id: item.id, action: item.action };
          const last = out[out.length - 1];
          if (last?.kind === "group") {
            last.items = [...last.items, row];
          } else {
            out.push({ kind: "group", id: item.id, items: [row] });
          }
        }
      }
      return out;
    });

    const status = useComputed$(() => {
      if (retryStatus) return retryStatus;
      const last = log[log.length - 1];
      if (last?.type === "permission" && last.permission.state === "pending") {
        return "Waiting for you..";
      }
      for (let i = log.length - 1; i >= 0; i--) {
        const item = log[i];
        if (item.type === "action" && (item.action.status === "in_progress" || item.action.status === "pending")) {
          return `${item.action.label}..`;
        }
      }
      // A background task's live log line beats a bare "Thinking.." - the
      // step completed instantly (backgrounded) but the work is right here.
      // Full-detail ambience; the simple view keeps labels + ticker only.
      if (showThoughts.value) {
        for (let i = log.length - 1; i >= 0; i--) {
          const item = log[i];
          if (item.type === "action" && item.action.liveLine) {
            return item.action.liveLine.slice(0, 120);
          }
        }
      }
      if (showThoughts.value && last?.type === "thought") {
        const tail = last.text.trim().slice(-140);
        return `Thinking.. ${tail}`;
      }
      return "Thinking..";
    });

    // Silent stretches are real (a reasoning model can think 30-50s before
    // its summary lands) - the pearl counts them up so stillness reads as
    // work, not a hang. Resets whenever anything new arrives.
    const lastChangeAt = useSignal(0);
    const nowTick = useSignal(0);
    useTask$(({ track }) => {
      track(() => status.value);
      track(() => log.length);
      lastChangeAt.value = Date.now();
    });
    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(({ track, cleanup }) => {
      const active = track(() => working);
      if (!active) return;
      const t = setInterval(() => (nowTick.value = Date.now()), 1000);
      cleanup(() => clearInterval(t));
    });
    const stillSecs = useComputed$(() => {
      if (!working || !lastChangeAt.value) return 0;
      const s = Math.floor((nowTick.value - lastChangeAt.value) / 1000);
      return s >= 6 ? s : 0;
    });

    const stats = useComputed$(() => {
      let steps = 0;
      const files = new Set<string>();
      for (const item of log) {
        if (item.type === "action") {
          steps++;
          for (const p of item.action.locations ?? []) files.add(p);
        }
      }
      return { steps, files: files.size };
    });

    const expanded = working || railOpen.value;

    return (
      <>
        {/* Collapsed stub - the finished turn's one-line audit trail. */}
        {!working && (
          <button
            onClick$={() => (railOpen.value = !railOpen.value)}
            class="flex items-center gap-2.5 py-1 mb-1 text-left font-mono text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)] bg-transparent border-none cursor-pointer"
          >
            <LuCheck class="h-3.5 w-3.5 shrink-0 text-green-600 dark:text-green-400" />
            <span
              class="inline-block w-6 h-[2px] rounded-full opacity-60 shrink-0"
              style={{ background: METAL_H }}
            />
            <span>
              {stats.value.steps} step{stats.value.steps === 1 ? "" : "s"}
              {stats.value.files > 0 &&
                ` - ${stats.value.files} file${stats.value.files === 1 ? "" : "s"}`}
              {formatDuration(durationMs) ? ` - ${formatDuration(durationMs)}` : ""}
            </span>
            <span class="shrink-0 text-[var(--text-link)]">
              <LuChevronRight class={`h-3 w-3 ${railOpen.value ? "hidden" : ""}`} />
              <LuChevronDown class={`h-3 w-3 ${railOpen.value ? "" : "hidden"}`} />
            </span>
          </button>
        )}

        {expanded &&
          flow.value.map((el) => {
            if (el.kind === "text") {
              // The AI speaking - full size, unboxed. Same register as the
              // final answer, so nothing ever resizes.
              return (
                <div
                  key={el.id}
                  class="markdown-content text-[var(--text-primary)] text-base leading-relaxed py-1.5 break-words overflow-hidden"
                  dangerouslySetInnerHTML={renderMarkdown(el.text)}
                />
              );
            }
            if (el.kind === "permission") {
              return (
                <AgentPermissionCard
                  key={el.id}
                  permission={el.permission}
                  onRespond$={
                    onPermissionRespond$ &&
                    // eslint-disable-next-line qwik/valid-lexical-scope
                    ((decision: "allow" | "reject", always: boolean) =>
                      onPermissionRespond$(el.permission.requestId, decision, always))
                  }
                  onOffscreenChange$={onPermissionOffscreen$}
                />
              );
            }
            return (
              <div key={el.id} class="relative pl-7 my-1.5">
                {/* The thread. */}
                <div
                  class="absolute left-[8px] top-1 bottom-1 w-[2px] rounded-full opacity-50"
                  style={{ background: METAL }}
                />
                {el.items.map((row) => {
                  if (row.kind === "plan") {
                    // The agent's live checklist on the thread: entries tick
                    // pending -> in progress -> done via class flips (the
                    // whole list is replaced each update - keys are the
                    // entry text, which is stable while statuses change).
                    return (
                      <div key={row.id} class="py-0.5">
                        {row.entries.map((en) => (
                          <div
                            key={en.content}
                            class="relative flex items-baseline gap-2 py-0.5 font-mono text-xs"
                          >
                            <span
                              class={`absolute -left-[22.5px] top-[5px] w-[7px] h-[7px] rounded-full border-2 border-[var(--bg-main)] ${
                                en.status === "completed"
                                  ? "bg-green-600 dark:bg-green-400"
                                  : en.status === "in_progress"
                                    ? "bg-[var(--text-link)] animate-pulse"
                                    : "bg-[var(--border-subtle)]"
                              }`}
                            />
                            <span
                              class={`min-w-0 truncate whitespace-nowrap ${
                                en.status === "in_progress"
                                  ? "text-[var(--text-secondary)]"
                                  : en.status === "completed"
                                    ? "text-[var(--text-muted)]"
                                    : "text-[var(--text-muted)] opacity-60"
                              }`}
                            >
                              {en.content}
                            </span>
                          </div>
                        ))}
                      </div>
                    );
                  }
                  if (row.kind === "thought") {
                    return showThoughts.value ? (
                      <div
                        key={row.id}
                        class="relative text-xs italic text-[var(--text-muted)] opacity-80 leading-relaxed py-0.5 max-w-[56ch] break-words overflow-hidden"
                      >
                        {row.text}
                      </div>
                    ) : null;
                  }
                  const a = row.action;
                  const hasDiff = !!a.diff?.lines?.length;
                  const hasOutput = !!a.output || hasDiff;
                  const open = !!openOutputs.value[a.toolCallId];
                  return (
                    <div key={row.id} class="overflow-hidden">
                      <button
                        disabled={!hasOutput}
                        onClick$={() => {
                          openOutputs.value = {
                            ...openOutputs.value,
                            [a.toolCallId]: !openOutputs.value[a.toolCallId],
                          };
                        }}
                        class={`relative flex w-full items-baseline gap-2 py-0.5 text-left font-mono text-xs text-[var(--text-muted)] bg-transparent border-none ${hasOutput ? "hover:text-[var(--text-secondary)] cursor-pointer" : "cursor-default"}`}
                      >
                        {/* Node on the thread - class flips only, never
                            element swaps, in this streaming list. */}
                        <span
                          class={`absolute -left-[22.5px] top-[6px] w-[7px] h-[7px] rounded-full border-2 border-[var(--bg-main)] ${
                            a.status === "completed"
                              ? "bg-green-600 dark:bg-green-400"
                              : a.status === "failed"
                                ? "bg-red-500"
                                : "bg-[var(--text-link)] animate-pulse"
                          }`}
                        />
                        <span class="min-w-0 truncate whitespace-nowrap">
                          {a.label}
                        </span>
                        {a.diff ? (
                          <span class="shrink-0">
                            <span class="text-green-600 dark:text-green-400">+{a.diff.added}</span>
                            {a.diff.removed > 0 && (
                              <span class="text-red-500 dark:text-red-400"> -{a.diff.removed}</span>
                            )}
                          </span>
                        ) : a.outputLines ? (
                          <span class="shrink-0 opacity-50">{a.outputLines} lines</span>
                        ) : null}
                        {hasOutput && (
                          <span class="ml-auto shrink-0 opacity-40">
                            <LuChevronRight class={`h-3 w-3 ${open ? "hidden" : ""}`} />
                            <LuChevronDown class={`h-3 w-3 ${open ? "" : "hidden"}`} />
                          </span>
                        )}
                      </button>
                      {open && hasDiff && (
                        <div class="mt-1 mb-1.5 max-h-64 overflow-y-auto">
                          <AgentDiffBlock lines={a.diff!.lines!} />
                        </div>
                      )}
                      {open && !hasDiff && a.output && (
                        <pre class="mt-1 mb-1.5 text-[11px] rounded-lg bg-[var(--bg-input)] border border-[var(--border-subtle)] p-2 whitespace-pre-wrap break-all font-mono text-[var(--text-secondary)] max-h-64 overflow-y-auto">
                          {a.detail && a.detail !== a.output ? `${a.detail}\n\n` : ""}
                          {a.output}
                        </pre>
                      )}
                    </div>
                  );
                })}
              </div>
            );
          })}

        {/* The pearl - the turn's one point of life. Its OWN element after
            the flow, so it exists no matter what the rail ends with: a step
            group, an answered permission receipt, or spoken text. It used to
            live inside the last group only - a turn whose tail was a
            permission receipt had NO live indicator at all, which made every
            Allow before a long reasoning stretch look dead. */}
        {working && (
          <div class="relative pl-7 my-1.5">
            <div
              class="absolute left-[8px] top-0 bottom-1 w-[2px] rounded-full opacity-50"
              style={{ background: METAL }}
            />
            <div class="relative flex items-center gap-2 py-1">
              <span
                class="absolute -left-[23.5px] top-[7px] w-[9px] h-[9px] rounded-full bg-[var(--text-link)] animate-pulse"
                style={{ boxShadow: "0 0 10px 1px rgba(89,201,255,0.55)" }}
              />
              <span class="min-w-0 truncate whitespace-nowrap font-mono text-xs font-semibold animate-pulse-text status-text-gradient">
                {status.value}
                {stillSecs.value > 0 ? ` ${stillSecs.value}s` : ""}
              </span>
              <button
                onClick$={() => {
                  showThoughts.value = !showThoughts.value;
                  try {
                    localStorage.setItem(
                      SHOW_THOUGHTS_KEY,
                      showThoughts.value ? "1" : "0",
                    );
                  } catch {
                    /* not persisted */
                  }
                }}
                title={showThoughts.value ? "Simple view - just the steps and asks" : "Full detail - thoughts and live logs"}
                class={`ml-1 shrink-0 bg-transparent border-none cursor-pointer ${showThoughts.value ? "text-[var(--text-secondary)]" : "text-[var(--text-muted)] opacity-40 hover:opacity-100"}`}
              >
                <LuBrain class="h-3.5 w-3.5" />
              </button>
            </div>
          </div>
        )}

        {/* The words being said right now - same register as settled text
            and the final answer, streaming outside the keyed list. */}
        {trailingNarration.value && (
          <div
            class="markdown-content text-[var(--text-primary)] text-base leading-relaxed py-1.5 break-words overflow-hidden"
            dangerouslySetInnerHTML={renderMarkdown(trailingNarration.value.text)}
          />
        )}
      </>
    );
  },
);
