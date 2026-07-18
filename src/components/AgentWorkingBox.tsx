import {
  component$,
  useComputed$,
  useSignal,
  useVisibleTask$,
  type QRL,
} from "@builder.io/qwik";
import {
  LuTerminal,
  LuPencil,
  LuFileText,
  LuGlobe,
  LuTrash2,
  LuSearch,
  LuFolderOpen,
  LuWrench,
  LuCheck,
  LuX,
  LuChevronRight,
  LuChevronDown,
  LuBrain,
} from "@qwikest/icons/lucide";
import { AgentPermissionCard } from "./AgentPermissionCard";
import ThemeAwareLottie from "./ThemeAwareLottie";
import { renderMarkdown } from "../utils/renderMarkdown";
import type { AgentAction, AgentLogItem, AgentPermission } from "../types";

interface AgentWorkingBoxProps {
  log: AgentLogItem[];
  /** True while the turn is streaming - keeps the active box animating. */
  working: boolean;
  theme: "light" | "dark";
  onPermissionRespond$?: QRL<
    (requestId: number, decision: "allow" | "reject", always: boolean) => void
  >;
  onPermissionOffscreen$?: QRL<(offscreen: boolean) => void>;
}

const SHOW_THOUGHTS_KEY = "agent-show-thoughts";

/** The working flow: full-size text and quiet step boxes interleaved in
 *  true order, all inside the turn's single bubble. TEXT NEVER RESIZES OR
 *  MOVES - the streaming tail, its settled flow position, and the final
 *  answer (promoted to the bubble body at turn end) all share the same
 *  full-size style, so every hand-off is invisible. Only the step boxes
 *  change shape: expanded and ticking while the turn works, collapsed to
 *  their summary line when it ends. */
/** Rows inside a step box: actions and (behind the toggle) the thoughts
 *  that came between them, in true order. Thinking lives INSIDE the box -
 *  the header's "Thinking.." is the collapsed face of the same thing. */
type GroupItem =
  | { kind: "action"; id: string; action: AgentAction }
  | { kind: "thought"; id: string; text: string };

type FlowElement =
  | { kind: "text"; id: string; text: string }
  | { kind: "group"; id: string; items: GroupItem[] }
  | { kind: "permission"; id: string; permission: AgentPermission };

function actionIcon(kind?: string) {
  switch (kind) {
    case "execute":
      return LuTerminal;
    case "edit":
    case "write":
      return LuPencil;
    case "delete":
      return LuTrash2;
    case "fetch":
      return LuGlobe;
    case "search":
    case "grep":
      return LuSearch;
    case "list":
      return LuFolderOpen;
    case "read":
      return LuFileText;
    default:
      return LuWrench;
  }
}

function groupSummary(items: GroupItem[]): string {
  const actions = items.filter((i) => i.kind === "action");
  if (actions.length === 0) return "Thoughts";
  const files = new Set<string>();
  for (const i of actions)
    for (const p of (i as { action: AgentAction }).action.locations ?? [])
      files.add(p);
  return files.size > 0
    ? `Worked in ${files.size} file${files.size === 1 ? "" : "s"} - ${actions.length} action${actions.length === 1 ? "" : "s"}`
    : `Did ${actions.length} action${actions.length === 1 ? "" : "s"}`;
}

