/**
 * "Bring your history" - import conversations exported from other AI apps
 * (ChatGPT, Claude, Perplexity) into an encrypted local archive.
 *
 * Stage 1 only: pick file -> parse -> encrypted archive -> instant summary.
 * Stage 2 (the background distiller that turns archives into memory) lands
 * next and will pick archives up from here.
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { LuDownload, LuTrash2 } from "@qwikest/icons/lucide";

interface ImportSummary {
  archive_id: string;
  source: string;
  file_name: string;
  conversation_count: number;
  message_count: number;
  earliest_us: number | null;
  latest_us: number | null;
  imported_at_us: number;
}

const SOURCE_NAMES: Record<string, string> = {
  chatgpt: "ChatGPT",
  claude: "Claude",
  perplexity: "Perplexity",
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
    cleanup(() => unsubscribe());
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
      busy.value = true;
      const { invoke } = await import("@tauri-apps/api/core");
      const summary = await invoke<ImportSummary>("import_conversations_scan", {
        path: selected,
      });
      justImported.value = summary;
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
        Bring your conversations from ChatGPT, Claude, or Perplexity - pick the
        export file (the .zip they email you, or the conversations .json inside
        it) and it becomes an encrypted archive on this machine. Nothing is
        uploaded anywhere.
      </p>
      <button
        onClick$={pickAndImport}
        disabled={busy.value}
        class="rounded-lg border border-[var(--border-subtle)] px-4 py-2 text-sm text-[var(--text-primary)] transition-colors hover:bg-[var(--bg-dropdown)] disabled:opacity-60"
      >
        {busy.value ? "Reading your export…" : "Choose export file"}
      </button>

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
              class="flex items-center justify-between rounded-lg border border-[var(--border-subtle)] px-3 py-2 text-sm"
            >
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
              <button
                onClick$={() => remove(a.archive_id)}
                title="Delete this imported archive"
                aria-label="Delete this imported archive"
                class="p-1 text-[var(--text-muted)] transition-colors hover:text-red-400"
              >
                <LuTrash2 class="h-4 w-4" />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
});
