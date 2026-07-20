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

import { $, useSignal, useStore, useVisibleTask$, type Signal } from "@builder.io/qwik";
import { v4 as uuidv4 } from "uuid";
import { startConversation, recordMessage } from "../utils/holochainTranscripts";
import {
  rememberConversationFolder,
  rememberLastConversation,
} from "../utils/conversationResume";
import type { AgentPermission, Message, SelectedAiModel } from "../types";
import type { UseChatState } from "./useChat";

export interface AgentSessionState {
  folderPath: string | null;
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
  /** Every file path the agent touched this session (viewer feed). */
  touchedFiles: string[];
}

export interface UseAgentSessionProps {
  chatState: UseChatState;
  selectedAi: Signal<SelectedAiModel>;
}

/** Dev-phase binary resolution: a saved override, else the dev build path.
 *  Replaced by install-gated download resolution when install gating lands. */
const DEV_BINARY_FALLBACK =
  "/home/solar/Documents/Flowsta/Projects/FlowstaAuth/your-own-ai-build/target/release/your-own-ai-build";

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

/** Short human handle for a permission receipt: the command if there is
 *  one, else the tool title. Receipts may shorten; the CARD never does. */
function receiptSubject(p: AgentPermission): string {
  const s = p.command || p.title;
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
    : "always in this folder";
}

/** Naive preview diff for edit asks: line add/remove markers by set
 *  membership. Good enough to see WHAT changes; not a real patch view. */
function previewDiff(oldText: string | null, newText: string): string {
  const oldLines = (oldText ?? "").split("\n");
  const newLines = newText.split("\n");
  if (!oldText) return newLines.map((l) => `+ ${l}`).join("\n");
  const oldSet = new Set(oldLines);
  const newSet = new Set(newLines);
  const out: string[] = [];
  for (const l of oldLines) if (!newSet.has(l)) out.push(`- ${l}`);
  for (const l of newLines) if (!oldSet.has(l)) out.push(`+ ${l}`);
  return out.length ? out.join("\n") : "(whitespace-only change)";
}

const basename = (p: string) => p.split("/").filter(Boolean).pop() || p;

/** Humanized step label + expandable detail from the tool call's real
 *  input (the `x.ai/tool` meta carries name/kind/label/input; rawInput is
 *  the fallback). "List `.`" becomes "Looking through the folder". */
