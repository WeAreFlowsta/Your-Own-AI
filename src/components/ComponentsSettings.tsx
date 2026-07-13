/**
 * Settings → Components: on-demand capability models (kept out of the base
 * install so the app stays lean). Today: the memory-recall embedding model and
 * vision projectors (mmproj) that let a downloaded chat model see attached images.
 * Download here, remove here to reclaim space. Discovery also happens contextually
 * (a first-use prompt) elsewhere; this is the manage/remove home.
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { LuBrain, LuEye, LuFileText, LuSparkles, LuCheck, LuDownload, LuTrash2 } from "@qwikest/icons/lucide";
import { modelManager, type DownloadProgress } from "../utils/modelManager";
import LiquidMetalButton from "./LiquidMetalButton";
import {
  EMBEDDING_MODEL,
  UTILITY_MODEL,
  VISION_PROJECTORS,
  OCR_MODELS,
  modelFamilies,
  type CapabilityModel,
} from "../data/recommended-models";

interface CardProps {
  model: CapabilityModel;
  icon: "brain" | "eye" | "scan" | "spark";
  activeLabel: string;
  /** Tauri command to stop the model's server on removal, if it runs one. */
  stopCommand?: string;
}

/** A capability's files (a single one, or several for multi-file components). */
const filesOf = (m: CapabilityModel) =>
  m.files ?? [{ filename: m.filename, downloadUrl: m.downloadUrl }];

const sizeLabel = (gb: number) =>
  gb >= 1 ? `~${gb.toFixed(1)} GB` : `~${Math.round(gb * 1024)} MB`;

/** The chat model a vision projector pairs with (the conductor's filename-prefix
 *  rule). A projector is inert without it, so we gate its download on the model. */
const pairedModelFor = (m: CapabilityModel): { name: string; filename: string } | null => {
  if (m.role !== "vision-projector") return null;
  const key = m.filename.toLowerCase().split("-mmproj")[0];
  if (!key) return null;
  for (const f of modelFamilies) {
    for (const v of f.variants) {
      if (v.filename.toLowerCase().startsWith(key)) {
        return { name: `${f.name} ${v.parameterCount}`, filename: v.filename };
      }
    }
  }
  return null;
};

