/**
 * Build Page - drive the Your Own AI Build coding agent from the app.
 *
 * The Tauri backend spawns the agent binary (`agent stdio`, ACP JSON-RPC)
 * and forwards its session as events; this page renders the conversation,
 * streams replies, and surfaces permission requests as explicit questions.
 * Nothing is auto-approved.
 */

import { component$, useSignal, useStore, useVisibleTask$, $ } from "@builder.io/qwik";
import { useNavigate, type DocumentHead } from "@builder.io/qwik-city";
import { LuArrowLeft } from "@qwikest/icons/lucide";
import { renderMarkdown } from "../../utils/renderMarkdown";

interface TranscriptItem {
  kind: "user" | "agent" | "tool" | "info";
  text: string;
  status?: string;
}

interface PermissionRequest {
  requestId: number;
  title: string;
  options: { optionId: string; name: string; kind?: string }[];
}

const DEFAULT_BINARY =
  "/home/solar/Documents/Flowsta/Projects/FlowstaAuth/your-own-ai-build/target/release/your-own-ai-build";
const DEFAULT_WORKSPACE = "/home/solar/build-playground";

export default component$(() => {
  const nav = useNavigate();
  const status = useSignal<"idle" | "starting" | "ready" | "working">("idle");
  const statusNote = useSignal("");
  const binaryPath = useSignal(DEFAULT_BINARY);
  const workspace = useSignal(DEFAULT_WORKSPACE);
  const input = useSignal("");
  const items = useStore<{ list: TranscriptItem[] }>({ list: [] });
  const permission = useSignal<PermissionRequest | null>(null);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    const { listen } = await import("@tauri-apps/api/event");

    const unReady = await listen<{ sessionId: string }>("agent-ready", () => {
      status.value = "ready";
      statusNote.value = "";
      items.list.push({ kind: "info", text: "Agent session ready." });
    });

    const unUpdate = await listen<any>("agent-update", (e) => {
      const update = e.payload?.params?.update;
      if (!update) return;
      const kind = update.sessionUpdate;
      if (kind === "agent_message_chunk") {
        const text = update.content?.text ?? "";
        const last = items.list[items.list.length - 1];
        if (last && last.kind === "agent") {
          last.text += text;
        } else {
          items.list.push({ kind: "agent", text });
        }
      } else if (kind === "tool_call" || kind === "tool_call_update") {
        const title = update.title || update.toolCallId || "tool";
        const st = update.status || "";
        const last = items.list[items.list.length - 1];
        if (last && last.kind === "tool" && last.text === title) {
          last.status = st;
        } else {
          items.list.push({ kind: "tool", text: title, status: st });
        }
      } else if (kind === "plan") {
        items.list.push({ kind: "info", text: "Plan updated." });
      } else if (kind === "turn_completed" && update.stop_reason === "error") {
        items.list.push({
          kind: "info",
          text: `Error: ${update.agent_result ?? "the agent hit an error"}`,
        });
        status.value = "ready";
      }
    });

    const unPermission = await listen<any>("agent-permission", (e) => {
      const params = e.payload?.params ?? {};
      const tc = params.toolCall ?? {};
      permission.value = {
        requestId: e.payload?.id,
        title: tc.title || params.title || "The agent asks for permission",
        options: (params.options ?? []).map((o: any) => ({
          optionId: o.optionId,
          name: o.name,
          kind: o.kind,
        })),
      };
    });

    const unTurn = await listen<any>("agent-turn", (e) => {
      status.value = "ready";
      const err = e.payload?.error;
      if (err) {
        items.list.push({
          kind: "info",
          text: `Error: ${err.message ?? JSON.stringify(err)}`,
        });
        return;
      }
      const stop = e.payload?.result?.stopReason;
      if (stop && stop !== "end_turn") {
        items.list.push({ kind: "info", text: `Turn ended: ${stop}` });
      }
    });

    const unExit = await listen<{ code: number | null }>("agent-exit", (e) => {
      status.value = "idle";
      items.list.push({
        kind: "info",
        text: `Agent exited (code ${e.payload?.code ?? "?"}).`,
      });
    });

    const unLog = await listen<string>("agent-log", () => {
      // stderr is noisy; keep it off the transcript. Hook up when debugging.
    });

    cleanup(() => {
      unReady();
      unUpdate();
      unPermission();
      unTurn();
      unExit();
      unLog();
    });
  });

  const startAgent = $(async () => {
    status.value = "starting";
    statusNote.value = "Starting agent...";
    items.list.push({ kind: "info", text: "Starting agent..." });
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("start_build_agent", {
        binary: binaryPath.value,
        cwd: workspace.value,
      });
    } catch (err) {
      status.value = "idle";
      statusNote.value = String(err);
      items.list.push({ kind: "info", text: `Failed to start: ${err}` });
    }
  });

  const stopAgent = $(async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("stop_build_agent");
  });

  const sendPrompt = $(async () => {
    const text = input.value.trim();
    if (!text || status.value !== "ready") return;
    input.value = "";
    items.list.push({ kind: "user", text });
    status.value = "working";
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("send_agent_prompt", { text });
    } catch (err) {
      status.value = "ready";
      items.list.push({ kind: "info", text: `Send failed: ${err}` });
    }
  });

  const answerPermission = $(async (optionId: string | null) => {
    const req = permission.value;
    if (!req) return;
    permission.value = null;
    const { invoke } = await import("@tauri-apps/api/core");
    await invoke("respond_agent_permission", {
      requestId: req.requestId,
      optionId,
    });
  });

  return (
    <div class="min-h-screen bg-gray-950 text-white flex flex-col">
      <main class="flex-1 flex flex-col max-w-4xl w-full mx-auto px-4 pb-4">
        <div class="py-4 flex items-start gap-3">
          <button
            class="mt-0.5 rounded-lg p-1.5 text-gray-400 hover:bg-white/10 hover:text-white"
            onClick$={async () => {
              await nav("/chat/");
            }}
            aria-label="Back"
          >
            <LuArrowLeft class="h-5 w-5" />
          </button>
          <div>
          <h1 class="text-xl font-semibold">Build</h1>
          <p class="text-sm text-gray-400">
            An AI agent that works in a folder on your computer - powered by
            your own models, asking before it acts.
          </p>
          </div>
        </div>

        {status.value === "idle" || status.value === "starting" ? (
          <div class="rounded-xl border border-white/10 bg-black/30 p-4 space-y-3">
            <label class="block text-sm text-gray-300">
              Agent binary
              <input
                class="mt-1 w-full rounded-lg bg-black/30 border border-white/10 px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                bind:value={binaryPath}
              />
            </label>
            <label class="block text-sm text-gray-300">
              Workspace folder
              <input
                class="mt-1 w-full rounded-lg bg-black/30 border border-white/10 px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                bind:value={workspace}
              />
            </label>
            <div class="flex justify-end">
              <button
                class="rounded-full bg-white/90 text-gray-900 px-5 py-2 text-sm font-semibold hover:bg-white disabled:opacity-50"
                disabled={status.value === "starting"}
                onClick$={startAgent}
              >
                {status.value === "starting" ? "Starting..." : "Start agent"}
              </button>
            </div>
            {statusNote.value && (
              <p class="text-xs text-red-400">{statusNote.value}</p>
            )}
          </div>
        ) : (
          <div class="flex-1 flex flex-col min-h-0">
            <div class="flex items-center justify-between py-2">
              <span class="text-xs text-gray-400">
                {status.value === "working" ? "Working..." : "Ready"} -{" "}
                {workspace.value}
              </span>
              <button
                class="rounded-full border border-white/20 px-4 py-1.5 text-xs text-gray-300 hover:bg-white/10"
                onClick$={stopAgent}
              >
                Stop agent
              </button>
            </div>

            <div class="flex-1 overflow-y-auto rounded-xl border border-white/10 bg-black/20 p-4 space-y-3">
              {items.list.map((item, i) => (
                <div key={i}>
                  {item.kind === "user" && (
                    <div class="text-sm bg-blue-600/20 border border-blue-500/30 rounded-lg px-3 py-2 ml-12">
                      {item.text}
                    </div>
                  )}
                  {item.kind === "agent" && (
                    <div
                      class="prose prose-invert prose-sm max-w-none text-sm"
                      dangerouslySetInnerHTML={renderMarkdown(item.text)}
                    />
                  )}
                  {item.kind === "tool" && (
                    <div class="text-xs text-gray-400 font-mono border-l-2 border-gray-700 pl-2">
                      {item.text}
                      {item.status ? ` - ${item.status}` : ""}
                    </div>
                  )}
                  {item.kind === "info" && (
                    <div class="text-xs text-gray-500 italic">{item.text}</div>
                  )}
                </div>
              ))}
            </div>

            <div class="pt-3 flex gap-2">
              <input
                class="flex-1 rounded-lg bg-black/30 border border-white/10 px-3 py-2 text-sm focus:outline-none focus:ring-2 focus:ring-blue-500"
                placeholder={
                  status.value === "working"
                    ? "Agent is working..."
                    : "Ask the agent to do something in the workspace..."
                }
                bind:value={input}
                onKeyDown$={(e) => {
                  if (e.key === "Enter") sendPrompt();
                }}
              />
              <button
                class="rounded-full bg-white/90 text-gray-900 px-5 py-2 text-sm font-semibold hover:bg-white disabled:opacity-50"
                disabled={status.value === "working"}
                onClick$={sendPrompt}
              >
                Send
              </button>
            </div>
          </div>
        )}

        {permission.value && (
          <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/60 p-4">
            <div class="w-full max-w-md rounded-xl border border-white/10 bg-gray-900 p-5 space-y-4">
              <h2 class="text-sm font-semibold">Permission request</h2>
              <p class="text-sm text-gray-300">{permission.value.title}</p>
              <div class="flex flex-col gap-2">
                {permission.value.options.map((opt) => (
                  <button
                    key={opt.optionId}
                    class="rounded-lg border border-white/15 px-4 py-2 text-sm text-left hover:bg-white/10"
                    onClick$={() => answerPermission(opt.optionId)}
                  >
                    {opt.name}
                  </button>
                ))}
                <button
                  class="rounded-lg px-4 py-2 text-sm text-left text-red-400 hover:bg-red-500/10"
                  onClick$={() => answerPermission(null)}
                >
                  Cancel
                </button>
              </div>
            </div>
          </div>
        )}
      </main>
    </div>
  );
});

export const head: DocumentHead = {
  title: "Build - Your Own AI",
};
