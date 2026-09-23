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
  LuChevronRight,
  LuChevronDown,
  LuInfo,
  LuRotateCcw,
  LuCopy,
  LuExternalLink,
} from "@qwikest/icons/lucide";
import { AgentPermissionCard } from "./AgentPermissionCard";
import { AgentDiffBlock } from "./AgentDiffBlock";
import { ActionIcon, formatElapsed, iconForKind } from "./ActionIcon";
import { renderMarkdown } from "../utils/renderMarkdown";
import { summaryOf } from "../utils/agentSummary";
import {
  AGENT_VIEW_EVENT,
  getAgentView,
  setAgentView,
  type AgentSurface,
  type AgentView,
} from "../utils/agentView";
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
  /** Only the LAST bubble carries the pearl - one point of life. */
  tipHere?: boolean;
  /** Kept for the message component; the rail no longer folds whole. */
  railOpen?: Signal<boolean>;
  /** apiDurationMs from the turn's usage - shown on the summary line. */
  durationMs?: number;
  /** The turn's token count, when known - shown on the pearl and summary. */
  tokens?: number;
  /** Undo this turn's file changes (the last finished turn only). */
  onUndoTurn$?: QRL<() => void>;
  /** This turn's changes were undone - the summary says so instead of offering it. */
  undone?: boolean;
  /** Live retry text ("Retrying (7/15) - context size exceeded..") - wins
   *  over everything on the pearl. */
  retryStatus?: string;
  /** Bare id of the online model the current call is waiting on, if online. */
  waitingOn?: string;
  /** Which surface this turn ran on: projects default to Detailed, chat
   *  with tools to Simple. */
  surface?: AgentSurface;
  /** For the header: "Teresa is working in Website". */
  aiName?: string;
  folderName?: string;
  onPermissionRespond$?: QRL<
    (requestId: number, decision: "allow" | "reject", always: boolean) => void
  >;
  onPermissionOffscreen$?: QRL<(offscreen: boolean) => void>;
}

/**
 * THE WORK RAIL - how an agent turn shows its work.
 *
 * The turn is a story: the AI SPEAKS (full-size, unboxed text) and between
 * its paragraphs the AI WORKS - one row per step with a grey glyph, a
 * plain label, what it touched, its time. Simple folds consecutive steps
 * of one family into one row ("Read 4 files and searched twice") and
 * hides thoughts; Detailed shows every row, thought and live line as it
 * arrives. Live and reopened render the same list, so a turn reads the
 * same later. Color means status only: a ring while a step runs, red
 * when it failed. Permission cards stay in place, full width.
 */
type GroupItem =
  | { kind: "action"; id: string; action: AgentAction }
  | { kind: "thought"; id: string; text: string; at?: number; endedAt?: number }
  | { kind: "plan"; id: string; entries: AgentPlanEntry[] };

type FlowElement =
  | { kind: "text"; id: string; text: string }
  | { kind: "group"; id: string; items: GroupItem[] }
  | { kind: "permission"; id: string; permission: AgentPermission }
  | { kind: "notice"; id: string; text: string };

/** A folded family of steps in Simple view. */
type Family = { id: string; icon: string; label: string; actions: AgentAction[]; added: number; removed: number };

type RailRow =
  | { kind: "action"; id: string; action: AgentAction; line?: string }
  | { kind: "family"; id: string; family: Family }
  | { kind: "thought"; id: string; text: string; at?: number; endedAt?: number }
  | { kind: "plan"; id: string; entries: AgentPlanEntry[] };

const familyOf = (a: AgentAction): string | null => {
  const icon = a.icon ?? iconForKind(a.kind);
  if (icon === "read" || icon === "folder" || icon === "search") return "explore";
  if (icon === "edit" || icon === "delete") return "edit";
  if (icon === "run") return "run";
  if (icon === "web") return "web";
  return null;
};

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
const times = (n: number) => (n === 1 ? "once" : n === 2 ? "twice" : `${n} times`);

/** "Read 4 files and searched twice" for a run of explore steps, etc. */
function familyLabel(family: string, actions: AgentAction[]): { label: string; icon: string } {
  const icons = actions.map((a) => a.icon ?? iconForKind(a.kind));
  if (family === "explore") {
    const files = icons.filter((i) => i === "read").length;
    const folders = icons.filter((i) => i === "folder").length;
    const searches = icons.filter((i) => i === "search").length;
    const parts: string[] = [];
    if (files) parts.push(`Read ${plural(files, "file", "files")}`);
    if (folders) parts.push(`${parts.length ? "looked" : "Looked"} through ${plural(folders, "folder", "folders")}`);
    if (searches) parts.push(`${parts.length ? "searched" : "Searched"} ${times(searches)}`);
    return { label: joinParts(parts), icon: files ? "read" : folders ? "folder" : "search" };
  }
  if (family === "edit") {
    const edits = icons.filter((i) => i === "edit").length;
    const dels = icons.filter((i) => i === "delete").length;
    const parts: string[] = [];
    if (edits) parts.push(`Edited ${plural(edits, "file", "files")}`);
    if (dels) parts.push(`${parts.length ? "deleted" : "Deleted"} ${plural(dels, "file", "files")}`);
    return { label: joinParts(parts), icon: edits ? "edit" : "delete" };
  }
  if (family === "run") return { label: `Ran ${plural(actions.length, "command", "commands")}`, icon: "run" };
  const searches = actions.filter((a) => /^Searched the web/.test(a.labelDone ?? a.label)).length;
  const pages = actions.length - searches;
  const parts: string[] = [];
  if (searches) parts.push(`Searched the web ${times(searches)}`);
  if (pages) parts.push(`${parts.length ? "read" : "Read"} ${plural(pages, "page", "pages")}`);
  return { label: joinParts(parts), icon: "web" };
}