/** One downloadable capability component — its own download/remove state. */
const ComponentCard = component$<CardProps>((props) => {
  const downloaded = useSignal(false);
  const downloading = useSignal(false);
  const percent = useSignal(0);
  const error = useSignal("");
  const checking = useSignal(true);
  const modelInstalled = useSignal(true);
  // A vision projector is inert without its paired chat model — gate the download.
  const paired = pairedModelFor(props.model);

  const refresh = $(async () => {
    const present = await Promise.all(
      filesOf(props.model).map((f) => modelManager.isModelDownloaded(f.filename)),
    );
    downloaded.value = present.every(Boolean);
    if (paired) modelInstalled.value = await modelManager.isModelDownloaded(paired.filename);
    checking.value = false;
  });

  const doDownload = $(async () => {
    error.value = "";
    downloading.value = true;
    percent.value = 0;
    try {
      const files = filesOf(props.model);
      for (let i = 0; i < files.length; i++) {
        if (await modelManager.isModelDownloaded(files[i].filename)) continue; // resume-safe: skip done files
        await modelManager.downloadModel(
          files[i].downloadUrl,
          files[i].filename,
          // Smooth 0–100 across all files.
          (p) => (percent.value = Math.round(((i + p.percent / 100) / files.length) * 100)),
        );
      }
      await refresh();
      // A device restore may be waiting on this exact model to rebuild the
      // per-AI memory index - run the walker now instead of at next launch.
      if (props.model.filename === EMBEDDING_MODEL.filename) {
        import("../utils/transcriptMemory")
          .then(({ rebuildMemoryIndexIfPending }) => rebuildMemoryIndexIfPending())
          .catch(() => {});
      }
    } catch (e) {
      error.value = e instanceof Error ? e.message : "Download failed";
    } finally {
      downloading.value = false;
    }
  });

  const doRemove = $(async () => {
    error.value = "";
    try {
      for (const f of filesOf(props.model)) {
        await modelManager.deleteModel(f.filename);
      }
      if (props.stopCommand) {
        try {
          await invoke(props.stopCommand);
        } catch {
          /* not running */
        }
      }
      await refresh();
    } catch (e) {
      error.value = e instanceof Error ? e.message : "Couldn't remove";
    }
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    await refresh();
    if (downloaded.value) return;
    // Reattach to (or resume) a download already in progress — parity with the
    // offline-models page, so leaving Settings mid-download no longer appears to
    // stop it. The conductor keeps downloading; we just rejoin the progress.
    const files = filesOf(props.model);
    const statuses = await Promise.all(files.map((f) => modelManager.downloadStatus(f.filename)));
    const names = new Set(files.map((f) => f.filename));
    if (statuses.some((s) => s.downloading)) {
      downloading.value = true;
      const unp = await listen<DownloadProgress>("model-download-progress", (e) => {
        if (names.has(e.payload.filename)) percent.value = e.payload.percent;
      });
      const unc = await listen<{ filename: string }>("model-download-complete", (e) => {
        if (names.has(e.payload.filename)) {
          downloading.value = false;
          refresh();
        }
      });
      cleanup(() => { unp(); unc(); });
    } else if (statuses.some((s) => s.has_partial)) {
      doDownload(); // stopped mid-way (e.g. an app restart) — resume from the .part
    }
  });

  const blocked = !!paired && !modelInstalled.value && !downloaded.value;

  return (
    <div class="flex items-start gap-4 p-4 rounded-xl bg-[var(--bg-main)] border border-[var(--border-subtle)]">
      <div class="w-10 h-10 rounded-lg bg-[var(--bg-dropdown)] flex items-center justify-center flex-shrink-0">
        {props.icon === "eye" ? (
          <LuEye class="w-5 h-5 text-[var(--text-secondary)]" />
        ) : props.icon === "scan" ? (
          <LuFileText class="w-5 h-5 text-[var(--text-secondary)]" />
        ) : props.icon === "spark" ? (
          <LuSparkles class="w-5 h-5 text-[var(--text-secondary)]" />
        ) : (
          <LuBrain class="w-5 h-5 text-[var(--text-secondary)]" />
        )}
      </div>
      <div class="min-w-0 flex-1">
        <h3 class="text-base font-semibold text-[var(--text-primary)] flex items-center gap-2">
          {props.model.name}
          <span class="text-xs font-normal text-[var(--text-muted)]">
            {sizeLabel(props.model.size)}
          </span>
        </h3>
        <p class="text-sm text-[var(--text-secondary)] mt-0.5">
          {props.model.description}
        </p>
        {blocked && (
          <p class="mt-1.5 text-xs text-amber-500/90">
            Install {paired!.name} from Offline Models first — downloading the model brings this in automatically.
          </p>
        )}
        {downloading.value && (
          <div class="mt-2">
            <div class="h-1.5 w-full rounded-full bg-[var(--border-subtle)] overflow-hidden">
              <div
                class="h-full bg-[var(--bg-button-primary)] transition-all"
                style={{ width: `${percent.value}%` }}
              />
            </div>
            <p class="text-[11px] text-[var(--text-muted)] mt-1">
              Downloading… {percent.value}%
            </p>
          </div>
        )}
        {error.value && <p class="text-xs text-red-400 mt-1">{error.value}</p>}
        {downloaded.value && !downloading.value && (
          <p class="mt-2 text-[11px] text-emerald-400/80 flex items-center gap-1">
            <LuCheck class="w-3 h-3" /> {props.activeLabel}
          </p>
        )}
      </div>

      <div class="flex-shrink-0">
        {checking.value ? null : downloaded.value ? (
          <LiquidMetalButton
            variant="danger"
            onClick$={doRemove}
            class="flex items-center gap-1.5 px-3 py-1.5 text-xs"
          >
            <LuTrash2 class="w-3.5 h-3.5" /> Remove
          </LiquidMetalButton>
        ) : downloading.value ? (
          <span class="flex items-center gap-1.5 px-3 py-1.5 text-xs text-[var(--text-muted)]">
            {percent.value}%
          </span>
        ) : blocked ? (
          <span class="px-3 py-1.5 text-xs text-[var(--text-muted)] whitespace-nowrap">
            Needs the model
          </span>
        ) : (
          <LiquidMetalButton
            onClick$={doDownload}
            class="flex items-center gap-1.5 px-3 py-1.5 text-xs"
          >
            <LuDownload class="w-3.5 h-3.5" /> Download
          </LiquidMetalButton>
        )}
      </div>
    </div>
  );
});

export default component$(() => {
  return (
    <section class="bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]">
      <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-1">
        Components
      </h2>
      <p class="text-sm text-[var(--text-secondary)] mb-4">
        Optional add-ons that download only when you want them, so the app stays
        light. Remove any to reclaim space.
      </p>

      <div class="flex flex-col gap-3">
        <ComponentCard
          model={EMBEDDING_MODEL}
          icon="brain"
          activeLabel="Installed — memory and smart routing are active."
          stopCommand="stop_embedding_server"
        />
        <ComponentCard
          model={UTILITY_MODEL}
          icon="spark"
          activeLabel="Installed — extraction and smart modes run on your device, even with online AIs."
          stopCommand="stop_utility_server"
        />
        {VISION_PROJECTORS.map((projector) => (
          <ComponentCard
            key={projector.id}
            model={projector}
            icon="eye"
            activeLabel="Installed — this model can now see images you attach."
          />
        ))}
        <ComponentCard
          model={OCR_MODELS}
          icon="scan"
          activeLabel="Installed — scanned PDFs are read on your device."
        />
      </div>
    </section>
  );
});
