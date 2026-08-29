/**
 * Folder (Build agent) session state for the chat.
 *
 * Owns the open-folder lifecycle and translates the agent bridge's Tauri
 * events (ACP session traffic) into the chat's message list. ONE reply
 * bubble per agent turn: the AI's current words stream into the bubble
 * body; everything along the way - tool steps with their real results,
 * superseded narration, model thoughts, permission asks - lives in the
 * bubble's working log (rendered by AgentWorkingBox). Text the AI was
 * saying gets demoted into the log as narration whenever more activity
 * follows it, so whatever is in the bubble body when the turn ends IS the
 * final answer, already streamed in place with full chrome.
 *
 * The conversation is the window: while a folder is open, every prompt
 * goes through the agent session instead of the direct model call.
 */

import { skillNameFromPath } from "../utils/skills";
import { $, useSignal, useStore, useVisibleTask$, type Signal } from "@builder.io/qwik";
import { v4 as uuidv4 } from "uuid";
import { startConversation, recordMessage } from "../utils/holochainTranscripts";
import {
  rememberConversationFolder,
  rememberLastConversation,
} from "../utils/conversationResume";
import type {
  AgentActionDiff,
  AgentPermission,
  Message,
  PermissionLedger,
  SelectedAiModel,
} from "../types";
import {
  buildSupportsAutoPermissions,
  permissionModeForFolder,
  setPermissionModeForFolder,
  type AgentPermissionMode,
} from "../utils/agentPermissions";
import { computeLineDiff } from "../utils/lineDiff";
import {
  getWorkspaceMemory,
  memoryPromptBlock,
  reviseWorkspaceMemory,
  saveWorkspaceMemory,
} from "../utils/workspaceMemory";
import type { UseChatState } from "./useChat";
import { extractOnlineError } from "../utils/onlineErrors";

export interface AgentSessionState {
  folderPath: string | null;
  /** project = a folder the person opened; tools = a scratch workspace the
   *  chat runs its tool-carrying turns in (no folder shown, no folder memory). */
  mode: "project" | "tools" | null;
  /** idle = no folder open; starting = process/handshake in flight;
   *  ready = session open, waiting for input; working = turn in flight;
   *  stopped = process exited while a folder was open (needs reopen). */
  status: "idle" | "starting" | "ready" | "working" | "stopped";
  statusNote: string;
  /** ACP request id of the permission ask currently awaiting an answer. */
  pendingPermissionId: number | null;
  /** True while the pending permission card is scrolled out of view -
   *  drives the floating jump pill. */
  pendingCardOffscreen: boolean;
  /** The current activity in a word or two ("Reading config.mjs..") -
   *  shown on the live pill when the user has scrolled away from the tip. */
  liveStatus: string;
  /** Set while the agent is retrying a failed model call ("Retrying (7/15)
   *  - context size exceeded..") - the rail's pearl shows this instead of a
   *  stale action label, so a retry loop never looks like a hang. */
  retryStatus: string;
  /** The model the local server routed the current agent call to (bare id,
   *  e.g. "gpt-5.6-sol") when it is ONLINE - so a silent stretch can be
   *  named for what it is: waiting on a provider, not local thinking. Empty
   *  for offline picks. */
  waitingOn: string;
  /** Why routing picked the serving model (from the agent-route event). */
  routeReason: string;
  /** Every file path the agent touched this session (viewer feed). */
  touchedFiles: string[];
  /** Set when a turn died on an overloaded upstream model: the explicit
   *  switch offer ("use <alt> for this session?"). Never a silent reroute. */
  overloadOffer: { failedName: string; alt: string; altName: string } | null;
  /** This session's permission mode: ask (default) or auto (ordinary
   *  project work runs unasked; every decision still recorded). */
  permissionMode: AgentPermissionMode;
  /** Installed harness supports auto permissions (v0.2.0+). When false the
   *  Auto controls are disabled and point at the update card. */
  autoPermissionsSupported: boolean;
}

/** Upstream-refusal signatures worth an offer: provider overload / rate
 *  limits, surfaced through the proxy as 429s. */
const OVERLOAD_RE = /overload|429|too many requests|rate.?limit/i;

/** A resumed conversation restores the transcript for the USER's eyes, but
 *  the agent process starts blank - it never saw those turns, so it forgets
 *  commands it gave ten minutes ago. This digest (built from the chain's
 *  own record) rides invisibly ahead of the first prompt of a session that
 *  opened onto existing history. Plain text = model-agnostic: whichever
 *  model serves the session reads the same past. */
function buildResumeDigest(messages: Message[]): string {
  const turns = messages.filter(
    (m) => (m.role === "user" || m.role === "assistant") && m.content && !m.error,
  );
  const recent = turns.slice(-12);
  const parts: string[] = [];
  let budget = 5000;
  for (const m of recent) {
    const cap = m.role === "user" ? 300 : 600;
    let text = m.content.replace(/\s+/g, " ").trim();
    if (text.length > cap) text = text.slice(0, cap) + " ..";
    if (text.length > budget) break;
    budget -= text.length;
    parts.push(`${m.role === "user" ? "User" : "You"}: ${text}`);
  }
  if (!parts.length) return "";
  return (
    "[Restored context - this conversation continued across a session restart. What happened earlier:]\n" +
    parts.join("\n") +
    "\n[End of restored context. Continue naturally - you can rely on the above, including any commands or plans you already worked out.]\n\n"
  );
}

export interface UseAgentSessionProps {
  chatState: UseChatState;
  selectedAi: Signal<SelectedAiModel>;
}

/** Where the agent lives: the recorded install path (written when the
 *  download completes, self-healed from the installer's record), with a
 *  repo-local fallback in DEV BUILDS ONLY so development never needs the
 *  download flow. Production has no fallback - no path recorded means not
 *  installed, and the install surfaces take over. */
const DEV_BINARY_FALLBACK = import.meta.env.DEV
  ? "/home/solar/Documents/Flowsta/Projects/FlowstaAuth/your-own-ai-build/target/release/your-own-ai-build"
  : "";

export function resolveBinaryPath(): string {
  try {
    return localStorage.getItem("build-binary-path") || DEV_BINARY_FALLBACK;
  } catch {
    return DEV_BINARY_FALLBACK;
  }
}

const RECENT_FOLDERS_KEY = "build-recent-folders";
const RECENT_FOLDERS_MAX = 6;

/** Recent workspaces, most-recent-first - feeds the header slot's menu. */
export function readRecentFolders(): string[] {
  try {
    const raw = JSON.parse(localStorage.getItem(RECENT_FOLDERS_KEY) || "[]");
    return Array.isArray(raw) ? raw.filter((p) => typeof p === "string") : [];
  } catch {
    return [];
  }
}

function recordRecentFolder(path: string) {
  try {
    const next = [path, ...readRecentFolders().filter((p) => p !== path)].slice(
      0,
      RECENT_FOLDERS_MAX,
    );
    localStorage.setItem(RECENT_FOLDERS_KEY, JSON.stringify(next));
  } catch {
    /* recents are a convenience */
  }
}

/** Name slug for the AI, matching the local server's slug rules. The bridge
 *  writes a `[model.<slug>]` entry into the agent's config (its catalog is
 *  config-defined - nothing is discovered from the server) and selects it
 *  with session/set_model; the entry's model string is `<slug>:agent`. */
function aiModelSlug(ai: SelectedAiModel): string {
  return (ai.label || ai.id)
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");
}

/** MCP tool names arrive as "<server>__<tool>" - never show that raw.
 *  Known tools get proper labels; unknown ones become readable words. */
function humanizeMcpName(name: string): string {
  const known: Record<string, string> = {
    "project-memory__remember_for_project": "Remember something for this project",
    "project-memory__read_project_memory": "Read the project's memory",
  };
  if (known[name]) return known[name];
  // "<server>__<tool>" -> "Tool words (server)": the server half says WHICH
  // integration the agent reached for, which is what a reader wants to know.
  const [server, ...rest] = name.split("__");
  const tool = rest.length ? rest.join("__") : name;
  const words = tool.replace(/[_-]+/g, " ").trim();
  const label = words.charAt(0).toUpperCase() + words.slice(1);
  return rest.length ? `${label} (${server.replace(/[_-]+/g, " ")})` : label;
}

/** Short human handle for a permission receipt: the command if there is
 *  one, else the tool title. Receipts may shorten; the CARD never does. */
function receiptSubject(p: AgentPermission): string {
  const s = p.command || p.detail || p.title;
  return s.length > 48 ? s.slice(0, 45) + "..." : s;
}

/** Step label for a just-allowed ask - the agent stays silent while it
 *  executes, so the rail grows the step itself the moment Allow is clicked. */
function permissionActionLabel(p: AgentPermission): string {
  const base = (s: string) => s.split("/").filter(Boolean).pop() || s;
  if (p.command) {
    const c = p.command.length > 48 ? p.command.slice(0, 45) + "..." : p.command;
    return `Running ${c}`;
  }
  if (p.kind === "edit" && p.locations?.length) return `Editing ${base(p.locations[0])}`;
  if (p.kind === "delete" && p.locations?.length) return `Deleting ${base(p.locations[0])}`;
  if (p.kind === "fetch") return "Fetching from the web";
  return p.title;
}

/** Honest scope wording for an "always" grant, derived from the agent's own
 *  option name - edits persist per session, commands/domains per folder. */
function alwaysScope(optionName: string): string {
  return optionName.toLowerCase().includes("session")
    ? "this session"
    : "always in this project";
}

/** First ACP diff content item on a tool call → a real rendered diff
 *  (permission cards and completed rail steps share this). */
