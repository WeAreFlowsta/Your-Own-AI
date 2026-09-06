import { renderAiDescription } from '../../utils/aiDescription';
/**
 * Welcome wizard - the first-run experience.
 *
 * Three steps, full window, no header: (1) download the model the app's
 * one grader recommends for this computer, (2) shape the three default AIs
 * - Personal keeps the characters, Work turns them into an Assistant, a
 * Coder and an Analyst - name, thumbnail and personality all editable,
 * (3) go. The download never blocks: it starts the moment step 1 is
 * confirmed and runs through steps 2 and 3, and the chat answers a held
 * question the moment the model lands (see utils/firstModel.ts).
 *
 * The root FirstModelIndicator owns the finish (load + assign to every AI)
 * so it happens whether or not this page is still open.
 */

import { component$, useSignal, useStore, useVisibleTask$, useContext, $ } from "@builder.io/qwik";
import { useNavigate, type DocumentHead } from "@builder.io/qwik-city";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { LuHardDriveDownload, LuCheck, LuAlertTriangle, LuPencil, LuArrowRight } from "@qwikest/icons/lucide";
import LiquidMetalButton from "../../components/LiquidMetalButton";
import { ThemeContext } from "../layout";
import logoLight from "../../assets/logo-light.svg";
import logo from "../../assets/logo.svg";
import type { SystemInfo } from "../../components/ModelDownloader";
import { useAiData, useAiDataActions } from "../../contexts/AiDataContext";
import { bundledArchetypes } from "../../data/bundled-archetypes";
import { THUMBNAIL_GALLERY } from "../../data/thumbnail-gallery";
import { MISSION_CORE } from "../../data/missionCore";
import type { CatalogModes } from "../../data/recommended-models";
import {
  DEFAULT_AI_PRESETS,
  DEFAULT_AI_PRESET_LABELS,
  WIZARD_PERSONALITY_IDS,
  type DefaultAiPreset,
} from "../../data/default-ai-presets";
import {
  getRecommendedModel,
  getUserFriendlyErrorMessage,
  firstModelInFlight,
  markFirstModelInFlight,
  clearFirstModelInFlight,
  FIRST_MODEL_READY,
} from "../../utils/firstModel";
import { refreshCatalogModes } from "../../utils/modelCache";
import { modelManager, type DownloadProgress } from "../../utils/modelManager";

const SEED_ORDER = ["veebo", "teresa", "reeves"];

/** Faces offered by the thumbnail picker: the three characters plus every
 *  distinct archetype face, named by the first archetype that carries it. */
const FACES: { name: string; path: string }[] = (() => {
  const out: { name: string; path: string }[] = [
    { name: "Veebo", path: "/bundled/veebo.jpg" },
    { name: "Teresa", path: "/bundled/teresa.jpg" },
    { name: "Reeves", path: "/bundled/reeves.jpg" },
  ];
  const seen = new Set(out.map((f) => f.path));
  for (const a of bundledArchetypes) {
    if (!a.thumbnailPath || seen.has(a.thumbnailPath)) continue;
    seen.add(a.thumbnailPath);
    out.push({ name: a.name, path: a.thumbnailPath });
  }
  return out;
})();

const COLORS = THUMBNAIL_GALLERY.filter((t) => t.group === "colors" || t.group === "gradients");

const STEPS = ["Your first model", "Meet your AIs", "Ready"];

