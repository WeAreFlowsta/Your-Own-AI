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
import type { AgentAction, AgentLogItem } from "../types";

interface AgentWorkingBoxProps {
  log: AgentLogItem[];
  /** True while the turn is streaming - keeps the box open + animating. */
  working: boolean;
  theme: "light" | "dark";
  onPermissionRespond$?: QRL<
    (requestId: number, decision: "allow" | "reject", always: boolean) => void
  >;
  onPermissionOffscreen$?: QRL<(offscreen: boolean) => void>;
}

const SHOW_THOUGHTS_KEY = "agent-show-thoughts";

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

/**
 * The agent turn's working box - lives INSIDE the reply bubble, above the
 * final answer. Steps tick with humanized labels and expandable real results,
 * the AI's mid-work narration streams muted between them, permission cards
 * interrupt at their true position, and a shimmer line at the bottom always
 * shows the current activity while the turn runs. Collapses to a one-line
 * summary when the turn ends. The "brain" toggle also streams the model's
 * live thinking (off by default).
 */
export const AgentWorkingBox = component$<AgentWorkingBoxProps>(
  ({ log, working, theme, onPermissionRespond$, onPermissionOffscreen$ }) => {
    const userExpanded = useSignal<boolean | undefined>(undefined);
    const showThoughts = useSignal(false);
    const openOutputs = useSignal<Record<string, boolean>>({});

    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(() => {
      try {
        showThoughts.value = localStorage.getItem(SHOW_THOUGHTS_KEY) === "1";
      } catch {
        /* default off */
      }
    });

    const expanded = useComputed$(() => userExpanded.value ?? working);

    // The CURRENT words render below the box at full size - they are what
    // the AI is saying right now, and if the turn ends here they ARE the
    // answer (finishTurn promotes them to the bubble body in place - no
    // visible move, no reprint). Only when more work follows do they slide
    // up into the box as muted history. Keeping the streaming text OUT of
    // the keyed list also matters: innerHTML updating next to keyed rows
    // made reconciliation duplicate the neighboring row.
    const trailingNarration = useComputed$(() => {
      const last = log[log.length - 1];
      return working && last?.type === "narration" ? last : null;
    });
    const boxItems = useComputed$(() =>
      trailingNarration.value ? log.slice(0, -1) : log,
    );

    const summary = useComputed$(() => {
      const actions = log.filter((i) => i.type === "action");
      const perms = log.filter((i) => i.type === "permission").length;
      const files = new Set<string>();
      for (const i of actions)
        for (const p of (i as { action: AgentAction }).action.locations ?? [])
          files.add(p);
      const parts: string[] = [];
      parts.push(
        files.size > 0
          ? `Worked in ${files.size} file${files.size === 1 ? "" : "s"} - ${actions.length} action${actions.length === 1 ? "" : "s"}`
          : `Did ${actions.length} action${actions.length === 1 ? "" : "s"}`,
      );
      if (perms > 0) parts.push(`${perms} permission${perms === 1 ? "" : "s"}`);
      return parts.join(" - ");
    });

    // The shimmer line: what is happening RIGHT NOW, specifically.
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

    return (
      <>
      {boxItems.value.length > 0 && (
      <div class="rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] text-sm overflow-hidden">
        <div class="flex items-center gap-1">
          <button
            onClick$={() => (userExpanded.value = !expanded.value)}
            class="flex flex-1 items-center gap-2 px-3 py-2 text-left text-[var(--text-muted)] hover:text-[var(--text-secondary)] min-w-0"
          >
            {expanded.value ? (
              <LuChevronDown class="h-3.5 w-3.5 shrink-0" />
            ) : (
              <LuChevronRight class="h-3.5 w-3.5 shrink-0" />
            )}
            {/* Liveness = the app's proven movers (Lottie + pulse gradient).
                NOT background-clip:text - WebKitGTK leaves stale glyph
                paints when animated clip-text changes, and the old status
                words end up painted over the rows below. */}
            {working ? (
              <>
                <span class="shrink-0 w-3.5 h-3.5 overflow-hidden flex items-center justify-center">
                  <ThemeAwareLottie type="thinking" theme={theme} size={14} />
                </span>
                <span class="flex-1 min-w-0 truncate animate-pulse-text status-text-gradient">
                  {status.value}
                </span>
              </>
            ) : (
              <span class="flex-1 min-w-0 truncate">{summary.value}</span>
            )}
          </button>
          <button
            onClick$={() => {
              showThoughts.value = !showThoughts.value;
              try {
                localStorage.setItem(SHOW_THOUGHTS_KEY, showThoughts.value ? "1" : "0");
              } catch {
                /* not persisted */
              }
            }}
            title={showThoughts.value ? "Hide thinking" : "Show thinking"}
            class={`px-3 py-2 shrink-0 ${showThoughts.value ? "text-[var(--text-secondary)]" : "text-[var(--text-muted)] opacity-50 hover:opacity-100"}`}
          >
            <LuBrain class="h-3.5 w-3.5" />
          </button>
        </div>

        {expanded.value && (
          <div class="px-3 pb-3 space-y-1.5">
            {/* Keys are the items' STABLE ids - index keys made Qwik
                re-match elements across item types during rapid inserts
                and mis-nest rows into each other (text over text). */}
            {boxItems.value.map((item) => {
              if (item.type === "thought") {
                return showThoughts.value ? (
                  <div
                    key={item.id}
                    class="text-xs italic text-[var(--text-muted)] opacity-80 leading-relaxed pl-5"
                  >
                    {item.text}
                  </div>
                ) : null;
              }
              if (item.type === "narration") {
                return (
                  <div
                    key={item.id}
                    class="markdown-content thinking-markdown text-xs text-[var(--text-secondary)] leading-relaxed pl-5 break-words overflow-hidden"
                    dangerouslySetInnerHTML={renderMarkdown(item.text)}
                  />
                );
              }
              if (item.type === "permission") {
                return (
                  <AgentPermissionCard
                    key={item.id}
                    permission={item.permission}
                    onRespond$={
                      onPermissionRespond$ &&
                      // eslint-disable-next-line qwik/valid-lexical-scope
                      ((decision: "allow" | "reject", always: boolean) =>
                        onPermissionRespond$(item.permission.requestId, decision, always))
                    }
                    onOffscreenChange$={onPermissionOffscreen$}
                  />
                );
              }
              const a = item.action;
              const Icon = actionIcon(a.kind);
              const hasOutput = !!a.output;
              const open = !!openOutputs.value[a.toolCallId];
              return (
                <div key={item.id} class="overflow-hidden">
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
                    {/* All three glyphs stay in the DOM; status only flips
                        classes. Swapping ELEMENTS in a ternary here left the
                        old glyph behind when status changed mid-stream
                        (gotcha: ternary branch swaps) - every done row wore
                        both its dot and its check. */}
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
                        {open ? (
                          <LuChevronDown class="h-3 w-3" />
                        ) : (
                          <LuChevronRight class="h-3 w-3" />
                        )}
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
      )}
      {/* The words being said right now. Same muted style as box narration
          so sliding into the box (when work follows) is seamless; the ONE
          restyle to full-size happens at turn end, when finishTurn promotes
          this text to the bubble body as the answer. */}
      {trailingNarration.value && (
        <div
          class="markdown-content thinking-markdown text-xs text-[var(--text-secondary)] leading-relaxed mt-1 px-3 break-words overflow-hidden"
          dangerouslySetInnerHTML={renderMarkdown(trailingNarration.value.text)}
        />
      )}
      </>
    );
  },
);