function extractDiff(content: unknown): AgentActionDiff | undefined {
  if (!Array.isArray(content)) return undefined;
  const d = content.find((c: any) => c?.type === "diff");
  if (!d || typeof d.newText !== "string") return undefined;
  const { lines, added, removed } = computeLineDiff(
    typeof d.oldText === "string" ? d.oldText : null,
    d.newText,
  );
  // A no-op edit (identical text) has nothing to show - skip the block
  // rather than render a lone fold marker.
  if (added === 0 && removed === 0) {
    return { path: typeof d.path === "string" ? d.path : "", added, removed, lines: [] };
  }
  return {
    path: typeof d.path === "string" ? d.path : "",
    added,
    removed,
    // Rendering cap - a full-file rewrite should not flood the DOM.
    lines: lines.length > 1200 ? [...lines.slice(0, 1200), { sign: "…" as const, text: "" }] : lines,
  };
}

const basename = (p: string) => p.split("/").filter(Boolean).pop() || p;

/** Humanized step label + expandable detail from the tool call's real
 *  input (the `x.ai/tool` meta carries name/kind/label/input; rawInput is
 *  the fallback). "List `.`" becomes "Looking through the folder". */
function humanizeAction(update: any): { label: string; kind?: string; detail?: string; waitFor?: string[] } {
  const meta = update?._meta?.["x.ai/tool"] ?? {};
  const input = { ...(update.rawInput ?? {}), ...(meta.input ?? {}) };
  const kind: string | undefined = meta.kind || update.kind;
  const path = input.path || input.target_file || input.file;
  const dir = input.directory || input.target_directory;
  const cmd = input.command;
  const term = input.query || input.pattern || input.regex;
  switch (kind) {
    case "list":
      return {
        kind,
        label:
          dir && dir !== "." ? `Looking through ${basename(dir)}/` : "Looking through the project",
        detail: dir,
      };
    case "read": {
      // A file inside an installed skill: say which skill is in use.
      const skill = skillNameFromPath(path);
      if (skill) return { kind, label: `Using skill: ${skill} (${basename(path)})`, detail: path };
      return { kind, label: path ? `Reading ${basename(path)}` : "Reading files", detail: path };
    }
    case "edit":
    case "write":
      return { kind, label: path ? `Editing ${basename(path)}` : "Editing files", detail: path };
    case "delete":
      return { kind, label: path ? `Deleting ${basename(path)}` : "Deleting files", detail: path };
    case "search":
    case "grep":
      return {
        kind,
        label: term ? `Searching for "${term}"` : "Searching the project",
        detail: term,
      };
    case "execute":
      return {
        kind,
        label: cmd ? `Running ${cmd.length > 48 ? cmd.slice(0, 45) + "..." : cmd}` : "Running a command",
        detail: cmd,
      };
    case "fetch":
      return { kind, label: input.url ? `Fetching ${input.url}` : "Fetching from the web", detail: input.url };
    default: {
      // The agent's "get output / wait" step (title "Background Task") is
      // the WAIT, not the command: it blocks on task_ids that are the
      // tool-call ids of earlier backgrounded execute steps - and those ids
      // are exactly the terminal log filenames. Name it as a wait and carry
      // the ids so the tailer can show the awaited task's live line HERE,
      // where the eye is, instead of a bare "Background Task..".
      const taskIds: string[] = Array.isArray(input.task_ids)
        ? input.task_ids.filter((t: unknown) => typeof t === "string")
        : typeof input.task_id === "string" ? [input.task_id] : [];
      if (taskIds.length) {
        return {
          kind: "wait",
          label: "Waiting for a background task",
          detail: taskIds.length > 1 ? `${taskIds.length} tasks` : undefined,
          waitFor: taskIds,
        };
      }
      // `use_tool` is the agent's dispatch for MCP tools: the real tool
      // lives in input.tool_name ("project-memory__remember_for_project"),
      // while the title is just "Use Tool" - which the rail showed verbatim,
      // hiding the one thing that mattered. Name the real tool; keep its
      // arguments as the expandable detail.
      const mcpName: string | undefined =
        typeof input.tool_name === "string" ? input.tool_name : undefined;
      if (mcpName) {
        let detail: string | undefined;
        try {
          detail = input.tool_input && Object.keys(input.tool_input).length
            ? JSON.stringify(input.tool_input, null, 1).slice(0, 400)
            : undefined;
        } catch { /* unserializable input - label alone */ }
        return { kind: "mcp", label: humanizeMcpName(mcpName), detail };
      }
      const raw = meta.label || update.title || "Working...";
      return { kind, label: raw.includes("__") ? humanizeMcpName(raw) : raw };
    }
  }
}

/** Pull the step's real result out of a completion update - directory
 *  trees, file text, command output - for the expandable view. */
function actionOutput(update: any): { output?: string; outputLines?: number } {
  let text = "";
  const ro = update.rawOutput;
  if (ro && typeof ro === "object") {
    const c = ro.Content?.content ?? ro.content;
    if (typeof c === "string") text = c;
  } else if (typeof ro === "string") {
    text = ro;
  }
  if (!text && Array.isArray(update.content)) {
    text = update.content
      .map((c: any) => c?.content?.text ?? "")
      .filter(Boolean)
      .join("\n");
  }
  if (!text) return {};
  const outputLines = text.split("\n").length;
  return {
    output: text.length > 4000 ? text.slice(0, 4000) + "\n..." : text,
    outputLines,
  };
}

