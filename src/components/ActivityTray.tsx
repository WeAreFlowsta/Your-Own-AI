/**
 * The activity tray: one bottom-right card for every background job, on
 * every page (mounted in the root layout).
 *
 * Rows come from four places and stack in the order they started:
 *  - downloads: models, the CUDA engine and the Projects helper all report
 *    through `model-download-progress` / `-complete` / `-failed`;
 *  - document reading: `corpus-progress`, with the pieces inside the
 *    current document and a Stop;
 *  - card writing after an import: documentSummaries' progress listener;
 *  - notes: `announceActivity(...)` from any feature (the first model's
 *    "is ready" line, a vision download's outcome).
 * The vision download keeps its own retry/dismiss through its context, so
 * its row is drawn from there and the matching download rows are hidden.
 *
 * Finished rows linger briefly, then clear; an error stays until dismissed.
 * Past three rows the oldest fold behind a "+N more" line, and the card
 * scrolls inside itself on a short window.
 */
import { component$, useStore, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { listen } from "@tauri-apps/api/event";
import { useVisionDownload } from "../contexts/VisionDownloadContext";
import { ACTIVITY_EVENT, labelForFile, formatBytes, type ActivityNote } from "../utils/activity";
import { firstModelInFlight } from "../utils/firstModel";
import type { CorpusProgress } from "../utils/corpus";
import type { DownloadProgress, DownloadComplete } from "../utils/modelManager";

interface Row {
  id: string;
  kind: "download" | "read" | "cards" | "note";
  title: string;
  detail: string;
  percent: number | null;
  state: "running" | "done" | "error";
  stoppable: boolean;
  startedAt: number;
}

const LINGER_MS = 8000;
const SHOWN = 3;

export const ActivityTray = component$(() => {
  const store = useStore<{ rows: Row[] }>({ rows: [] });
  const expanded = useSignal(false);
  const stopping = useSignal(false);
  const vision = useVisionDownload();

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    const timers = new Map<string, ReturnType<typeof setTimeout>>();
    const upsert = (id: string, patch: Partial<Row> & Pick<Row, "kind" | "title">) => {
      const i = store.rows.findIndex((r) => r.id === id);
      if (i >= 0) {
        store.rows[i] = { ...store.rows[i], ...patch };
      } else {
        store.rows = [
          ...store.rows,
          { id, detail: "", percent: null, state: "running", stoppable: false, startedAt: Date.now(), ...patch },
        ];
      }
    };
    const remove = (id: string) => {
      store.rows = store.rows.filter((r) => r.id !== id);
      const t = timers.get(id);
      if (t) clearTimeout(t);
      timers.delete(id);
    };
    const linger = (id: string, ms: number) => {
      const t = timers.get(id);
      if (t) clearTimeout(t);
      if (ms > 0) timers.set(id, setTimeout(() => remove(id), ms));
    };

    const first = () => firstModelInFlight();
    const downloadTitle = (filename: string) => {
      const f = first();
      return f && f.filename === filename ? `Your first model: ${f.label}` : labelForFile(filename);
    };

    const unProgress = await listen<DownloadProgress>("model-download-progress", (e) => {
      const p = e.payload;
      const f = first();
      const isFirst = !!f && f.filename === p.filename;
      upsert(`dl:${p.filename}`, {
        kind: "download",
        title: downloadTitle(p.filename),
        percent: p.percent,
        state: "running",
        detail:
          (p.total ? `${formatBytes(p.downloaded)} of ${formatBytes(p.total)}` : "Starting...") +
          (isFirst ? " · your AIs answer the moment it lands" : ""),
      });
    });
    const unComplete = await listen<DownloadComplete>("model-download-complete", (e) => {
      const id = `dl:${e.payload.filename}`;
      const f = first();
      // The first model's own finisher announces "is ready" once the model
      // is loaded and assigned; its download row simply goes.
      if (f && f.filename === e.payload.filename) {
        remove(id);
        return;
      }
      upsert(id, { kind: "download", title: downloadTitle(e.payload.filename), percent: 100, state: "done", detail: "Downloaded" });
      linger(id, LINGER_MS);
    });
    const unFailed = await listen<{ filename: string; error: string }>("model-download-failed", (e) => {
      const id = `dl:${e.payload.filename}`;
      upsert(id, { kind: "download", title: downloadTitle(e.payload.filename), state: "error", detail: e.payload.error });
    });

    const unCorpus = await listen<CorpusProgress>("corpus-progress", (e) => {
      const p = e.payload;
      if (p.phase === "done") {
        if (p.added > 0 || p.failed > 0) {
          const parts: string[] = [];
          if (p.added > 0) parts.push(`${p.added} document${p.added === 1 ? "" : "s"} in`);
          if (p.failed > 0) parts.push(`${p.failed} couldn't be read`);
          upsert("read", { kind: "read", title: "Reading documents", state: "done", percent: 100, stoppable: false, detail: parts.join(" · ") });
          linger("read", LINGER_MS);
        } else {
          remove("read");
        }
        return;
      }
      const where = p.total > 1 ? `${p.done + 1} of ${p.total}: ` : "";
      const pieces =
        p.phase === "embedding" && p.pieces_total
          ? ` · ${(p.pieces_done ?? 0).toLocaleString()} of ${p.pieces_total.toLocaleString()} pieces`
          : "";
      const percent =
        p.phase === "embedding" && p.pieces_total
          ? Math.round((((p.done + (p.pieces_done ?? 0) / p.pieces_total) / Math.max(1, p.total)) * 100))
          : p.total > 0
            ? Math.round((p.done / p.total) * 100)
            : null;
      upsert("read", {
        kind: "read",
        title: "Reading documents",
        state: "running",
        stoppable: true,
        percent,
        detail: `${p.phase === "reading" ? "Reading" : "Remembering"} ${where}${p.file}${pieces}`,
      });
    });

    const { onCardProgress } = await import("../utils/documentSummaries");
    const unCards = onCardProgress((done, total, file) => {
      if (done >= total) {
        upsert("cards", { kind: "cards", title: "Writing document cards", state: "done", percent: 100, detail: `${total} card${total === 1 ? "" : "s"} written` });
        linger("cards", LINGER_MS);
        return;
      }
      upsert("cards", {
        kind: "cards",
        title: "Writing document cards",
        state: "running",
        percent: total ? Math.round((done / total) * 100) : null,
        detail: `${done + 1} of ${total}: ${file}`,
      });
    });

    const onNote = (ev: Event) => {
      const n = (ev as CustomEvent<ActivityNote>).detail;
      if (!n?.id) return;
      upsert(`note:${n.id}`, { kind: "note", title: n.title, detail: n.detail ?? "", state: n.state, percent: null });
      linger(`note:${n.id}`, n.ttlMs ?? 0);
    };
    window.addEventListener(ACTIVITY_EVENT, onNote);

    cleanup(() => {
      unProgress();
      unComplete();
      unFailed();
      unCorpus();
      unCards();
      window.removeEventListener(ACTIVITY_EVENT, onNote);
      for (const t of timers.values()) clearTimeout(t);
    });
  });

  const dismiss = $((id: string) => {
    store.rows = store.rows.filter((r) => r.id !== id);
  });

  const stopReading = $(async () => {
    stopping.value = true;
    try {
      const { corpusCancel } = await import("../utils/corpus");
      await corpusCancel();
    } finally {
      stopping.value = false;
    }
  });

  // The vision download draws from its context (it owns retry and dismiss);
  // the raw download rows for its files are hidden so it shows once.
  const vis = vision.state.active;
  const visionFiles = new Set(vis ? vis.files.map((f) => f.filename) : []);
  const rows = store.rows.filter((r) => !(r.kind === "download" && visionFiles.has(r.id.slice(3))));
  const total = rows.length + (vis ? 1 : 0);
  if (total === 0) return null;

  const hidden = !expanded.value && rows.length > SHOWN ? rows.length - SHOWN : 0;
  const shown = hidden ? rows.slice(rows.length - SHOWN) : rows;
  const visFile = vis ? vis.files[vis.currentIndex] : null;

  return (
    <div
      class="fixed bottom-4 right-4 z-[60] w-80 max-w-[calc(100vw-2rem)] max-h-[calc(100vh-5rem)] overflow-y-auto rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] shadow-lg p-3 space-y-3"
      role="status"
      aria-live="polite"
    >
      {hidden > 0 && (
        <button
          type="button"
          class="text-xs text-[var(--text-link)] hover:underline"
          onClick$={() => (expanded.value = true)}
        >
          +{hidden} more
        </button>
      )}

      {vis && (
        <div>
          {vis.status === "downloading" && (
            <>
              <div class="flex items-center gap-2 mb-1.5">
                <span class="inline-block w-2 h-2 rounded-full bg-[var(--bg-button-primary)] animate-pulse" />
                <span class="text-sm font-medium text-[var(--text-primary)]">Downloading vision model</span>
              </div>
              <p class="text-xs text-[var(--text-muted)] mb-1.5 truncate">
                {visFile?.label}
                {vis.files.length > 1 ? ` (${vis.currentIndex + 1} of ${vis.files.length})` : ""}
              </p>
              <div class="w-full h-2 rounded-full bg-[var(--bg-main)] overflow-hidden">
                <div class="h-full bg-[var(--bg-button-primary)] transition-all duration-200" style={{ width: `${vis.percent}%` }} />
              </div>
              <p class="text-xs text-[var(--text-muted)] mt-1">
                {vis.percent}%{vis.total ? ` · ${formatBytes(vis.downloaded)} of ${formatBytes(vis.total)}` : ""}
              </p>
            </>
          )}
          {vis.status === "done" && (
            <div class="flex items-center justify-between gap-2">
              <span class="text-sm text-[var(--text-primary)]">Vision model ready</span>
              <button type="button" onClick$={vision.dismiss$} class="text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)]">
                Dismiss
              </button>
            </div>
          )}
          {vis.status === "error" && (
            <>
              <p class="text-sm text-red-400 mb-1">Vision download failed</p>
              <p class="text-xs text-[var(--text-muted)] mb-2 break-words">{vis.error}</p>
              <div class="flex items-center gap-3">
                <button type="button" onClick$={vision.retry$} class="text-sm text-[var(--text-link)] hover:underline">
                  Try again
                </button>
                <button type="button" onClick$={vision.dismiss$} class="text-sm text-[var(--text-muted)] hover:text-[var(--text-secondary)]">
                  Dismiss
                </button>
              </div>
            </>
          )}
        </div>
      )}

      {shown.map((r) => (
        <div key={r.id}>
          <div class="flex items-start justify-between gap-2">
            <div class="min-w-0 flex items-center gap-2">
              {r.state === "running" && (
                <span class="inline-block w-2 h-2 flex-shrink-0 rounded-full bg-[var(--bg-button-primary)] animate-pulse" />
              )}
              <span class={`text-sm font-medium truncate ${r.state === "error" ? "text-red-400" : "text-[var(--text-primary)]"}`}>
                {r.title}
              </span>
            </div>
            {r.state === "running" && r.stoppable && (
              <button
                type="button"
                disabled={stopping.value}
                onClick$={stopReading}
                class="flex-shrink-0 text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)] disabled:opacity-60"
              >
                {stopping.value ? "Stopping..." : "Stop"}
              </button>
            )}
            {r.state !== "running" && (
              <button
                type="button"
                onClick$={() => dismiss(r.id)}
                class="flex-shrink-0 text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)]"
              >
                Dismiss
              </button>
            )}
          </div>
          {r.detail && <p class="text-xs text-[var(--text-muted)] mt-0.5 break-words">{r.detail}</p>}
          {r.state === "running" && r.percent !== null && (
            <div class="w-full h-2 mt-1.5 rounded-full bg-[var(--bg-main)] overflow-hidden">
              <div class="h-full bg-[var(--bg-button-primary)] transition-all duration-200" style={{ width: `${r.percent}%` }} />
            </div>
          )}
          {r.kind === "read" && r.state === "running" && (
            <p class="text-[11px] text-[var(--text-muted)] mt-1">
              Keeps reading while the app is open. Your AIs draw on each document as soon as it is in.
            </p>
          )}
        </div>
      ))}
    </div>
  );
});
