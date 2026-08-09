/**
 * "Bring your history" - import conversations exported from other AI apps
 * (ChatGPT, Claude, Perplexity) or coding assistants (Claude Code, Aider)
 * into an encrypted local archive.
 *
 * Stage 1: pick file/folder -> parse -> encrypted archive -> instant
 * summary. Stage 2: the background distiller turns archives into memory.
 * Adoption writes an archive into a chosen AI's conversations. Coding
 * sources add a "What to bring" choice (user-only vs full) applied at
 * parse time.
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { LuDownload, LuTrash2, LuChevronDown } from "@qwikest/icons/lucide";
import LiquidMetalButton from "./LiquidMetalButton";

interface ImportSummary {
  archive_id: string;
  source: string;
  file_name: string;
  conversation_count: number;
  message_count: number;
  earliest_us: number | null;
  latest_us: number | null;
  imported_at_us: number;
  adopted_by: { ai_id: string; ai_name: string }[];
}

interface AiOption {
  id: string;
  name: string;
}

const SOURCE_NAMES: Record<string, string> = {
  chatgpt: "ChatGPT",
  claude: "Claude",
  perplexity: "Perplexity",
  "claude-code": "Claude Code",
  aider: "Aider",
};

function sourceName(source: string): string {
  return SOURCE_NAMES[source] ?? source;
}

function yearOf(us: number | null): string | null {
  if (!us) return null;
  return new Date(us / 1000).getFullYear().toString();
}

function dateRange(s: ImportSummary): string {
  const from = yearOf(s.earliest_us);
  const to = yearOf(s.latest_us);
  if (!from) return "";
  return from === to ? from : `${from} - ${to}`;
}

interface DistillView {
  state: string;
  archiveId: string | null;
  processed: number;
  total: number;
  error: string | null;
}

export default component$(() => {
  const archives = useSignal<ImportSummary[]>([]);
  const busy = useSignal(false);
  const error = useSignal("");
  const justImported = useSignal<ImportSummary | null>(null);
  const distill = useSignal<DistillView>({
    state: "idle",
    archiveId: null,
    processed: 0,
    total: 0,
    error: null,
  });
  const learnedIds = useSignal<string[]>([]);
  const aiOptions = useSignal<AiOption[]>([]);
  const adoptPick = useSignal<Record<string, string>>({});
  const adoptBusyId = useSignal<string | null>(null);
  const adoptProgress = useSignal<{ done: number; total: number } | null>(null);
  /** Which archive's AI dropdown is open (custom menu - a native <select>
   *  popup is GTK-themed on Linux/webkit and ignores our theme). */
  const adoptMenuOpenId = useSignal<string | null>(null);
  /** One primary CTA opens a source chooser: an AI chat app vs a coding
   *  assistant. The coding branch adds the "What to bring" choice -
   *  user_only is its suggested default (90% of a session is tool traffic
   *  and the assistant's prose is rarely about the user). The choice
   *  applies at parse time: a user-only archive never stores assistant
   *  text at all. */
  const chooserOpen = useSignal(false);
  const codingOpen = useSignal(false);
  const bringMode = useSignal<"user_only" | "full">("user_only");

  const refresh = $(async () => {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      archives.value = await invoke<ImportSummary[]>("import_archives_list");
      const { distilledArchiveIds } = await import("../utils/importDistiller");
      learnedIds.value = distilledArchiveIds();
    } catch {
      // Non-fatal: the card still offers the import button.
    }
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    refresh();
    const { subscribeDistill, distilledArchiveIds } = await import(
      "../utils/importDistiller"
    );
    const unsubscribe = subscribeDistill((s) => {
      distill.value = { ...s };
      if (s.state === "done") learnedIds.value = distilledArchiveIds();
    });
    // Active provisioned AIs for the adoption picker; default = last used.
    const { getLocalCustomAis } = await import("../utils/localAiStorage");
    const ais = await getLocalCustomAis();
    aiOptions.value = ais
      .filter((ai) => ai.status === "active" && ai.agentPubKey)
      .map((ai) => ({ id: ai.id, name: ai.name }));
    const { listen } = await import("@tauri-apps/api/event");
    const unlisten = await listen<{ done: number; total: number }>(
      "import-adopt-progress",
      (e) => {
        adoptProgress.value = { done: e.payload.done, total: e.payload.total };
      },
    );
    cleanup(() => {
      unsubscribe();
      unlisten();
    });
    // Catch-up pass: any adopted archive whose summaries were interrupted
    // (or waiting on models) finishes here. Idempotent per conversation.
    const adoptedAiIds = new Set(
      archives.value.flatMap((a) => (a.adopted_by ?? []).map((x) => x.ai_id)),
    );
    if (adoptedAiIds.size > 0) {
      const { summarizeAdoptedConversations } = await import(
        "../utils/importSummaries"
      );
      for (const aiId of adoptedAiIds) {
        summarizeAdoptedConversations(aiId).catch(() => {});
      }
    }
  });

  const adopt = $(async (archiveId: string) => {
    // Same resolution as the dropdown label: explicit pick, else last-used
    // AI, else the first AI that hasn't already adopted this archive.
    const adopted =
      archives.value.find((x) => x.archive_id === archiveId)?.adopted_by ?? [];
    const avail = aiOptions.value.filter(
      (o) => !adopted.some((x) => x.ai_id === o.id),
    );
    const lastAiId =
      typeof localStorage !== "undefined" ? localStorage.getItem("lastAiId") : null;
    const ai =
      avail.find((o) => o.id === adoptPick.value[archiveId]) ??
      avail.find((o) => o.id === lastAiId) ??
      avail[0];
    if (!ai) return;
    error.value = "";
    adoptBusyId.value = archiveId;
    adoptProgress.value = null;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke<number>("import_archive_adopt", {
        archiveId,
        aiId: ai.id,
        aiName: ai.name,
      });
      await refresh();
      // Give the adopting AI recall over its new history - background,
      // idempotent, quietly retried by the next card mount if models
      // aren't ready yet.
      import("../utils/importSummaries")
        .then((m) => m.summarizeAdoptedConversations(ai.id))
        .catch(() => {});
    } catch (e) {
      error.value = String(e);
    } finally {
      adoptBusyId.value = null;
      adoptProgress.value = null;
    }
  });

  const runImport = $(async (path: string, mode: "user_only" | "full") => {
    busy.value = true;
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      const summary = await invoke<ImportSummary>("import_conversations_scan", {
        path,
        mode: mode === "user_only" ? "user_only" : null,
      });
      justImported.value = summary;
      chooserOpen.value = false;
      codingOpen.value = false;
      await refresh();
      // Learning starts right away and continues in the background - it
      // survives navigating away and resumes on the next launch if needed.
      const { startDistill } = await import("../utils/importDistiller");
      await startDistill(summary.archive_id);
    } catch (e) {
      error.value = String(e);
    } finally {
      busy.value = false;
    }
  });

  const pickAndImport = $(async () => {
    error.value = "";
    justImported.value = null;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open({
        multiple: false,
        filters: [{ name: "Chat export", extensions: ["zip", "json"] }],
      });
      if (!selected || Array.isArray(selected)) return;
      await runImport(selected, "full");
    } catch (e) {
      error.value = String(e);
    }
  });

  /** Coding sessions: pick a folder of .jsonl sessions (Claude Code) or a
   *  single session/history file (a .jsonl, or Aider's
   *  .aider.chat.history.md). */
  const pickCodingAndImport = $(async (directory: boolean) => {
    error.value = "";
    justImported.value = null;
    try {
      const { open } = await import("@tauri-apps/plugin-dialog");
      const selected = await open(
        directory
          ? { multiple: false, directory: true }
          : {
              multiple: false,
              filters: [
                { name: "Coding sessions", extensions: ["jsonl", "md"] },
              ],
            },
      );
      if (!selected || Array.isArray(selected)) return;
      await runImport(selected, bringMode.value);
    } catch (e) {
      error.value = String(e);
    }
  });

  const remove = $(async (archiveId: string) => {
    try {
      const { invoke } = await import("@tauri-apps/api/core");
      await invoke("import_archive_delete", { archiveId });
      const { clearDistillState } = await import("../utils/importDistiller");
      clearDistillState(archiveId);
      if (justImported.value?.archive_id === archiveId) justImported.value = null;
      await refresh();
    } catch (e) {
      error.value = String(e);
    }
  });

  return (
    <div class="generic-container mt-6 rounded-2xl p-6">
      <div class="mb-2 flex items-center gap-2">
        <LuDownload class="h-5 w-5 text-[var(--text-secondary)]" />
        <h3 class="font-semibold text-base text-[var(--text-primary)]">
          Import your history
        </h3>
      </div>
      <p class="mb-4 text-sm text-[var(--text-secondary)]">
        Bring your conversations with you - from AI chat apps like ChatGPT,
        Claude, and Perplexity, or from coding assistants like Claude Code and
        Aider. Your history becomes an encrypted archive on this machine.
        Nothing is uploaded anywhere.
      </p>
      <LiquidMetalButton
        onClick$={() => {
          chooserOpen.value = !chooserOpen.value;
          if (!chooserOpen.value) codingOpen.value = false;
        }}
        disabled={busy.value}
        class="px-4 py-2 text-sm"
      >
        {busy.value ? "Reading your history…" : "Import your history"}
      </LiquidMetalButton>

      {chooserOpen.value && (
        <div class="mt-3">
          <p class="mb-2 text-xs font-medium text-[var(--text-primary)]">
            Where is your history coming from?
          </p>
          <div class="flex flex-col gap-2 sm:flex-row">
            <button
              onClick$={pickAndImport}
              disabled={busy.value}
              class="flex-1 rounded-xl border border-[var(--border-subtle)] p-3 text-left transition-colors hover:border-[var(--bg-button-primary)] disabled:opacity-60"
            >
              <span class="block text-sm font-medium text-[var(--text-primary)]">
                An AI chat app
              </span>
              <span class="mt-1 block text-xs text-[var(--text-secondary)]">
                ChatGPT, Claude, or Perplexity - pick the export file they
                give you.
              </span>
            </button>
            <button
              onClick$={() => (codingOpen.value = !codingOpen.value)}
              disabled={busy.value}
              class={`flex-1 rounded-xl border p-3 text-left transition-colors hover:border-[var(--bg-button-primary)] disabled:opacity-60 ${
                codingOpen.value
                  ? "border-[var(--bg-button-primary)]"
                  : "border-[var(--border-subtle)]"
              }`}
            >
              <span class="block text-sm font-medium text-[var(--text-primary)]">
                A coding assistant
              </span>
              <span class="mt-1 block text-xs text-[var(--text-secondary)]">
                Claude Code or Aider sessions already on this machine.
              </span>
            </button>
          </div>
        </div>
      )}

      {chooserOpen.value && codingOpen.value && (
        <div class="mt-3 rounded-lg border border-[var(--border-subtle)] p-3">
          <p class="mb-2 text-xs font-medium text-[var(--text-primary)]">
            What to bring
          </p>
          <div class="space-y-1.5">
            <label class="flex cursor-pointer items-start gap-2 text-xs text-[var(--text-secondary)]">
              <input
                type="radio"
                name="bring-mode"
                checked={bringMode.value === "user_only"}
                onChange$={() => (bringMode.value = "user_only")}
                class="mt-0.5 accent-[var(--bg-button-primary)]"
              />
              <span>
                <span class="font-medium text-[var(--text-primary)]">
                  Just what you said
                </span>{" "}
                - only your own messages. The usual pick: it is what teaches
                your AIs about you.
              </span>
            </label>
            <label class="flex cursor-pointer items-start gap-2 text-xs text-[var(--text-secondary)]">
              <input
                type="radio"
                name="bring-mode"
                checked={bringMode.value === "full"}
                onChange$={() => (bringMode.value = "full")}
                class="mt-0.5 accent-[var(--bg-button-primary)]"
              />
              <span>
                <span class="font-medium text-[var(--text-primary)]">
                  Full conversations
                </span>{" "}
                - also keeps the assistant's replies.
              </span>
            </label>
          </div>
          <p class="mt-2 text-xs text-[var(--text-muted)]">
            Commands, file contents, and other tool output are never imported
            with either choice.
          </p>
          <div class="mt-3 flex flex-wrap items-center gap-2">
            <LiquidMetalButton
              variant="secondary"
              onClick$={() => pickCodingAndImport(true)}
              disabled={busy.value}
              class="px-3 py-1.5 text-xs"
            >
              {busy.value ? "Reading sessions…" : "Choose session folder"}
            </LiquidMetalButton>
            <LiquidMetalButton
              variant="secondary"
              onClick$={() => pickCodingAndImport(false)}
              disabled={busy.value}
              class="px-3 py-1.5 text-xs"
            >
              Choose a single file
            </LiquidMetalButton>
          </div>
          <p class="mt-2 text-xs text-[var(--text-muted)]">
            Claude Code: pick your .claude/projects folder (or one project
            inside it). Aider: pick the .aider.chat.history.md file in your
            repo.
          </p>
        </div>
      )}

      {justImported.value && (
        <p class="mt-3 text-sm text-emerald-500">
          Imported {justImported.value.conversation_count} conversations (
          {justImported.value.message_count} messages) from{" "}
          {sourceName(justImported.value.source)}
          {dateRange(justImported.value) ? `, ${dateRange(justImported.value)}` : ""}.
        </p>
      )}
      {error.value && <p class="mt-3 text-sm text-red-400">{error.value}</p>}

      {distill.value.state === "running" && (
        <p class="mt-3 text-sm text-[var(--text-secondary)]">
          Learning from your history: {distill.value.processed} of{" "}
          {distill.value.total} messages… You can leave this page - it keeps
          going, and resumes next launch if you close the app.
        </p>
      )}
      {distill.value.state === "waiting-model" && (
        <p class="mt-3 text-sm text-[var(--text-secondary)]">
          Learning will start once the reasoning component finishes
          downloading (Settings → Components) - it resumes automatically.
        </p>
      )}
      {distill.value.state === "done" && (
        <p class="mt-3 text-sm text-emerald-500">
          Finished learning from your imported history - anything personal it
          found is now in the memory list above.
        </p>
      )}
      {distill.value.state === "error" && distill.value.error && (
        <p class="mt-3 text-sm text-red-400">{distill.value.error}</p>
      )}

      {archives.value.length > 0 && (
        <div class="mt-4 space-y-2">
          {archives.value.map((a) => (
            <div
              key={a.archive_id}
              class="group rounded-lg border border-[var(--border-subtle)] px-3 py-2 text-sm"
            >
              <div class="flex items-center justify-between">
                <span class="text-[var(--text-secondary)]">
                  <span class="font-semibold text-[var(--text-primary)]">
                    {sourceName(a.source)}
                  </span>{" "}
                  · {a.conversation_count} conversations
                  {dateRange(a) ? ` · ${dateRange(a)}` : ""}
                  {learnedIds.value.includes(a.archive_id) && (
                    <span class="ml-2 text-emerald-500">· learned</span>
                  )}
                </span>
                <div class="opacity-0 transition-opacity group-hover:opacity-100">
                  <LiquidMetalButton
                    variant="danger"
                    onClick$={() => remove(a.archive_id)}
                    title="Delete this imported archive"
                    class="p-1"
                  >
                    <LuTrash2 class="h-4 w-4" />
                  </LiquidMetalButton>
                </div>
              </div>

              {(a.adopted_by ?? []).length > 0 && (
                <p class="mt-1 text-xs text-[var(--text-muted)]">
                  In {a.adopted_by.map((x) => x.ai_name).join(", ")}
                  {"'"}s conversations.
                </p>
              )}

              {adoptBusyId.value === a.archive_id ? (
                <p class="mt-2 text-xs text-[var(--text-secondary)]">
                  Adding to conversations
                  {adoptProgress.value
                    ? `: ${adoptProgress.value.done} of ${adoptProgress.value.total}`
                    : "…"}
                  {" "}- original dates preserved.
                </p>
              ) : (
                aiOptions.value.length > 0 && (
                  <div class="mt-2 flex flex-wrap items-center gap-2">
                    {/* Custom dropdown - a native <select> popup is
                        GTK-themed on Linux/webkit and ignores our theme. */}
                    <div class="relative">
                      <button
                        type="button"
                        onClick$={() =>
                          (adoptMenuOpenId.value =
                            adoptMenuOpenId.value === a.archive_id
                              ? null
                              : a.archive_id)
                        }
                        class="flex items-center justify-between gap-2 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-main)] px-2.5 py-1 text-xs text-[var(--text-primary)] focus:outline-none focus:border-[var(--bg-button-primary)]"
                      >
                        <span>
                          {(() => {
                            const avail = aiOptions.value.filter(
                              (o) =>
                                !(a.adopted_by ?? []).some((x) => x.ai_id === o.id),
                            );
                            const lastAiId =
                              typeof localStorage !== "undefined"
                                ? localStorage.getItem("lastAiId")
                                : null;
                            const picked =
                              avail.find((o) => o.id === adoptPick.value[a.archive_id]) ??
                              avail.find((o) => o.id === lastAiId) ??
                              avail[0];
                            return picked?.name ?? "No AI available";
                          })()}
                        </span>
                        <LuChevronDown class="h-3.5 w-3.5 flex-shrink-0 text-[var(--text-muted)]" />
                      </button>
                      {adoptMenuOpenId.value === a.archive_id && (
                        <>
                          <div
                            class="fixed inset-0 z-40"
                            onClick$={() => (adoptMenuOpenId.value = null)}
                          />
                          <div class="absolute left-0 top-full z-50 mt-1 max-h-60 w-full min-w-[150px] overflow-auto rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-dropdown)] py-1 shadow-xl">
                            {aiOptions.value
                              .filter(
                                (o) =>
                                  !(a.adopted_by ?? []).some((x) => x.ai_id === o.id),
                              )
                              .map((o) => (
                                <button
                                  key={o.id}
                                  type="button"
                                  onClick$={() => {
                                    adoptPick.value = {
                                      ...adoptPick.value,
                                      [a.archive_id]: o.id,
                                    };
                                    adoptMenuOpenId.value = null;
                                  }}
                                  class={`block w-full px-3 py-1.5 text-left text-xs transition-colors hover:bg-[var(--bg-card)] ${
                                    o.id === adoptPick.value[a.archive_id]
                                      ? "font-medium text-[var(--text-primary)]"
                                      : "text-[var(--text-secondary)]"
                                  }`}
                                >
                                  {o.name}
                                </button>
                              ))}
                          </div>
                        </>
                      )}
                    </div>
                    <LiquidMetalButton
                      variant="secondary"
                      onClick$={() => adopt(a.archive_id)}
                      disabled={adoptBusyId.value !== null}
                      class="px-2.5 py-1 text-xs"
                    >
                      {(a.adopted_by ?? []).length > 0
                        ? "Add to another AI's conversations"
                        : "Add to this AI's conversations"}
                    </LiquidMetalButton>
                  </div>
                )
              )}
            </div>
          ))}
        </div>
      )}
    </div>
  );
});