export function useAgentSession(props: UseAgentSessionProps) {
  const state = useStore<AgentSessionState>({
    folderPath: null,
    mode: null,
    status: "idle",
    statusNote: "",
    pendingPermissionId: null,
    pendingCardOffscreen: false,
    liveStatus: "",
    retryStatus: "",
    waitingOn: "",
    routeReason: "",
    touchedFiles: [],
    overloadOffer: null,
    permissionMode: "ask",
    autoPermissionsSupported: true,
  });

  // A prompt waiting for the session: typed before the handshake finished,
  // typed mid-turn, or typed as the answer to a permission card.
  const queued = useSignal<string | null>(null);
  // The most recent user prompt - the overload offer resends it after the
  // user accepts a model switch, so the failed question gets its answer.
  const lastPrompt = useSignal<string>("");
  // Set when a session comes up with conversation history already on
  // screen (a resume, or a folder reopen mid-conversation): the next
  // prompt carries the restored-context digest.
  const digestPending = useSignal(false);
  // Workspace memory: loaded from the chain at agent-ready, injected ahead
  // of the session's FIRST prompt, revised at session end when real work
  // happened.
  const workspaceMemory = useSignal("");
  const memoryPending = useSignal(false);
  const toolsPending = useSignal(false);
  const sessionTurns = useSignal(0);
  // Rolling digest refreshed at every turn end - session-end paths (New,
  // close, folder switch) fire AFTER the chat may already be reset, so the
  // reviser can never rebuild it from live messages.
  const sessionDigest = useSignal("");

  /** Distill a session digest into the project's memory (cheap routed
   *  model call, fire-and-forget - never blocks the UI). Clears the
   *  project's pending-digest safety copy on success or no-change. */
  const distillMemory$ = $(async (folderPath: string, digest: string) => {
    const ai = props.selectedAi.value;
    if (!ai.aiConfig?.agentPubKey || !digest) return;
    const clearPending = () => {
      try {
        localStorage.removeItem(`project-pending-digest:${folderPath}`);
      } catch {
        /* safety copy only */
      }
    };
    const revised = await reviseWorkspaceMemory(ai.id, workspaceMemory.value, digest);
    if (!revised || revised === workspaceMemory.value.trim()) {
      clearPending();
      return;
    }
    const ok = await saveWorkspaceMemory(
      { agentPubKey: ai.aiConfig.agentPubKey, label: ai.label },
      folderPath,
      revised,
    );
    if (ok) {
      workspaceMemory.value = revised;
      clearPending();
      console.log("[WorkspaceMemory] Revision written for", folderPath);
    }
  });

  /** Session end housekeeping: when the session did real work, distill it. */
  const reviseMemory$ = $(async (folderPath: string) => {
    if (sessionTurns.value === 0) return;
    sessionTurns.value = 0;
    await distillMemory$(folderPath, sessionDigest.value);
  });
  // The message id of the current turn's reply bubble.
  const turnId = useSignal<string | null>(null);

  const invokeTauri = $(async (cmd: string, args?: Record<string, unknown>) => {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke(cmd, args);
  });

  /** Record a user turn to the Holochain transcript (starting the
   *  conversation if needed) - the agent path bypasses useChat's recording,
   *  and "Holochain holds everything" is a headline feature. Text turns
   *  now; the rail/cards on-chain are the launch release gate. */
  const recordUserTurn = $(async (text: string) => {
    const ai = props.selectedAi.value;
    const agentKey = ai.aiConfig?.agentPubKey;
    if (!agentKey) {
      // No agent key = this AI never provisioned - NOTHING in this
      // conversation will record. Same loudness as the assistant side.
      console.error("[Agent] User turn NOT recorded - AI has no agent key (provisioning failed?)");
      return;
    }
    const model = ai.aiConfig?.model || "agent";
    if (!props.chatState.conversationHash) {
      const title = text.length > 80 ? text.slice(0, 80) + "..." : text;
      const hash = await startConversation(agentKey, ai.label, model, title);
      if (!hash) {
        console.error("[Agent] User turn NOT recorded - starting the conversation failed (see warning above)");
        return;
      }
      props.chatState.conversationHash = hash;
      props.chatState.messageSequence = 0;
      if (state.folderPath && state.mode === "project") rememberConversationFolder(hash, state.folderPath);
      rememberLastConversation({ hash, agentKey, aiId: ai.id, title });
    }
    const seq = props.chatState.messageSequence;
    props.chatState.messageSequence = seq + 1;
    const actionHash = await recordMessage(
      agentKey,
      props.chatState.conversationHash!,
      "user",
      text,
      seq,
      model,
    );
    if (!actionHash) {
      console.error(`[Agent] User turn record FAILED (seq ${seq}) - see warning above`);
    }
  });

  const recordAssistantTurn = $(async (bubble: Message) => {
    const ai = props.selectedAi.value;
    const agentKey = ai.aiConfig?.agentPubKey;
    const hash = props.chatState.conversationHash;
    // A turn can honestly end on an action with no closing words - the
    // working log is still the answer's substance and must reach the
    // transcript ("Holochain holds everything"). Only a turn with neither
    // words nor work has nothing to record.
    if (!agentKey || !hash) {
      // This is a LOST TURN - say so loudly instead of skipping in silence
      // (a missing conversation hash here has already cost two answers).
      console.error(
        `[Agent] Turn NOT recorded - missing ${agentKey ? "conversation hash" : "agent key"}`,
      );
      return;
    }
    if (!bubble.content && !(bubble.agentLog ?? []).length) return;
    const seq = props.chatState.messageSequence;
    props.chatState.messageSequence = seq + 1;
    const tokens = bubble.tokens;
    // Persist the working log WITHOUT step outputs (labels, statuses,
    // receipts, thoughts, counts are the audit trail; raw outputs are
    // heavy and reproducible). Old entries read back with no log at all.
    // ⚠️ The transcript DNA rejects entries over 1 MiB of ciphertext - a
    // 99-tool session's un-slimmed log crossed it and the WHOLE turn was
    // lost. Permission items get the same treatment as steps (receipt,
    // not bulk), and a size ladder below guarantees the write fits.
    let items = (bubble.agentLog ?? []).map((i) => {
      if (i.type === "action") {
        const { output: _output, diff, liveLine: _live, ...action } = i.action;
        return {
          ...i,
          action: diff
            ? // Keep the receipt (path + counts), drop the heavy lines.
              { ...action, diff: { path: diff.path, added: diff.added, removed: diff.removed } }
            : action,
        };
      }
      if (i.type === "thought" && i.text.length > 2000) {
        return { ...i, text: i.text.slice(0, 2000) + ".." };
      }
      if (i.type === "permission") {
        const p = i.permission;
        return {
          ...i,
          permission: {
            ...p,
            command:
              p.command && p.command.length > 500
                ? p.command.slice(0, 500) + ".."
                : p.command,
            detail:
              p.detail && p.detail.length > 2000 ? p.detail.slice(0, 2000) + ".." : p.detail,
            diff: p.diff
              ? { path: p.diff.path, added: p.diff.added, removed: p.diff.removed }
              : undefined,
            options: [],
          },
        };
      }
      return i;
    });
    // Size ladder: never lose a whole turn to an oversized log. Each rung
    // trades detail for fit; the last keeps the story's ends with an
    // honest gap marker.
    const jsonSize = (x: unknown) => JSON.stringify(x).length;
    const LOG_BUDGET = 700_000;
    if (jsonSize(items) > LOG_BUDGET) {
      items = items.filter((i) => i.type !== "thought");
    }
    if (jsonSize(items) > LOG_BUDGET) {
      items = items.map((i) =>
        i.type === "action" && i.action.detail && i.action.detail.length > 200
          ? { ...i, action: { ...i.action, detail: i.action.detail.slice(0, 200) + ".." } }
          : i,
      );
    }
    if (jsonSize(items) > LOG_BUDGET && items.length > 80) {
      // Permission decisions are the audit trail - they survive the trim
      // wherever they sat; only steps/narration in the middle are dropped.
      const head = items.slice(0, 40);
      const tail = items.slice(-40);
      const middlePerms = items.slice(40, -40).filter((i) => i.type === "permission");
      items = [
        ...head,
        ...middlePerms,
        {
          id: "log-trimmed",
          type: "narration" as const,
          text: `.. ${items.length - 80 - middlePerms.length} steps trimmed to fit the transcript ..`,
        },
        ...tail,
      ];
    }
    const actionHash = await recordMessage(
      agentKey,
      hash,
      "assistant",
      bubble.content,
      seq,
      bubble.servedBy || ai.aiConfig?.model || "agent",
      undefined,
      tokens && tokens.total_tokens
        ? {
            prompt_tokens: tokens.prompt_tokens ?? 0,
            completion_tokens: tokens.completion_tokens ?? 0,
            total_tokens: tokens.total_tokens,
          }
        : undefined,
      // Routing provenance: folder decisions belong in the on-chain audit
      // exactly like chat turns (the Settings ledger is only a live window).
      bubble.routingReason
        ? { routing_reason: bubble.routingReason, routing_task: "agent" }
        : undefined,
      {
        agentLog: items.length || bubble.permissionLedger
          ? { items, stats: bubble.agentStats, permissions: bubble.permissionLedger }
          : undefined,
        folderPath: state.folderPath ?? undefined,
      },
    );
    // recordMessage returns the entry's action hash, or null after logging
    // the failure - one line either way, so the console always answers
    // "did this turn reach the chain?".
    if (actionHash) {
      console.log(`[Agent] Turn recorded on-chain (seq ${seq})`);
    } else {
      console.error(`[Agent] Turn record FAILED (seq ${seq}) - see warning above`);
    }
  });

  /** The turn's single reply bubble, pushed at Enter. Mounting it anchors
   *  the question to the top and shows the avatar + action bar instantly. */
  const startTurnBubble = $((userText: string, attachedFiles?: string[]) => {
    const id = uuidv4();
    turnId.value = id;
    props.chatState.messages = [
      ...props.chatState.messages,
      {
        id: uuidv4(),
        role: "user",
        content: userText,
        model: "user",
        ...(attachedFiles?.length ? { attachedFiles } : {}),
      },
      {
        id,
        role: "assistant",
        content: "",
        model: props.selectedAi.value.id,
        aiLabel: props.selectedAi.value.label,
        aiImageUrl: props.selectedAi.value.imageUrl || undefined,
        isLoading: true,
        agentTurn: true,
        agentLog: [],
      },
    ];
    props.chatState.isLoading = true;
    // Fire-and-forget - chat always works even if the conductor is down.
    recordUserTurn(userText).catch(() => {});
  });

  /** Send a prompt into the live session (session must be ready). */
  const dispatchPrompt = $(async (text: string) => {
    state.status = "working";
    state.liveStatus = "Thinking..";
    props.chatState.isLoading = true;
    // First prompt after a resume: the restored-context digest rides ahead
    // of the question ON THE WIRE only - the bubble and the transcript keep
    // the clean question (the digest is derived from the chain; recording
    // it again would just duplicate history).
    let wire = text;
    if (digestPending.value) {
      digestPending.value = false;
      // Exclude the just-asked question + its loading bubble (appended by
      // startTurnBubble before dispatch on every path).
      const digest = buildResumeDigest(props.chatState.messages.slice(0, -2));
      if (digest) wire = digest + text;
    }
    if (memoryPending.value) {
      memoryPending.value = false;
      // Workspace memory leads (durable folder truths), then any restored
      // conversation context, then the question.
      const block = memoryPromptBlock(workspaceMemory.value);
      if (block) wire = block + wire;
    }
    if (toolsPending.value) {
      toolsPending.value = false;
      // How to use the tools this AI carries (from their listings) - once
      // per session, ahead of the first question, on the wire only.
      try {
        const { toolsGuidanceBlock } = await import("../utils/mcp");
        const block = await toolsGuidanceBlock(props.selectedAi.value.aiConfig?.mcp);
        if (block) wire = block + wire;
      } catch { /* no guidance this session */ }
    }
    try {
      await invokeTauri("send_agent_prompt", { text: wire });
    } catch (err) {
      state.status = "ready";
      props.chatState.isLoading = false;
      // The turn bubble has no turn to receive - drop it if still empty.
      const id = turnId.value;
      props.chatState.messages = props.chatState.messages.filter(
        (m) => !(m.id === id && m.content === "" && !(m.agentLog ?? []).length),
      );
      turnId.value = null;
      props.chatState.error = JSON.stringify({
        code: "AGENT_SEND_FAILED",
        message: String(err),
      });
    }
  });

  /** User prompt entry point while a folder is open. */
  /** Send a prompt. `extra.context` (attached documents' extracted text)
   *  rides on the WIRE ahead of the prompt but never into the bubble - the
   *  bubble shows a chip per `extra.files` name instead. Same split as the
   *  resume digest in dispatchPrompt: what the model reads vs what the
   *  user sees are different strings. */
  const sendPrompt$ = $(async (text: string, extra?: { context?: string; files?: string[] }) => {
    if (!text.trim() && !extra?.context) return;
    const wire = extra?.context ? `${extra.context}\n\n${text}` : text;
    lastPrompt.value = wire;
    state.overloadOffer = null;
    props.chatState.error = null;
    if (state.status !== "ready" && state.status !== "starting" && state.status !== "working") {
      props.chatState.error = JSON.stringify({
        code: "AGENT_NOT_RUNNING",
        message: "The project's agent is not running. Reopen the project.",
      });
      return;
    }
    await startTurnBubble(text, extra?.files);
    if (state.status === "ready") {
      await dispatchPrompt(wire);
    } else {
      // Working: turns are strictly sequential over ACP - hold until the
      // current turn completes. Starting: hold until agent-ready.
      queued.value = wire;
    }
  });

  /** Overload offer answers: accept = pin the alternative for the rest of
   *  this workspace session (router-side override, cleared on close), then
   *  re-ask the failed question. The Agent slot setting stays untouched. */
  const acceptOverloadOffer$ = $(async () => {
    const offer = state.overloadOffer;
    if (!offer) return;
    state.overloadOffer = null;
    try {
      await invokeTauri("set_agent_online_override", { model: offer.alt });
    } catch {
      return;
    }
    if (lastPrompt.value) await sendPrompt$(lastPrompt.value);
  });

  const dismissOverloadOffer$ = $(() => {
    state.overloadOffer = null;
  });

  // Live background-task visibility. The terminal log is the truth and it
  // OUTLIVES the turn: a backgrounded command keeps writing after its step
  // completes, after the user stops the turn, and into the next turn.
  // The old tailer only watched the CURRENT turn's steps and only while a
  // turn was running - a real session stopped a turn while a 10-minute
  // script ran, started another, and the rail went silent (and its pearl
  // froze on the last line it had read: "Hungary to sink barges" while
  // the file had long moved on). Now: tail execute steps from the recent
  // bubbles, keep tailing while a turn runs OR any watched log is still
  // growing, and let a log that has gone quiet fall back to its final
  // line rather than posing as live.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track, cleanup }) => {
    const working = track(() => props.chatState.isLoading);
    track(() => props.chatState.messages.length);
    if (!state.folderPath) return;
    // Bytes seen per log - growth means the task is still alive.
    const seen = new Map<string, number>();
    let idleTicks = 0;
    const timer = setInterval(async () => {
      // Recent bubbles' execute steps (last 3 agent turns): the previous
      // turn's background task is exactly the one still running.
      const bubbles = props.chatState.messages
        .filter((m) => m.role === "assistant" && (m.agentLog?.length ?? 0) > 0)
        .slice(-3);
      // Execute steps by their own id, plus every id a wait step is
      // blocked on (the awaited task IS an execute step, possibly from a
      // previous turn) - both read the same terminal log.
      const ids = Array.from(new Set(bubbles
        .flatMap((b) => b.agentLog ?? [])
        .flatMap((i) => {
          if (i.type !== "action") return [];
          if (i.action.kind === "execute") return [i.action.toolCallId];
          if (i.action.waitFor?.length) return i.action.waitFor;
          return [];
        })
        .filter(Boolean))).slice(-8);
      if (!ids.length) return;
      try {
        const logs = (await invokeTauri("read_agent_task_logs", {
          toolCallIds: ids,
        })) as Record<string, string>;
        if (!Object.keys(logs).length) return;
        let anyGrew = false;
        const alive = new Set<string>();
        for (const [id, text] of Object.entries(logs)) {
          const prev = seen.get(id) ?? -1;
          if (text.length !== prev) {
            // First sight of a log proves nothing about liveness (a
            // finished task's log is "new" to a freshly armed tailer) -
            // count it alive only while the turn is working; after that,
            // only real growth between ticks does.
            if (prev !== -1) { anyGrew = true; alive.add(id); }
            else if (working) alive.add(id);
            seen.set(id, text.length);
          }
        }
        // Not working and nothing has grown for a while: the background
        // jobs are done - stop polling until the next turn re-arms us.
        if (!working) idleTicks = anyGrew ? 0 : idleTicks + 1;
        props.chatState.messages = props.chatState.messages.map((m) => {
          if (!bubbles.some((b) => b.id === m.id)) return m;
          return {
            ...m,
            agentLog: (m.agentLog ?? []).map((i) => {
              if (i.type !== "action") return i;
              // A wait step reads the log of the task it is blocked on; an
              // execute step reads its own.
              const logId = i.action.waitFor?.length ? i.action.waitFor[0] : i.action.toolCallId;
              const tail = logs[logId];
              if (tail === undefined) return i;
              const lines = tail.split("\n").map((l) => l.trim()).filter(Boolean);
              // liveLine only while the log is still growing: a finished
              // task's last line must not sit in the pearl as if current.
              return {
                ...i,
                action: {
                  ...i.action,
                  output: tail.trim(),
                  outputLines: lines.length,
                  liveLine: alive.has(logId) ? lines[lines.length - 1] : undefined,
                },
              };
            }),
          };
        });
        if (!working && idleTicks >= 5) clearInterval(timer);
      } catch {
        /* logs are a live convenience */
      }
    }, 2000);
    cleanup(() => clearInterval(timer));
  });

  const openFolder$ = $(async (path: string) => {
    // Replacing a live session? The outgoing folder's memory revision
    // happens first (fire-and-forget on ITS folder path).
    const outgoing = state.folderPath;
    if (outgoing && sessionTurns.value > 0) {
      reviseMemory$(outgoing).catch(() => {});
    }
    state.folderPath = path;
    state.mode = "project";
    state.status = "starting";
    state.statusNote = "Starting the agent...";
    state.touchedFiles = [];
    // This folder's permission mode (its own choice, else the Settings
    // default; off unless the user turned it on) - the session opens with it.
    // An old harness (< v0.2.0) cannot honor our Auto semantics (folder
    // boundary, decision records): force Ask and disable the toggle.
    try {
      const st = (await invokeTauri("build_install_status")) as {
        installed: boolean;
        installed_version: string | null;
      };
      state.autoPermissionsSupported =
        !st.installed || buildSupportsAutoPermissions(st.installed_version);
    } catch {
      state.autoPermissionsSupported = true;
    }
    state.permissionMode = state.autoPermissionsSupported
      ? permissionModeForFolder(path)
      : "ask";
    recordRecentFolder(path);
    try {
      await invokeTauri("start_build_agent", {
        binary: resolveBinaryPath(),
        cwd: path,
        model: aiModelSlug(props.selectedAi.value),
        // The raw model setting + eagerness let the bridge resolve which
        // model will actually SERVE agent turns, and write that model's
        // true context window into the agent's config.
        aiModel: props.selectedAi.value.aiConfig?.model ?? null,
        eagerness: localStorage.getItem("smartRoutingEagerness") || "balanced",
        // Identity for the project-memory MCP server: notes the agent
        // saves deliberately are written to THIS AI's chain, labeled.
        agentKey: props.selectedAi.value.aiConfig?.agentPubKey ?? null,
        aiLabel: props.selectedAi.value.label ?? null,
        permissionMode: state.permissionMode,
        // Tools this AI carries (Add-ons > Tools) - the bridge resolves them.
        mcpNames: props.selectedAi.value.aiConfig?.mcp ?? [],
      });
    } catch (err) {
      state.status = "idle";
      state.folderPath = null;
      props.chatState.error = JSON.stringify({
        code: "AGENT_START_FAILED",
        message: String(err),
      });
    }
  });

  /**
   * Tools in chat: the agent harness behind an ordinary conversation. The AI
   * carries MCP tools, so its turns run through a session in a per-AI
   * scratch workspace - same loop, same approvals, same steps in the thread -
   * with none of the project trappings (no folder pill, no recent folder, no
   * folder memory, no project-memory notes). Returns false when it cannot
   * start (Build not installed, no agent-ready model): the caller answers
   * directly instead.
   */
  const openToolsSession$ = $(async (): Promise<boolean> => {
    const ai = props.selectedAi.value;
    const aiId = ai.aiConfig?.id;
    if (!aiId || !ai.aiConfig?.mcp?.length) return false;
    let path: string;
    try {
      path = (await invokeTauri("tool_session_dir", { aiId })) as string;
    } catch {
      return false;
    }
    if (state.folderPath === path && (state.status === "ready" || state.status === "working" || state.status === "starting")) {
      return true;
    }
    if (state.folderPath && state.mode === "project" && sessionTurns.value > 0) {
      reviseMemory$(state.folderPath).catch(() => {});
    }
    state.folderPath = path;
    state.mode = "tools";
    state.status = "starting";
    state.statusNote = "Getting your tools ready...";
    state.touchedFiles = [];
    try {
      const st = (await invokeTauri("build_install_status")) as { installed: boolean; installed_version: string | null };
      if (!st.installed) {
        state.folderPath = null;
        state.mode = null;
        state.status = "idle";
        return false;
      }
      state.autoPermissionsSupported = buildSupportsAutoPermissions(st.installed_version);
    } catch {
      state.autoPermissionsSupported = true;
    }
    state.permissionMode = state.autoPermissionsSupported ? permissionModeForFolder(path) : "ask";
    try {
      await invokeTauri("start_build_agent", {
        binary: resolveBinaryPath(),
        cwd: path,
        model: aiModelSlug(ai),
        aiModel: ai.aiConfig?.model ?? null,
        eagerness: localStorage.getItem("smartRoutingEagerness") || "balanced",
        // No project-memory server for a tools session: notes belong to folders.
        agentKey: null,
        aiLabel: ai.label ?? null,
        permissionMode: state.permissionMode,
        mcpNames: ai.aiConfig?.mcp ?? [],
      });
      return true;
    } catch (err) {
      state.status = "idle";
      state.folderPath = null;
      state.mode = null;
      console.warn("[Agent] tools session did not start:", err);
      return false;
    }
  });

  const closeFolder$ = $(async () => {
    const closing = state.folderPath;
    if (closing && sessionTurns.value > 0 && state.mode === "project") {
      reviseMemory$(closing).catch(() => {});
    }
    state.folderPath = null;
    state.mode = null;
    state.status = "idle";
    state.statusNote = "";
    state.pendingPermissionId = null;
    state.overloadOffer = null;
    queued.value = null;
    props.chatState.isLoading = false;
    try {
      await invokeTauri("stop_build_agent");
    } catch {
      // Already gone is fine.
    }
  });

  const cancelTurn$ = $(async () => {
    try {
      await invokeTauri("cancel_agent_turn");
    } catch {
      // Session already ended; agent-turn/agent-exit handlers clean up.
    }
  });

  /** Update a permission item (by ACP request id) wherever it lives. */
  const updatePermission = $(
    (requestId: number, mutate: (p: AgentPermission) => AgentPermission) => {
      props.chatState.messages = props.chatState.messages.map((m) => {
        if (!m.agentLog?.some((i) => i.type === "permission" && i.permission.requestId === requestId)) {
          return m;
        }
        return {
          ...m,
          agentLog: m.agentLog.map((i) =>
            i.type === "permission" && i.permission.requestId === requestId
              ? { ...i, permission: mutate(i.permission) }
              : i,
          ),
        };
      });
    },
  );

  /** Switch this project's permission mode (remembered for the folder) and
   *  apply it to the open session live - the harness honours it from the
   *  next ask. */
  const setPermissionMode$ = $(async (mode: AgentPermissionMode) => {
    if (mode !== "ask" && !state.autoPermissionsSupported) return;
    state.permissionMode = mode;
    if (state.folderPath) setPermissionModeForFolder(state.folderPath, mode);
    if (state.status !== "idle" && state.status !== "stopped") {
      try {
        await invokeTauri("set_agent_permission_mode", { mode });
      } catch (err) {
        console.warn("[Agent] permission mode switch did not reach the agent:", err);
      }
    }
  });

  /** Answer the pending permission card with a button. `always` upgrades to
   *  the agent's always-variant option when it offers one. */
  const respondPermission$ = $(
    async (requestId: number, decision: "allow" | "reject", always: boolean) => {
      let perm: AgentPermission | undefined;
      for (const m of props.chatState.messages) {
        for (const i of m.agentLog ?? []) {
          if (i.type === "permission" && i.permission.requestId === requestId) perm = i.permission;
        }
      }
      if (!perm || perm.state !== "pending") return;

      const wantKinds =
        decision === "allow"
          ? always
            ? ["allow_always", "allow_once"]
            : ["allow_once", "allow_always"]
          : always
            ? ["reject_always", "reject_once"]
            : ["reject_once", "reject_always"];
      let option = undefined as (typeof perm.options)[number] | undefined;
      for (const k of wantKinds) {
        option = perm.options.find((o) => o.kind === k);
        if (option) break;
      }
      option ??= perm.options[0];
      if (!option) return;

      const scoped = option.kind?.endsWith("always");
      const receipt =
        decision === "allow"
          ? `Allowed: ${receiptSubject(perm)} - ${scoped ? alwaysScope(option.name) : "once"}`
          : `Declined: ${receiptSubject(perm)}${scoped ? ` - ${alwaysScope(option.name)}` : ""}`;

      await updatePermission(requestId, (p) => ({
        ...p,
        state: "answered",
        receipt,
        decision,
        scope: scoped ? "always" : "once",
        optionKind: option!.kind,
        answeredAt: new Date().toISOString(),
        via: "button",
      }));
      state.pendingPermissionId = null;

      if (decision === "allow") {
        // The agent executes silently after a grant - grow the step on the
        // rail NOW so Allow is visibly consequential. The agent's eventual
        // completion update merges into this node by tool call id.
        const label = permissionActionLabel(perm);
        state.liveStatus = `${label}..`;
        const tcId = perm.toolCallId ?? `perm-${requestId}`;
        props.chatState.messages = props.chatState.messages.map((m) => {
          const log = m.agentLog;
          if (!log?.some((i) => i.type === "permission" && i.permission.requestId === requestId)) {
            return m;
          }
          if (log.some((i) => i.type === "action" && i.action.toolCallId === tcId)) {
            return m;
          }
          return {
            ...m,
            agentLog: [
              ...log,
              {
                id: `action-${tcId}`,
                type: "action" as const,
                action: {
                  toolCallId: tcId,
                  label,
                  kind: perm.kind,
                  status: "in_progress" as const,
                  locations: perm.locations,
                  detail: perm.command ?? perm.locations?.[0],
                },
              },
            ],
          };
        });
      } else {
        state.liveStatus = "Thinking..";
      }

      await invokeTauri("respond_agent_permission", {
        requestId,
        optionId: option.optionId,
      });
    },
  );

  /** Typing while a card waits = decline with instructions: reject once,
   *  show the reply in place, send the text as the next prompt when the
   *  turn ends. The rest of the interrupted turn lands in a fresh bubble
   *  below the user's reply. */
  const answerPermissionByReply$ = $(async (text: string) => {
    const requestId = state.pendingPermissionId;
    if (requestId === null) return;
    let perm: AgentPermission | undefined;
    for (const m of props.chatState.messages) {
      for (const i of m.agentLog ?? []) {
        if (i.type === "permission" && i.permission.requestId === requestId) perm = i.permission;
      }
    }
    if (!perm || perm.state !== "pending") return;

    const option =
      perm.options.find((o) => o.kind === "reject_once") ??
      perm.options.find((o) => o.kind?.startsWith("reject"));
    await updatePermission(requestId, (p) => ({
      ...p,
      state: "answered",
      receipt: "Declined - you replied instead",
      decision: "reject",
      scope: "once",
      optionKind: option?.kind,
      answeredAt: new Date().toISOString(),
      via: "reply",
    }));
    state.pendingPermissionId = null;
    state.liveStatus = "Thinking..";
    await startTurnBubble(text);
    queued.value = text;
    if (option) {
      await invokeTauri("respond_agent_permission", {
        requestId,
        optionId: option.optionId,
      });
    }
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    const { listen } = await import("@tauri-apps/api/event");

    const mutateTurn = (mutate: (m: Message) => Message) => {
      const id = turnId.value;
      if (!id) return;
      props.chatState.messages = props.chatState.messages.map((m) =>
        m.id === id ? mutate(m) : m,
      );
    };

    const unReady = await listen<{ sessionId: string }>("agent-ready", async () => {
      state.status = "ready";
      state.statusNote = "";
      // A session that comes up with history already on screen is a resume
      // (or a mid-conversation reopen) - the agent itself starts blank, so
      // the next prompt must carry the restored-context digest.
      digestPending.value = props.chatState.messages.some(
        (m) => m.role === "assistant" && !!m.content,
      );
      sessionTurns.value = 0;
      sessionDigest.value = "";
      // Load this folder's workspace memory; the first prompt carries it.
      // Fire-and-forget - a slow chain read must not delay readiness, and
      // a prompt sent before it lands simply goes without (next session
      // catches up).
      memoryPending.value = false;
      toolsPending.value = !!props.selectedAi.value.aiConfig?.mcp?.length;
      const folder = state.mode === "project" ? state.folderPath : null;
      if (folder) {
        // The first prompt always carries at least the tool hint; the
        // memory content joins it when the chain read lands in time.
        memoryPending.value = true;
        getWorkspaceMemory(folder)
          .then((m) => {
            workspaceMemory.value = m.content;
            // A leftover pending digest means an earlier session ended
            // without its memory distillation (cancel, quit, crash) -
            // catch up now, with the memory freshly loaded.
            try {
              const leftover = localStorage.getItem(`project-pending-digest:${folder}`);
              if (leftover) {
                console.log("[WorkspaceMemory] Catching up an undistilled session for", folder);
                distillMemory$(folder, leftover).catch(() => {});
              }
            } catch {
              /* safety copy only */
            }
          })
          .catch(() => {});
      }
      const q = queued.value;
      if (q) {
        queued.value = null;
        await dispatchPrompt(q);
      }
    });

    const unUpdate = await listen<any>("agent-update", (e) => {
      const update = e.payload?.params?.update;
      if (!update) return;
      const kind = update.sessionUpdate;

      if (kind === "retry_state") {
        // The agent retries failed model calls with backoff (up to 15x) -
        // without surfacing it, a retry loop is indistinguishable from a
        // hang. Show attempt count + the reason's meat on the pearl/pill.
        if (update.type === "retrying") {
          const raw = String(update.reason ?? "model call failed");
          const reason = (raw.split(": ").pop() ?? raw).replace(/\.$/, "");
          const text = `Retrying (${update.attempt}/${update.max_retries}) - ${reason.slice(0, 80)}..`;
          state.retryStatus = text;
          state.liveStatus = text;
        } else {
          state.retryStatus = "";
          state.liveStatus = "Thinking..";
        }
        return;
      }
      // Any real progress means the retry resolved.
      if (state.retryStatus) state.retryStatus = "";

      if (kind === "user_message_chunk") {
        // The agent echoes our prompt with the number it gave this turn -
        // the key for undoing the turn's file changes later.
        const pi = update?._meta?.promptIndex;
        if (typeof pi === "number") mutateTurn((m) => (m.promptIndex === pi ? m : { ...m, promptIndex: pi }));
      } else if (kind === "agent_message_chunk") {
        // ALL text streams as narration in the working box during the turn
        // (the opening words are part of the work story, Cursor-style). The
        // bubble body is written ONCE, at turn end - ChatMessage's reveal
        // machinery is forward-only, so content must never shrink; the
        // demote-and-clear design duplicated text on screen.
        const text = update.content?.text ?? "";
        mutateTurn((m) => {
          const log = [...(m.agentLog ?? [])];
          const last = log[log.length - 1];
          if (last?.type === "narration") {
            log[log.length - 1] = { ...last, text: last.text + text };
          } else {
            log.push({ id: uuidv4(), type: "narration", text });
          }
          return { ...m, agentLog: log };
        });
      } else if (kind === "agent_thought_chunk") {
        const text = update.content?.text ?? "";
        mutateTurn((m) => {
          const log = [...(m.agentLog ?? [])];
          const last = log[log.length - 1];
          if (last?.type === "thought") {
            log[log.length - 1] = { ...last, text: last.text + text };
          } else {
            log.push({ id: uuidv4(), type: "thought", text });
          }
          return { ...m, agentLog: log };
        });
      } else if (kind === "turn_completed") {
        // The agent narrates its own turn stats - stamp them so the action
        // bar (Tokens, Model) and the collapsed stub can be honest instead
        // of empty. The actual upstream model comes from modelUsage.
        // (Stats first: a failed turn's finish below records the message.)
        const usage = update.usage;
        if (usage) {
          const modelKey = usage.modelUsage
            ? Object.keys(usage.modelUsage)[0]
            : undefined;
          // Origin comes from what actually served the turn, not the AI's
          // setting: local models are always .gguf files, so a non-gguf
          // server on a pinned-online OR Auto AI is an online pick (Auto
          // routing kimi showed "on device" when this read the setting).
          const aiModel = props.selectedAi.value.aiConfig?.model || "";
          const servedOnline =
            aiModel.startsWith("online:") ||
            (aiModel.startsWith("auto:") &&
              !!modelKey &&
              !modelKey.toLowerCase().endsWith(".gguf"));
          const folder = state.folderPath?.split("/").filter(Boolean).pop();
          mutateTurn((m) => ({
            ...m,
            tokens: {
              prompt_tokens: usage.inputTokens,
              completion_tokens: usage.outputTokens,
              total_tokens: usage.totalTokens,
            },
            servedBy: modelKey
              ? servedOnline
                ? `online:${modelKey}`
                : modelKey
              : m.servedBy,
            routingReason: state.routeReason
              ? `Agent session in ${folder ?? "your project"} - ${state.routeReason}`
              : `Agent session in ${folder ?? "your project"}`,
            agentStats: {
              durationMs: usage.apiDurationMs,
              modelCalls: usage.modelCalls,
            },
          }));
        }
        // A failed turn's REAL reason lives here (agent_result), not in
        // the RPC response's generic "Internal error" - finish with it
        // now; the later response is a no-op (finishTurn runs once).
        if (update.stop_reason === "error") {
          const raw =
            typeof update.agent_result === "string" ? update.agent_result : "";
          const online = raw ? extractOnlineError(raw) : null;
          if (online) {
            // Raise the standard billing/auth card; the bubble stays human.
            props.chatState.error = JSON.stringify(online);
            finishTurn("The online model couldn't continue - details below.");
          } else {
            finishTurn(
              raw ? `The agent hit a problem: ${raw}` : "The agent hit an error.",
            );
          }
        }
      } else if (kind === "plan") {
        // The agent's live task plan. Protocol: every update carries the
        // complete list - replace the plan item's entries in place so the
        // checklist keeps its position in the story and just ticks along.
        const entries = (update.entries ?? [])
          .map((en: any) => ({
            content: String(en?.content ?? ""),
            priority: en?.priority,
            status: en?.status ?? "pending",
          }))
          .filter((en: any) => en.content);
        if (entries.length) {
          const active = entries.find((en: any) => en.status === "in_progress");
          state.liveStatus = active ? `${active.content}..` : "Planning..";
          mutateTurn((m) => {
            const log = [...(m.agentLog ?? [])];
            const idx = log.findIndex((i) => i.type === "plan");
            if (idx >= 0) {
              log[idx] = { ...log[idx], type: "plan", entries };
            } else {
              log.push({ id: `plan-${uuidv4()}`, type: "plan", entries });
            }
            return { ...m, agentLog: log };
          });
        }
      } else if (kind === "auto_compact_started") {
        // Mid-turn context compaction is a real model call that can run a
        // minute - it goes ON THE RAIL as a step, because the pearl derives
        // its label from in-progress steps (a status field alone never
        // reaches the tip) and the transcript should hold it too.
        state.liveStatus = "Condensing its working notes..";
        mutateTurn((m) => ({
          ...m,
          agentLog: [
            ...(m.agentLog ?? []),
            {
              id: `compact-${uuidv4()}`,
              type: "action",
              action: {
                toolCallId: `compact-${(m.agentLog ?? []).length}`,
                // The agent compacting its OWN context window (a summary call on the
                // session's model) - not the AI's persistent memory. The old label
                // ("Tidying the conversation memory") read as the latter and confused a
                // real session; name the thing precisely.
                label: "Condensing its working notes to keep going",
                kind: "compact",
                status: "in_progress",
              },
            },
          ],
        }));
      } else if (kind === "auto_compact_completed") {
        state.liveStatus = "Thinking..";
        mutateTurn((m) => ({
          ...m,
          agentLog: (m.agentLog ?? []).map((i) =>
            i.type === "action" && i.action.kind === "compact" && i.action.status === "in_progress"
              ? { ...i, action: { ...i.action, status: "completed" as const } }
              : i,
          ),
        }));
      } else if (kind === "tool_call" || kind === "tool_call_update") {
        const toolCallId = update.toolCallId || uuidv4();
        const human = humanizeAction(update);
        // The agent's plan tool call (todo_write, kind "plan") is the same
        // information as the ACP plan update that renders the checklist -
        // a bare "Plan" action row on top of it is noise.
        if (human.kind === "plan") {
          state.liveStatus = "Planning..";
          return;
        }
        const out = actionOutput(update);
        const diff = extractDiff(update.content);
        if (!update.status || update.status === "in_progress" || update.status === "pending") {
          state.liveStatus = `${human.label}..`;
        } else {
          state.liveStatus = "Thinking..";
        }
        const locations = (update.locations ?? [])
          .map((l: any) => l?.path)
          .filter(Boolean);
        for (const p of locations) {
          if (!state.touchedFiles.includes(p)) state.touchedFiles = [...state.touchedFiles, p];
        }
        mutateTurn((m) => {
          const log = [...(m.agentLog ?? [])];
          let idx = -1;
          for (let i = log.length - 1; i >= 0; i--) {
            const item = log[i];
            if (item.type === "action" && item.action.toolCallId === toolCallId) {
              idx = i;
              break;
            }
          }
          if (idx >= 0) {
            const prevItem = log[idx] as { id: string; type: "action"; action: any };
            const prev = prevItem.action;
            log[idx] = {
              ...prevItem,
              action: {
                ...prev,
                status: update.status || prev.status,
                kind: human.kind ?? prev.kind,
                // The first update often has the best label; only upgrade
                // when the new one is more specific than the fallback.
                label: human.label !== "Working..." ? human.label : prev.label,
                detail: human.detail ?? prev.detail,
                locations: locations.length ? locations : prev.locations,
                output: out.output ?? prev.output,
                outputLines: out.outputLines ?? prev.outputLines,
                diff: diff ?? prev.diff,
                waitFor: human.waitFor ?? prev.waitFor,
              },
            };
          } else {
            log.push({
              id: `action-${toolCallId}`,
              type: "action",
              action: {
                toolCallId,
                label: human.label,
                kind: human.kind,
                status: update.status || "in_progress",
                locations,
                detail: human.detail,
                diff,
                waitFor: human.waitFor,
                ...out,
              },
            });
          }
          return { ...m, agentLog: log };
        });
      }
    });

    // Which model the local server just routed an agent call to - the
    // pearl names online waits after it ("Waiting on gpt-5.6-sol..").
    const unRoute = await listen<any>("agent-route", (e) => {
      const model = typeof e.payload?.model === "string" ? e.payload.model : "";
      state.waitingOn = e.payload?.online ? model.replace(/^online:/, "") : "";
      // Why routing picked it - shown on the turn's Model button (e.g.
      // "online by default (Ornith on your device is as capable)").
      state.routeReason = typeof e.payload?.reason === "string" ? e.payload.reason : "";
    });

    // App-side hints about the agent's work that the agent itself may not
    // word for the user - today: a web search refused because the AI is
    // offline-only, with how to turn it on. Rendered as narration in the
    // rail at its true position.
    const unHint = await listen<any>("agent-hint", (e) => {
      const text = typeof e.payload?.text === "string" ? e.payload.text : "";
      if (!text) return;
      mutateTurn((m) => {
        const log = [...(m.agentLog ?? [])];
        // Once per turn per hint kind - the agent may retry the tool.
        const kind = String(e.payload?.kind ?? "");
        if (kind && log.some((i) => i.id === `hint-${kind}`)) return m;
        // sticky = must outlive the fold: a notice, not narration.
        const type = e.payload?.sticky ? ("notice" as const) : ("narration" as const);
        log.push({ id: kind ? `hint-${kind}` : uuidv4(), type, text });
        return { ...m, agentLog: log };
      });
    });

    const unPermission = await listen<any>("agent-permission", (e) => {
      const params = e.payload?.params ?? {};
      const tc = params.toolCall ?? {};
      const locations = (tc.locations ?? [])
        .map((l: any) => l?.path)
        .filter(Boolean);
      const permission: AgentPermission = {
        requestId: e.payload?.id,
        toolCallId: typeof tc.toolCallId === "string" ? tc.toolCallId : undefined,
        title: tc.title
          ? tc.title.includes("__")
            ? humanizeMcpName(tc.title)
            : tc.title
          : "The agent asks for permission",
        kind: tc.kind,
        command:
          typeof tc.rawInput?.command === "string" ? tc.rawInput.command : undefined,
        // The exact payload of a tool ask (the note being remembered, the
        // query being run..) - the card must show what is actually asked.
        detail: (() => {
          const input = tc.rawInput;
          if (!input || typeof input !== "object" || typeof input.command === "string") {
            return undefined;
          }
          // use_tool dispatch: the ask's real payload is tool_input (the
          // note being remembered), never the tool_name string.
          const payload =
            input.tool_input && typeof input.tool_input === "object" ? input.tool_input : input;
          if (typeof payload.note === "string") return payload.note;
          const strings = Object.entries(payload).filter(
            ([, v]) => typeof v === "string" && (v as string).trim(),
          );
          if (strings.length === 1) return strings[0][1] as string;
          return strings.length
            ? strings.map(([k, v]) => `${k}: ${v}`).join("\n")
            : undefined;
        })(),
        diff: extractDiff(tc.content),
        locations,
        options: (params.options ?? []).map((o: any) => ({
          optionId: o.optionId,
          name: o.name,
          kind: o.kind,
        })),
        state: "pending",
        // Why this ask reached the user (the harness's prompt trigger) -
        // meaningful under Auto: "Auto stopped here because ..".
        promptReason:
          typeof params._meta?.["flowsta/promptReason"] === "string"
            ? params._meta["flowsta/promptReason"]
            : undefined,
      };
      state.pendingPermissionId = permission.requestId;
      state.pendingCardOffscreen = false;
      state.liveStatus = "Waiting for you..";
      mutateTurn((m) => {
        // Idempotent by request id: a re-delivered event (seen once in the
        // wild as two identical pending cards) must not add a second card.
        if (
          (m.agentLog ?? []).some(
            (i) => i.type === "permission" && i.permission.requestId === permission.requestId,
          )
        ) {
          return m;
        }
        return {
          ...m,
          agentLog: [
            ...(m.agentLog ?? []),
            { id: `perm-${permission.requestId}`, type: "permission", permission },
          ],
        };
      });
    });

    // A permission the APP answered on the user's behalf (the harness asks
    // before every project-memory read; the app allows it by policy). It
    // never shows a card, but it belongs in the same record as the asks
    // the user answered - logged already-answered, via "auto".
    const unPermissionAuto = await listen<any>("agent-permission-auto", (e) => {
      const params = e.payload?.params ?? {};
      const tc = params.toolCall ?? {};
      const opt = (params.options ?? []).find((o: any) => o.optionId === e.payload?.optionId);
      const scoped = typeof opt?.kind === "string" && opt.kind.endsWith("always");
      const permission: AgentPermission = {
        requestId: e.payload?.id,
        toolCallId: typeof tc.toolCallId === "string" ? tc.toolCallId : undefined,
        title: tc.title
          ? tc.title.includes("__")
            ? humanizeMcpName(tc.title)
            : tc.title
          : "Read this project's memory",
        kind: tc.kind,
        locations: (tc.locations ?? []).map((l: any) => l?.path).filter(Boolean),
        options: [],
        state: "answered",
        receipt: "Allowed automatically - reading this project's memory (app policy)",
        decision: "allow",
        scope: scoped ? "always" : "once",
        optionKind: opt?.kind,
        answeredAt: new Date().toISOString(),
        via: "auto",
      };
      mutateTurn((m) => {
        if (
          (m.agentLog ?? []).some(
            (i) => i.type === "permission" && i.permission.requestId === permission.requestId,
          )
        ) {
          return m;
        }
        return {
          ...m,
          agentLog: [
            ...(m.agentLog ?? []),
            { id: `perm-${permission.requestId}`, type: "permission", permission },
          ],
        };
      });
    });

    // Every permission decision the harness made, live (fork notification
    // `flowsta/permission_decided`, teed off its PermissionEvent stream).
    // Auto-approved actions become answered permission items (via "auto",
    // with which judge allowed them) so the rail shows a receipt and the
    // record holds the decision; everything else - policy-allowed reads,
    // earlier grants, the prompted ones (which already have their card) -
    // rolls into the turn's compact ledger.
    const unDecided = await listen<any>("agent-permission-decided", (e) => {
      const ev = e.payload?.event;
      if (!ev || typeof ev !== "object") return;
      // Our vocabulary in the user's records, not the wire protocol's.
      const rawReason: string = String(ev.decision_reason ?? "");
      const reason = rawReason === "yolo" ? "approve_everything" : rawReason;
      const autoApproved = !!ev.auto_approved;
      const prompted = !!ev.user_prompted;
      mutateTurn((m) => {
        const prev: PermissionLedger = m.permissionLedger ?? {
          byReason: {},
          total: 0,
          autoApproved: 0,
          prompted: 0,
        };
        const byReason = { ...prev.byReason, [reason || "unknown"]: (prev.byReason[reason || "unknown"] ?? 0) + 1 };
        const ledger: PermissionLedger = {
          byReason,
          total: prev.total + 1,
          autoApproved: prev.autoApproved + (autoApproved ? 1 : 0),
          prompted: prev.prompted + (prompted ? 1 : 0),
          mode: typeof ev.permission_mode === "string" ? ev.permission_mode : prev.mode,
        };
        let agentLog = m.agentLog;
        if (autoApproved && !prompted && String(ev.decision) === "allow") {
          const toolCallId = typeof ev.tool_id === "string" ? ev.tool_id : undefined;
          const requestId = -1 - (prev.total + 1); // negative: never collides with ACP ids
          const detail: string | undefined =
            typeof ev.access_detail === "string" && ev.access_detail.trim()
              ? ev.access_detail.slice(0, 500)
              : undefined;
          const autoReason: AgentPermission["autoReason"] =
            reason === "auto_fast_path"
              ? "fast_path"
              : reason === "auto_classifier_allow"
                ? "model"
                : reason === "approve_everything"
                  ? "always"
                  : "app_policy";
          const subject = detail ?? String(ev.tool_name ?? "an action");
          const why =
            autoReason === "fast_path"
              ? "routine work in the project"
              : autoReason === "model"
                ? "judged ordinary work by the AI"
                : autoReason === "always"
                  ? "your approve-everything setting"
                  : "on the routine list";
          const permission: AgentPermission = {
            requestId,
            toolCallId,
            title: String(ev.tool_name ?? "action"),
            kind: typeof ev.access_kind === "string" ? ev.access_kind : undefined,
            command: typeof ev.access_kind === "string" && ev.access_kind.toLowerCase().includes("bash") ? detail : undefined,
            detail: typeof ev.access_kind === "string" && ev.access_kind.toLowerCase().includes("bash") ? undefined : detail,
            options: [],
            state: "answered",
            receipt: `Auto: ${subject.length > 80 ? subject.slice(0, 80) + ".." : subject} - ${why}`,
            decision: "allow",
            scope: "once",
            answeredAt: typeof ev.timestamp === "string" ? ev.timestamp : new Date().toISOString(),
            via: "auto",
            autoReason,
          };
          agentLog = [
            ...(agentLog ?? []),
            { id: `perm-auto-${toolCallId ?? requestId}`, type: "permission", permission },
          ];
        }
        return { ...m, permissionLedger: ledger, agentLog };
      });
    });

    // Turn ids whose transcript entry has been written. Recording dedupes
    // HERE, independent of the UI's once-per-turn guard - a stray event
    // that flips the loading flag early must never cost the chain its
    // entry ("Holochain holds everything" - a lost answer reads as "this
    // question never got an answer" on resume).

  /** The turn's answer when the model never spoke AFTER its last steps: the
   *  last SUBSTANTIAL spoken passage anywhere in the log (rail hints and
   *  trim markers are not speech). Returns [content, logWithoutIt] or null. */
  const promoteLastSpeech = (
    content: string,
    log: NonNullable<Message["agentLog"]>,
  ): { content: string; log: NonNullable<Message["agentLog"]> } | null => {
    if (content !== "") return null;
    for (let i = log.length - 1; i >= 0; i--) {
      const item = log[i];
      if (item.type !== "narration") continue;
      if (item.id.startsWith("hint-") || item.id === "log-trimmed") continue;
      if (item.text.trim().length < 40) continue;
      return { content: item.text, log: [...log.slice(0, i), ...log.slice(i + 1)] };
    }
    return null;
  };

    const recordedTurnIds = new Set<string>();

    const recordTurnOnce = (id: string | null) => {
      if (!id || recordedTurnIds.has(id)) return;
      // An early finish (or a late-arriving trailing chunk) can leave the
      // answer stuck in the log as narration - promote it first.
      mutateTurn((m) => {
        const promoted = promoteLastSpeech(m.content, m.agentLog ?? []);
        return promoted ? { ...m, content: promoted.content, agentLog: promoted.log } : m;
      });
      const bubble = props.chatState.messages.find((m) => m.id === id);
      if (!bubble || (!bubble.content && !(bubble.agentLog ?? []).length)) return;
      recordedTurnIds.add(id);
      recordAssistantTurn(bubble).catch((e) =>
        console.error("[Agent] Transcript record failed:", e),
      );
    };

    /** Checks after edits: the project's own check command ran on the
     *  agent's Stop hook; its last outcome for this folder goes on the
     *  turn as a notice (readable after the fold, kept in the record). */
    const stampChecks = async (id: string | null, folder: string | null) => {
      if (!id || !folder) return;
      try {
        const r = (await invokeTauri("project_checks_last", { folder })) as
          | { status: string; command: string; summary: string; at: number }
          | null;
        if (!r || r.status === "none" || Date.now() / 1000 - r.at > 600) return;
        const text =
          r.status === "passed"
            ? `Checks passed (${r.command}).`
            : `Checks failed (${r.command}):\n${r.summary}`.trim();
        props.chatState.messages = props.chatState.messages.map((m) =>
          m.id === id && !(m.agentLog ?? []).some((i) => i.id === `checks-${r.at}`)
            ? { ...m, agentLog: [...(m.agentLog ?? []), { id: `checks-${r.at}`, type: "notice" as const, text }] }
            : m,
        );
      } catch {
        /* no checks line - the hook may not have run */
      }
    };

    const finishTurn = (errorText?: string) => {
      // Once per turn: turn_completed (with the informative error) and the
      // RPC response (with a generic "Internal error") both land here.
      if (!props.chatState.isLoading) return;
      void stampChecks(turnId.value, state.folderPath);
      mutateTurn((m) => {
        let log = (m.agentLog ?? []).map((i) =>
          i.type === "action" &&
          (i.action.status === "in_progress" || i.action.status === "pending")
            ? {
                ...i,
                action: {
                  ...i.action,
                  status: errorText ? ("failed" as const) : ("completed" as const),
                },
              }
            : i,
        );
        // The turn's last words ARE the answer: promote the last spoken
        // passage into the bubble body (content goes "" -> answer exactly
        // once - it must never shrink). WHEREVER it sits: a turn that speaks
        // its conclusion mid-way and then finishes on silent tool steps used
        // to fold everything into the stub and show nothing until a click.
        let content = m.content;
        const promoted = promoteLastSpeech(content, log);
        if (promoted) {
          content = promoted.content;
          log = promoted.log;
        }
        return { ...m, isLoading: false, error: errorText ?? m.error, content, agentLog: log };
      });
      // A bubble that never got anything (cancelled before output) is noise.
      const id = turnId.value;
      props.chatState.messages = props.chatState.messages.filter(
        (m) =>
          !(m.id === id && m.content === "" && !(m.agentLog ?? []).length && !m.error),
      );
      recordTurnOnce(id);
      props.chatState.isLoading = false;
      state.liveStatus = "";
      state.retryStatus = "";
      if (state.status === "working") state.status = "ready";
      if (!errorText) {
        sessionTurns.value++;
        sessionDigest.value = buildResumeDigest(props.chatState.messages);
        // Quit-proof: keep the digest on disk as we go. A cancel or app
        // quit never distills - the NEXT open of this project notices the
        // leftover and catches up.
        if (state.folderPath) {
          try {
            localStorage.setItem(
              `project-pending-digest:${state.folderPath}`,
              sessionDigest.value,
            );
          } catch {
            /* safety copy only */
          }
        }
      }
      // A turn killed by an overloaded upstream model earns the explicit
      // switch offer - only for Auto AIs (routing owns the pick there; a
      // pinned AI's model is the user's own setting, not ours to swap).
      if (
        errorText &&
        OVERLOAD_RE.test(errorText) &&
        (props.selectedAi.value.aiConfig?.model || "").startsWith("auto:")
      ) {
        invokeTauri("alternate_online_agent")
          .then((r: any) => {
            if (r?.alt) {
              state.overloadOffer = {
                failedName: r.failed_name,
                alt: r.alt,
                altName: r.alt_name,
              };
            }
          })
          .catch(() => {});
      }
    };

    const unTurn = await listen<any>("agent-turn", async (e) => {
      const err = e.payload?.error;
      const stop = e.payload?.result?.stopReason;
      if (err) {
        // Terminal model errors arrive HERE (the RPC response), with the
        // real reason in message and/or data - billing/auth codes raise
        // the standard card instead of raw provider JSON in the bubble.
        const raw = [
          err.message ? String(err.message) : "",
          err.data ? JSON.stringify(err.data) : "",
        ]
          .filter(Boolean)
          .join(" ");
        const online = raw ? extractOnlineError(raw) : null;
        if (online) {
          props.chatState.error = JSON.stringify(online);
          finishTurn("The online model couldn't continue - details below.");
        } else {
          finishTurn(err.message ? String(err.message) : "The agent hit an error.");
        }
      } else if (stop === "refusal") {
        finishTurn("The agent declined to continue this task.");
      } else {
        finishTurn();
      }
      // The net: even when finishTurn short-circuited (loading flag already
      // cleared by a stray event), turn end still records the turn.
      recordTurnOnce(turnId.value);
      const q = queued.value;
      if (q && state.status === "ready") {
        queued.value = null;
        // The queued prompt's bubble already exists (typed mid-turn) - but
        // if the interrupted turn streamed its tail into it, give the new
        // prompt a clean one.
        const id = turnId.value;
        const bubble = props.chatState.messages.find((m) => m.id === id);
        if (!bubble || bubble.content !== "" || (bubble.agentLog ?? []).length) {
          const newId = uuidv4();
          turnId.value = newId;
          props.chatState.messages = [
            ...props.chatState.messages,
            {
              id: newId,
              role: "assistant",
              content: "",
              model: props.selectedAi.value.id,
              aiLabel: props.selectedAi.value.label,
              aiImageUrl: props.selectedAi.value.imageUrl || undefined,
              isLoading: true,
              agentTurn: true,
              agentLog: [],
            },
          ];
        } else {
          props.chatState.messages = props.chatState.messages.map((m) =>
            m.id === id ? { ...m, isLoading: true } : m,
          );
        }
        await dispatchPrompt(q);
      }
    });

    // Model-selection failure must be VISIBLE: quietly running the agent on
    // a different model than the AI's face is the one substitution the trust
    // story forbids.
    const unLog = await listen<string>("agent-log", (e) => {
      if (
        typeof e.payload === "string" &&
        e.payload.startsWith("couldn't set model")
      ) {
        props.chatState.messages = [
          ...props.chatState.messages,
          {
            id: uuidv4(),
            role: "assistant",
            content: "",
            model: props.selectedAi.value.id,
            aiLabel: props.selectedAi.value.label,
            error: `This project's agent couldn't switch to ${props.selectedAi.value.label}'s model and is running on its default instead. (${e.payload})`,
          },
        ];
      }
    });

    const unExit = await listen<{ code: number | null }>("agent-exit", (e) => {
      const wasOpen = state.folderPath !== null;
      const midTurn = props.chatState.isLoading;
      state.status = wasOpen ? "stopped" : "idle";
      state.statusNote = wasOpen
        ? `The agent stopped (code ${e.payload?.code ?? "?"}).`
        : "";
      queued.value = null;
      // A card must never sit there looking answerable after its agent died.
      if (state.pendingPermissionId !== null) {
        const requestId = state.pendingPermissionId;
        state.pendingPermissionId = null;
        props.chatState.messages = props.chatState.messages.map((m) =>
          m.agentLog?.some((i) => i.type === "permission" && i.permission.requestId === requestId)
            ? {
                ...m,
                agentLog: m.agentLog.map((i) =>
                  i.type === "permission" && i.permission.requestId === requestId
                    ? {
                        ...i,
                        permission: {
                          ...i.permission,
                          state: "expired" as const,
                          answeredAt: new Date().toISOString(),
                          via: "expired" as const,
                        },
                      }
                    : i,
                ),
              }
            : m,
        );
      }
      if (midTurn) {
        finishTurn(wasOpen ? "The agent stopped before finishing." : undefined);
      }
    });

    cleanup(() => {
      unReady();
      unUpdate();
      unPermission();
      unPermissionAuto();
      unDecided();
      unHint();
      unRoute();
      unTurn();
      unLog();
      unExit();
    });
  });

  /** Undo every file change of one finished agent turn (the agent's hunk
   *  tracker rejects the turn's hunks: edits reverted, created files
   *  removed, deleted files restored). Marks the turn and says so on its
   *  rail, in the record. */
  const undoTurn$ = $(async (messageId: string) => {
    const m = props.chatState.messages.find((x) => x.id === messageId);
    if (!m || m.undone) return;
    if (typeof m.promptIndex !== "number") {
      props.chatState.messages = props.chatState.messages.map((x) =>
        x.id === messageId
          ? { ...x, agentLog: [...(x.agentLog ?? []), { id: uuidv4(), type: "notice" as const, text: "Couldn't undo: this turn's changes are not tracked (the project session was started by an older agent)." }] }
          : x,
      );
      return;
    }
    try {
      const n = Number(await invokeTauri("agent_undo_turn", { promptIndex: m.promptIndex })) || 0;
      props.chatState.messages = props.chatState.messages.map((x) =>
        x.id === messageId
          ? {
              ...x,
              undone: true,
              agentLog: [
                ...(x.agentLog ?? []),
                { id: uuidv4(), type: "notice" as const, text: n > 0 ? `Undone: ${n} change${n === 1 ? "" : "s"} from this turn put back the way they were.` : "Undone: nothing was left to put back (the files already matched)." },
              ],
            }
          : x,
      );
    } catch (e) {
      const text = typeof e === "string" ? e : "Couldn't undo this turn.";
      props.chatState.messages = props.chatState.messages.map((x) =>
        x.id === messageId ? { ...x, agentLog: [...(x.agentLog ?? []), { id: uuidv4(), type: "notice" as const, text }] } : x,
      );
    }
  });

  return {
    agentState: state,
    openToolsSession$,
    undoTurn$,
    openFolder$,
    closeFolder$,
    sendPrompt$,
    cancelTurn$,
    respondPermission$,
    answerPermissionByReply$,
    acceptOverloadOffer$,
    dismissOverloadOffer$,
    setPermissionMode$,
  };
}
