/**
 * Settings → Engines: the inference engines that run your models. The bundled
 * engine (Vulkan on Linux/Windows, Metal on Apple Silicon) ships with the app
 * and runs on any GPU; optional engines that only help specific hardware - the
 * CUDA engine for NVIDIA GPUs first - download on demand and can be removed.
 * The CUDA card only appears on machines with an NVIDIA GPU (or with the
 * engine already installed, so it can always be removed).
 */
import { component$, useSignal, useVisibleTask$, $ } from "@builder.io/qwik";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { LuCheck, LuCpu, LuDownload, LuLink, LuTrash2, LuZap } from "@qwikest/icons/lucide";
import LiquidMetalButton from "./LiquidMetalButton";
import { SAMPLING_BOUNDS, SAMPLING_DEFAULTS, globalSampling, setGlobalSampling, type SamplingOverrides } from "../utils/sampling";
import TuneSlider from "./TuneSlider";

interface ExternalEngineInfo {
  url: string | null;
  healthy: boolean;
  models: string[];
  models_info: { id: string; overall: number | null }[];
  /** Measured at connect time with a one-shot mini-generation. */
  tps: number | null;
  error: string | null;
}

interface EngineStatus {
  supported: boolean;
  /** This machine's NVIDIA GPU generation can execute the CUDA build
   *  (true when unknown - only a positive too-old reading gates). */
  gpu_supported: boolean;
  installed: boolean;
  stale_version_installed: boolean;
  /** What the next chat-server start will use. */
  active_backend: "bundled" | "cuda";
  /** What the RUNNING chat server was spawned with, if one is running. */
  running_backend: "bundled" | "cuda" | null;
  tag: string;
  download_url: string | null;
}

/** The zip filename the Rust download emits progress events under. */
const zipNameFor = (tag: string) =>
  `llama-server-cuda-${tag.replace(/^llama-/, "")}.zip`;

interface MlxEngineStatus {
  supported: boolean;
  installed: boolean;
  stale_version_installed: boolean;
  tag: string;
  download_url: string;
  download_mb: number;
}

/** The tarball filename the MLX engine download emits progress under. */
const mlxTarballFor = (tag: string) => `SwiftLM-${tag}-macos-arm64.tar.gz`;

