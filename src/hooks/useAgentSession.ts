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
import { describeAction, subjectOfLabel, errorOf } from "../utils/actionLabels";
import { MCP_PRESETS } from "../utils/mcp";
import { summaryOf } from "../utils/agentSummary";
import { uiLog } from "../utils/uiLog";
import { lastKnownEntitled } from "../utils/entitlement";
import { $, useSignal, useStore, useVisibleTask$, type Signal } from "@builder.io/qwik";
import { v4 as uuidv4 } from "uuid";
import { startConversation, recordMessage, waitForHolochainReady } from "../utils/holochainTranscripts";
import {
  rememberConversationFolder,
  rememberLastConversation,
} from "../utils/conversationResume";
import type {
  AgentAction,
  AgentActionDiff,
  AgentPermission,
  Message,
  PermissionLedger,
  SelectedAiModel,
  LibraryDocGiven,
  AgentLogItem,
} from "../types";
import {
  buildSupportsAutoPermissions,
  permissionModeForFolder,
  permissionModeForTools,
  setPermissionModeForFolder,
  setPermissionModeForTools,
  type AgentPermissionMode,
} from "../utils/agentPermissions";
import { computeLineDiff } from "../utils/lineDiff";
import { activeTools } from "../utils/carry";
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
  /** The AI a tools session belongs to - another AI's turns never ride it. */
  sessionAiId: string | null;
  /** When the last turn finished cleanly; null once the next one is sent.
   *  The folder chip says "done" while it is set. */
  lastFinishedAt: number | null;
  /** This session's coder is a small local model on an offline-only AI
   *  (the bridge said so at open): a failed turn gets the online door. */
  smallCoder: boolean;
  /** The tool set the open tools session started with (joined names) - a
   *  changed set opens a fresh session, since the harness fixes tools at start. */
  sessionTools: string;
  /** How the session's tools were launched (mcp_tools_signature). A changed
   *  setting changes it, and the next message opens a fresh session. */
  sessionToolsSig: string;
  /** Tool calls this session has made - a tools conversation that has used
   *  a tool stays in its session (utils/toolsGate.ts, "sticky"). */
  sessionToolCalls: number;
  /** Tool servers that failed to start for this session (from their stderr
   *  logs) - surfaced as notices on the next turn, then cleared. */
  toolStartFailures: { name: string; tail: string }[];
  /** App hints that arrived before a turn existed (a session-start notice,
   *  e.g. a tight context window) - shown on the next turn, once each. */
  startNotices: { kind: string; text: string }[];
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
  /** The last route event said the person's own server serves this turn. */
  lastRouteServer: boolean;
  /** The struggle notice has been shown this session (once is enough). */
  struggleShown: boolean;
  /** The struggle the person is being told about right now (a modal). */
  struggle: { text: string; bigger?: string; entitled: boolean } | null;
  /** Why routing picked the serving model (from the agent-route event). */
  routeReason: string;
  /** Generation tok/s of the latest model call, from the engine's own timings. */
  lastCallTps: number;
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

/** A thought that is still open ends the moment anything else arrives -
 *  its row then says how long it took. */