export default component$(() => {
  const nav = useNavigate();
  const { theme } = useContext(ThemeContext);
  const aiData = useAiData();
  const { editUserAi, refreshThumbnail } = useAiDataActions();

  const step = useSignal<1 | 2 | 3>(1);
  const systemInfo = useSignal<SystemInfo | null>(null);
  const gpuUnusable = useSignal(false);
  const catalogModes = useSignal<CatalogModes | null>(null);

  const downloading = useSignal(false);
  const downloadLabel = useSignal("");
  const progress = useSignal<DownloadProgress | null>(null);
  const ready = useSignal(false);
  const error = useSignal<string | null>(null);

  const preset = useSignal<DefaultAiPreset>("personal");
  const applying = useSignal(false);
  const slotIds = useSignal<string[]>([]);
  const names = useStore<Record<string, string>>({});
  const thumbPickerFor = useSignal<string | null>(null);
  const personaPickerFor = useSignal<string | null>(null);
  const galleryTab = useSignal<"faces" | "colors">("faces");

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    try {
      systemInfo.value = await invoke<SystemInfo>("get_system_info");
    } catch (e) {
      console.warn("[Welcome] system info unavailable:", e);
    }
    try {
      const s = await invoke<{ active: boolean; device_unsupported?: string | null }>("gpu_safe_mode_status");
      gpuUnusable.value = s.active || !!s.device_unsupported;
    } catch {
      /* GPU sizing stands */
    }
    try {
      catalogModes.value = await refreshCatalogModes();
    } catch (e) {
      console.warn("[Welcome] catalog grading failed:", e);
    }

    // Back on this page mid-download (page change, restart): reflect the
    // download that is already running - the root indicator resumes it.
    const inFlight = firstModelInFlight();
    if (inFlight) {
      downloading.value = true;
      downloadLabel.value = inFlight.label;
      try {
        const st = await modelManager.downloadStatus(inFlight.filename);
        if (st.total_bytes > 0) {
          progress.value = {
            filename: inFlight.filename,
            downloaded: st.downloaded_bytes,
            total: st.total_bytes,
            percent: Math.floor((st.downloaded_bytes / st.total_bytes) * 100),
          };
        }
      } catch {
        /* progress arrives with the next event */
      }
    }

    const unProgress = await listen<DownloadProgress>("model-download-progress", (e) => {
      const f = firstModelInFlight();
      if (f && e.payload.filename === f.filename) progress.value = e.payload;
    });
    const onReady = () => {
      ready.value = true;
      downloading.value = false;
    };
    window.addEventListener(FIRST_MODEL_READY, onReady);
    cleanup(() => {
      unProgress();
      window.removeEventListener(FIRST_MODEL_READY, onReady);
    });
  });

  // The three default AIs, in seed order, once the store has them.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const ais = track(() => aiData.userDefinedAis);
    if (slotIds.value.length > 0 || ais.length === 0) return;
    const rank = (id: string) => {
      const i = SEED_ORDER.indexOf(id);
      return i < 0 ? 99 : i;
    };
    const three = ais
      .filter((a) => a.status === "active")
      .sort((a, b) => rank(a.baseArchetypeId) - rank(b.baseArchetypeId))
      .slice(0, 3);
    slotIds.value = three.map((a) => a.id);
    for (const a of three) names[a.id] = a.name;
  });

  const startDownload$ = $(async () => {
    const r = getRecommendedModel(systemInfo.value, gpuUnusable.value, catalogModes.value);
    if (r.pending) return;
    const label = `${r.familyName} ${r.variant.parameterCount}`;
    downloading.value = true;
    downloadLabel.value = label;
    error.value = null;
    progress.value = null;
    markFirstModelInFlight(r.variant.filename, label);
    try {
      await modelManager.downloadModel(r.variant.downloadUrl, r.variant.filename, (p) => {
        progress.value = p;
      });
      // The root FirstModelIndicator loads it, assigns it to every AI and
      // announces FIRST_MODEL_READY - this page only reflects that.
    } catch (err) {
      console.error("[Welcome] download failed:", err);
      error.value = getUserFriendlyErrorMessage(err);
      downloading.value = false;
      clearFirstModelInFlight();
    }
  });

  const downloadAndContinue$ = $(async () => {
    step.value = 2;
    void startDownload$();
  });

  const saveThumb$ = $(async (aiId: string, path: string) => {
    try {
      const res = await fetch(path);
      const buf = await (await res.blob()).arrayBuffer();
      await invoke("save_ai_thumbnail", { aiId, thumbnailData: Array.from(new Uint8Array(buf)) });
      await refreshThumbnail(aiId);
    } catch (e) {
      console.warn("[Welcome] thumbnail save failed:", e);
    }
  });

  const applyPreset$ = $(async (kind: DefaultAiPreset) => {
    if (applying.value || preset.value === kind) return;
    applying.value = true;
    preset.value = kind;
    try {
      const slots = DEFAULT_AI_PRESETS[kind];
      for (let i = 0; i < slotIds.value.length && i < slots.length; i++) {
        const id = slotIds.value[i];
        const s = slots[i];
        const arch = bundledArchetypes.find((a) => a.id === s.archetypeId);
        await editUserAi(id, {
          name: s.name,
          description: s.description,
          baseArchetypeId: s.archetypeId,
          systemPrompt: `${MISSION_CORE}\n\n${arch?.systemPromptTemplate ?? ""}`,
        });
        names[id] = s.name;
        await saveThumb$(id, s.thumbnail);
      }
    } finally {
      applying.value = false;
    }
  });

  const commitName$ = $(async (aiId: string) => {
    const n = (names[aiId] || "").trim();
    if (!n) {
      const ai = aiData.userDefinedAis.find((a) => a.id === aiId);
      names[aiId] = ai?.name ?? "";
      return;
    }
    await editUserAi(aiId, { name: n });
  });

  const setPersona$ = $(async (aiId: string, archetypeId: string) => {
    const arch = bundledArchetypes.find((a) => a.id === archetypeId);
    if (!arch) return;
    await editUserAi(aiId, {
      baseArchetypeId: archetypeId,
      systemPrompt: `${MISSION_CORE}\n\n${arch.systemPromptTemplate}`,
    });
    personaPickerFor.value = null;
  });

  const pickThumb$ = $(async (aiId: string, path: string) => {
    thumbPickerFor.value = null;
    await saveThumb$(aiId, path);
  });

  const recommended = getRecommendedModel(systemInfo.value, gpuUnusable.value, catalogModes.value);
  const modelLabel = `${recommended.familyName} ${recommended.variant.parameterCount}`;
  const pct = progress.value?.percent ?? null;
  const personalities = WIZARD_PERSONALITY_IDS
    .map((id) => bundledArchetypes.find((a) => a.id === id))
    .filter((a): a is NonNullable<typeof a> => !!a);

  return (
    <div class="min-h-screen w-full bg-[var(--bg-main)] text-[var(--text-primary)] flex flex-col relative overflow-x-hidden">
      {/* Soft glow - the one decorative element; everything else is content. */}
      <div
        class="pointer-events-none absolute inset-0"
        style={{
          background:
            "radial-gradient(60% 45% at 15% 0%, rgba(59,130,246,0.16), transparent 70%), radial-gradient(50% 40% at 90% 100%, rgba(16,185,129,0.12), transparent 70%)",
        }}
      />

      {/* Top bar: wordmark + step rail + download pill */}
      <header class="relative z-10 flex items-center justify-between gap-4 px-6 md:px-10 py-5">
        <img src={theme.value === "dark" ? logoLight : logo} alt="Your Own AI" width={160} height={40} class="h-8 w-auto" />
        <ol class="hidden sm:flex items-center gap-3 text-xs">
          {STEPS.map((label, i) => {
            const n = (i + 1) as 1 | 2 | 3;
            const done = step.value > n || (n === 3 && ready.value);
            const current = step.value === n;
            return (
              <li key={label} class="flex items-center gap-2">
                <span
                  class={`inline-flex items-center justify-center w-5 h-5 rounded-full border text-[10px] font-semibold ${
                    done
                      ? "bg-[var(--bg-button-primary)] border-transparent text-[var(--text-button-primary)]"
                      : current
                        ? "border-[var(--text-primary)] text-[var(--text-primary)]"
                        : "border-[var(--border-subtle)] text-[var(--text-muted)]"
                  }`}
                >
                  {done ? <LuCheck class="w-3 h-3" /> : n}
                </span>
                <span class={current ? "text-[var(--text-primary)] font-medium" : "text-[var(--text-muted)]"}>{label}</span>
                {i < STEPS.length - 1 && <span class="w-6 h-px bg-[var(--border-subtle)]" />}
              </li>
            );
          })}
        </ol>
        {(downloading.value || ready.value) && (
          <div class="flex items-center gap-2 rounded-full border border-[var(--border-subtle)] bg-[var(--bg-card)] px-3 py-1.5 text-xs">
            {ready.value ? (
              <>
                <LuCheck class="w-3.5 h-3.5 text-green-500" />
                <span>{downloadLabel.value} ready</span>
              </>
            ) : (
              <>
                <span class="inline-block w-2 h-2 rounded-full bg-[var(--bg-button-primary)] animate-pulse" />
                <span>
                  {downloadLabel.value}
                  {pct !== null ? ` · ${pct}%` : ""}
                </span>
              </>
            )}
          </div>
        )}
      </header>

      <main class="relative z-10 flex-1 flex flex-col items-center px-6 md:px-10 pb-16">
        <div class="w-full max-w-3xl">
          {/* ---------------- Step 1: first model ---------------- */}
          {step.value === 1 && (
            <section class="pt-6 md:pt-14">
              <p class="text-sm uppercase tracking-widest text-[var(--text-muted)] mb-3">Step 1 of 3</p>
              <h1 class="font-varela text-3xl md:text-5xl font-bold leading-tight mb-4">
                Welcome. Let's get your first model.
              </h1>
              <p class="text-base md:text-lg text-[var(--text-secondary)] mb-8 max-w-2xl">
                Models run on this computer. Nothing you say leaves it, and there is nothing to pay.
                This is the one we picked for your hardware.
              </p>

              <div class="rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-6 md:p-8 mb-6">
                <div class="flex items-start justify-between gap-4 flex-wrap">
                  <div>
                    <p class="text-xs uppercase tracking-widest text-[var(--text-muted)] mb-1">Recommended for this computer</p>
                    <h2 class="font-varela text-2xl md:text-3xl font-bold">
                      {recommended.pending ? "One moment.." : modelLabel}
                    </h2>
                  </div>
                  {!recommended.pending && (
                    <span class="rounded-full bg-[var(--bg-main)] border border-[var(--border-subtle)] px-3 py-1 text-xs text-[var(--text-secondary)]">
                      {recommended.variant.size} GB download
                    </span>
                  )}
                </div>
                <p class="mt-4 text-sm md:text-base text-[var(--text-secondary)] leading-relaxed">{recommended.reason}</p>
                {recommended.tight && (
                  <p class="mt-3 text-xs text-amber-500">
                    Expect slow answers on this machine. For fast ones, the optional plan adds GPT, Grok, Kimi,
                    DeepSeek and more, running online at full speed on any computer - every price shown up front,
                    in Settings after this download.
                  </p>
                )}
              </div>

              {error.value && (
                <div class="mb-6 p-4 rounded-xl border border-red-300 dark:border-red-700 bg-red-50 dark:bg-red-900/30 flex items-start gap-3">
                  <LuAlertTriangle class="w-5 h-5 text-red-600 dark:text-red-400 flex-shrink-0 mt-0.5" />
                  <div>
                    <p class="text-sm font-medium text-red-800 dark:text-red-200">Download failed</p>
                    <p class="text-sm text-red-700 dark:text-red-300">{error.value}</p>
                  </div>
                </div>
              )}

              <div class="flex flex-col-reverse sm:flex-row sm:items-center sm:justify-end gap-4">
                <LiquidMetalButton
                  onClick$={downloadAndContinue$}
                  disabled={!!recommended.pending}
                  class="px-6 py-3 font-semibold text-base flex items-center justify-center gap-2"
                >
                  <LuHardDriveDownload class="w-5 h-5" />
                  {recommended.pending ? "Checking what fits.." : "Download and continue"}
                </LiquidMetalButton>
              </div>
              <p class="mt-8 text-xs text-[var(--text-muted)] max-w-2xl">
                Used Your Own AI before, with a Flowsta Vault backup? You can restore your AIs and
                conversations any time from Settings, under Your Flowsta Identity.
              </p>
            </section>
          )}

          {/* ---------------- Step 2: meet your AIs ---------------- */}
          {step.value === 2 && (
            <section class="pt-6 md:pt-10">
              <p class="text-sm uppercase tracking-widest text-[var(--text-muted)] mb-3">Step 2 of 3</p>
              <h1 class="font-varela text-3xl md:text-5xl font-bold leading-tight mb-3">Meet your AIs.</h1>
              <p class="text-base md:text-lg text-[var(--text-secondary)] mb-6 max-w-2xl">
                You start with three. Pick the kind that fits you, then change any name, picture or
                personality right here - the names are yours to type over. All three share the model
                that is downloading now.
              </p>

              <div class="inline-flex rounded-full border border-[var(--border-subtle)] bg-[var(--bg-card)] p-1 mb-6">
                {(["personal", "work"] as DefaultAiPreset[]).map((k) => (
                  <button
                    key={k}
                    type="button"
                    disabled={applying.value}
                    onClick$={() => applyPreset$(k)}
                    class={`px-4 py-1.5 rounded-full text-sm font-medium transition ${
                      preset.value === k
                        ? "bg-[var(--bg-button-primary)] text-[var(--text-button-primary)]"
                        : "text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
                    }`}
                  >
                    {DEFAULT_AI_PRESET_LABELS[k].title}
                  </button>
                ))}
              </div>
              <p class="text-sm text-[var(--text-muted)] mb-6 -mt-3">{DEFAULT_AI_PRESET_LABELS[preset.value].blurb}</p>

              <div class="grid gap-4 sm:grid-cols-3">
                {slotIds.value.map((id) => {
                  const ai = aiData.userDefinedAis.find((a) => a.id === id);
                  if (!ai) return null;
                  const arch = bundledArchetypes.find((a) => a.id === ai.baseArchetypeId);
                  const thumb = aiData.thumbnailObjectUrls[id] || arch?.thumbnailPath || "/generic-ai-placeholder.svg";
                  return (
                    <div key={id} class="rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-4 flex flex-col gap-3">
                      <button
                        type="button"
                        class="relative group rounded-xl overflow-hidden aspect-square w-full"
                        title="Change picture"
                        onClick$={() => {
                          galleryTab.value = preset.value === "work" ? "colors" : "faces";
                          thumbPickerFor.value = id;
                        }}
                      >
                        <img src={thumb} alt={names[id] || ai.name} width={256} height={256} class="w-full h-full object-cover" />
                        <span class="absolute bottom-2 right-2 inline-flex items-center gap-1 rounded-full bg-black/60 text-white text-[11px] px-2 py-1 opacity-80 group-hover:opacity-100">
                          <LuPencil class="w-3 h-3" /> Picture
                        </span>
                      </button>
                      <label class="block">
                        <span class="flex items-center justify-between text-[11px] uppercase tracking-widest text-[var(--text-muted)] mb-1">
                          <span>Name</span>
                          <span class="inline-flex items-center gap-1 normal-case tracking-normal text-[var(--text-link)]"><LuPencil class="w-3 h-3" /> Change</span>
                        </span>
                        <input
                          type="text"
                          value={names[id] ?? ai.name}
                          maxLength={40}
                          placeholder="Give this AI a name"
                          class="w-full rounded-lg border border-dashed border-[var(--text-muted)] bg-[var(--bg-main)] px-3 py-2 text-base font-semibold text-[var(--text-primary)] focus:outline-none focus:border-solid focus:ring-2 focus:ring-[var(--bg-button-primary)]"
                          onInput$={(_, el) => {
                            names[id] = el.value;
                          }}
                          onChange$={() => commitName$(id)}
                        />
                      </label>
                      <p class="text-xs text-[var(--text-secondary)] leading-relaxed min-h-[2.5rem]">{renderAiDescription(ai)}</p>
                      <button
                        type="button"
                        class="mt-auto flex items-center justify-between rounded-lg border border-[var(--border-subtle)] px-3 py-2 text-xs hover:border-[var(--text-muted)]"
                        onClick$={() => {
                          personaPickerFor.value = id;
                        }}
                      >
                        <span class="text-[var(--text-muted)]">
                          Personality: <span class="text-[var(--text-primary)] font-medium">{arch?.name ?? "Custom"}</span>
                        </span>
                        <span class="text-[var(--text-link)]">Change</span>
                      </button>
                    </div>
                  );
                })}
                {slotIds.value.length === 0 && (
                  <p class="sm:col-span-3 text-sm text-[var(--text-muted)]">Setting up your AIs..</p>
                )}
              </div>

              <p class="mt-6 text-sm text-[var(--text-muted)] max-w-2xl">
                There's much more to shape on the Your AIs page later - a voice, a longer or shorter
                style, skills, tools and more AIs whenever you want them.
              </p>

              <div class="mt-8 flex flex-col-reverse sm:flex-row sm:items-center sm:justify-between gap-4">
                <button
                  type="button"
                  class="text-sm text-[var(--text-muted)] hover:text-[var(--text-secondary)] text-left"
                  onClick$={() => {
                    step.value = 1;
                  }}
                >
                  Back
                </button>
                <LiquidMetalButton
                  onClick$={() => {
                    step.value = 3;
                  }}
                  disabled={applying.value}
                  class="px-6 py-3 font-semibold text-base flex items-center justify-center gap-2"
                >
                  Looks good
                  <LuArrowRight class="w-5 h-5" />
                </LiquidMetalButton>
              </div>
            </section>
          )}

          {/* ---------------- Step 3: ready ---------------- */}
          {step.value === 3 && (
            <section class="pt-6 md:pt-14">
              <p class="text-sm uppercase tracking-widest text-[var(--text-muted)] mb-3">Step 3 of 3</p>
              {ready.value ? (
                <>
                  <h1 class="font-varela text-3xl md:text-5xl font-bold leading-tight mb-4">You're all set.</h1>
                  <p class="text-base md:text-lg text-[var(--text-secondary)] mb-8 max-w-2xl">
                    {downloadLabel.value} is on this computer and your three AIs are using it. Ask anything.
                  </p>
                </>
              ) : error.value ? (
                <>
                  <h1 class="font-varela text-3xl md:text-5xl font-bold leading-tight mb-4">The download stopped.</h1>
                  <p class="text-base md:text-lg text-[var(--text-secondary)] mb-3 max-w-2xl">{error.value}</p>
                  <p class="text-sm text-[var(--text-muted)] mb-8 max-w-2xl">
                    Your AIs are ready; they just need a model. Try again here, or pick one on the Offline Models page later.
                  </p>
                </>
              ) : (
                <>
                  <h1 class="font-varela text-3xl md:text-5xl font-bold leading-tight mb-4">Your AIs are ready. The model is on its way.</h1>
                  <p class="text-base md:text-lg text-[var(--text-secondary)] mb-3 max-w-2xl">
                    {downloadLabel.value}
                    {pct !== null ? ` is ${pct}% downloaded.` : " is downloading."} You don't have to wait: go ahead and ask
                    any of your AIs a question. The answer arrives the moment the model lands.
                  </p>
                  <p class="text-sm text-[var(--text-muted)] mb-8 max-w-2xl">
                    The message field shows the progress while you wait, and a note appears when the model is ready.
                  </p>
                  {pct !== null && (
                    <div class="w-full max-w-md h-2 rounded-full bg-[var(--bg-card)] border border-[var(--border-subtle)] overflow-hidden mb-8">
                      <div class="h-full bg-[var(--bg-button-primary)] transition-all duration-300" style={{ width: `${pct}%` }} />
                    </div>
                  )}
                </>
              )}
              <div class="flex flex-col-reverse sm:flex-row sm:items-center sm:justify-between gap-4">
                <button
                  type="button"
                  class="text-sm text-[var(--text-muted)] hover:text-[var(--text-secondary)] text-left"
                  onClick$={() => {
                    step.value = 2;
                  }}
                >
                  Back to my AIs
                </button>
                <div class="flex flex-col-reverse sm:flex-row gap-3">
                  {error.value && !downloading.value && (
                    <LiquidMetalButton variant="secondary" onClick$={startDownload$} class="px-6 py-3 font-semibold text-base">
                      Try the download again
                    </LiquidMetalButton>
                  )}
                  <LiquidMetalButton onClick$={() => nav("/chat")} class="px-6 py-3 font-semibold text-base flex items-center justify-center gap-2">
                    {ready.value ? "Start chatting" : "Take me to my AIs"}
                    <LuArrowRight class="w-5 h-5" />
                  </LiquidMetalButton>
                </div>
              </div>
            </section>
          )}
        </div>
      </main>

      {/* ---------------- Thumbnail picker ---------------- */}
      {thumbPickerFor.value && (
        <div
          class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60"
          onClick$={() => {
            thumbPickerFor.value = null;
          }}
        >
          <div
            class="w-full max-w-2xl max-h-[85vh] overflow-y-auto rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-5"
            onClick$={(e) => e.stopPropagation()}
          >
            <div class="flex items-center justify-between mb-4">
              <h3 class="font-varela text-lg font-bold">Pick a picture</h3>
              <div class="inline-flex rounded-full border border-[var(--border-subtle)] p-0.5 text-xs">
                {(["faces", "colors"] as const).map((t) => (
                  <button
                    key={t}
                    type="button"
                    onClick$={() => {
                      galleryTab.value = t;
                    }}
                    class={`px-3 py-1 rounded-full ${galleryTab.value === t ? "bg-[var(--bg-button-primary)] text-[var(--text-button-primary)]" : "text-[var(--text-secondary)]"}`}
                  >
                    {t === "faces" ? "Faces" : "Colors"}
                  </button>
                ))}
              </div>
            </div>
            <div class="grid grid-cols-4 sm:grid-cols-6 gap-3">
              {(galleryTab.value === "faces" ? FACES : COLORS).map((t) => (
                <button
                  key={t.path}
                  type="button"
                  title={t.name}
                  class="rounded-xl overflow-hidden aspect-square border border-transparent hover:border-[var(--text-primary)] focus:outline-none focus:border-[var(--text-primary)]"
                  onClick$={() => pickThumb$(thumbPickerFor.value!, t.path)}
                >
                  <img src={t.path} alt={t.name} width={96} height={96} class="w-full h-full object-cover" />
                </button>
              ))}
            </div>
            <p class="mt-4 text-xs text-[var(--text-muted)]">Your own photo, or a generated one, can go on any AI from the Your AIs page.</p>
          </div>
        </div>
      )}

      {/* ---------------- Personality picker ---------------- */}
      {personaPickerFor.value && (
        <div
          class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60"
          onClick$={() => {
            personaPickerFor.value = null;
          }}
        >
          <div
            class="w-full max-w-2xl max-h-[85vh] overflow-y-auto rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-5"
            onClick$={(e) => e.stopPropagation()}
          >
            <h3 class="font-varela text-lg font-bold mb-1">Pick a personality</h3>
            <p class="text-xs text-[var(--text-muted)] mb-4">Six to start with. All eighteen, and your own, live on the Your AIs page.</p>
            <div class="grid gap-3 sm:grid-cols-2">
              {personalities.map((a) => {
                const current = aiData.userDefinedAis.find((x) => x.id === personaPickerFor.value)?.baseArchetypeId === a.id;
                return (
                  <button
                    key={a.id}
                    type="button"
                    onClick$={() => setPersona$(personaPickerFor.value!, a.id)}
                    class={`text-left rounded-xl border p-3 hover:border-[var(--text-primary)] ${
                      current ? "border-[var(--bg-button-primary)]" : "border-[var(--border-subtle)]"
                    }`}
                  >
                    <div class="flex items-center justify-between">
                      <span class="font-semibold text-sm">{a.name}</span>
                      {current && <LuCheck class="w-4 h-4 text-green-500" />}
                    </div>
                    <p class="mt-1 text-xs text-[var(--text-secondary)] leading-relaxed">{renderAiDescription({ name: a.name, description: a.description, baseArchetypeId: a.id })}</p>
                  </button>
                );
              })}
            </div>
          </div>
        </div>
      )}
    </div>
  );
});

export const head: DocumentHead = {
  title: "Welcome - Your Own AI",
};
