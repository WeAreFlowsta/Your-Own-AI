/**
 * Folder (Build agent) session state for the chat.
 *
 * Owns the open-folder lifecycle and translates the agent bridge's Tauri
 * events (ACP session traffic) into the SAME message list the chat renders:
 * agent text streams into normal assistant bubbles, contiguous tool calls
 * group into one activity-run message, permission requests become inline
 * card messages that the user answers (or answers by simply replying).
 *
 * The conversation is the window: while a folder is open, every prompt goes
 * through the agent session instead of the direct model call.
 */

import { $, useSignal, useStore, useVisibleTask$, type Signal } from "@builder.io/qwik";
import { v4 as uuidv4 } from "uuid";
import type { AgentPermission, AgentRun, Message, SelectedAiModel } from "../types";
import type { UseChatState } from "./useChat";

export interface AgentSessionState {
  folderPath: string | null;
  /** idle = no folder open; starting = process/handshake in flight;
   *  ready = session open, waiting for input; working = turn in flight;
   *  stopped = process exited while a folder was open (needs reopen). */
  status: "idle" | "starting" | "ready" | "working" | "stopped";
  statusNote: string;
  /** Message id of the permission card currently awaiting an answer. */
  pendingPermissionId: string | null;
  /** True while the pending permission card is scrolled out of view -
   *  drives the floating jump pill. */
  pendingCardOffscreen: boolean;
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

function resolveBinaryPath(): string {
  try {
    return localStorage.getItem("build-binary-path") || DEV_BINARY_FALLBACK;
  } catch {
    return DEV_BINARY_FALLBACK;
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

export function useAgentSession(props: UseAgentSessionProps) {
  const state = useStore<AgentSessionState>({
    folderPath: null,
    status: "idle",
    statusNote: "",
    pendingPermissionId: null,
    pendingCardOffscreen: false,
    touchedFiles: [],
  });

  // A prompt waiting for the session: typed before the handshake finished,
  // or typed as the answer to a permission card (sent when the turn ends).
  // `rendered` = the user bubble is already in the transcript.
  const queued = useSignal<{ text: string; rendered: boolean } | null>(null);

  /** The optimistic assistant bubble pushed at Enter - same trick the direct
   *  chat path uses: mounting a loading bubble is what anchors the question
   *  to the top and shows the avatar + action bar instantly, instead of a
   *  silent gap until the first streamed chunk. */
  const turnPlaceholder = $(
    (): Message => ({
      id: uuidv4(),
      role: "assistant",
      content: "",
      model: props.selectedAi.value.id,
      aiLabel: props.selectedAi.value.label,
      aiImageUrl: props.selectedAi.value.imageUrl || undefined,
      isLoading: true,
      agentTurn: true,
    }),
  );

  const invokeTauri = $(async (cmd: string, args?: Record<string, unknown>) => {
    const { invoke } = await import("@tauri-apps/api/core");
    return invoke(cmd, args);
  });

  /** Send a prompt into the live session (session must be ready). */
  const dispatchPrompt = $(async (text: string) => {
    state.status = "working";
    props.chatState.isLoading = true;
    try {
      await invokeTauri("send_agent_prompt", { text });
    } catch (err) {
      state.status = "ready";
      props.chatState.isLoading = false;
      // The optimistic placeholder has no turn to receive - drop it.
      const msgs = props.chatState.messages;
      const last = msgs[msgs.length - 1];
      if (last?.agentTurn && last.isLoading && last.content === "") {
        props.chatState.messages = msgs.slice(0, -1);
      }
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
    // One assignment for question + placeholder, mirroring useChat's
    // optimistic append. isLoading flips in the same flush so the turn's
    // scroll spacer is in the layout before the placeholder's anchor runs.
    props.chatState.messages = [
      ...props.chatState.messages,
      { id: uuidv4(), role: "user", content: text, model: "user" },
      await turnPlaceholder(),
    ];
    props.chatState.isLoading = true;
    if (state.status === "ready") {
      await dispatchPrompt(text);
    } else {
      // Working: turns are strictly sequential over ACP - hold until the
      // current turn completes. Starting: hold until agent-ready.
      queued.value = { text, rendered: true };
    }
  });

  const openFolder$ = $(async (path: string) => {
    state.folderPath = path;
    state.status = "starting";
    state.statusNote = "Starting the agent...";
    state.touchedFiles = [];
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

  /** Answer the pending permission card with a button. `always` upgrades to
   *  the agent's always-variant option when it offers one. */
  const respondPermission$ = $(
    async (messageId: string, decision: "allow" | "reject", always: boolean) => {
      const msg = props.chatState.messages.find((m) => m.id === messageId);
      const perm = msg?.agentPermission;
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

      props.chatState.messages = props.chatState.messages.map((m) =>
        m.id === messageId && m.agentPermission
          ? {
              ...m,
              agentPermission: { ...m.agentPermission, state: "answered" as const, receipt },
            }
          : m,
      );
      state.pendingPermissionId = null;
      await invokeTauri("respond_agent_permission", {
        requestId: perm.requestId,
        optionId: option.optionId,
      });
    },
  );

  /** Typing while a card waits = decline with instructions: reject once,
   *  show the reply in place, send the text as the next prompt when the
   *  turn ends. Reads the same as a native followup answer. */
  const answerPermissionByReply$ = $(async (text: string) => {
    const id = state.pendingPermissionId;
    if (!id) return;
    const msg = props.chatState.messages.find((m) => m.id === id);
    const perm = msg?.agentPermission;
    if (!perm || perm.state !== "pending") return;

    const option =
      perm.options.find((o) => o.kind === "reject_once") ??
      perm.options.find((o) => o.kind?.startsWith("reject"));
    props.chatState.messages = props.chatState.messages.map((m) =>
      m.id === id && m.agentPermission
        ? {
            ...m,
            agentPermission: {
              ...m.agentPermission,
              state: "answered" as const,
              receipt: "Declined - you replied instead",
            },
          }
        : m,
    );
    state.pendingPermissionId = null;
    props.chatState.messages = [
      ...props.chatState.messages,
      { id: uuidv4(), role: "user", content: text, model: "user" },
      await turnPlaceholder(),
    ];
    queued.value = { text, rendered: true };
    if (option) {
      await invokeTauri("respond_agent_permission", {
        requestId: perm.requestId,
        optionId: option.optionId,
      });
    }
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    const { listen } = await import("@tauri-apps/api/event");
    const ai = () => props.selectedAi.value;

    /** A bubble that is open for streaming but has no text yet - either the
     *  turn placeholder or a fresh segment. */
    const isEmptyPlaceholder = (m: Message | undefined): boolean =>
      !!m &&
      m.role === "assistant" &&
      !!m.isLoading &&
      !m.agentRun &&
      !m.agentPermission &&
      m.content === "";

    const appendAssistantText = (text: string) => {
      const msgs = props.chatState.messages;
      const last = msgs[msgs.length - 1];
      if (last && last.role === "assistant" && last.isLoading && !last.agentRun && !last.agentPermission) {
        props.chatState.messages = [
          ...msgs.slice(0, -1),
          { ...last, content: last.content + text },
        ];
      } else {
        // Text resuming after activity: a new segment bubble. It prints in
        // place (no re-anchor, no space reservation) so the view stays put.
        props.chatState.messages = [
          ...msgs,
          {
            id: uuidv4(),
            role: "assistant",
            content: text,
            model: ai().id,
            aiLabel: ai().label,
            aiImageUrl: ai().imageUrl || undefined,
            isLoading: true,
            agentTurn: true,
            agentSegment: true,
          },
        ];
      }
    };

    const upsertToolCall = (update: any) => {
      const toolCallId = update.toolCallId || uuidv4();
      const action = {
        toolCallId,
        title: update.title || "Working...",
        kind: update.kind,
        status: (update.status || "in_progress") as
          | "pending"
          | "in_progress"
          | "completed"
          | "failed",
        locations: (update.locations ?? [])
          .map((l: any) => l?.path)
          .filter(Boolean),
      };
      for (const p of action.locations) {
        if (!state.touchedFiles.includes(p)) state.touchedFiles = [...state.touchedFiles, p];
      }

      const withAction = (m: Message): Message => {
        const run = m.agentRun!;
        const existing = run.actions.findIndex((a) => a.toolCallId === toolCallId);
        const actions =
          existing >= 0
            ? run.actions.map((a, i) =>
                i === existing
                  ? {
                      ...a,
                      status: action.status,
                      title: update.title || a.title,
                      kind: update.kind ?? a.kind,
                      locations: action.locations.length ? action.locations : a.locations,
                    }
                  : a,
              )
            : [...run.actions, action];
        return { ...m, agentRun: { ...run, actions } };
      };

      const msgs = props.chatState.messages;
      const last = msgs[msgs.length - 1];
      const prev = msgs[msgs.length - 2];
      if (last?.agentRun && !last.agentRun.done) {
        props.chatState.messages = [...msgs.slice(0, -1), withAction(last)];
      } else if (isEmptyPlaceholder(last) && prev?.agentRun && !prev.agentRun.done) {
        // Still the same contiguous run, waiting bubble after it - keep the
        // bubble open below and extend the run above it.
        props.chatState.messages = [...msgs.slice(0, -2), withAction(prev), last];
      } else if (isEmptyPlaceholder(last)) {
        // First activity of this stretch: slide the run card in ABOVE the
        // waiting bubble. The bubble stays open (and keeps its anchor) for
        // the text that follows - no empty bubble is ever stranded.
        props.chatState.messages = [
          ...msgs.slice(0, -1),
          {
            id: uuidv4(),
            role: "assistant",
            content: "",
            model: ai().id,
            agentRun: { actions: [action] } as AgentRun,
          },
          last,
        ];
      } else {
        // A streamed text bubble precedes us: close it so later chunks open
        // a fresh segment below, and start a new run after it.
        const closed =
          last && last.role === "assistant" && last.isLoading && !last.agentRun
            ? [...msgs.slice(0, -1), { ...last, isLoading: false }]
            : msgs;
        props.chatState.messages = [
          ...closed,
          {
            id: uuidv4(),
            role: "assistant",
            content: "",
            model: ai().id,
            agentRun: { actions: [action] } as AgentRun,
          },
        ];
      }
    };

    const finishTurn = (errorText?: string) => {
      props.chatState.messages = props.chatState.messages
        .map((m) => {
          if (m.agentRun && !m.agentRun.done) {
            return { ...m, agentRun: { ...m.agentRun, done: true } };
          }
          if (m.isLoading) {
            return { ...m, isLoading: false, error: errorText ?? m.error };
          }
          return m;
        })
        // A placeholder that never received text (turn ended on activity or
        // was cancelled) would linger as a bare empty bubble - drop it.
        // Errors stay: an empty bubble WITH an error renders the error.
        .filter(
          (m) =>
            !(
              m.agentTurn &&
              m.content === "" &&
              !m.error &&
              !m.agentRun &&
              !m.agentPermission
            ),
        );
      props.chatState.isLoading = false;
      if (state.status === "working") state.status = "ready";
    };

    const unReady = await listen<{ sessionId: string }>("agent-ready", async () => {
      state.status = "ready";
      state.statusNote = "";
      const q = queued.value;
      if (q) {
        queued.value = null;
        await dispatchPrompt(q.text);
      }
    });

    const unUpdate = await listen<any>("agent-update", (e) => {
      const update = e.payload?.params?.update;
      if (!update) return;
      const kind = update.sessionUpdate;
      if (kind === "agent_message_chunk") {
        appendAssistantText(update.content?.text ?? "");
      } else if (kind === "tool_call" || kind === "tool_call_update") {
        upsertToolCall(update);
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
      const id = uuidv4();
      state.pendingPermissionId = id;
      const card: Message = {
        id,
        role: "assistant",
        content: "",
        model: ai().id,
        aiLabel: ai().label,
        agentPermission: permission,
      };
      const msgs = props.chatState.messages;
      const last = msgs[msgs.length - 1];
      // Same placement rule as activity: the card slides in above a waiting
      // bubble so the bubble stays open for the text that follows.
      props.chatState.messages = isEmptyPlaceholder(last)
        ? [...msgs.slice(0, -1), card, last]
        : [...msgs, card];
    });

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
        await dispatchPrompt(q.text);
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
            model: ai().id,
            aiLabel: ai().label,
            error: `This folder's agent couldn't switch to ${ai().label}'s model and is running on its default instead. (${e.payload})`,
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
      if (state.pendingPermissionId) {
        props.chatState.messages = props.chatState.messages.map((m) =>
          m.agentPermission?.state === "pending"
            ? { ...m, agentPermission: { ...m.agentPermission, state: "expired" as const } }
            : m,
        );
        state.pendingPermissionId = null;
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