function joinParts(parts: string[]): string {
  if (parts.length <= 1) return parts[0] ?? "";
  return `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}`;
}

/** The rows of one group as a view shows them. Pure: nothing here may be
 *  captured by a closure, so it takes the view and the maps as arguments. */
function rowsFor(items: GroupItem[], simple: boolean, waitLines: Record<string, string>, knownIds: Set<string>): RailRow[] {
  const rows: RailRow[] = [];
  let run: { family: string; actions: AgentAction[]; id: string } | null = null;
  const flush = () => {
    if (!run) return;
    if (run.actions.length >= 2) {
      const { label, icon } = familyLabel(run.family, run.actions);
      let added = 0, removed = 0;
      for (const a of run.actions) {
        added += a.diff?.added ?? 0;
        removed += a.diff?.removed ?? 0;
      }
      rows.push({ kind: "family", id: `family-${run.id}`, family: { id: run.id, icon, label, actions: run.actions, added, removed } });
    } else {
      for (const a of run.actions) rows.push({ kind: "action", id: `action-${a.toolCallId}`, action: a, line: waitLines[a.toolCallId] });
    }
    run = null;
  };
  for (const it of items) {
    if (it.kind === "thought") {
      if (simple) continue;
      flush();
      rows.push(it);
      continue;
    }
    if (it.kind === "plan") {
      flush();
      rows.push(it);
      continue;
    }
    const a = it.action;
    if (a.parent && knownIds.has(a.parent)) continue; // shown under its helper
    if (simple && a.icon === "wait" && a.waitFor?.some((id) => knownIds.has(id))) {
      // The wait folds into the step it waits on.
      continue;
    }
    const fam = simple ? familyOf(a) : null;
    const settled = a.status === "completed" && a.liveLine === undefined;
    if (fam && settled) {
      if (run && run.family === fam) run.actions.push(a);
      else {
        flush();
        run = { family: fam, actions: [a], id: a.toolCallId };
      }
      continue;
    }
    flush();
    rows.push({ kind: "action", id: `action-${a.toolCallId}`, action: a, line: waitLines[a.toolCallId] });
  }
  flush();
  return rows;
}

function stepState(a: AgentAction, working: boolean) {
  const failed = a.status === "failed";
  const running = a.status === "in_progress" || a.status === "pending";
  const still = !working && a.liveLine !== undefined && !failed;
  return { failed, running: running || still, still };
}

function elapsedOf(a: AgentAction, running: boolean, now: number): string | undefined {
  if (a.startedAt == null) return undefined;
  const end = a.endedAt ?? (running ? now : undefined);
  if (end == null) return undefined;
  const ms = end - a.startedAt;
  return ms >= 10_000 ? formatElapsed(ms) : undefined;
}

/** A task log that follows its own tail: full scrollback, pinned to the
 *  bottom while streaming - scrolling up unpins (read history in peace),
 *  scrolling back down re-pins. */
const LiveLogPanel = component$<{ text: string; live: boolean }>(({ text, live }) => {
  const ref = useSignal<HTMLElement>();
  const pinned = useSignal(true);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    track(() => text);
    const el = ref.value;
    if (el && (pinned.value || !live)) el.scrollTop = el.scrollHeight;
  });

  return (
    <pre
      ref={ref}
      onScroll$={() => {
        const el = ref.value;
        if (el) pinned.value = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
      }}
      class="mt-1 mb-1.5 text-[11px] rounded-lg bg-[var(--bg-input)] border border-[var(--border-subtle)] p-2 whitespace-pre-wrap break-all font-mono text-[var(--text-secondary)] max-h-64 overflow-y-auto"
    >
      {text}
    </pre>
  );
});

const SEG_ON =
  "rounded-full px-3 py-[3px] text-[11px] font-medium text-[var(--text-primary)] bg-[var(--bg-main)] shadow-[inset_0_0_0_1px_var(--border-input)] cursor-pointer border-none";
const SEG_OFF =
  "rounded-full px-3 py-[3px] text-[11px] text-[var(--text-muted)] bg-transparent hover:text-[var(--text-secondary)] cursor-pointer border-none";

