import { renderAiDescription } from '../../utils/aiDescription';
import { EMBEDDING_MODEL } from "../../data/recommended-models";
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
import { listen, emit } from "@tauri-apps/api/event";
import { LuHardDriveDownload, LuCheck, LuAlertTriangle, LuPencil, LuArrowRight } from "@qwikest/icons/lucide";
import LiquidMetalButton from "../../components/LiquidMetalButton";
import FlowstaWizardStep from "../../components/FlowstaWizardStep";
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

  // The starters are seeded as the Personal set; the wizard shows Work
  // first and applies it by itself on a fresh install (below).
  const preset = useSignal<DefaultAiPreset>("personal");
  const applying = useSignal(false);
  // The Work set is applied by itself once, to starters nobody has touched.
  const workTried = useSignal(false);
  // Someone who already uses Your Own AI on another computer: the Vault on
  // this one, the session, and what a sign-in at this step brought.
  const vault = useSignal<{ installed: boolean; unlocked: boolean } | null>(null);
  const session = useSignal<{ signed_in: boolean } | null>(null);
  const bringing = useSignal(false);
  const bringError = useSignal("");
  /** Set when this identity has AIs on another device: the set choice goes. */
  const fromAccount = useSignal<{ device: string | null } | null>(null);
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

  // The Vault on this computer and the session, for the one line at step 2
  // that someone who already uses Your Own AI elsewhere can take. A profile
  // that is signed in already and has other devices shows their AIs.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    const read = async () => {
      try {
        const [v, se] = await Promise.all([
          invoke<{ installed: boolean; unlocked: boolean }>("flowsta_vault_status"),
          invoke<{ signed_in: boolean }>("flowsta_session"),
        ]);
        vault.value = v;
        session.value = se;
        if (se.signed_in && !fromAccount.value) {
          const st = await invoke<{ devices?: { name: string | null }[] } | null>("config_sync_status").catch(() => null);
          if (st?.devices?.length) fromAccount.value = { device: st.devices[0].name ?? null };
        }
      } catch {
        if (!session.value) session.value = { signed_in: false };
      }
    };
    void read();
    const t = setInterval(() => { if (!bringing.value && !session.value?.signed_in) void read(); }, 5000);
    cleanup(() => clearInterval(t));
  });

  // The three default AIs. Their order is fixed when the slots are first
  // captured (seed order: Veebo, Teresa, Reeves - the store does not keep
  // it) and kept from then on: ranking again by archetype or name would
  // reshuffle the cards under the person's hands (the Work set gives all
  // three the same personality) and send the next edit to the wrong AI
  // (2026-10-08). The slots are recaptured only when an id vanishes - the
  // context re-keys each AI in place to its agent key seconds after first
  // launch, and a stale id finds no AI and renders nothing (the "no AIs to
  // edit" walkthrough, same day). Nothing has been typed by then, but a
  // typed name is carried by position all the same.
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const ais = track(() => aiData.userDefinedAis);
    track(() => ais.map((a) => a.id).join("\n"));
    if (ais.length === 0) return;
    if (slotIds.value.length > 0 && slotIds.value.every((id) => ais.some((a) => a.id === id))) return;
    // By the slot each starter was seeded into (stamped once, kept through
    // every edit), then by personality for an AI with no stamp. Personality
    // alone ties under the Work set (all three share one), and a recapture
    // then came back in whatever order the list had: the Work set applied
    // by itself showed Analyst, Coder, Assistant (2026-11-17).
    const rank = (a: { seedSlot?: number; baseArchetypeId: string }) => {
      if (a.seedSlot) return a.seedSlot;
      const i = SEED_ORDER.indexOf(a.baseArchetypeId);
      return i < 0 ? 99 : 10 + i;
    };
    const three = ais
      .filter((a) => a.status === "active")
      .sort((a, b) => rank(a) - rank(b))
      .slice(0, 3);
    const typed = slotIds.value.map((old) => names[old]);
    slotIds.value = three.map((a) => a.id);
    three.forEach((a, i) => {
      names[a.id] = typed[i] !== undefined && typed[i] !== "" ? typed[i] : a.name;
    });
  });

  // Which set the three AIs are, read from the AIs themselves (the wizard
  // keeps no record of the choice: coming back to it used to show Personal
  // whatever they were).
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const ids = track(() => slotIds.value.join("\n"));
    const shown = track(() => slotIds.value.map((id) => aiData.userDefinedAis.find((a) => a.id === id)?.name ?? "").join("\n"));
    if (!ids || applying.value) return;
    for (const kind of ["work", "personal"] as DefaultAiPreset[]) {
      if (DEFAULT_AI_PRESETS[kind].map((sl) => sl.name).join("\n") === shown) preset.value = kind;
    }
  });

  // One download, however many times the button is pressed. Coming back to
  // step 1 mid-download and pressing it again used to start a second
  // download, which the app refuses - and the refusal was taken for a
  // failure, which cleared the first-model marker while the real download
  // ran on unwatched (walkthrough 2026-10-08). Now: already running means
  // attach; already on disk means finish; only a fresh start can fail.
  const startDownload$ = $(async () => {
    if (downloading.value || ready.value) return;
    const r = getRecommendedModel(systemInfo.value, gpuUnusable.value, catalogModes.value);
    if (r.pending) return;
    const label = `${r.familyName} ${r.variant.parameterCount}`;
    downloading.value = true;
    downloadLabel.value = label;
    error.value = null;
    try {
      const st = await modelManager.downloadStatus(r.variant.filename);
      if (st.downloading) {
        // Running already (a Try-again that raced the resume, a second
        // press): reflect it; the root indicator carries it to the finish.
        markFirstModelInFlight(r.variant.filename, label);
        if (st.total_bytes > 0) {
          progress.value = {
            filename: r.variant.filename,
            downloaded: st.downloaded_bytes,
            total: st.total_bytes,
            percent: Math.floor((st.downloaded_bytes / st.total_bytes) * 100),
          };
        }
        return;
      }
    } catch {
      /* no status is no download: start one */
    }
    progress.value = null;
    markFirstModelInFlight(r.variant.filename, label);
    try {
      await modelManager.downloadModel(r.variant.downloadUrl, r.variant.filename, (p) => {
        progress.value = p;
      });
      // The memory component rides the first download (130 MB): recall and
      // smart routing work from the first conversation, nothing to ask.
      try {
        if (!(await modelManager.isModelDownloaded(EMBEDDING_MODEL.filename))) {
          void modelManager.downloadModel(EMBEDDING_MODEL.downloadUrl, EMBEDDING_MODEL.filename, () => {}).catch(() => {});
        }
      } catch { /* the documents offer covers it later */ }
      // The root FirstModelIndicator loads it, assigns it to every AI and
      // announces FIRST_MODEL_READY - this page only reflects that.
    } catch (err) {
      const msg = String((err as any)?.message ?? err);
      if (msg.includes("already in progress")) return; // attached above or by the indicator
      if (msg.includes("already downloaded")) {
        // The file is complete on disk: let the indicator finish it (load,
        // assign to the AIs, announce) exactly as a fresh download would.
        await emit("model-download-complete", { filename: r.variant.filename });
        return;
      }
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

  // The person pressed a set themselves. Kept apart from `preset`, which
  // also follows what the AIs are named: a stale write that puts the old
  // names back for a moment reads as "personal" there, and is no choice.
  const pickedByHand = useSignal(false);

  const applyPreset$ = $(async (kind: DefaultAiPreset, again = false) => {
    if (applying.value || (preset.value === kind && !again)) return;
    applying.value = true;
    preset.value = kind;
    try {
      const slots = DEFAULT_AI_PRESETS[kind];
      for (let i = 0; i < slotIds.value.length && i < slots.length; i++) {
        const id = slotIds.value[i];
        const s = slots[i];
        const arch = bundledArchetypes.find((a) => a.id === s.archetypeId);
        const updated = await editUserAi(id, {
          name: s.name,
          description: s.description,
          baseArchetypeId: s.archetypeId,
          systemPrompt: `${MISSION_CORE}\n\n${arch?.systemPromptTemplate ?? ""}`,
        });
        if (!updated) {
          console.warn("[Welcome] preset edit found no AI for slot", i, id);
          continue;
        }
        names[id] = s.name;
        await saveThumb$(id, s.thumbnail);
      }
    } finally {
      applying.value = false;
    }
  });

  // Work first: on a fresh install the three starters become the Work set
  // with nobody pressing anything. Only starters nobody has touched, made
  // on this computer, with their keys settled (the context re-keys each AI
  // seconds after first launch, and an edit under a stale id is lost), and
  // only while this computer has not signed in: an identity's own AIs are
  // never changed by a default. Someone who already has AIs elsewhere and
  // signs in later gets theirs in place of these (an unused starter gives
  // way, `config_sync::starter_gives_way`).
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ track }) => {
    const ids = track(() => slotIds.value.join("\n"));
    const signedIn = track(() => session.value?.signed_in);
    track(() => fromAccount.value);
    track(() => aiData.userDefinedAis.map((a) => `${a.id}:${a.agentPubKey ?? ""}`).join("\n"));
    if (workTried.value || !ids || signedIn === undefined) return;
    if (signedIn || fromAccount.value) {
      workTried.value = true;
      return;
    }
    const three = slotIds.value.map((id) => aiData.userDefinedAis.find((a) => a.id === id));
    if (three.length !== 3 || three.some((a) => !a)) return;
    const settled = three.every((a) => !!a!.agentPubKey && a!.id === a!.agentPubKey);
    if (!settled) return; // the re-key is still under way: this runs again when it lands
    const untouched = three.every(
      (a, i) =>
        a!.updatedAt === undefined &&
        (!a!.foundingKey || a!.foundingKey === a!.agentPubKey) &&
        a!.name === DEFAULT_AI_PRESETS.personal[i].name,
    );
    workTried.value = true;
    if (!untouched) return;
    // Applied, then read back, and applied again if a slot did not hold.
    // The context is still writing the list at this moment on a slow
    // machine (each AI's new key, then its picture's move): a write of the
    // whole list that began before an edit here puts the old name back
    // (seen on a CI runner 2026-11-17: Veebo, Coder, Analyst).
    const want = DEFAULT_AI_PRESETS.work.map((sl) => sl.name).join("\n");
    const shown = () => slotIds.value.map((id) => aiData.userDefinedAis.find((a) => a.id === id)?.name ?? "").join("\n");
    for (let attempt = 0, held = 0; attempt < 6 && held < 2; attempt++) {
      if (shown() !== want) {
        held = 0;
        await applyPreset$("work", true);
      }
      await new Promise((r) => setTimeout(r, 1500));
      // Someone chose the other set meanwhile: theirs stands. Only a
      // press counts: the names going back to the starters' for a moment
      // (the stale write this loop exists for) is not a choice.
      if (pickedByHand.value) return;
      if (shown() === want) held++;
    }
  });

  // "Used Your Own AI before?": sign in here, then read the other devices
  // at once. When there are any, this computer's unused starters have just
  // become the identity's AIs, and step 2 shows them in place of the choice.
  const bringMyAis$ = $(async () => {
    if (bringing.value) return;
    bringing.value = true;
    bringError.value = "";
    workTried.value = true;
    try {
      await invoke("flowsta_sign_in");
      session.value = { signed_in: true };
      const st = await invoke<{ devices?: { name: string | null }[] }>("config_sync_now").catch(() => null);
      if (st?.devices?.length) fromAccount.value = { device: st.devices[0].name ?? null };
    } catch (e) {
      const msg = String(e);
      bringError.value = msg.includes("vault_locked")
        ? "Your Vault is locked. Unlock it and try again."
        : msg.includes("denied") || msg.includes("rejected")
          ? "You declined the sign-in in your Vault."
          : "Sign-in did not finish. You can try again, or sign in at the last step.";
    } finally {
      bringing.value = false;
    }
  });

  // The identity's AIs just took the starters' places: the name fields
  // follow (they hold what was typed, by AI).
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    const here = track(() => fromAccount.value);
    const shown = track(() => slotIds.value.map((id) => aiData.userDefinedAis.find((a) => a.id === id)?.name ?? "").join("\n"));
    if (!here || !shown) return;
    for (const id of slotIds.value) {
      const ai = aiData.userDefinedAis.find((a) => a.id === id);
      if (ai) names[id] = ai.name;
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
          <div data-testid="welcome-progress" class="flex items-center gap-2 rounded-full border border-[var(--border-subtle)] bg-[var(--bg-card)] px-3 py-1.5 text-xs">
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
              <h1 data-testid="welcome-title" class="font-varela text-3xl md:text-5xl font-bold leading-tight mb-4">
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
                    <h2 data-testid="welcome-recommended" data-pending={recommended.pending ? "1" : "0"} class="font-varela text-2xl md:text-3xl font-bold">
                      {recommended.pending ? "One moment.." : modelLabel}
                    </h2>
                  </div>
                  {!recommended.pending && (
                    <span
                      data-testid="welcome-model-state"
                      class={`rounded-full border px-3 py-1 text-xs ${
                        ready.value
                          ? "border-green-600/40 bg-green-500/10 text-green-500"
                          : downloading.value
                            ? "border-[var(--bg-button-primary)] bg-[var(--bg-main)] text-[var(--text-primary)]"
                            : "border-[var(--border-subtle)] bg-[var(--bg-main)] text-[var(--text-secondary)]"
                      }`}
                    >
                      {ready.value
                        ? "Ready"
                        : downloading.value
                          ? `Downloading${pct !== null ? ` · ${pct}%` : ""}`
                          : `${recommended.variant.size} GB download`}
                    </span>
                  )}
                </div>
                <p class="mt-4 text-sm md:text-base text-[var(--text-secondary)] leading-relaxed">{recommended.reason}</p>
                {downloading.value && pct !== null && (
                  <div class="mt-4 w-full h-2 rounded-full bg-[var(--bg-main)] border border-[var(--border-subtle)] overflow-hidden">
                    <div class="h-full bg-[var(--bg-button-primary)] transition-all duration-300" style={{ width: `${pct}%` }} />
                  </div>
                )}
                {recommended.tight && (
                  <p class="mt-3 text-xs text-amber-500">
                    Expect slow answers on this machine. Online models run at full speed on any computer; they are
                    an optional paid service, on the Online Models page after this download. Everything offline
                    stays free.
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
                  testId="welcome-download"
                  class="px-6 py-3 font-semibold text-base flex items-center justify-center gap-2"
                >
                  {downloading.value || ready.value ? <LuArrowRight class="w-5 h-5" /> : <LuHardDriveDownload class="w-5 h-5" />}
                  {recommended.pending
                    ? "Checking what fits.."
                    : downloading.value || ready.value
                      ? "Continue"
                      : error.value
                        ? "Try the download again"
                        : "Download and continue"}
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
              <h1 class="font-varela text-3xl md:text-5xl font-bold leading-tight mb-3">
                {fromAccount.value ? "Your AIs are here." : "Meet your AIs."}
              </h1>
              {fromAccount.value ? (
                <p class="text-base md:text-lg text-[var(--text-secondary)] mb-6 max-w-2xl" data-testid="welcome-from-account">
                  They came from {fromAccount.value.device ?? "your other computer"}, with their names and pictures. Their conversations follow.
                </p>
              ) : (
                <p class="text-base md:text-lg text-[var(--text-secondary)] mb-6 max-w-2xl">
                  You start with three. Which set?
                </p>
              )}

              {/* One quiet line for someone who already uses Your Own AI on
                  another computer, shown only when a Vault is running here.
                  A first-time person with no Vault never sees it. */}
              {!fromAccount.value && vault.value?.installed && session.value && !session.value.signed_in && (
                <p class="text-sm text-[var(--text-muted)] mb-4 max-w-2xl" data-testid="welcome-bring-ais">
                  Used Your Own AI before?{" "}
                  {vault.value.unlocked ? (
                    <button
                      type="button"
                      disabled={bringing.value}
                      data-testid="welcome-bring-ais-sign-in"
                      onClick$={bringMyAis$}
                      class="underline text-[var(--text-secondary)] hover:text-[var(--text-primary)] disabled:opacity-60"
                    >
                      {bringing.value ? "Waiting for your Vault.." : "Sign in to bring your AIs here."}
                    </button>
                  ) : (
                    <span>Unlock your Vault, then sign in to bring your AIs here.</span>
                  )}
                  {bringError.value && <span class="block mt-1 text-red-400">{bringError.value}</span>}
                </p>
              )}

              <div class={`grid gap-3 sm:grid-cols-2 mb-6 max-w-2xl ${fromAccount.value ? "hidden" : ""}`}>
                {(["work", "personal"] as DefaultAiPreset[]).map((k) => {
                  const on = preset.value === k;
                  return (
                    <button
                      key={k}
                      type="button"
                      disabled={applying.value}
                      aria-pressed={on}
                      data-testid="welcome-preset-option"
                      data-preset={k}
                      onClick$={() => { pickedByHand.value = true; return applyPreset$(k); }}
                      class={`text-left rounded-xl border-2 p-4 transition hover:border-[var(--text-primary)] ${
                        on ? "border-[var(--bg-button-primary)] bg-[var(--bg-card)]" : "border-[var(--border-subtle)]"
                      }`}
                    >
                      <div class="flex items-center justify-between">
                        <span class="font-varela font-bold text-base">{DEFAULT_AI_PRESET_LABELS[k].title}</span>
                        {on && <LuCheck class="w-5 h-5 text-green-500" />}
                      </div>
                      <p class="mt-1 text-sm text-[var(--text-secondary)]">{DEFAULT_AI_PRESET_LABELS[k].blurb}</p>
                    </button>
                  );
                })}
              </div>

              <div class="grid gap-4 sm:grid-cols-3">
                {slotIds.value.map((id) => {
                  const ai = aiData.userDefinedAis.find((a) => a.id === id);
                  if (!ai) return null;
                  const arch = bundledArchetypes.find((a) => a.id === ai.baseArchetypeId);
                  const thumb = aiData.thumbnailObjectUrls[id] || arch?.thumbnailPath || "/generic-ai-placeholder.svg";
                  return (
                    <div key={id} data-testid="welcome-ai-card" data-ai-id={id} class="rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-card)] p-4 flex flex-col gap-3">
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
                      <button
                        type="button"
                        data-testid="welcome-personality-change"
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
                Click Edit on the Your AIs page to change these and other options later.
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
              <FlowstaWizardStep />
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
            <p class="text-xs text-[var(--text-muted)] mb-4">Six to start with. The rest, and your own, live on the Your AIs page.</p>
            <div class="grid gap-3 sm:grid-cols-2">
              {personalities.map((a) => {
                const current = aiData.userDefinedAis.find((x) => x.id === personaPickerFor.value)?.baseArchetypeId === a.id;
                return (
                  <button
                    key={a.id}
                    type="button"
                    data-testid="welcome-personality-option"
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