function humanizeAction(update: any): { label: string; kind?: string; detail?: string } {
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
          dir && dir !== "." ? `Looking through ${basename(dir)}/` : "Looking through the folder",
        detail: dir,
      };
    case "read":
      return { kind, label: path ? `Reading ${basename(path)}` : "Reading files", detail: path };
    case "edit":
    case "write":
      return { kind, label: path ? `Editing ${basename(path)}` : "Editing files", detail: path };
    case "delete":
      return { kind, label: path ? `Deleting ${basename(path)}` : "Deleting files", detail: path };
    case "search":
    case "grep":
      return {
        kind,
        label: term ? `Searching for "${term}"` : "Searching the folder",
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
    default:
      return { kind, label: meta.label || update.title || "Working..." };
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
    status: "idle",
    statusNote: "",
    pendingPermissionId: null,
    pendingCardOffscreen: false,
    liveStatus: "",
    touchedFiles: [],
  });

  // A prompt waiting for the session: typed before the handshake finished,
  // typed mid-turn, or typed as the answer to a permission card.
  const queued = useSignal<string | null>(null);
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
    if (!agentKey) return;
    const model = ai.aiConfig?.model || "agent";
    if (!props.chatState.conversationHash) {
      const title = text.length > 80 ? text.slice(0, 80) + "..." : text;
      const hash = await startConversation(agentKey, ai.label, model, title);
      if (!hash) return;
      props.chatState.conversationHash = hash;
      props.chatState.messageSequence = 0;
      if (state.folderPath) rememberConversationFolder(hash, state.folderPath);
      rememberLastConversation({ hash, agentKey, aiId: ai.id, title });
    }
    const seq = props.chatState.messageSequence;
    props.chatState.messageSequence = seq + 1;
    await recordMessage(
      agentKey,
      props.chatState.conversationHash!,
      "user",
      text,
      seq,
      model,
    );
  });

  const recordAssistantTurn = $(
    async (content: string, servedBy?: string, tokens?: Message["tokens"]) => {
      const ai = props.selectedAi.value;
      const agentKey = ai.aiConfig?.agentPubKey;
      const hash = props.chatState.conversationHash;
      if (!agentKey || !hash || !content) return;
      const seq = props.chatState.messageSequence;
      props.chatState.messageSequence = seq + 1;
      await recordMessage(
        agentKey,
        hash,
        "assistant",
        content,
        seq,
        servedBy || ai.aiConfig?.model || "agent",
        undefined,
        tokens && tokens.total_tokens
          ? {
              prompt_tokens: tokens.prompt_tokens ?? 0,
              completion_tokens: tokens.completion_tokens ?? 0,
              total_tokens: tokens.total_tokens,
            }
          : undefined,
      );
    },
  );

  /** The turn's single reply bubble, pushed at Enter. Mounting it anchors
   *  the question to the top and shows the avatar + action bar instantly. */
  const startTurnBubble = $((userText: string) => {
    const id = uuidv4();
    turnId.value = id;
    props.chatState.messages = [
      ...props.chatState.messages,
      { id: uuidv4(), role: "user", content: userText, model: "user" },
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
    try {
      await invokeTauri("send_agent_prompt", { text });
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
  const sendPrompt$ = $(async (text: string) => {
    if (!text.trim()) return;
    if (state.status !== "ready" && state.status !== "starting" && state.status !== "working") {
      props.chatState.error = JSON.stringify({
        code: "AGENT_NOT_RUNNING",
        message: "The folder's agent is not running. Reopen the folder.",
      });
      return;
    }
    await startTurnBubble(text);
    if (state.status === "ready") {
      await dispatchPrompt(text);
    } else {
      // Working: turns are strictly sequential over ACP - hold until the
      // current turn completes. Starting: hold until agent-ready.
      queued.value = text;
    }
  });

  const openFolder$ = $(async (path: string) => {
    state.folderPath = path;
    state.status = "starting";
    state.statusNote = "Starting the agent...";
    state.touchedFiles = [];
    recordRecentFolder(path);
    try {
      await invokeTauri("start_build_agent", {
        binary: resolveBinaryPath(),
        cwd: path,
        model: aiModelSlug(props.selectedAi.value),
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

  const closeFolder$ = $(async () => {
    state.folderPath = null;
    state.status = "idle";
    state.statusNote = "";
    state.pendingPermissionId = null;
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

      await updatePermission(requestId, (p) => ({ ...p, state: "answered", receipt }));
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

      if (kind === "agent_message_chunk") {
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
        const usage = update.usage;
        if (usage) {
          const modelKey = usage.modelUsage
            ? Object.keys(usage.modelUsage)[0]
            : undefined;
          const online = props.selectedAi.value.aiConfig?.model?.startsWith("online:");
          const folder = state.folderPath?.split("/").filter(Boolean).pop();
          mutateTurn((m) => ({
            ...m,
            tokens: {
              prompt_tokens: usage.inputTokens,
              completion_tokens: usage.outputTokens,
              total_tokens: usage.totalTokens,
            },
            servedBy: modelKey
              ? online
                ? `online:${modelKey}`
                : modelKey
              : m.servedBy,
            routingReason: `Agent session in ${folder ?? "your folder"}`,
            agentStats: {
              durationMs: usage.apiDurationMs,
              modelCalls: usage.modelCalls,
            },
          }));
        }
      } else if (kind === "tool_call" || kind === "tool_call_update") {
        const toolCallId = update.toolCallId || uuidv4();
        const human = humanizeAction(update);
        const out = actionOutput(update);
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
                ...out,
              },
            });
          }
          return { ...m, agentLog: log };
        });
      }
    });

    const unPermission = await listen<any>("agent-permission", (e) => {
      const params = e.payload?.params ?? {};
      const tc = params.toolCall ?? {};
      const locations = (tc.locations ?? [])
        .map((l: any) => l?.path)
        .filter(Boolean);
      const diffs = (tc.content ?? [])
        .filter((c: any) => c?.type === "diff")
        .map((c: any) =>
          [c.path, previewDiff(c.oldText ?? null, c.newText ?? "")]
            .filter(Boolean)
            .join("\n"),
        );
      const permission: AgentPermission = {
        requestId: e.payload?.id,
        toolCallId: typeof tc.toolCallId === "string" ? tc.toolCallId : undefined,
        title: tc.title || "The agent asks for permission",
        kind: tc.kind,
        command:
          typeof tc.rawInput?.command === "string" ? tc.rawInput.command : undefined,
        diff: diffs.length ? diffs.join("\n\n") : undefined,
        locations,
        options: (params.options ?? []).map((o: any) => ({
          optionId: o.optionId,
          name: o.name,
          kind: o.kind,
        })),
        state: "pending",
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

    const finishTurn = (errorText?: string) => {
      mutateTurn((m) => {
        let log = (m.agentLog ?? []).map((i) =>
          i.type === "action" && (i.action.status === "in_progress" || i.action.status === "pending")
            ? {
                ...i,
                action: {
                  ...i.action,
                  status: errorText ? ("failed" as const) : ("completed" as const),
                },
              }
            : i,
        );
        // The turn's last words ARE the answer: promote the trailing
        // narration into the bubble body (content goes "" -> answer exactly
        // once - it must never shrink). Earlier narration stays in the box.
        let content = m.content;
        const last = log[log.length - 1];
        if (content === "" && last?.type === "narration") {
          content = last.text;
          log = log.slice(0, -1);
        }
        return { ...m, isLoading: false, error: errorText ?? m.error, content, agentLog: log };
      });
      // A bubble that never got anything (cancelled before output) is noise.
      const id = turnId.value;
      props.chatState.messages = props.chatState.messages.filter(
        (m) =>
          !(m.id === id && m.content === "" && !(m.agentLog ?? []).length && !m.error),
      );
      // Record the answer to the transcript (fire-and-forget).
      const bubble = props.chatState.messages.find((m) => m.id === id);
      if (bubble?.content) {
        recordAssistantTurn(bubble.content, bubble.servedBy, bubble.tokens).catch(
          () => {},
        );
      }
      props.chatState.isLoading = false;
      state.liveStatus = "";
      if (state.status === "working") state.status = "ready";
    };

    const unTurn = await listen<any>("agent-turn", async (e) => {
      const err = e.payload?.error;
      const stop = e.payload?.result?.stopReason;
      if (err) {
        finishTurn(err.message ? String(err.message) : "The agent hit an error.");
      } else if (stop === "refusal") {
        finishTurn("The agent declined to continue this task.");
      } else {
        finishTurn();
      }
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
            error: `This folder's agent couldn't switch to ${props.selectedAi.value.label}'s model and is running on its default instead. (${e.payload})`,
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
                    ? { ...i, permission: { ...i.permission, state: "expired" as const } }
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
      unTurn();
      unLog();
      unExit();
    });
  });

  return {
    agentState: state,
    openFolder$,
    closeFolder$,
    sendPrompt$,
    cancelTurn$,
    respondPermission$,
    answerPermissionByReply$,
  };
}