export default component$(() => {
  const status = useSignal<EngineStatus | null>(null);
  const isNvidia = useSignal(false);
  const cudaLaddered = useSignal(false);
  const downloading = useSignal(false);
  const percent = useSignal(0);
  const error = useSignal("");

  const refresh = $(async () => {
    try {
      status.value = await invoke<EngineStatus>("engine_status");
    } catch (e) {
      console.warn("[Engines] status failed:", e);
    }
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    await refresh();
    try {
      const info = await invoke<{ gpu_name?: string | null }>("get_system_info");
      isNvidia.value = (info.gpu_name ?? "").toLowerCase().includes("nvidia");
    } catch {
      isNvidia.value = false;
    }
    try {
      const safe = await invoke<{ cuda_disabled?: boolean }>("gpu_safe_mode_status");
      cudaLaddered.value = !!safe.cuda_disabled;
    } catch {
      /* no safety state yet */
    }
    // Live progress for the engine download (same event stream as models).
    const zip = status.value ? zipNameFor(status.value.tag) : null;
    // A download started elsewhere (the home offer card, an earlier visit)
    // is still running in the app - reattach to it.
    if (zip) {
      try {
        const st = await invoke<{ downloading: boolean; downloaded_bytes: number; total_bytes: number }>(
          "download_status",
          { filename: zip },
        );
        if (st.downloading) {
          downloading.value = true;
          percent.value = st.total_bytes > 0 ? Math.floor((st.downloaded_bytes / st.total_bytes) * 100) : 0;
        }
      } catch {
        /* no status = nothing running */
      }
    }
    const unp = await listen<{ filename: string; percent: number }>(
      "model-download-progress",
      (e) => {
        if (zip && e.payload.filename === zip) {
          downloading.value = true;
          percent.value = e.payload.percent;
        }
      },
    );
    const unDone = await listen("engine-installed", async () => {
      downloading.value = false;
      await refresh();
    });
    const unFail = await listen<string>("engine-install-failed", (e) => {
      downloading.value = false;
      error.value = e.payload;
    });
    cleanup(() => {
      unp();
      unDone();
      unFail();
    });
  });

  const doDownload = $(async () => {
    error.value = "";
    downloading.value = true;
    percent.value = 0;
    try {
      await invoke("download_cuda_engine", {});
      await refresh();
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    } finally {
      downloading.value = false;
    }
  });

  const doRemove = $(async () => {
    error.value = "";
    try {
      await invoke("remove_cuda_engine");
      await refresh();
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    }
  });

  // MLX engine (preview) - Apple Silicon only; the card renders nowhere else.
  const mlxStatus = useSignal<MlxEngineStatus | null>(null);
  const mlxDownloading = useSignal(false);
  const mlxPercent = useSignal(0);
  const mlxError = useSignal("");
  const refreshMlx = $(async () => {
    try {
      mlxStatus.value = await invoke<MlxEngineStatus>("mlx_engine_status");
    } catch {
      /* command missing = old backend; card stays hidden */
    }
  });
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    await refreshMlx();
    if (!mlxStatus.value?.supported) return;
    const tarball = mlxTarballFor(mlxStatus.value.tag);
    const unp = await listen<{ filename: string; percent: number }>(
      "model-download-progress",
      (e) => {
        if (e.payload.filename === tarball) {
          mlxDownloading.value = true;
          mlxPercent.value = e.payload.percent;
        }
      },
    );
    cleanup(() => unp());
  });
  const doMlxDownload = $(async () => {
    mlxError.value = "";
    mlxDownloading.value = true;
    mlxPercent.value = 0;
    try {
      await invoke("download_mlx_engine", {});
      await refreshMlx();
    } catch (e) {
      mlxError.value = e instanceof Error ? e.message : String(e);
    } finally {
      mlxDownloading.value = false;
    }
  });
  const doMlxRemove = $(async () => {
    mlxError.value = "";
    try {
      await invoke("remove_mlx_engine");
      await refreshMlx();
    } catch (e) {
      mlxError.value = e instanceof Error ? e.message : String(e);
    }
  });

  const retryCuda = $(async () => {
    try {
      await invoke("gpu_retry");
      cudaLaddered.value = false;
      await refresh();
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    }
  });

  // External engine (connect-mode): the user's own OpenAI-compatible server.
  const external = useSignal<ExternalEngineInfo | null>(null);
  const externalUrl = useSignal("");
  const externalBusy = useSignal(false);
  const externalError = useSignal("");

  const refreshExternal = $(async () => {
    try {
      external.value = await invoke<ExternalEngineInfo>("external_engine_info");
    } catch {
      /* command unavailable */
    }
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(() => {
    refreshExternal();
  });

  const connectExternal = $(async () => {
    externalError.value = "";
    externalBusy.value = true;
    try {
      external.value = await invoke<ExternalEngineInfo>("set_external_engine", {
        url: externalUrl.value,
      });
      externalUrl.value = "";
    } catch (e) {
      externalError.value = e instanceof Error ? e.message : String(e);
    } finally {
      externalBusy.value = false;
    }
  });

  const disconnectExternal = $(async () => {
    externalError.value = "";
    try {
      await invoke("remove_external_engine");
      external.value = {
        url: null, healthy: false, models: [], models_info: [], tps: null, error: null,
      };
    } catch (e) {
      externalError.value = e instanceof Error ? e.message : String(e);
    }
  });

  const s = status.value;
  // The CUDA card earns its place only on NVIDIA machines - except when the
  // engine is already installed (always removable) or was laddered out.
  const showCuda =
    !!s && s.supported && (isNvidia.value || s.installed || s.stale_version_installed);

  // Machine fine-tune (FINE_TUNE_PANEL): worker threads + the global
  // generation layer every AI inherits unless it sets its own.
  const threads = useSignal<number | null>(null);
  const maxThreads = useSignal(32);
  const gTemp = useSignal<number | null>(null);
  const gTopP = useSignal<number | null>(null);
  const gMinP = useSignal<number | null>(null);
  const gRepeat = useSignal<number | null>(null);
  const tuneNote = useSignal("");
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    const g = globalSampling();
    gTemp.value = g.temperature ?? null;
    gTopP.value = g.topP ?? null;
    gMinP.value = g.minP ?? null;
    gRepeat.value = g.repeatPenalty ?? null;
    maxThreads.value = Math.max(2, navigator.hardwareConcurrency || 32);
    try {
      const { load } = await import("@tauri-apps/plugin-store");
      const store = await load("settings.json");
      const t = await store.get<number>("engineThreads");
      threads.value = t || null;
    } catch { /* fresh */ }
  });
  // Pages apply instantly (modals are where Save lives): every slider
  // release persists after a beat, and Auto is the undo.
  const saveTimer = useSignal(0);
  const saveTune = $(async () => {
    tuneNote.value = "";
    const g: SamplingOverrides = {};
    if (gTemp.value != null) g.temperature = gTemp.value;
    if (gTopP.value != null) g.topP = gTopP.value;
    if (gMinP.value != null) g.minP = gMinP.value;
    if (gRepeat.value != null) g.repeatPenalty = gRepeat.value;
    setGlobalSampling(g);
    try {
      await invoke("tuning_set_engine_threads", {
        threads: threads.value != null && threads.value >= 1 ? Math.round(threads.value) : null,
      });
      tuneNote.value = "Saved - generation settings reach new replies now; threads at the next model load.";
    } catch (e) {
      tuneNote.value = `Could not save: ${e}`;
    }
  });
  const saveSoon = $(() => {
    clearTimeout(saveTimer.value);
    saveTimer.value = window.setTimeout(() => saveTune(), 400);
  });

  return (
    <section class="bg-[var(--bg-card)] rounded-2xl p-6 border border-[var(--border-subtle)]">
      <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-1">
        Engines
      </h2>
      <p class="text-sm text-[var(--text-secondary)] mb-4">
        The engine runs your offline models. The standard one works on any
        graphics card; optional engines get more out of specific hardware.
      </p>

      <div class="flex flex-col gap-3">
        {/* Bundled engine - always present, never removable. */}
        <div class="flex items-start gap-4 p-4 rounded-xl bg-[var(--bg-main)] border border-[var(--border-subtle)]">
          <div class="w-10 h-10 rounded-lg bg-[var(--bg-dropdown)] flex items-center justify-center flex-shrink-0">
            <LuCpu class="w-5 h-5 text-[var(--text-secondary)]" />
          </div>
          <div class="min-w-0 flex-1">
            <h3 class="text-base font-semibold text-[var(--text-primary)]">
              Standard engine
            </h3>
            <p class="text-sm text-[var(--text-secondary)] mt-0.5">
              Ships with the app. Runs on any graphics card (NVIDIA, AMD,
              Intel, Apple) and falls back to your processor when needed.
            </p>
            {s?.active_backend === "bundled" && (
              <p class="mt-2 text-[11px] text-emerald-400/80 flex items-center gap-1">
                <LuCheck class="w-3 h-3" /> Active
              </p>
            )}
          </div>
        </div>

        {/* CUDA engine - NVIDIA machines only. */}
        {showCuda && (
          <div class="flex items-start gap-4 p-4 rounded-xl bg-[var(--bg-main)] border border-[var(--border-subtle)]">
            <div class="w-10 h-10 rounded-lg bg-[var(--bg-dropdown)] flex items-center justify-center flex-shrink-0">
              <LuZap class="w-5 h-5 text-[var(--text-secondary)]" />
            </div>
            <div class="min-w-0 flex-1">
              <h3 class="text-base font-semibold text-[var(--text-primary)] flex items-center gap-2">
                CUDA engine
                <span class="text-xs font-normal text-[var(--text-muted)]">~850 MB</span>
              </h3>
              <p class="text-sm text-[var(--text-secondary)] mt-0.5">
                Built for NVIDIA graphics cards. Reads long prompts and
                documents much faster; generation speed varies by card
                generation. Needs an NVIDIA driver from 2023 or newer.
              </p>
              {!s.gpu_supported && (
                <p class="mt-1.5 text-xs text-amber-500/90">
                  Your graphics card's generation isn't supported by this
                  engine - the standard engine is used instead. Everything
                  still works.
                </p>
              )}
              {s.gpu_supported && s.stale_version_installed && !s.installed && (
                <p class="mt-1.5 text-xs text-amber-500/90">
                  Update available - the app was updated and needs a matching
                  engine. Download to update.
                </p>
              )}
              {cudaLaddered.value && (
                <p class="mt-1.5 text-xs text-amber-500/90">
                  Turned off after repeated crashes - your chats use the
                  standard engine.{" "}
                  <button
                    class="underline hover:text-[var(--text-primary)]"
                    onClick$={retryCuda}
                  >
                    Try CUDA again
                  </button>
                </p>
              )}
              {downloading.value && (
                <div class="mt-2">
                  <div class="h-1.5 w-full rounded-full bg-[var(--border-subtle)] overflow-hidden">
                    <div
                      class={`h-full bg-[var(--bg-button-primary)] transition-all ${percent.value >= 100 ? "animate-pulse" : ""}`}
                      style={{ width: `${percent.value}%` }}
                    />
                  </div>
                  <p class="text-[11px] text-[var(--text-muted)] mt-1">
                    {percent.value >= 100 ? "Installing…" : `Downloading… ${percent.value}%`}
                  </p>
                </div>
              )}
              {error.value && <p class="text-xs text-red-400 mt-1">{error.value}</p>}
              {s.installed && !downloading.value && !cudaLaddered.value && (
                <p class="mt-2 text-[11px] text-emerald-400/80 flex items-center gap-1">
                  <LuCheck class="w-3 h-3" />
                  {s.running_backend === "cuda"
                    ? "Active - powering your chats."
                    : s.active_backend === "cuda"
                      ? "Installed - in use from now on."
                      : "Installed."}
                </p>
              )}
            </div>

            <div class="flex-shrink-0">
              {s.installed && !downloading.value ? (
                <LiquidMetalButton
                  variant="danger"
                  onClick$={doRemove}
                  class="flex items-center gap-1.5 px-3 py-1.5 text-xs"
                >
                  <LuTrash2 class="w-3.5 h-3.5" /> Remove
                </LiquidMetalButton>
              ) : downloading.value ? (
                <span class="flex items-center gap-1.5 px-3 py-1.5 text-xs text-[var(--text-muted)]">
                  {percent.value >= 100 ? "Installing…" : `${percent.value}%`}
                </span>
              ) : !s.gpu_supported ? (
                // No Download and no Update for a card that cannot execute
                // this engine - offering either recreates the crash loop.
                <span class="px-3 py-1.5 text-xs text-[var(--text-muted)] whitespace-nowrap">
                  Not for this card
                </span>
              ) : (
                <LiquidMetalButton
                  onClick$={doDownload}
                  class="flex items-center gap-1.5 px-3 py-1.5 text-xs"
                >
                  <LuDownload class="w-3.5 h-3.5" />
                  {s.stale_version_installed ? "Update" : "Download"}
                </LiquidMetalButton>
              )}
            </div>
          </div>
        )}

        {/* MLX engine (preview) - Apple Silicon Macs only. */}
        {mlxStatus.value?.supported && (
          <div class="flex items-start gap-4 p-4 rounded-xl bg-[var(--bg-main)] border border-[var(--border-subtle)]">
            <div class="w-10 h-10 rounded-lg bg-[var(--bg-dropdown)] flex items-center justify-center flex-shrink-0">
              <LuZap class="w-5 h-5 text-[var(--text-secondary)]" />
            </div>
            <div class="min-w-0 flex-1">
              <h3 class="text-base font-semibold text-[var(--text-primary)] flex items-center gap-2">
                Apple Silicon engine (MLX)
                <span class="text-[10px] font-normal uppercase tracking-wide px-1.5 py-0.5 rounded bg-[var(--bg-dropdown)] text-[var(--text-muted)]">
                  preview
                </span>
                <span class="text-xs font-normal text-[var(--text-muted)]">
                  ~{mlxStatus.value.download_mb} MB
                </span>
              </h3>
              <p class="text-sm text-[var(--text-secondary)] mt-0.5">
                Runs models built for Apple's MLX framework. Models offer
                an MLX version where one exists; everything else keeps the
                standard engine. In this preview it serves chats - projects,
                vision and memory stay on the standard engine. Whether it is
                faster depends on your Mac; nothing changes unless you
                install it.
              </p>
              {mlxStatus.value.stale_version_installed && !mlxStatus.value.installed && (
                <p class="mt-1.5 text-xs text-amber-500/90">
                  Update available - the app was updated and needs a matching
                  engine. Download to update.
                </p>
              )}
              {mlxDownloading.value && (
                <div class="mt-2">
                  <div class="h-1.5 w-full rounded-full bg-[var(--border-subtle)] overflow-hidden">
                    <div
                      class={`h-full bg-[var(--bg-button-primary)] transition-all ${mlxPercent.value >= 100 ? "animate-pulse" : ""}`}
                      style={{ width: `${mlxPercent.value}%` }}
                    />
                  </div>
                  <p class="text-[11px] text-[var(--text-muted)] mt-1">
                    {mlxPercent.value >= 100 ? "Installing…" : `Downloading… ${mlxPercent.value}%`}
                  </p>
                </div>
              )}
              {mlxError.value && <p class="text-xs text-red-400 mt-1">{mlxError.value}</p>}
              {mlxStatus.value.installed && !mlxDownloading.value && (
                <p class="mt-2 text-[11px] text-emerald-400/80 flex items-center gap-1">
                  <LuCheck class="w-3 h-3" /> Installed - models with an MLX
                  version use it for chats.
                </p>
              )}
            </div>
            <div class="flex-shrink-0">
              {mlxStatus.value.installed && !mlxDownloading.value ? (
                <LiquidMetalButton
                  variant="danger"
                  onClick$={doMlxRemove}
                  class="flex items-center gap-1.5 px-3 py-1.5 text-xs"
                >
                  <LuTrash2 class="w-3.5 h-3.5" /> Remove
                </LiquidMetalButton>
              ) : mlxDownloading.value ? (
                <span class="flex items-center gap-1.5 px-3 py-1.5 text-xs text-[var(--text-muted)]">
                  {mlxPercent.value >= 100 ? "Installing…" : `${mlxPercent.value}%`}
                </span>
              ) : (
                <LiquidMetalButton
                  onClick$={doMlxDownload}
                  class="flex items-center gap-1.5 px-3 py-1.5 text-xs"
                >
                  <LuDownload class="w-3.5 h-3.5" />
                  {mlxStatus.value.stale_version_installed ? "Update" : "Download"}
                </LiquidMetalButton>
              )}
            </div>
          </div>
        )}

        {/* External engine - the user's own OpenAI-compatible server. */}
        <div class="flex items-start gap-4 p-4 rounded-xl bg-[var(--bg-main)] border border-[var(--border-subtle)]">
          <div class="w-10 h-10 rounded-lg bg-[var(--bg-dropdown)] flex items-center justify-center flex-shrink-0">
            <LuLink class="w-5 h-5 text-[var(--text-secondary)]" />
          </div>
          <div class="min-w-0 flex-1">
            <h3 class="text-base font-semibold text-[var(--text-primary)]">
              Your own server
            </h3>
            <p class="text-sm text-[var(--text-secondary)] mt-0.5">
              Connect any OpenAI-compatible server you run - another machine
              with llama.cpp, a vLLM box, a Mac cluster. Its models appear in
              the AI model picker; your conversations go only to it.
            </p>
            {external.value?.url ? (
              <div class="mt-2">
                <p class="text-[11px] flex items-center gap-1.5">
                  <span
                    class={`inline-block w-2 h-2 rounded-full ${
                      external.value.healthy ? "bg-green-500" : "bg-red-500"
                    }`}
                  />
                  <span class="text-[var(--text-secondary)] font-mono">
                    {external.value.url}
                  </span>
                  <span class="text-[var(--text-muted)]">
                    {external.value.healthy
                      ? `- ${external.value.models.length} model${external.value.models.length === 1 ? "" : "s"}${
                          external.value.tps ? ` · ~${Math.round(external.value.tps)} tok/s measured` : ""
                        }`
                      : `- unreachable${external.value.error ? ` (${external.value.error})` : ""}`}
                  </span>
                </p>
              </div>
            ) : (
              <div class="mt-2 flex gap-2">
                <input
                  type="text"
                  placeholder="http://192.168.1.20:8000"
                  value={externalUrl.value}
                  disabled={externalBusy.value}
                  onInput$={(_, el) => (externalUrl.value = el.value)}
                  class="flex-1 min-w-0 rounded-full bg-[var(--bg-input)] px-4 py-1.5 text-sm text-[var(--text-primary)] placeholder-[var(--text-muted)] border border-[var(--border-subtle)] focus:outline-none"
                />
                <LiquidMetalButton
                  onClick$={connectExternal}
                  class="px-3 py-1.5 text-xs whitespace-nowrap"
                >
                  {externalBusy.value ? "Checking…" : "Connect"}
                </LiquidMetalButton>
              </div>
            )}
            {externalError.value && (
              <p class="text-xs text-red-400 mt-1">{externalError.value}</p>
            )}
          </div>
          {external.value?.url && (
            <div class="flex-shrink-0">
              <LiquidMetalButton
                variant="danger"
                onClick$={disconnectExternal}
                class="flex items-center gap-1.5 px-3 py-1.5 text-xs"
              >
                <LuTrash2 class="w-3.5 h-3.5" /> Disconnect
              </LiquidMetalButton>
            </div>
          )}
        </div>
      </div>

      {/* Machine fine-tune: this computer's overrides. Empty = automatic. */}
      <div class="mt-6 pt-5 border-t border-[var(--border-subtle)]">
        <h3 class="font-semibold text-[var(--text-primary)] mb-1">Fine-tune this computer</h3>
        <p class="text-sm text-[var(--text-secondary)] mb-3">
          For people who like to turn the dials. Empty fields mean the automatics decide (the value in
          the box). Generation settings here are the layer every AI inherits unless it sets its own in
          its form; each model also has its own Fine-tune on the Offline Models page.
        </p>
        <div class="grid gap-4 sm:grid-cols-2">
          <div class="sm:col-span-2">
            <TuneSlider label="Worker threads (next model load)" value={threads.value}
              autoLabel="Auto (engine default)" autoValue={Math.round(maxThreads.value / 2)}
              min={1} max={maxThreads.value} step={1} unit="threads"
              onChange$={(v) => { threads.value = v; saveSoon(); }} />
          </div>
          <TuneSlider label="Creativity (temperature)" value={gTemp.value}
            autoLabel={`Model default (${SAMPLING_DEFAULTS.temperature})`} autoValue={SAMPLING_DEFAULTS.temperature}
            min={SAMPLING_BOUNDS.temperature.min} max={SAMPLING_BOUNDS.temperature.max} step={SAMPLING_BOUNDS.temperature.step}
            onChange$={(v) => { gTemp.value = v; saveSoon(); }} />
          <TuneSlider label="Word variety (top-p)" value={gTopP.value}
            autoLabel={`Model default (${SAMPLING_DEFAULTS.topP})`} autoValue={SAMPLING_DEFAULTS.topP}
            min={SAMPLING_BOUNDS.topP.min} max={SAMPLING_BOUNDS.topP.max} step={SAMPLING_BOUNDS.topP.step}
            onChange$={(v) => { gTopP.value = v; saveSoon(); }} />
          <TuneSlider label="Rare-word floor (min-p)" value={gMinP.value}
            autoLabel={`Model default (${SAMPLING_DEFAULTS.minP})`} autoValue={SAMPLING_DEFAULTS.minP}
            min={SAMPLING_BOUNDS.minP.min} max={SAMPLING_BOUNDS.minP.max} step={SAMPLING_BOUNDS.minP.step}
            onChange$={(v) => { gMinP.value = v; saveSoon(); }} />
          <TuneSlider label="Repetition brake (repeat penalty)" value={gRepeat.value}
            autoLabel={`Model default (${SAMPLING_DEFAULTS.repeatPenalty})`} autoValue={SAMPLING_DEFAULTS.repeatPenalty}
            min={SAMPLING_BOUNDS.repeatPenalty.min} max={SAMPLING_BOUNDS.repeatPenalty.max} step={SAMPLING_BOUNDS.repeatPenalty.step}
            onChange$={(v) => { gRepeat.value = v; saveSoon(); }} />
        </div>
        {tuneNote.value && <p class="mt-3 text-xs text-[var(--text-secondary)]">{tuneNote.value}</p>}
      </div>
    </section>
  );
});