export const AgentWorkingBox = component$<AgentWorkingBoxProps>(
  ({ log, working, theme, onPermissionRespond$, onPermissionOffscreen$ }) => {
    const showThoughts = useSignal(false);
    const openGroups = useSignal<Record<string, boolean>>({});
    const openOutputs = useSignal<Record<string, boolean>>({});

    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(() => {
      try {
        showThoughts.value = localStorage.getItem(SHOW_THOUGHTS_KEY) === "1";
      } catch {
        /* default off */
      }
    });

    // While working, the trailing narration streams in a stable element at
    // the END of the flow (streaming innerHTML inside the keyed list
    // duplicates neighbors) - identical styling to a settled text block, so
    // its later move into the list is invisible.
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
          // Actions AND thoughts both live inside the step box, in order.
          const row: GroupItem =
            item.type === "thought"
              ? { kind: "thought", id: item.id, text: item.text }
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

    // The one live status: the last flow element's group while working.
    const status = useComputed$(() => {
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
      if (showThoughts.value && last?.type === "thought") {
        const tail = last.text.trim().slice(-140);
        return `Thinking.. ${tail}`;
      }
      return "Thinking..";
    });

    const lastFlowId = useComputed$(() => flow.value[flow.value.length - 1]?.id);

    return (
      <>
        {flow.value.map((el) => {
          if (el.kind === "text") {
            return (
              <div
                key={el.id}
                class="markdown-content bg-[var(--bg-assistant-message)] p-2 pl-0 rounded-lg text-[var(--text-primary)] text-base leading-relaxed break-words overflow-hidden"
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
          const isActive = working && el.id === lastFlowId.value;
          const expanded = openGroups.value[el.id] ?? working;
          const actionCount = el.items.filter((i) => i.kind === "action").length;
          // A thoughts-only stretch: while active it's the "Thinking.." box
          // (the header IS the collapsed thinking view); once done it only
          // exists when the thinking view is on.
          if (actionCount === 0 && !showThoughts.value && !isActive) {
            return null;
          }
          return (
            <div
              key={el.id}
              class="rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] text-sm overflow-hidden my-1"
            >
              <div class="flex items-center gap-1">
                <button
                  onClick$={() => {
                    openGroups.value = { ...openGroups.value, [el.id]: !expanded };
                  }}
                  class="flex flex-1 items-center gap-2 px-3 py-2 text-left text-[var(--text-muted)] hover:text-[var(--text-secondary)] min-w-0"
                >
                  {/* Both header modes stay in the DOM; classes flip.
                      (Element swaps in streaming UI leave orphans.) */}
                  <span
                    class={`flex items-center gap-2 min-w-0 ${isActive ? "" : "hidden"}`}
                  >
                    <span class="shrink-0 w-3.5 h-3.5 overflow-hidden flex items-center justify-center">
                      <ThemeAwareLottie type="thinking" theme={theme} size={14} />
                    </span>
                    <span class="min-w-0 truncate whitespace-nowrap animate-pulse-text status-text-gradient">
                      {status.value}
                    </span>
                  </span>
                  <span
                    class={`flex items-center gap-2 min-w-0 ${isActive ? "hidden" : ""}`}
                  >
                    <LuChevronRight class={`h-3.5 w-3.5 shrink-0 ${expanded ? "hidden" : ""}`} />
                    <LuChevronDown class={`h-3.5 w-3.5 shrink-0 ${expanded ? "" : "hidden"}`} />
                    <span class="min-w-0 truncate whitespace-nowrap">
                      {groupSummary(el.items)}
                    </span>
                  </span>
                </button>
                {isActive && (
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
                    title={showThoughts.value ? "Hide thinking" : "Show thinking"}
                    class={`px-3 py-2 shrink-0 ${showThoughts.value ? "text-[var(--text-secondary)]" : "text-[var(--text-muted)] opacity-50 hover:opacity-100"}`}
                  >
                    <LuBrain class="h-3.5 w-3.5" />
                  </button>
                )}
              </div>

              {expanded && (
                <div class="px-3 pb-3 space-y-1.5">
                  {el.items.map((row) => {
                    if (row.kind === "thought") {
                      return showThoughts.value ? (
                        <div
                          key={row.id}
                          class="text-xs italic text-[var(--text-muted)] opacity-80 leading-relaxed pl-5 break-words overflow-hidden"
                        >
                          {row.text}
                        </div>
                      ) : null;
                    }
                    const a = row.action;
                    const Icon = actionIcon(a.kind);
                    const hasOutput = !!a.output;
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
                          class={`flex w-full items-center gap-2 text-left text-[var(--text-muted)] ${hasOutput ? "hover:text-[var(--text-secondary)] cursor-pointer" : "cursor-default"}`}
                        >
                          {/* Glyphs flip classes only - never swap elements
                              in a streaming list. */}
                          <span class="shrink-0 w-3.5 h-3.5 flex items-center justify-center">
                            <LuCheck
                              class={`h-3.5 w-3.5 text-green-600 dark:text-green-400 ${a.status === "completed" ? "" : "hidden"}`}
                            />
                            <LuX
                              class={`h-3.5 w-3.5 text-red-500 dark:text-red-400 ${a.status === "failed" ? "" : "hidden"}`}
                            />
                            <span
                              class={`w-1.5 h-1.5 rounded-full bg-[var(--text-link)] animate-pulse ${a.status === "completed" || a.status === "failed" ? "hidden" : ""}`}
                            />
                          </span>
                          <Icon class="h-3.5 w-3.5 shrink-0 opacity-70" />
                          <span class="min-w-0 truncate whitespace-nowrap text-xs">
                            {a.label}
                            {a.outputLines ? (
                              <span class="opacity-60"> - {a.outputLines} lines</span>
                            ) : null}
                          </span>
                          {hasOutput && (
                            <span class="ml-auto shrink-0 opacity-50">
                              <LuChevronRight class={`h-3 w-3 ${open ? "hidden" : ""}`} />
                              <LuChevronDown class={`h-3 w-3 ${open ? "" : "hidden"}`} />
                            </span>
                          )}
                        </button>
                        {open && hasOutput && (
                          <pre class="mt-1 ml-5 text-[11px] rounded-lg bg-[var(--bg-input)] border border-[var(--border-subtle)] p-2 whitespace-pre-wrap break-all font-mono text-[var(--text-secondary)] max-h-64 overflow-y-auto">
                            {a.detail && a.detail !== a.output ? `${a.detail}\n\n` : ""}
                            {a.output}
                          </pre>
                        )}
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
          );
        })}
        {/* The words being said right now - identical style to a settled
            text block and to the final answer, so nothing ever appears to
            move or resize. */}
        {trailingNarration.value && (
          <div
            class="markdown-content bg-[var(--bg-assistant-message)] p-2 pl-0 rounded-lg text-[var(--text-primary)] text-base leading-relaxed break-words overflow-hidden"
            dangerouslySetInnerHTML={renderMarkdown(trailingNarration.value.text)}
          />
        )}
      </>
    );
  },
);