function closeThought(log: AgentLogItem[]): AgentLogItem[] {
  const now = Date.now();
  for (let i = 0; i < log.length; i++) {
    const it = log[i];
    if (it.type === "thought" && it.endedAt == null) log[i] = { ...it, endedAt: now };
  }
  return log;
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
    sessionAiId: null,
    lastFinishedAt: null,
    smallCoder: false,
    sessionTools: "",
    sessionToolsSig: "",
    sessionToolCalls: 0,
    toolStartFailures: [],
    startNotices: [],
    status: "idle",
    statusNote: "",
    pendingPermissionId: null,
    pendingCardOffscreen: false,
    liveStatus: "",
    retryStatus: "",
    waitingOn: "",
    lastRouteServer: false,
    struggleShown: false,
    struggle: null,
    routeReason: "",
    /** Generation tok/s of the latest model call, from the engine's own timings. */
    lastCallTps: 0,
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
      // Right after launch the conductor may still be starting; a record
      // that lands late beats a conversation that never exists. This runs
      // off the turn's critical path (fire-and-forget from the bubble).
      await waitForHolochainReady(90_000);
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
        const { output: _output, diff, liveLine, specificity: _s, ...action } = i.action as any;
        // The task's last printed line survives (200 chars) so a reopened
        // turn still says what it last did; the full log stays on disk.
        const lastLine = action.lastLine ?? (liveLine ? String(liveLine).slice(0, 200) : undefined);
        return {
          ...i,
          action: diff
            ? // Keep the receipt (path + counts), drop the heavy lines.
              { ...action, lastLine, diff: { path: diff.path, added: diff.added, removed: diff.removed } }
            : { ...action, lastLine },
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
      // Permission decisions are the audit trail and narration is the
      // spoken story - both survive the trim wherever they sat; only the
      // steps in the middle are dropped.
      const head = items.slice(0, 40);
      const tail = items.slice(-40);
      const middleKept = items.slice(40, -40).filter((i) => i.type === "permission" || i.type === "narration");
      items = [
        ...head,
        ...middleKept,
        {
          id: "log-trimmed",
          type: "narration" as const,
          text: `.. ${items.length - 80 - middleKept.length} steps trimmed to fit the transcript ..`,
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
      // Built from whatever the bubble carries - it used to exist only when
      // there was a routing reason, which would drop everything else.
      bubble.routingReason || bubble.library?.length
        ? {
            ...(bubble.routingReason ? { routing_reason: bubble.routingReason, routing_task: "agent" } : {}),
            ...(bubble.library?.length
              ? { library: bubble.library.map(({ doc_id, name, passages, best }) => ({ doc_id, name, passages, best })) }
              : {}),
          }
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
  const startTurnBubble = $((userText: string, attachedFiles?: string[], library?: LibraryDocGiven[], surface?: "project" | "tools") => {
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
        agentSurface: surface ?? (state.mode === "tools" ? "tools" : "project"),
        // The AI's own documents whose passages rode with this prompt
        // (shown under Sources, recorded with the turn as names and counts).
        ...(library?.length ? { library } : {}),
        // A tool server that died at start is invisible otherwise - say so
        // on the turn, once, with the reason its log gave.
        agentLog: [
          ...state.toolStartFailures.map((f) => ({
            id: `toolstart-${f.name}-${id}`,
            type: "notice" as const,
            text: `${f.name} didn't start, so its tools are not available this session. Its log says: ${f.tail}`,
          })),
          ...state.startNotices.map((n) => ({
            id: `hint-${n.kind || uuidv4()}`,
            type: "notice" as const,
            text: n.text,
          })),
        ],
      },
    ];
    state.toolStartFailures = [];
    state.startNotices = [];
    props.chatState.isLoading = true;
    // Fire-and-forget - chat always works even if the conductor is down.
    recordUserTurn(userText).catch(() => {});
  });

  /** The turn's bubble goes up the moment the person sends, before the
   *  documents search that rides with the prompt (an embedding model cold
   *  start plus the search took ~3 s on 09-23 with nothing on screen, while
   *  a direct chat moves at once). sendPrompt$ then fills in the rest. */
  const prepared = useSignal(false);
  /** A turn that struggled, to be told once the turn is over. */
  const strugglePending = useSignal<{ turnId: string; model: string; pinned: boolean; entitled: boolean } | null>(null);
  /** Skill name (lower case) -> glyph name, from the installed skills. */
  const skillGlyphs = useSignal<Record<string, string>>({});
  const prepareTurn$ = $(async (text: string, files?: string[], surface?: "project" | "tools", status?: string) => {
    if (state.status === "working") return; // mid-turn: sendPrompt$ interjects
    if (!text.trim()) return;
    // Before the session exists, the wait is the session (its tool servers
    // starting, a model loading); once it is up, the documents search. A
    // caller that only knows "something is coming" passes its own word.
    const word = status ?? (state.status === "ready" ? "Looking through documents.." : "Getting your tools ready..");
    if (prepared.value) {
      // Raised already (on the keystroke, before the tools gate): only the
      // word changes.
      state.liveStatus = word;
      return;
    }
    await startTurnBubble(text, files, undefined, surface);
    prepared.value = true;
    state.liveStatus = word;
  });

  /** The bubble raised by prepareTurn$ was for a session that did not open:
   *  take it down again so the direct chat path can speak instead. */
  const discardPreparedTurn$ = $(() => {
    if (!prepared.value) return;
    prepared.value = false;
    const id = turnId.value;
    turnId.value = null;
    props.chatState.isLoading = false;
    state.liveStatus = "";
    const msgs = props.chatState.messages;
    const at = msgs.findIndex((m) => m.id === id);
    props.chatState.messages = at > 0 ? [...msgs.slice(0, at - 1), ...msgs.slice(at + 1)] : msgs.filter((m) => m.id !== id);
  });

  /** Send a prompt into the live session (session must be ready). */
  const dispatchPrompt = $(async (text: string) => {
    state.status = "working";
    state.lastFinishedAt = null;
    state.lastRouteServer = false;
    state.liveStatus = "Thinking..";
    props.chatState.isLoading = true;
    // A turn in flight is lost if the chat page unmounts (its listeners go
    // with it). The root layout reads this flag to hold navigation with a
    // question until the session is lifted to app level.
    try { (window as unknown as { __yoaiTurnRunning?: boolean }).__yoaiTurnRunning = true; } catch { /* fine */ }
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
        const block = await toolsGuidanceBlock(activeTools(props.selectedAi.value.aiConfig), state.mode === "tools");
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
  const sendPrompt$ = $(async (text: string, extra?: { context?: string; files?: string[]; library?: LibraryDocGiven[] }) => {
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
    if (state.status === "working") {
      // The agent is mid-turn: say it to the agent NOW. It takes the message
      // in at its next safe point (between steps) without cancelling the
      // turn - the way a message typed to a working agent should land. The
      // listener scope owns the turn's bubble, so it does the split there.
      window.dispatchEvent(
        new CustomEvent("yoai-agent-interject", { detail: { text, wire, files: extra?.files, library: extra?.library } }),
      );
      return;
    }
    if (prepared.value) {
      prepared.value = false;
      // The bubble is up already (prepareTurn$): add the documents that
      // rode along, so Sources can name them.
      if (extra?.library?.length) {
        const id = turnId.value;
        const library = extra.library;
        props.chatState.messages = props.chatState.messages.map((m) => (m.id === id ? { ...m, library } : m));
      }
    } else {
      await startTurnBubble(text, extra?.files, extra?.library);
    }
    if (state.status === "ready") {
      await dispatchPrompt(wire);
    } else {
      // Starting: hold until agent-ready.
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

  /** The struggle modal was answered or waved away. */
  const dismissStruggle$ = $(() => {
    state.struggle = null;
  });

  /** Send the last prompt again (after a setting changed what serves it). */
  const resendLast$ = $(async () => {
    if (lastPrompt.value && state.status === "ready") await sendPrompt$(lastPrompt.value);
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
      // A wait names a TASK id; the log on disk is named by the tool-call
      // id of the command that owns it. Translate through `taskId`.
      const owner: Record<string, string> = {};
      for (const b of bubbles) for (const i of b.agentLog ?? []) {
        if (i.type === "action" && i.action.taskId) owner[i.action.taskId] = i.action.toolCallId;
      }
      const ids = Array.from(new Set(bubbles
        .flatMap((b) => b.agentLog ?? [])
        .flatMap((i) => {
          if (i.type !== "action") return [];
          if (i.action.kind === "execute") return [i.action.toolCallId];
          if (i.action.waitFor?.length) return i.action.waitFor.map((w) => owner[w] ?? w);
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
              // A wait step reads the log of the task it is blocked on - of
              // several, the one still growing, else the last named; an
              // execute step reads its own.
              const waits = (i.action.waitFor ?? []).map((w) => owner[w] ?? w);
              const logId = waits.length
                ? waits.find((w) => alive.has(w)) ?? waits.filter((w) => logs[w] !== undefined).pop() ?? waits[0]
                : i.action.toolCallId;
              const tail = logs[logId];
              if (tail === undefined) return i;
              const lines = tail.split("\n").map((l) => l.trim()).filter(Boolean);
              const last = lines[lines.length - 1];
              // liveLine only while the log is still growing: a finished
              // task's last line must not sit in the pearl as if current.
              // lastLine is what it printed last, kept for the record.
              return {
                ...i,
                action: {
                  ...i.action,
                  output: tail.trim(),
                  outputLines: lines.length,
                  liveLine: alive.has(logId) && i.action.status !== "completed" && i.action.status !== "failed" ? last : undefined,
                  lastLine: last ? last.slice(0, 200) : i.action.lastLine,
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
    // The session is started for THIS AI (its model, its tools). Recorded
    // so a later switch of AI reopens the session for the new one instead
    // of sending the new AI's turns through the old AI's session (Eric,
    // 09-23: Teresa's turn ran in Veebo's session with Obsidian).
    state.sessionAiId = props.selectedAi.value.aiConfig?.id ?? null;
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
        eagerness: localStorage.getItem("routingOnlineShare") || "frontier",
        // Identity for the project-memory MCP server: notes the agent
        // saves deliberately are written to THIS AI's chain, labeled.
        agentKey: props.selectedAi.value.aiConfig?.agentPubKey ?? null,
        aiLabel: props.selectedAi.value.label ?? null,
        permissionMode: state.permissionMode,
        // Tools this AI carries (Add-ons > Tools) - the bridge resolves them.
        mcpNames: activeTools(props.selectedAi.value.aiConfig),
      });
      // Warm the embedding model now, while the person types: the
      // documents search that rides with the first prompt paid the
      // model's cold start at send (~2 s of nothing on screen, 09-23).
      import("../utils/embeddings")
        .then(({ embedTexts }) => embedTexts(["warm up"]).catch(() => {}))
        .catch(() => {});
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
    const names = activeTools(ai.aiConfig);
    if (!aiId || !names.length) return false;
    let path: string;
    try {
      path = (await invokeTauri("tool_session_dir", { aiId })) as string;
    } catch {
      return false;
    }
    const sig = (await invokeTauri("mcp_tools_signature", { names }).catch(() => "")) as string;
    if (
      state.folderPath === path &&
      state.sessionAiId === aiId &&
      state.sessionTools === names.join(",") &&
      state.sessionToolsSig === sig &&
      (state.status === "ready" || state.status === "working" || state.status === "starting")
    ) {
      return true;
    }
    if (state.folderPath && state.mode === "project" && sessionTurns.value > 0) {
      reviseMemory$(state.folderPath).catch(() => {});
    }
    state.folderPath = path;
    state.mode = "tools";
    state.sessionAiId = aiId;
    state.sessionTools = names.join(",");
    state.sessionToolsSig = sig;
    state.sessionToolCalls = 0;
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
    state.permissionMode = state.autoPermissionsSupported ? permissionModeForTools(aiId) : "ask";
    try {
      await invokeTauri("start_build_agent", {
        binary: resolveBinaryPath(),
        cwd: path,
        model: aiModelSlug(ai),
        aiModel: ai.aiConfig?.model ?? null,
        eagerness: localStorage.getItem("routingOnlineShare") || "frontier",
        // No project-memory server for a tools session: notes belong to folders.
        agentKey: null,
        aiLabel: ai.label ?? null,
        permissionMode: state.permissionMode,
        mcpNames: names,
      });
      return true;
      // Warm the embedding model now, while the person types: the
      // documents search that rides with the first prompt paid the
      // model's cold start at send (~2 s of nothing on screen, 09-23).
      import("../utils/embeddings")
        .then(({ embedTexts }) => embedTexts(["warm up"]).catch(() => {}))
        .catch(() => {});
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
    state.sessionAiId = null;
    state.lastFinishedAt = null;
    state.smallCoder = false;
    state.struggleShown = false;
    state.sessionTools = "";
    state.sessionToolsSig = "";
    state.sessionToolCalls = 0;
    state.status = "idle";
    state.statusNote = "";
    try { (window as unknown as { __yoaiTurnRunning?: boolean }).__yoaiTurnRunning = false; } catch { /* fine */ }
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
    if (state.mode === "tools") {
      if (state.sessionAiId) setPermissionModeForTools(state.sessionAiId, mode);
    } else if (state.folderPath) {
      setPermissionModeForFolder(state.folderPath, mode);
    }
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
      // A desktop-class client is offered one extra option first: switch
      // the whole session to approve-everything. Never pick it here - the
      // card's "always" is a grant for THIS action, nothing wider.
      const choices = perm.options.filter((o) => o.optionId !== "enable-always-approve");
      let option = undefined as (typeof perm.options)[number] | undefined;
      for (const k of wantKinds) {
        option = choices.find((o) => o.kind === k);
        if (option) break;
      }
      option ??= choices[0];
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

    // A message sent while the agent works (see sendPrompt$). The running
    // bubble is closed as far as it got and recorded; the person's message
    // and a fresh bubble follow, so the conversation reads in the order it
    // happened. Steps still running move to the fresh bubble - their
    // updates keep arriving and must find them.
    const onInterject = async (ev: Event) => {
      const { text, wire, files, library } = (ev as CustomEvent).detail as {
        text: string;
        wire: string;
        files?: string[];
        library?: LibraryDocGiven[];
      };
      const oldId = turnId.value;
      const isOpen = (i: { type: string; action?: { status?: string } }) =>
        i.type === "action" && (i.action?.status === "in_progress" || i.action?.status === "pending");
      const running = (props.chatState.messages.find((m) => m.id === oldId)?.agentLog ?? []).filter(isOpen);
      mutateTurn((m) => ({
        ...m,
        isLoading: false,
        agentLog: (m.agentLog ?? []).filter((i) => !isOpen(i)),
      }));
      recordTurnOnce(oldId);
      await startTurnBubble(text, files, library);
      if (running.length) {
        mutateTurn((m) => ({ ...m, agentLog: [...running, ...(m.agentLog ?? [])] }));
      }
      try {
        await invokeTauri("agent_interject", { text: wire });
      } catch (e) {
        // An agent without the method: hold the message until the turn ends.
        console.warn("[Agent] interjection not taken - it waits for the turn to end:", e);
        queued.value = wire;
      }
    };
    window.addEventListener("yoai-agent-interject", onInterject);
    // Stop on a still-running row: a background task or a helper. The
    // harness ends it and says so (task_completed / subagent_finished), so
    // the row closes on its word, not ours.
    const onStopTask = async (ev: Event) => {
      const { id, helper } = (ev as CustomEvent).detail as { id: string; helper: boolean };
      if (!id) return;
      uiLog(`[rail] stop ${helper ? "helper" : "task"} ${id}`);
      try {
        await invokeTauri("agent_stop_task", { taskId: id, helper });
      } catch (e) {
        uiLog(`[rail] stop failed: ${String(e)}`);
      }
    };
    window.addEventListener("yoai-agent-stop-task", onStopTask);

    const unReady = await listen<{ sessionId: string }>("agent-ready", async () => {
      state.status = "ready";
      state.statusNote = "";
      // Fetched tool icons, for the rail's tool steps.
      import("../utils/toolIcons").then((m) => m.loadToolIcons()).catch(() => {});
      // The installed skills' own icons, for the rail's skill steps.
      try {
        const { listSkills } = await import("../utils/skills");
        const m: Record<string, string> = {};
        for (const s of await listSkills()) if (s.icon) m[s.name.toLowerCase()] = s.icon;
        skillGlyphs.value = m;
      } catch {
        /* no skills listing */
      }
      // A session that comes up with history already on screen is a resume
      // (or a mid-conversation reopen) - the agent itself starts blank, so
      // the next prompt must carry the restored-context digest.
      digestPending.value = props.chatState.messages.some(
        (m) => m.role === "assistant" && !!m.content,
      );
      sessionTurns.value = 0;
      sessionDigest.value = "";
      // Tool servers start with the session; a few seconds later their
      // stderr logs know whether they lived. Ask once, surface on the turn.
      state.toolStartFailures = [];
      const carried = props.selectedAi.value.aiConfig?.mcp ?? [];
      if (carried.length) {
        setTimeout(async () => {
          try {
            const failures = (await invokeTauri("mcp_start_failures", { names: carried })) as { name: string; tail: string }[];
            if (failures?.length) {
              state.toolStartFailures = failures;
              // A turn already on screen (typed during startup) gets it now.
              const current = turnId.value;
              if (current && props.chatState.isLoading) {
                props.chatState.messages = props.chatState.messages.map((m) =>
                  m.id === current
                    ? { ...m, agentLog: [...(m.agentLog ?? []), ...failures.map((f) => ({ id: `toolstart-${f.name}-${current}`, type: "notice" as const, text: `${f.name} didn't start, so its tools are not available this session. Its log says: ${f.tail}` }))] }
                    : m,
                );
                state.toolStartFailures = [];
              }
            }
          } catch { /* no verdict */ }
        }, 4000);
      }
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

    // Helper (subagent) sessions. The harness streams a helper's own tool
    // calls, text and thoughts under the CHILD session id, on the same
    // connection. Its steps nest under the helper's row; its text and
    // thoughts are never the AI's own (the AI's reply carries the helper's
    // result when the helper returns). Child session id -> helper tool-call id.
    const helperSessions = new Map<string, string>();

    const unUpdate = await listen<any>("agent-update", (e) => {
      const update = e.payload?.params?.update;
      if (!update) return;
      const kind = update.sessionUpdate;
      const fromSession = typeof e.payload?.params?.sessionId === "string" ? e.payload.params.sessionId : undefined;
      const helperOf = fromSession ? helperSessions.get(fromSession) : undefined;
      if (helperOf && kind !== "tool_call" && kind !== "tool_call_update") return;

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
      // Any real progress means the retry resolved, and the prompt is in.
      if (state.retryStatus) state.retryStatus = "";
      if (state.liveStatus.startsWith("Taking in ")) state.liveStatus = "Thinking..";

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
          const log = closeThought([...(m.agentLog ?? [])]);
          const last = log[log.length - 1];
          if (last?.type === "narration") {
            log[log.length - 1] = { ...last, text: last.text + text };
          } else {
            log.push({ id: uuidv4(), type: "narration", text, at: Date.now() });
          }
          return { ...m, agentLog: log };
        });
      } else if (kind === "agent_thought_chunk") {
        const text = update.content?.text ?? "";
        mutateTurn((m) => {
          const log = [...(m.agentLog ?? [])];
          const last = log[log.length - 1];
          if (last?.type === "thought" && last.endedAt == null) {
            log[log.length - 1] = { ...last, text: last.text + text };
          } else {
            log.push({ id: uuidv4(), type: "thought", text, at: Date.now() });
          }
          return { ...m, agentLog: log };
        });
      } else if (kind === "subagent_spawned" || kind === "subagent_progress" || kind === "subagent_finished") {
        // The harness's word on a helper: started (with its session id, so
        // its steps can nest), a progress tick every couple of seconds, and
        // the end with its result. All ride the parent session.
        const helperId = String(update.subagent_id ?? "");
        uiLog(`[rail] ${kind} ${helperId}${update.child_session_id ? ` session ${update.child_session_id}` : ""}${update.status ? ` ${update.status}` : ""}`);
        if (!helperId) return;
        mutateTurn((m) => {
          const log = [...(m.agentLog ?? [])];
          let idx = -1;
          if (kind === "subagent_spawned") {
            // The newest helper row not yet tied to a helper, by preference
            // the one whose brief matches the description.
            const brief = String(update.description ?? "").trim().slice(0, 40);
            for (let i = log.length - 1; i >= 0; i--) {
              const it = log[i];
              if (it.type !== "action" || it.action.icon !== "helper" || it.action.helperId) continue;
              if (idx < 0) idx = i;
              if (brief && (it.action.label ?? "").includes(brief)) {
                idx = i;
                break;
              }
            }
          } else {
            idx = log.findIndex((it) => it.type === "action" && it.action.helperId === helperId);
          }
          if (idx < 0) return m;
          const item = log[idx] as { id: string; type: "action"; action: AgentAction };
          const a = item.action;
          if (kind === "subagent_spawned") {
            if (typeof update.child_session_id === "string") helperSessions.set(update.child_session_id, a.toolCallId);
            log[idx] = { ...item, action: { ...a, helperId, status: "in_progress", endedAt: undefined, liveLine: "Starting" } };
          } else if (kind === "subagent_progress") {
            const n = Number(update.tool_call_count ?? 0);
            log[idx] = { ...item, action: { ...a, liveLine: n ? `${n} tool call${n === 1 ? "" : "s"} so far` : a.liveLine } };
          } else {
            const stopped = update.status === "cancelled";
            const failed = update.status !== "completed" && !stopped;
            const n = Number(update.tool_calls ?? 0);
            const firstLine = typeof update.output === "string"
              ? update.output.split("\n").map((l: string) => l.trim()).find((l: string) => l)
              : undefined;
            const reason = typeof update.error === "string" && update.error.trim() ? update.error.trim().slice(0, 500) : "The helper failed.";
            log[idx] = {
              ...item,
              action: {
                ...a,
                helperDone: true,
                status: failed ? "failed" : "completed",
                endedAt: a.endedAt ?? Date.now(),
                liveLine: undefined,
                lastLine: stopped ? "Stopped" : firstLine?.slice(0, 200) ?? (n ? `${n} tool call${n === 1 ? "" : "s"}` : a.lastLine),
                error: failed ? a.error ?? reason : a.error,
              },
            };
          }
          return { ...m, agentLog: log };
        });
      } else if (kind === "task_backgrounded") {
        const callId = String(update.tool_call_id ?? "");
        const taskId = String(update.task_id ?? "");
        uiLog(`[rail] task_backgrounded ${callId} task ${taskId}`);
        if (callId && taskId) {
          mutateTurn((m) => {
            const log = closeThought([...(m.agentLog ?? [])]);
            const i = log.findIndex((it) => it.type === "action" && it.action.toolCallId === callId);
            if (i < 0) return m;
            const item = log[i] as { id: string; type: "action"; action: AgentAction };
            log[i] = {
              ...item,
              action: { ...item.action, taskId, status: "in_progress", endedAt: undefined, liveLine: item.action.liveLine ?? "Running in the background" },
            };
            uiLog(`[rail] ${callId} runs in the background as task ${taskId} - Stop is on its row`);
            return { ...m, agentLog: log };
          });
        }
      } else if (kind === "background_tasks" || kind === "task_completed") {
        // The harness's own word on background tasks (v0.4.0): a durable
        // list snapshot, or one task's completion with its output tail.
        // Rows keyed by the task id (the tool-call id of the backgrounded
        // command) take the status, the end time and the last line from
        // it, so a task that outlives the turn still ends on screen.
        const rows: any[] = kind === "background_tasks"
          ? (update.tasks ?? [])
          : update.task_snapshot
            ? [{
                task_id: update.task_snapshot.task_id,
                status: update.task_snapshot.exit_code === 0 || (!update.task_snapshot.exit_code && !update.task_snapshot.signal) ? "completed" : "failed",
                ended_at: update.task_snapshot.end_time ?? null,
                exit_code: update.task_snapshot.exit_code ?? null,
                signal: update.task_snapshot.signal ?? null,
                output: typeof update.task_snapshot.output === "string" ? update.task_snapshot.output : undefined,
                explicitly_killed: !!update.task_snapshot.explicitly_killed,
              }]
            : [];
        if (rows.length) {
          mutateTurn((m) => {
            const log = [...(m.agentLog ?? [])];
            let changed = false;
            for (const r of rows) {
              for (let i = 0; i < log.length; i++) {
                const item = log[i];
                if (item.type !== "action" || (item.action.toolCallId !== r.task_id && item.action.taskId !== r.task_id)) continue;
                const a = item.action;
                // A task the harness has already ended is settled: a later
                // list snapshot (status "failed", signal "killed", nothing
                // more) must not repaint a stopped row as failed.
                if (a.taskDone) continue;
                const done = r.status === "completed" || r.status === "failed";
                const lastLine = typeof r.output === "string" && r.output.trim()
                  ? r.output.trim().split("\n").pop()!.slice(0, 200)
                  : undefined;
                const stopped = !!r.explicitly_killed || r.signal === "killed";
                const failed = r.status === "failed" && !stopped;
                log[i] = {
                  ...item,
                  action: {
                    ...a,
                    status: failed ? "failed" : done ? "completed" : a.status,
                    taskDone: done ? true : a.taskDone,
                    endedAt: done ? a.endedAt ?? Date.now() : a.endedAt,
                    lastLine: stopped ? "Stopped" : lastLine ?? a.lastLine,
                    liveLine: done ? undefined : a.liveLine,
                    error: failed
                      ? a.error ?? (r.exit_code != null ? `exit code ${r.exit_code}` : r.signal ? `stopped by ${r.signal}` : "The task failed.")
                      : a.error,
                  },
                };
                changed = true;
              }
            }
            return changed ? { ...m, agentLog: log } : m;
          });
        }
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
          // The person's own server is neither: the route event said so,
          // or the AI is pinned to one of its models.
          const servedServer = state.lastRouteServer || aiModel.startsWith("external:");
          const servedOnline =
            !servedServer &&
            (aiModel.startsWith("online:") ||
              (aiModel.startsWith("auto:") &&
                !!modelKey &&
                !modelKey.toLowerCase().endsWith(".gguf")));
          // The folder's own name, on either path separator; a tools session
          // runs in a hidden workspace whose path (and account name) is not
          // for the screen - it is named for what it is.
          const rawFolder = state.folderPath ?? "";
          const hiddenToolsWorkspace = /[\\/]tool-sessions[\\/]/.test(rawFolder);
          const folder = hiddenToolsWorkspace
            ? null
            : rawFolder.split(/[\\/]/).filter(Boolean).pop();
          // The cost line: a tools session carries every tool's definition
          // on every turn (about 10k tokens) - say so where the tokens show.
          const toolCount = state.sessionTools ? state.sessionTools.split(",").filter(Boolean).length : 0;
          const where = hiddenToolsWorkspace
            ? toolCount > 0
              ? `Tools session · ${toolCount} tool${toolCount === 1 ? "" : "s"} carried - their definitions ride every turn (about 10k tokens)`
              : "Tools session"
            : `Agent session in ${folder ?? "your project"}`;
          mutateTurn((m) => ({
            ...m,
            tokens: {
              prompt_tokens: usage.inputTokens,
              completion_tokens: usage.outputTokens,
              total_tokens: usage.totalTokens,
              // The engine's own measure of the last call (agent-call-timing).
              ...(state.lastCallTps > 0 ? { tokens_per_second: state.lastCallTps } : {}),
            },
            servedBy: modelKey
              ? servedServer
                ? `external:${modelKey}`
                : servedOnline
                  ? `online:${modelKey}`
                  : modelKey
              : m.servedBy,
            routingReason: state.routeReason ? `${where} - ${state.routeReason}` : where,
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
            const log = closeThought([...(m.agentLog ?? [])]);
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
        // The subject of an earlier step in this turn, so a wait or a stop
        // is named after the task it concerns ("Waiting for generate.sh").
        const turnLog = props.chatState.messages.find((m) => m.id === turnId.value)?.agentLog ?? [];
        const subjectOf = (id: string) => {
          for (const item of turnLog) {
            if (item.type === "action" && (item.action.toolCallId === id || item.action.taskId === id || item.action.helperId === id)) {
              return item.action.helperId === id ? "the helper" : subjectOfLabel(item.action.labelDone ?? item.action.label);
            }
          }
          return undefined;
        };
        const serverLabel = (server: string) => MCP_PRESETS.find((m) => m.id === server)?.title;
        const human = describeAction(update, { subjectOf, skillNameFromPath, serverLabel });
        if (human.specificity === 0 && kind === "tool_call") {
          // A tool the table does not know: say so in the log, so the next
          // fixture can be added (the label falls back to the agent's title).
          console.warn("[rail] unknown tool, title used:", human.name ?? update.name, update.kind, update.title);
        }
        // The agent's plan tool call (todo_write, kind "plan") is the same
        // information as the ACP plan update that renders the checklist -
        // a bare "Plan" action row on top of it is noise.
        if (human.kind === "plan") {
          state.liveStatus = "Planning..";
          return;
        }
        if (kind === "tool_call") state.sessionToolCalls += 1;
        // A skill step wears the skill's own glyph when its SKILL.md names one.
        const skillName = human.kind === "skill" ? human.label.replace(/^Using skill: /, "").trim().toLowerCase() : "";
        const glyph = skillName ? skillGlyphs.value[skillName] : undefined;
        const out = actionOutput(update);
        const diff = extractDiff(update.content);
        // A backgrounded command's result names its task: "<task-id>…</task-id>".
        // Waits and kills refer to that id, and the tailer reads the log of
        // the step that owns it.
        const taskId = out.output ? /<task-id>\s*([^<\s]+)\s*<\/task-id>/.exec(out.output)?.[1] : undefined;
        // A call the harness refused ("Tool `x` was not executed: …") is a
        // failed step whatever status rode the update.
        const refused = !!out.output && /was not executed/i.test(out.output);
        // The harness reports a command that exited non-zero, timed out or
        // was killed as a COMPLETED call with the facts in its raw output
        // (`exit_code`, `timed_out`, `signal`; 09-24: `exit 3` read as done).
        // That is a failed step, with what it printed as the reason.
        const ro = update.rawOutput;
        const exitCode = ro && typeof ro === "object" && typeof ro.exit_code === "number" ? (ro.exit_code as number) : undefined;
        const timedOut = !!(ro && typeof ro === "object" && ro.timed_out);
        const signal = ro && typeof ro === "object" && typeof ro.signal === "string" ? (ro.signal as string) : undefined;
        const commandFailed = (exitCode !== undefined && exitCode !== 0) || timedOut || !!signal;
        const status = refused || commandFailed ? "failed" : update.status;
        const error = refused
          ? out.output!.slice(0, 500)
          : commandFailed
            ? [
                (out.output ?? "").trim().split("\n").filter(Boolean).slice(-3).join("\n"),
                timedOut ? "The command timed out." : signal ? `Stopped by ${signal}.` : `Exit code ${exitCode}.`,
              ].filter(Boolean).join("\n").slice(0, 500)
            : errorOf(update);
        const finished = status === "completed" || status === "failed";
        uiLog(`[rail] ${kind} ${human.name ?? update.name ?? "?"} ${status ?? "-"} ${toolCallId}${taskId ? ` task ${taskId}` : ""}${helperOf ? ` in helper ${helperOf}` : ""}`);
        if (!status || status === "in_progress" || status === "pending") {
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
          const log = closeThought([...(m.agentLog ?? [])]);
          let idx = -1;
          for (let i = log.length - 1; i >= 0; i--) {
            const item = log[i];
            if (item.type === "action" && item.action.toolCallId === toolCallId) {
              idx = i;
              break;
            }
          }
          if (idx < 0 && kind === "tool_call_update" && human.specificity === 0) return m;
          if (idx >= 0) {
            const prevItem = log[idx] as { id: string; type: "action"; action: any };
            const prev = prevItem.action;
            // A later update replaces the label only with one at least as
            // specific: a wait named after its task never gives way to a
            // bare "Background Task", and a title never beats the table.
            const upgrade = human.specificity >= (prev.specificity ?? 0);
            const wasOver = prev.status === "completed" || prev.status === "failed";
            const nowOpen = status === "in_progress" || status === "pending";
            if (wasOver && nowOpen) uiLog(`[rail] REOPENED ${toolCallId} "${prev.label}" by ${kind} ${status}`);
            // A helper the harness has already announced keeps running after
            // its start call returns (a backgrounded helper's call answers
            // "started"); its row ends on the harness's finished event.
            const holdHelper = prev.icon === "helper" && !!prev.helperId && !prev.helperDone && status === "completed";
            // A backgrounded command's start call answers at once with its
            // task id; the row runs until the harness reports the task ended.
            const holdTask = !!(taskId ?? prev.taskId) && !prev.taskDone && status === "completed" && !refused;
            if (holdTask) uiLog(`[rail] hold ${toolCallId} running: task ${taskId ?? prev.taskId} not ended yet`);
            log[idx] = {
              ...prevItem,
              action: {
                ...prev,
                status: holdHelper || holdTask ? prev.status : status || prev.status,
                parent: prev.parent ?? helperOf,
                kind: upgrade ? human.kind : prev.kind,
                taskId: taskId ?? prev.taskId,
                icon: upgrade ? human.icon : prev.icon,
                label: upgrade ? human.label : prev.label,
                labelDone: upgrade ? human.labelDone : prev.labelDone,
                specificity: Math.max(human.specificity, prev.specificity ?? 0),
                name: human.name ?? prev.name,
                server: human.server ?? prev.server,
                glyph: glyph ?? prev.glyph,
                detail: human.detail ?? prev.detail,
                locations: locations.length ? locations : prev.locations,
                output: out.output ?? prev.output,
                outputLines: out.outputLines ?? prev.outputLines,
                diff: diff ?? prev.diff,
                waitFor: human.waitFor ?? prev.waitFor,
                endedAt: finished && !holdHelper && !holdTask ? prev.endedAt ?? Date.now() : prev.endedAt,
                // A finished step's live line is over: what a command
                // printed stays behind the row, not under it.
                liveLine: finished ? undefined : prev.liveLine,
                error: error ?? prev.error,
              },
            };
          } else {
            log.push({
              id: `action-${toolCallId}`,
              type: "action",
              action: {
                toolCallId,
                label: human.label,
                labelDone: human.labelDone,
                kind: human.kind,
                icon: human.icon,
                specificity: human.specificity,
                name: human.name,
                server: human.server,
                glyph,
                status: status || "in_progress",
                locations,
                detail: human.detail,
                diff,
                waitFor: human.waitFor,
                taskId,
                parent: helperOf,
                startedAt: Date.now(),
                endedAt: finished ? Date.now() : undefined,
                error,
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
    // The model server taking a long prompt in (the harness's instructions
    // and tools are ~10k tokens; on a small card that is a silent minute):
    // the pearl says how far along it is instead of "Thinking..".
    const unProgress = await listen<any>("llm-prompt-progress", (e) => {
      if (state.status !== "working") return;
      const tokens = Number(e.payload?.tokens);
      const progress = Number(e.payload?.progress);
      if (!(tokens >= 1500) || !Number.isFinite(progress)) return;
      if (state.liveStatus === "Thinking.." || state.liveStatus.startsWith("Taking in ")) {
        state.liveStatus = `Taking in the project so far · ${Math.round(progress * 100)}%`;
      }
    });
    const unTiming = await listen<any>("agent-call-timing", (e) => {
      const tps = Number(e.payload?.tokens_per_second);
      state.lastCallTps = Number.isFinite(tps) && tps > 0 ? tps : 0;
    });
    const unRoute = await listen<any>("agent-route", (e) => {
      const model = typeof e.payload?.model === "string" ? e.payload.model : "";
      // The pearl names what the turn waits on: an online model, or the
      // person's own server (Settings > Engines) serving the turn.
      state.waitingOn = e.payload?.online
        ? model.replace(/^online:/, "")
        : e.payload?.server
          ? "your server"
          : "";
      state.lastRouteServer = !!e.payload?.server;
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
      const kind = String(e.payload?.kind ?? "");
      if (kind === "small-coder") state.smallCoder = true;
      // Before any turn exists (a session-start notice): keep it for the
      // first turn's bubble instead of dropping it on the floor.
      if (!turnId.value) {
        if (!state.startNotices.some((n) => kind && n.kind === kind)) state.startNotices.push({ kind, text });
        return;
      }
      mutateTurn((m) => {
        const log = [...(m.agentLog ?? [])];
        // Once per turn per hint kind - the agent may retry the tool.
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
        if (String(ev.decision) !== "allow" && typeof ev.tool_id === "string") {
          // Auto stopped this call. The step ends red and says why, so a
          // glyph never breathes on for a call that was never run.
          const why =
            reason === "auto_classifier_block"
              ? "Auto stopped here: not ordinary project work. Allow it if you want it run."
              : reason === "auto_outside_workspace"
                ? "Auto stopped here: this touches something outside the project folder."
                : `Not allowed to run here${reason ? ` (${reason.replace(/_/g, " ")})` : ""}.`;
          agentLog = (agentLog ?? []).map((i) =>
            i.type === "action" && i.action.toolCallId === ev.tool_id && i.action.status !== "completed"
              ? { ...i, action: { ...i.action, status: "failed" as const, error: i.action.error ?? why, endedAt: i.action.endedAt ?? Date.now() } }
              : i,
          );
        }
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

    /** A project turn that ran over 30 s and ended while the window was
     *  not in front: the desktop's own popup, name and summary, never the
     *  reply text. Off with the Settings switch. */
    const notifyIfAway = (id: string | null, failed: boolean) => {
      if (!id || state.mode !== "project") return;
      try {
        if (localStorage.getItem("agent-notify") === "off") return;
        if (document.hasFocus() && !document.hidden) return;
      } catch {
        return;
      }
      setTimeout(async () => {
        const m = props.chatState.messages.find((x) => x.id === id);
        if (!m) return;
        const log = m.agentLog ?? [];
        let first: number | undefined, last: number | undefined;
        for (const item of log) {
          const at = item.type === "action" ? item.action.startedAt : (item as any).at;
          const end = item.type === "action" ? item.action.endedAt : (item as any).endedAt;
          if (typeof at === "number" && (first === undefined || at < first)) first = at;
          if (typeof end === "number" && (last === undefined || end > last)) last = end;
        }
        const ms = first !== undefined ? (last ?? Date.now()) - first : 0;
        if (ms < 30_000) return;
        const ai = props.selectedAi.value.aiConfig?.name ?? "Your AI";
        const folder = (state.folderPath ?? "").split(/[\\/]/).filter(Boolean).pop();
        const mins = Math.round(ms / 60000);
        const took = mins >= 1 ? `${mins} min` : `${Math.round(ms / 1000)} s`;
        try {
          const n = await import("@tauri-apps/plugin-notification");
          let ok = await n.isPermissionGranted();
          if (!ok) ok = (await n.requestPermission()) === "granted";
          if (!ok) return;
          n.sendNotification({
            title: failed ? `${ai} stopped in ${folder ?? "the project"}` : `${ai} finished in ${folder ?? "the project"}`,
            body: `${summaryOf(log) || "Done"} · ${took}`,
          });
        } catch {
          /* no notifications here */
        }
      }, 0);
    };

    const finishTurn = (errorText?: string) => {
      // Once per turn: turn_completed (with the informative error) and the
      // RPC response (with a generic "Internal error") both land here.
      if (!props.chatState.isLoading) {
        uiLog("[rail] finishTurn skipped: loading already false");
        return;
      }
      void stampChecks(turnId.value, state.folderPath);
      notifyIfAway(turnId.value, !!errorText);
      mutateTurn((m) => {
        for (const i of m.agentLog ?? []) {
          if (i.type === "action" && (i.action.status === "in_progress" || i.action.status === "pending")) {
            uiLog(`[rail] turn end: open ${i.action.toolCallId} "${i.action.label}" task=${i.action.taskId ?? "-"} helper=${i.action.helperId ?? "-"}`);
          } else if (i.type === "thought" && i.endedAt == null) {
            uiLog(`[rail] turn end: open thought ${i.id}`);
          }
        }
        let log = closeThought([...(m.agentLog ?? [])]).map((i) => {
          if (i.type !== "action") return i;
          const open = i.action.status === "in_progress" || i.action.status === "pending";
          // A turn that died mid-command: its foreground steps are over,
          // whatever their logs were saying - the stub must not read
          // "still running" under the error. Backgrounded tasks keep
          // their line; the tailer clears it when the log goes quiet.
          const dropLive = !!errorText && !i.action.waitFor?.length && i.action.liveLine !== undefined;
          if (!open && !dropLive) return i;
          // What the harness has not ended outlives the turn: a backgrounded
          // task, a helper. Their rows keep running until its word comes
          // (or the turn died, when everything ends).
          const outlives =
            (!!i.action.taskId && !i.action.taskDone) ||
            (i.action.icon === "helper" && !!i.action.helperId && !i.action.helperDone);
          if (open && outlives && !errorText) {
            uiLog(`[rail] turn end: ${i.action.toolCallId} outlives the turn (${i.action.taskId ? "task" : "helper"})`);
            return i;
          }
          return {
            ...i,
            action: {
              ...i.action,
              status: open ? (errorText ? ("failed" as const) : ("completed" as const)) : i.action.status,
              liveLine: dropLive ? undefined : i.action.liveLine,
            },
          };
        });
        // A small model struggling shows on the record: two or more steps
        // that ended red (our own faults excluded - a refusal, a server
        // away, a tool that never started), or a tools turn that never got
        // one tool call to work. Said once per session, in plain words,
        // with the way up (Eric, 09-24). The pinned-smaller case is filled
        // in below, once the best local model is known.
        if (!state.struggleShown && !state.smallCoder) {
          const ourFault = (e: string | undefined) =>
            !!e && /was not executed|did not answer|stopped answering|didn't start|not connected/i.test(e);
          let red = 0;
          let toolOk = false;
          for (const i of log) {
            if (i.type !== "action") continue;
            if (i.action.status === "failed" && !ourFault(i.action.error)) red++;
            if (i.action.icon === "mcp" && i.action.status === "completed") toolOk = true;
          }
          const toolsTurn = state.mode === "tools" && !!state.sessionTools;
          const struggled = red >= 2 || (toolsTurn && !toolOk);
          const aiModel = props.selectedAi.value.aiConfig?.model || "";
          const offline = aiModel === "auto:offline" || aiModel === "auto:my-hardware" || aiModel.toLowerCase().endsWith(".gguf");
          if (struggled && offline) {
            state.struggleShown = true;
            let entitled = false;
            try {
              entitled = lastKnownEntitled() === "yes";
            } catch {
              /* unknown = no plan */
            }
            // Told as a modal the person cannot miss (a line inside a folded
            // rail went unseen, 09-25). The pinned-smaller case waits for the
            // better model's name below; the rest is said now.
            strugglePending.value = { turnId: m.id, model: aiModel, pinned: aiModel.toLowerCase().endsWith(".gguf"), entitled };
            uiLog(`[rail] struggle: ${red} red step(s), tools turn ${toolsTurn} ok ${toolOk}, ai ${aiModel}, entitled ${entitled}`);
          }
        }
        if (errorText && state.smallCoder) {
          // The door again, on the failure itself (the rail draws it under
          // any notice whose id starts with hint-small-coder).
          log = [
            ...log,
            {
              id: `hint-small-coder-fail-${m.id}`,
              type: "notice" as const,
              text: "That failed on the small model running on this computer. Online routing would hand a step like this to a bigger model.",
            },
          ];
        }
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
      // The struggle, said once the turn is over: which model on this
      // computer does better (when the AI is pinned to a weaker one), and
      // the plan's online models when the account has them.
      if (strugglePending.value) {
        const { model, pinned, entitled } = strugglePending.value;
        strugglePending.value = null;
        const pretty = (f: string) => f.replace(/\.gguf$/i, "").replace(/-Q\d[^-]*$/i, "");
        const online = entitled
          ? " Your plan's online models do them in one go."
          : " Online models do them in one go. They are an optional paid service, and everything offline stays free.";
        const say = (bigger?: string) => {
          const text = bigger
            ? `Tasks like this are hard for ${pretty(model)}, the model this AI is set to. ${pretty(bigger)} on this computer does better at them.${online}`
            : `Tasks like this need more than the models this computer can run.${online}`;
          state.struggle = { text, bigger, entitled };
        };
        if (pinned) {
          invokeTauri("agent_best_local")
            .then((best) => say(typeof best === "string" && best && best.toLowerCase() !== model.toLowerCase() ? best : undefined))
            .catch(() => say(undefined));
        } else {
          say(undefined);
        }
      }
      state.liveStatus = "";
      state.retryStatus = "";
      if (state.status === "working") state.status = "ready";
      if (!errorText) {
        state.lastFinishedAt = Date.now();
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
      try { (window as unknown as { __yoaiTurnRunning?: boolean }).__yoaiTurnRunning = false; } catch { /* fine */ }
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
              agentSurface: state.mode === "tools" ? "tools" : "project",
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
      try { (window as unknown as { __yoaiTurnRunning?: boolean }).__yoaiTurnRunning = false; } catch { /* fine */ }
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
      window.removeEventListener("yoai-agent-interject", onInterject);
      window.removeEventListener("yoai-agent-stop-task", onStopTask);
      unReady();
      unUpdate();
      unPermission();
      unPermissionAuto();
      unDecided();
      unHint();
      unRoute();
      unTiming();
      unProgress();
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
    prepareTurn$,
    discardPreparedTurn$,
    resendLast$,
    dismissStruggle$,
    cancelTurn$,
    respondPermission$,
    answerPermissionByReply$,
    acceptOverloadOffer$,
    dismissOverloadOffer$,
    setPermissionMode$,
  };
}
