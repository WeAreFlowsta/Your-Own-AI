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
  installed: boolean;
  stale_version_installed: boolean;
  active_backend: "bundled" | "cuda";
  tag: string;
  download_url: string | null;
}

/** The zip filename the Rust download emits progress events under. */
const zipNameFor = (tag: string) =>
  `llama-server-cuda-${tag.replace(/^llama-/, "")}.zip`;

export default component$(() => {
  const status = useSignal<EngineStatus | null>(null);
  const isNvidia = useSignal(false);
  const cudaLaddered = useSignal(false);
  const downloading = useSignal(false);
  const percent = useSignal(0);
  const error = useSignal("");
  const justInstalled = useSignal(false);

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
    const unp = await listen<{ filename: string; percent: number }>(
      "model-download-progress",
      (e) => {
        if (zip && e.payload.filename === zip) {
          downloading.value = true;
          percent.value = e.payload.percent;
        }
      },
    );
    cleanup(() => unp());
  });

  const doDownload = $(async () => {
    error.value = "";
    downloading.value = true;
    percent.value = 0;
    try {
      await invoke("download_cuda_engine", {});
      justInstalled.value = true;
      await refresh();
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
    } finally {
      downloading.value = false;
    }
  });

  const doRemove = $(async () => {
    error.value = "";
    justInstalled.value = false;
    try {
      await invoke("remove_cuda_engine");
      await refresh();
    } catch (e) {
      error.value = e instanceof Error ? e.message : String(e);
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
                <span class="text-xs font-normal text-[var(--text-muted)]">~400 MB</span>
              </h3>
              <p class="text-sm text-[var(--text-secondary)] mt-0.5">
                Built for NVIDIA graphics cards. Often faster than the
                standard engine, especially on long prompts.
              </p>
              {s.stale_version_installed && !s.installed && (
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
              {s.installed && !downloading.value && !cudaLaddered.value && (
                <p class="mt-2 text-[11px] text-emerald-400/80 flex items-center gap-1">
                  <LuCheck class="w-3 h-3" />
                  {s.active_backend === "cuda"
                    ? "Active - powering your chats."
                    : justInstalled.value
                      ? "Installed - takes over the next time a model loads."
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
                  {percent.value}%
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
    </section>
  );
});