export const AgentWorkingBox = component$<AgentWorkingBoxProps>(
  ({
    log,
    working,
    tipHere = true,
    durationMs,
    tokens,
    retryStatus,
    waitingOn,
    surface = "project",
    aiName,
    folderName,
    onPermissionRespond$,
    onPermissionOffscreen$,
    onUndoTurn$,
    undone = false,
  }) => {
    const view = useSignal<AgentView>("detailed");
    const openOutputs = useSignal<Record<string, boolean>>({});
    const openFamilies = useSignal<Record<string, boolean>>({});
    const openThoughts = useSignal<Record<string, boolean>>({});
    const planOpen = useSignal(false);
    const now = useSignal(Date.now());

    // The view for THIS surface, and every change made anywhere (the
    // header control, Settings, Ctrl+O) lands here at once.
    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(({ cleanup }) => {
      view.value = getAgentView(surface);
      const onChange = (ev: Event) => {
        const d = (ev as CustomEvent).detail as { surface: AgentSurface; view: AgentView };
        if (d?.surface === surface) view.value = d.view;
      };
      window.addEventListener(AGENT_VIEW_EVENT, onChange);
      cleanup(() => window.removeEventListener(AGENT_VIEW_EVENT, onChange));
    });

    const detailed = useComputed$(() => view.value === "detailed");

    // A background task from this turn may still be writing after the
    // turn: its row keeps its clock while its line is fresh.
    const hasLive = useComputed$(() => log.some((i) => i.type === "action" && i.action.liveLine !== undefined));

    // A clock for elapsed times: ticks once a second while anything moves.
    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(({ track, cleanup }) => {
      const active = track(() => working || hasLive.value);
      if (!active) return;
      now.value = Date.now();
      const t = setInterval(() => (now.value = Date.now()), 1000);
      cleanup(() => clearInterval(t));
    });

    // While working, the trailing narration streams in a stable element at
    // the END of the flow (streaming innerHTML inside the keyed list
    // duplicates neighbors) - identical styling to a settled text block.
    const trailingNarration = useComputed$(() => {
      const last = log[log.length - 1];
      return working && last?.type === "narration" ? last : null;
    });

    // Detailed while working caps the rail's height with its own scroller.
    // The page's own scroll follows the pearl (ChatContainer), but a nested
    // scroller does not follow on its own: the newest rows sat out of view
    // and the jump pill could not reach them (Eric, 09-23). It follows its
    // bottom until the person scrolls up inside it; the pill re-arms it.
    const railRef = useSignal<HTMLElement>();
    const railFollow = useSignal(true);
    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(({ track, cleanup }) => {
      track(() => flow.value);
      const el = railRef.value;
      if (el && railFollow.value) {
        requestAnimationFrame(() => {
          el.scrollTop = el.scrollHeight;
        });
      }
      const onJump = () => {
        railFollow.value = true;
        const r = railRef.value;
        if (r) r.scrollTop = r.scrollHeight;
      };
      window.addEventListener("yoai-rail-jump", onJump);
      cleanup(() => window.removeEventListener("yoai-rail-jump", onJump));
    });

    const flow = useComputed$<FlowElement[]>(() => {
      const items = trailingNarration.value ? log.slice(0, -1) : log;
      const out: FlowElement[] = [];
      for (const item of items) {
        if (item.type === "narration") {
          out.push({ kind: "text", id: item.id, text: item.text });
        } else if (item.type === "notice") {
          out.push({ kind: "notice", id: item.id, text: item.text });
        } else if (item.type === "permission") {
          out.push({ kind: "permission", id: item.id, permission: item.permission });
        } else {
          const row: GroupItem =
            item.type === "thought"
              ? { kind: "thought", id: item.id, text: item.text, at: (item as any).at, endedAt: (item as any).endedAt }
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

    /** A wait's live line and elapsed belong to the step it waits on when
     *  Simple hides the wait row. */
    const waitLines = useComputed$(() => {
      const owner: Record<string, string> = {};
      for (const item of log) if (item.type === "action" && item.action.taskId) owner[item.action.taskId] = item.action.toolCallId;
      const m: Record<string, string> = {};
      for (const item of log) {
        if (item.type === "action" && item.action.waitFor?.length && item.action.liveLine) {
          for (const id of item.action.waitFor) m[owner[id] ?? id] = item.action.liveLine;
        }
      }
      return m;
    });

    const knownIds = useComputed$(() => {
      const s = new Set<string>();
      for (const item of log) {
        if (item.type !== "action") continue;
        s.add(item.action.toolCallId);
        if (item.action.taskId) s.add(item.action.taskId);
      }
      return s;
    });

    /** The harness is condensing the session's notes: a model call of its
     *  own, so seconds without an update are expected, not "quiet". */
    const condensing = useComputed$(() =>
      log.some((i) => i.type === "action" && i.action.kind === "compact" && i.action.status === "in_progress"),
    );

    /** A helper's steps (recorded with `parent` = the helper's tool-call
     *  id) nest under its row instead of running loose in the list. */
    const childrenOf = useComputed$(() => {
      const m: Record<string, AgentAction[]> = {};
      for (const item of log) {
        if (item.type === "action" && item.action.parent) {
          (m[item.action.parent] ??= []).push(item.action);
        }
      }
      return m;
    });


    const status = useComputed$(() => {
      if (retryStatus) return retryStatus;
      const last = log[log.length - 1];
      if (last?.type === "permission" && last.permission.state === "pending") {
        return "Waiting for you";
      }
      for (let i = log.length - 1; i >= 0; i--) {
        const item = log[i];
        if (item.type === "action" && (item.action.status === "in_progress" || item.action.status === "pending")) {
          if (item.action.liveLine && detailed.value) return item.action.liveLine.slice(0, 120);
          return item.action.label;
        }
      }
      for (let i = log.length - 1; i >= 0; i--) {
        const item = log[i];
        if (item.type === "action" && item.action.liveLine) {
          return detailed.value ? item.action.liveLine.slice(0, 120) : item.action.label;
        }
      }
      if (detailed.value && last?.type === "thought") {
        const tail = last.text.replace(/CONTEXT_OVERFLOW[A-Z_]*/g, "").trim().slice(-140);
        if (tail) return `Thinking: ${tail}`;
      }
      return "Thinking";
    });

    // Silent stretches are real (a reasoning model can think 30-50s before
    // its summary lands) - the pearl counts them up so stillness reads as
    // work, not a hang. Resets whenever anything new arrives.
    const lastChangeAt = useSignal(0);
    useTask$(({ track }) => {
      track(() => status.value);
      track(() => log.length);
      lastChangeAt.value = Date.now();
    });
    const stillSecs = useComputed$(() => {
      if (!working || !lastChangeAt.value) return 0;
      const s = Math.floor((now.value - lastChangeAt.value) / 1000);
      return s >= 6 ? s : 0;
    });
    const shownStatus = useComputed$(() => {
      if (status.value === "Thinking" && waitingOn && stillSecs.value >= 15) {
        return `Waiting on ${waitingOn} - no reply yet`;
      }
      return status.value;
    });

    /** When the turn began: the earliest recorded time in the log. */
    const startedAt = useComputed$(() => {
      let t: number | undefined;
      for (const item of log) {
        const at = item.type === "action" ? item.action.startedAt : (item as any).at;
        if (typeof at === "number" && (t === undefined || at < t)) t = at;
      }
      return t;
    });
    const turnElapsed = useComputed$(() => {
      if (durationMs && !working) return formatElapsed(durationMs);
      if (startedAt.value === undefined) return null;
      let end = now.value;
      if (!working) {
        end = startedAt.value;
        for (const item of log) {
          const e = item.type === "action" ? item.action.endedAt : (item as any).endedAt;
          if (typeof e === "number" && e > end) end = e;
        }
      }
      const ms = end - startedAt.value;
      return ms >= 1000 ? formatElapsed(ms) : null;
    });

    const changed = useComputed$(() => {
      const files = new Set<string>();
      let added = 0, removed = 0;
      for (const item of log) {
        if (item.type !== "action") continue;
        const a = item.action;
        if (a.diff) {
          files.add(a.diff.path || a.toolCallId);
          added += a.diff.added;
          removed += a.diff.removed;
        } else if ((a.icon ?? iconForKind(a.kind)) === "edit") {
          for (const p of a.locations ?? []) files.add(p);
        }
      }
      return { files: files.size, added, removed };
    });

    const summary = useComputed$(() => summaryOf(log));

    /** What "show every change" opens: each edit row, and the family it
     *  may be folded into. */
    const changeTargets = useComputed$(() => {
      const outputs: string[] = [];
      const families: string[] = [];
      for (const item of log) if (item.type === "action" && item.action.diff) outputs.push(item.action.toolCallId);
      for (const el of flow.value) {
        if (el.kind !== "group") continue;
        for (const r of rowsFor(el.items, !detailed.value, waitLines.value, knownIds.value)) if (r.kind === "family" && r.family.added + r.family.removed > 0) families.push(r.family.id);
      }
      return { outputs, families };
    });


    return (
      <div class="my-1 rounded-xl border border-[var(--border-divider)] bg-[var(--bg-main)] px-3 pt-2 pb-2">
        {/* Header: who is working where (live), or the turn's summary
            (finished); the view control on the right. */}
        <div class="flex items-center justify-between gap-3 text-xs text-[var(--text-secondary)]">
          <div class="flex min-w-0 items-center gap-2">
            {working ? (
              <>
                <span class="h-2 w-2 shrink-0 rounded-full bg-[var(--text-link)] animate-pulse" />
                <span class="min-w-0 truncate">
                  {aiName ? `${aiName} is working` : "Working"}
                  {folderName ? (
                    <>
                      {" in "}
                      <span class="text-[var(--text-primary)]">{folderName}</span>
                    </>
                  ) : null}
                </span>
              </>
            ) : (
              <span class="min-w-0 truncate text-[var(--text-muted)]">
                {summary.value || "No steps"}
                {turnElapsed.value ? ` · ${turnElapsed.value}` : ""}
                {tokens ? ` · ${tokens >= 1000 ? `${(tokens / 1000).toFixed(1)}k` : tokens} tokens` : ""}
              </span>
            )}
          </div>
          <div class="flex shrink-0 items-center gap-2">
            {!working && changed.value.files > 0 && (
              <button
                type="button"
                title="Show every change"
                onClick$={() => {
                  const open: Record<string, boolean> = { ...openOutputs.value };
                  const fams: Record<string, boolean> = { ...openFamilies.value };
                  for (const id of changeTargets.value.outputs) open[id] = true;
                  for (const id of changeTargets.value.families) fams[id] = true;
                  openOutputs.value = open;
                  openFamilies.value = fams;
                }}
                class="flex items-center gap-1.5 rounded-full border-none bg-transparent px-2 py-[2px] text-[11px] text-[var(--text-muted)] hover:text-[var(--text-secondary)] cursor-pointer"
              >
                {plural(changed.value.files, "file changed", "files changed")}
                {changed.value.added + changed.value.removed > 0 && (
                  <span class="font-mono">
                    <span class="text-green-600 dark:text-green-400">+{changed.value.added}</span>
                    {changed.value.removed > 0 && <span class="text-red-500 dark:text-red-400"> -{changed.value.removed}</span>}
                  </span>
                )}
              </button>
            )}
            <div
              class="flex items-center gap-[2px] rounded-full border border-[var(--border-subtle)] p-[2px]"
              role="group"
              aria-label="How much to show"
              title="Ctrl+O switches"
            >
              <button
                type="button"
                class={view.value === "simple" ? SEG_ON : SEG_OFF}
                aria-pressed={view.value === "simple"}
                onClick$={() => setAgentView(surface, "simple")}
              >
                Simple
              </button>
              <button
                type="button"
                class={view.value === "detailed" ? SEG_ON : SEG_OFF}
                aria-pressed={view.value === "detailed"}
                onClick$={() => setAgentView(surface, "detailed")}
              >
                Detailed
              </button>
            </div>
          </div>
        </div>

        {/* Undo: the whole turn's file changes back to how they were. */}
        {!working && changed.value.files > 0 && (undone || onUndoTurn$) && (
          <div class="mt-1.5 flex items-center gap-2 text-xs text-[var(--text-muted)]">
            {undone ? (
              <span>This turn's file changes were undone.</span>
            ) : (
              <button
                type="button"
                onClick$={() => onUndoTurn$?.()}
                class="btn-liquid-metal btn-secondary-metal flex items-center gap-1.5 px-3 py-1 text-xs"
                title="Put every file this turn changed back how it was"
              >
                <span class="shader-inner-fill" />
                <span class="btn-content flex items-center gap-1.5">
                  <LuRotateCcw class="h-3 w-3" />
                  Undo this turn's changes
                </span>
              </button>
            )}
          </div>
        )}

        {/* The story: speech and work in the order they happened. Detailed
            caps the height so the reply below stays in reach. */}
        <div
          ref={railRef}
          class={`mt-2 flex flex-col gap-1 ${detailed.value && working ? "max-h-[70vh] overflow-y-auto" : ""}`}
          onScroll$={(_, el) => {
            railFollow.value = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
          }}
        >
          {flow.value.map((el) => {
            if (el.kind === "notice") {
              return (
                <div key={el.id} class="flex items-start gap-2 py-1 text-sm leading-snug text-[var(--text-secondary)]">
                  <LuInfo class="mt-0.5 h-3.5 w-3.5 shrink-0" />
                  <span class="min-w-0 break-words">{el.text}</span>
                </div>
              );
            }
            if (el.kind === "text") {
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
            const rows = rowsFor(el.items, !detailed.value, waitLines.value, knownIds.value);
            return (
              <div key={el.id} class="flex flex-col gap-0.5 py-0.5">
                {rows.map((row) => {
                  if (row.kind === "plan") {
                    const entries = row.entries;
                    const doneCount = entries.filter((e) => e.status === "completed").length;
                    const current = entries.find((e) => e.status === "in_progress") ?? entries.find((e) => e.status === "pending");
                    const shown = planOpen.value || !current ? entries : [current];
                    return (
                      <div key={row.id} class="rounded-lg px-2 py-1">
                        <button
                          type="button"
                          onClick$={() => (planOpen.value = !planOpen.value)}
                          class="block w-full max-w-full bg-transparent border-none p-0 text-left cursor-pointer"
                        >
                          <div class="flex min-w-0 items-center gap-2.5 text-[13px]">
                            <ActionIcon icon="plan" />
                            <span class="min-w-0 truncate text-[var(--text-secondary)]">
                              {current ? current.content : "Plan done"}
                            </span>
                            <span class="shrink-0 text-xs text-[var(--text-muted)]">
                              {doneCount} of {entries.length}
                            </span>
                            <span class="ml-auto shrink-0 text-[var(--text-muted)] opacity-60">
                              <LuChevronRight class={`h-3.5 w-3.5 ${planOpen.value ? "hidden" : ""}`} />
                              <LuChevronDown class={`h-3.5 w-3.5 ${planOpen.value ? "" : "hidden"}`} />
                            </span>
                          </div>
                        </button>
                        {planOpen.value && (
                          <div class="ml-8 mt-1 flex flex-col gap-0.5">
                            {shown.map((en) => (
                              <div key={en.content} class="flex items-center gap-2 text-xs">
                                <span
                                  class={`inline-block h-[7px] w-[7px] shrink-0 rounded-full ${
                                    en.status === "completed"
                                      ? "bg-green-600 dark:bg-green-400"
                                      : en.status === "in_progress"
                                        ? "bg-[var(--text-link)]"
                                        : "bg-[var(--border-subtle)]"
                                  }`}
                                />
                                <span class={en.status === "completed" ? "text-[var(--text-muted)]" : "text-[var(--text-secondary)]"}>{en.content}</span>
                              </div>
                            ))}
                          </div>
                        )}
                      </div>
                    );
                  }
                  if (row.kind === "thought") {
                    const isOpen = !!openThoughts.value[row.id];
                    const end = row.endedAt ?? (working ? now.value : undefined);
                    const secs = row.at != null && end != null ? Math.max(1, Math.round((end - row.at) / 1000)) : null;
                    const live = working && row.endedAt == null;
                    return (
                      <div key={row.id} class="rounded-lg px-2 py-1">
                        <button
                          type="button"
                          onClick$={() => (openThoughts.value = { ...openThoughts.value, [row.id]: !isOpen })}
                          class="block w-full max-w-full bg-transparent border-none p-0 text-left cursor-pointer"
                        >
                          <div class="flex min-w-0 items-center gap-2.5 text-[13px]">
                            <ActionIcon icon="think" status={live ? "in_progress" : "completed"} />
                            <span class="min-w-0 truncate text-[var(--text-muted)]">
                              {live ? "Thinking" : secs != null ? `Thought for ${secs} s` : "Thought"}
                            </span>
                            <span class="ml-auto shrink-0 text-[var(--text-muted)] opacity-60">
                              <LuChevronRight class={`h-3.5 w-3.5 ${isOpen ? "hidden" : ""}`} />
                              <LuChevronDown class={`h-3.5 w-3.5 ${isOpen ? "" : "hidden"}`} />
                            </span>
                          </div>
                        </button>
                        {isOpen && (
                          <div class="ml-8 mt-1 mb-1 max-h-64 overflow-y-auto text-xs italic leading-relaxed text-[var(--text-muted)] break-words">
                            {row.text}
                          </div>
                        )}
                      </div>
                    );
                  }
                  if (row.kind === "family") {
                    const f = row.family;
                    const isOpen = !!openFamilies.value[f.id];
                    return (
                      <div key={row.id} class="overflow-hidden">
                        <button
                          type="button"
                          onClick$={() => (openFamilies.value = { ...openFamilies.value, [f.id]: !isOpen })}
                          class="block w-full max-w-full rounded-lg px-2 py-1 text-left text-[13px] bg-transparent border-none hover:bg-[var(--bg-card)] cursor-pointer"
                        >
                          <div class="flex min-w-0 max-w-full overflow-hidden items-center gap-2.5">
                            <ActionIcon icon={f.icon} status="completed" />
                            <span class="min-w-0 truncate text-[var(--text-secondary)]">{f.label}</span>
                            {f.added + f.removed > 0 && (
                              <span class="shrink-0 font-mono text-xs">
                                <span class="text-green-600 dark:text-green-400">+{f.added}</span>
                                {f.removed > 0 && <span class="text-red-500 dark:text-red-400"> -{f.removed}</span>}
                              </span>
                            )}
                            <span class="ml-auto shrink-0 text-[var(--text-muted)] opacity-60">
                              <LuChevronRight class={`h-3.5 w-3.5 ${isOpen ? "hidden" : ""}`} />
                              <LuChevronDown class={`h-3.5 w-3.5 ${isOpen ? "" : "hidden"}`} />
                            </span>
                          </div>
                        </button>
                        {isOpen && (
                          <div class="ml-4 flex flex-col gap-0.5 border-l border-[var(--border-divider)] pl-2">
                            {f.actions.map((a) => (
                              <ActionRow
                                key={a.toolCallId}
                                action={a}
                                open={!!openOutputs.value[a.toolCallId]}
                                onToggle$={() => {
                                  openOutputs.value = { ...openOutputs.value, [a.toolCallId]: !openOutputs.value[a.toolCallId] };
                                }}
                                elapsed={elapsedOf(a, false, now.value)}
                                {...stepState(a, working)}
                                live={false}
                                line={undefined}
                              />
                            ))}
                          </div>
                        )}
                      </div>
                    );
                  }
                  const a = row.action;
                  const st = stepState(a, working);
                  const explicit = openOutputs.value[a.toolCallId];
                  // Detailed shows every step open (its file or command, its
                  // output as it comes); a failed step opens in both views.
                  // A click on the row overrides either way.
                  const open = explicit !== undefined ? explicit : (st.failed && !!a.error) || detailed.value;
                  return (
                    <ActionRow
                      key={row.id}
                      action={a}
                      open={open}
                      onToggle$={() => {
                        openOutputs.value = { ...openOutputs.value, [a.toolCallId]: !open };
                      }}
                      elapsed={elapsedOf(a, st.running, now.value)}
                      failed={st.failed}
                      running={st.running}
                      still={st.still}
                      live={a.liveLine !== undefined && (working || st.still)}
                      line={a.liveLine ?? row.line ?? (!working && (a.taskId || a.icon === "helper") ? a.lastLine : undefined)}
                      steps={childrenOf.value[a.toolCallId]}
                      openChildren={openOutputs.value}
                      onToggleChild$={(id: string) => {
                        openOutputs.value = { ...openOutputs.value, [id]: !openOutputs.value[id] };
                      }}
                    />
                  );
                })}
              </div>
            );
          })}

          {trailingNarration.value && (
            <div key="trailing-narration-wrap">
              <div
                key="trailing-narration"
                class="markdown-content text-[var(--text-primary)] text-base leading-relaxed py-1.5 break-words overflow-hidden"
                dangerouslySetInnerHTML={renderMarkdown(trailingNarration.value.text)}
              />
            </div>
          )}
        </div>

        {/* The pearl: what is happening right now, how long, how much. */}
        {working && tipHere && (
          <div key="live-pearl" class="mt-2 flex items-center gap-2 border-t border-[var(--border-divider)] pt-2 text-xs text-[var(--text-secondary)]">
            <span class="h-2 w-2 shrink-0 rounded-full bg-[var(--text-link)] animate-pulse" />
            <span class="min-w-0 flex-grow truncate whitespace-nowrap">
              {shownStatus.value}
              {stillSecs.value > 0 && !condensing.value ? ` · ${stillSecs.value} s quiet` : ""}
            </span>
            <span class="shrink-0 text-[var(--text-muted)]">
              {turnElapsed.value ?? ""}
              {tokens ? ` · ${tokens >= 1000 ? `${(tokens / 1000).toFixed(1)}k` : tokens} tokens` : ""}
            </span>
          </div>
        )}
      </div>
    );
  },
);

interface ActionRowProps {
  action: AgentAction;
  open: boolean;
  onToggle$: QRL<() => void>;
  /** A helper's own steps, shown under it. */
  steps?: AgentAction[];
  openChildren?: Record<string, boolean>;
  onToggleChild$?: QRL<(id: string) => void>;
  elapsed?: string;
  failed: boolean;
  running: boolean;
  /** Finished turn, task still writing. */
  still: boolean;
  live: boolean;
  line?: string;
}

/** One step: `[icon] label … what it touched · elapsed · status ›`, and
 *  under it, when open, the diff, the output or the live log. */
const ActionRow = component$<ActionRowProps>(({ action: a, open, onToggle$, elapsed, failed, running, still, live, line, steps, openChildren, onToggleChild$ }) => {
  const hasDiff = !!a.diff?.lines?.length;
  const hasOutput = !!a.output || hasDiff || (failed && !!a.error) || !!a.detail;
  const label = a.status === "completed" && !still && a.labelDone ? a.labelDone : a.label;
  const icon = a.icon ?? iconForKind(a.kind);
  const isFile = icon === "read" || icon === "edit" || icon === "delete";
  const path = isFile ? (a.locations?.[0] ?? (a.detail && /[\\/]/.test(a.detail) ? a.detail : undefined)) : undefined;
  return (
    <div class="overflow-hidden">
      <button
        type="button"
        disabled={!hasOutput}
        onClick$={onToggle$}
        class={`block w-full max-w-full rounded-lg px-2 py-1 text-left text-[13px] bg-transparent border-none ${
          running ? "bg-[var(--bg-user-message)]" : ""
        } ${hasOutput ? "hover:bg-[var(--bg-card)] cursor-pointer" : "cursor-default"}`}
      >
        <div class="flex min-w-0 max-w-full items-center gap-2.5">
          <ActionIcon icon={a.icon} kind={a.kind} brand={a.server} status={failed ? "failed" : running ? "in_progress" : "completed"} />
          <span
            class={`min-w-0 truncate whitespace-nowrap ${
              failed ? "text-red-500 dark:text-red-400" : running ? "text-[var(--text-primary)]" : "text-[var(--text-secondary)]"
            }`}
          >
            {label}
          </span>
          {a.diff ? (
            <span class="shrink-0 font-mono text-xs">
              <span class="text-green-600 dark:text-green-400">+{a.diff.added}</span>
              {a.diff.removed > 0 && <span class="text-red-500 dark:text-red-400"> -{a.diff.removed}</span>}
            </span>
          ) : a.outputLines && !failed ? (
            <span class="shrink-0 text-xs text-[var(--text-muted)]">{a.outputLines} lines</span>
          ) : null}
          {failed && (
            <span class="shrink-0 text-xs text-red-500 dark:text-red-400">{elapsed ? `Failed after ${elapsed}` : "Failed"}</span>
          )}
          {!failed && elapsed && <span class="shrink-0 text-xs text-[var(--text-muted)]">{elapsed}</span>}
          {still && <span class="shrink-0 text-xs text-[var(--text-muted)]">still running</span>}
          {hasOutput && (
            <span class="ml-auto shrink-0 text-[var(--text-muted)] opacity-60">
              <LuChevronRight class={`h-3.5 w-3.5 ${open ? "hidden" : ""}`} />
              <LuChevronDown class={`h-3.5 w-3.5 ${open ? "" : "hidden"}`} />
            </span>
          )}
        </div>
        {line && !open && (
          <div class="ml-8 mt-0.5 truncate whitespace-nowrap font-mono text-xs text-[var(--text-muted)]">{line}</div>
        )}
      </button>
      {open && (
        <div class="ml-8 mb-1.5">
          {(a.detail || path) && (
            <div class="mt-1 flex items-center gap-2 text-xs text-[var(--text-muted)]">
              <span class="min-w-0 truncate font-mono">{path ?? a.detail}</span>
              {icon === "run" && a.detail && (
                <button
                  type="button"
                  title="Copy the command"
                  onClick$={async () => {
                    try {
                      await navigator.clipboard.writeText(a.detail ?? "");
                    } catch {
                      /* no clipboard */
                    }
                  }}
                  class="shrink-0 rounded-md border-none bg-transparent p-1 text-[var(--text-muted)] hover:text-[var(--text-secondary)] cursor-pointer"
                >
                  <LuCopy class="h-3.5 w-3.5" />
                </button>
              )}
              {path && (
                <button
                  type="button"
                  title="Open this file"
                  onClick$={async () => {
                    try {
                      const { openPath } = await import("@tauri-apps/plugin-opener");
                      await openPath(path);
                    } catch {
                      /* not on this computer */
                    }
                  }}
                  class="shrink-0 rounded-md border-none bg-transparent p-1 text-[var(--text-muted)] hover:text-[var(--text-secondary)] cursor-pointer"
                >
                  <LuExternalLink class="h-3.5 w-3.5" />
                </button>
              )}
              {failed && (
                <button
                  type="button"
                  onClick$={() => {
                    try {
                      window.dispatchEvent(
                        new CustomEvent("yoai-agent-interject", {
                          detail: { text: `The step "${label}" failed. Try it again.` },
                        }),
                      );
                    } catch {
                      /* not in a window */
                    }
                  }}
                  class="ml-auto btn-liquid-metal btn-secondary-metal flex shrink-0 items-center gap-1.5 px-3 py-[3px] text-[11px]"
                >
                  <span class="shader-inner-fill" />
                  <span class="btn-content flex items-center gap-1.5">
                    <LuRotateCcw class="h-3 w-3" />
                    Try again
                  </span>
                </button>
              )}
            </div>
          )}
          {failed && a.error && (
            <pre class="mt-1 max-h-40 overflow-auto whitespace-pre-wrap rounded-md border border-red-500/30 bg-[var(--bg-main)] px-2.5 py-2 font-mono text-xs text-[var(--text-secondary)]">
              {a.error}
            </pre>
          )}
          {hasDiff && (
            <div class="mt-1 max-h-64 overflow-y-auto">
              <AgentDiffBlock lines={a.diff!.lines!} />
            </div>
          )}
          {!hasDiff && a.output && (
            <LiveLogPanel text={a.output} live={live} />
          )}
          {!hasDiff && !a.output && !failed && line && (
            <div class="mt-1 font-mono text-xs text-[var(--text-muted)]">{line}</div>
          )}
        </div>
      )}
      {steps && steps.length > 0 && (
        <div class="ml-4 flex flex-col gap-0.5 border-l border-[var(--border-divider)] pl-2">
          {steps.map((c) => (
            <ActionRow
              key={c.toolCallId}
              action={c}
              open={!!openChildren?.[c.toolCallId]}
              onToggle$={() => onToggleChild$?.(c.toolCallId)}
              failed={c.status === "failed"}
              running={c.status === "in_progress" || c.status === "pending"}
              still={false}
              live={false}
              line={c.lastLine}
            />
          ))}
        </div>
      )}
    </div>
  );
});
