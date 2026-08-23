/**
 * Model Downloader Component (Qwik)
 *
 * Allows users to browse and download AI models from Hugging Face.
 * Shows recommendations based on system RAM and provides progress tracking.
 *
 * UX: Models grouped by category with badges, progressive disclosure of
 * technical details, and a "Recommended for You" highlight.
 */

import {
  component$,
  useStore,
  useSignal,
  useVisibleTask$,
  $,
} from '@builder.io/qwik';
import {
  modelFamilies,
  VISION_PROJECTORS,
  type ModelVariant,
  type ModelFamily,
  getBestVariantForSystem,
  isVariantSuitable,
  isFamilyRunnable,
  getGPUStatus,
  getModality,
  getRunMode,
  formatContext,
  type Capability,
  traitInfo,
  capabilityInfo,
} from '../data/recommended-models';
import { modelManager, type DownloadProgress } from '../utils/modelManager';
import ConfirmModal from './ConfirmModal';
import {
  ensureMedicalModel,
  getMedicalModel,
  isMedicalSpecialist,
  medicalPromptDone,
  pinCurrentNonSpecialist,
  setMedicalModel,
  setMedicalPromptDone,
} from '../utils/medicalModel';
import {
  LuHardDriveDownload,
  LuCheck,
  LuTrash2,
  LuChevronDown,
  LuX,
  LuPlus,
  LuInfo,
  LuPauseCircle,
  LuPlayCircle,
  LuCopy,
} from '@qwikest/icons/lucide';
import { getPausedModels, setModelPaused } from '../utils/modelPrefs';
import { getModelSpeeds } from '../utils/modelSpeed';
import { LiquidMetalBorder } from './LiquidMetalBorder';
import LiquidMetalButton from './LiquidMetalButton';
import { Callout } from './Callout';
import type { LocalModel } from '../types';
import { formatModelDisplayName } from '../utils/modelNameFormatter';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import DeleteModelModal from './DeleteModelModal';
import CustomModelModal from './CustomModelModal';
import { useAiData, useAiDataActions } from '../contexts/AiDataContext';

/** Shape persisted to localStorage while a download is in progress */
interface ActiveDownload {
  familyId: string;       // model family id, or 'custom'
  familyName: string;
  /** The file currently downloading (the model, then its vision projector). */
  filename: string;
  /** 'vision' while the paired projector downloads after the model itself. */
  stage?: 'model' | 'vision';
  /** The chat model to load at finalize - `filename` during the vision stage
   *  is the projector, which must never be loaded as a model. */
  modelFilename?: string;
  isFirstModel: boolean;
  startedAt: number;
}

/** In-flight downloads keyed by family id, persisted so the cards reattach
 *  after navigating away. Several can run at once (the backend downloads
 *  each file independently and reports progress per file). */
const ACTIVE_DOWNLOADS_KEY = 'activeModelDownloads';
/** The pre-0.5.2 single-slot record; folded into the map once, then gone. */
const LEGACY_ACTIVE_DOWNLOAD_KEY = 'activeModelDownload';

function readActiveDownloads(): Record<string, ActiveDownload> {
  let map: Record<string, ActiveDownload> = {};
  try {
    map = JSON.parse(localStorage.getItem(ACTIVE_DOWNLOADS_KEY) || '{}') || {};
  } catch {
    map = {};
  }
  const legacy = localStorage.getItem(LEGACY_ACTIVE_DOWNLOAD_KEY);
  if (legacy) {
    try {
      const a = JSON.parse(legacy) as ActiveDownload;
      if (a?.familyId) map[a.familyId] = a;
    } catch { /* unreadable - drop it */ }
    localStorage.removeItem(LEGACY_ACTIVE_DOWNLOAD_KEY);
  }
  return map;
}

function writeActiveDownloads(map: Record<string, ActiveDownload>) {
  if (Object.keys(map).length === 0) localStorage.removeItem(ACTIVE_DOWNLOADS_KEY);
  else localStorage.setItem(ACTIVE_DOWNLOADS_KEY, JSON.stringify(map));
}

function rememberActiveDownload(a: ActiveDownload) {
  const map = readActiveDownloads();
  map[a.familyId] = a;
  writeActiveDownloads(map);
}

function forgetActiveDownload(familyId: string) {
  const map = readActiveDownloads();
  delete map[familyId];
  writeActiveDownloads(map);
}

/** The vision projector that pairs with a model file (same filename-prefix
 *  rule the loader uses), or undefined for models that do not see images. */
function projectorFor(family: ModelFamily, variant: ModelVariant) {
  if (!getModality(family).in.includes('vision')) return undefined;
  const vlc = variant.filename.toLowerCase();
  return VISION_PROJECTORS.find((p) => {
    const key = p.filename.toLowerCase().split('-mmproj')[0];
    return !!key && vlc.startsWith(key);
  });
}

/** What a card says while it downloads. A vision model is two files; say
 *  which one is in flight and count them, so the bar filling twice reads as
 *  "1 of 2, then 2 of 2" instead of a download that started over. */
function downloadLabel(
  download: { progress: DownloadProgress | null; stage: 'model' | 'vision' } | undefined,
  twoFiles: boolean,
): string {
  if (!download) return 'Downloading';
  const pct = download.progress ? ` · ${Math.round(download.progress.percent ?? 0)}%` : '';
  if (!twoFiles) return `Downloading${pct}`;
  return download.stage === 'vision'
    ? `Downloading vision support · 2 of 2${pct}`
    : `Downloading model · 1 of 2${pct}`;
}

// One finalize per file: the downloading path's own completion and the
// resume path's listener can both fire for a single download (navigate
// away and back mid-download) - the duplicate must be a no-op.
const finalizeInFlight = new Set<string>();

export interface SystemInfo {
  /** True when the GPU shares system RAM (Intel/AMD integrated) - sized as CPU. */
  gpu_integrated?: boolean;
  total_memory_gb: number;
  used_memory_gb: number;
  cpu_count: number;
  cpu_brand?: string;
  os_name: string;
  os_version: string;
  gpu_name: string | null;
  total_vram_gb: number | null;
}

interface ModelDownloaderProps {
  systemInfo: SystemInfo | null;
  onModelSelected$?: (filename: string) => void;
  onClose$?: () => void;
}

/**
 * Convert technical error messages to user-friendly messages
 */
function getUserFriendlyErrorMessage(error: unknown): string {
  const errorMessage = error instanceof Error ? error.message : String(error);

  if (errorMessage.includes('No space left on device') || errorMessage.includes('os error 28')) {
    return 'Download Failed: Please free up some disk space and try again';
  }
  if (errorMessage.includes('Network error') || errorMessage.includes('Failed to fetch')) {
    return 'Download Failed: Network connection error. Please check your internet connection';
  }
  if (errorMessage.includes('Permission denied') || errorMessage.includes('os error 13')) {
    return 'Download Failed: Permission denied. Please check file permissions';
  }
  if (errorMessage.includes('401') || errorMessage.includes('403') || errorMessage.includes('Unauthorized')) {
    return 'Download Failed: The model file is not accessible. This model may have been removed or requires authentication';
  }
  if (errorMessage.includes('404') || errorMessage.includes('Not Found')) {
    return 'Download Failed: Model file not found. The download link may be broken';
  }
  return errorMessage.startsWith('Download Failed:') ? errorMessage : `Download Failed: ${errorMessage}`;
}

export const ModelDownloader = component$<ModelDownloaderProps>(({ systemInfo }) => {
  const aiData = useAiData();
  const {
    updateAllAisWithFirstModel,
    updateCustomAi,
    refreshUserAis,
  } = useAiDataActions();

  const store = useStore({
    downloadedModels: [] as LocalModel[],
    // Per downloaded model: how it fits THIS machine (grade, trained vs
    // actual runtime context, agent-capable template). Keyed by filename.
    modelFits: {} as Record<
      string,
      {
        fit: 'green' | 'yellow' | 'red';
        context_max: number;
        context_runtime: number;
        agent_template_ok: boolean;
        need_gb: number;
        moe_offload?: boolean;
      }
    >,
    appVersion: '',
    /** The GPU exists but cannot run models (safe mode, or the engine's
     *  own device verdict) - every sizing decision on this page then plans
     *  for the processor instead. */
    gpuUnusable: false,
    /** Beta-build-only escape hatch: lets us download/test models the fit
     *  check would refuse (e.g. Muse 30B on an 8GB card - partial offload).
     *  Gated on the version string containing "beta" (or dev), so it
     *  REMOVES ITSELF from stable release builds. Manual downloads only -
     *  auto-routing fit picks are untouched. */
    isBetaBuild: false,
    betaFitOverride: false,
    engineBackend: '',
    systemInfoCopied: false,
    /** In-flight downloads keyed by family id ('custom' for a pasted URL).
     *  Each card shows its own progress and only its own button locks -
     *  downloads run side by side. `stage` says which file is in flight:
     *  the model, or the vision projector that auto-follows a vision model
     *  (so the second file doesn't read as a stalled or repeated download). */
    downloads: {} as Record<string, { progress: DownloadProgress | null; stage: 'model' | 'vision' }>,
    error: null as string | null,
    modelsDirectory: '',
    successMessage: null as string | null,
    // A medical specialist just finished downloading and the user's health
    // model is something else: ask ONCE whether to switch (visible choice,
    // never a silent hijack).
    medicalOffer: null as string | null,
    selectedVariants: {} as Record<string, ModelVariant>,
    deleteModalOpen: false,
    modelToDelete: null as { filename: string; displayName: string } | null,
    /** familyId awaiting license agreement before its download proceeds. */
    licensePrompt: null as string | null,
    isDeleting: false,
    customModelModalOpen: false,
    openDropdowns: {} as Record<string, boolean>,
    expandedDetails: {} as Record<string, boolean>,
    selectedTask: 'all' as 'all' | Capability | 'vision',
    sortBy: 'new' as 'new' | 'name-asc' | 'name-desc' | 'size-asc' | 'size-desc',
    sortOpen: false,
    showTooBig: false,
    pausedModels: [] as string[],
    /** This machine's measured generation speed per model file (tok/s). */
    modelSpeeds: {} as Record<string, number>,
    // True until the first listModels() resolves, so the section can show a spinner
    // instead of popping in late. Only the initial load flips this (set in finally).
    loadingModels: true,
    /** The user's chosen health-questions model (mirror of medicalModel.ts). */
    medicalChoice: null as string | null,
    /** Downloaded-models inventory expanded/collapsed. */
    inventoryOpen: true,
  });

  const successBannerRef = useSignal<HTMLDivElement>();

  // Initialize selected variants based on system capabilities
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    // A GPU in safe mode (or with the engine's own unusable-device verdict)
    // must size as CPU, or every card and badge plans for hardware that
    // will never run a model. Resolved BEFORE the variant math below.
    try {
      const s = await invoke<{ active: boolean; device_unsupported?: string | null }>(
        'gpu_safe_mode_status',
      );
      store.gpuUnusable = s.active || !!s.device_unsupported;
    } catch {
      /* GPU sizing stands */
    }
    const totalRAM = systemInfo?.total_memory_gb || 8;
    // Integrated graphics share system RAM - size them as CPU, not as a card.
    const totalVRAM = store.gpuUnusable
      ? null
      : systemInfo?.gpu_integrated ? null : (systemInfo?.total_vram_gb || null);
    const freeRAM = systemInfo ? Math.max(1, systemInfo.total_memory_gb - systemInfo.used_memory_gb) : null;

    const initialVariants: Record<string, ModelVariant> = {};
    modelFamilies.forEach((family) => {
      const bestVariant = getBestVariantForSystem(family, totalRAM, totalVRAM, freeRAM);
      if (bestVariant) {
        initialVariants[family.id] = bestVariant;
      } else if (family.variants.length > 0) {
        initialVariants[family.id] = family.variants[0];
      }
    });
    store.selectedVariants = initialVariants;
  });

  // Load downloaded models and models directory
  const loadModels = $(async () => {
    store.medicalChoice = getMedicalModel();
    try {
      // The inventory lists damaged files too (so they can be deleted);
      // every other surface reads the usable list.
      const models = await modelManager.listAllModels();
      store.downloadedModels = models;
      store.modelSpeeds = getModelSpeeds();
    } catch (error) {
      console.error('Failed to load models:', error);
    } finally {
      store.loadingModels = false;
    }
    // Fit assessment for the downloaded cards (grade + runtime context).
    // Separate and non-blocking: the cards render immediately and the fit
    // line fills in when the probe (a --list-devices run) answers.
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const fits = await invoke<
        {
          name: string;
          fit: 'green' | 'yellow' | 'red';
          context_max: number;
          context_runtime: number;
          agent_template_ok: boolean;
          need_gb: number;
          moe_offload?: boolean;
        }[]
      >('assess_model_fit');
      store.modelFits = Object.fromEntries(fits.map((f) => [f.name, f]));
    } catch (e) {
      console.warn('Fit assessment failed:', e);
    }
  });

  const getModelsDir = $(async () => {
    try {
      const dir = await modelManager.getModelsDirectory();
      store.modelsDirectory = dir;
    } catch (error) {
      console.error('Failed to get models directory:', error);
    }
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(() => {
    loadModels();
    getModelsDir();
    store.pausedModels = [...getPausedModels()];
    store.modelSpeeds = getModelSpeeds();
    import('@tauri-apps/api/app').then(({ getVersion }) =>
      getVersion().then((v) => {
        store.appVersion = v;
        store.isBetaBuild = v.includes('beta') || import.meta.env.DEV;
        if (store.isBetaBuild) {
          store.betaFitOverride =
            localStorage.getItem('beta-fit-override') === 'on';
        }
      }).catch(() => {}),
    );
    invoke<{ installed: boolean; active_backend: 'bundled' | 'cuda' }>('engine_status')
      .then((e) => {
        store.engineBackend =
          e.active_backend === 'cuda'
            ? 'CUDA (downloaded engine)'
            : 'Bundled (Vulkan / Metal)';
      })
      .catch(() => {});
  });

  /** Privacy-safe support snapshot: everything model-fit depends on, and
   *  deliberately nothing personal - no folder paths (they contain the
   *  account name), no identifiers of any kind. */
  const copySystemInfo = $(async () => {
    // Computed HERE, never captured: the component-body totalRAM/freeRAM
    // consts are declared AFTER this QRL, and a $() closure over a const
    // declared later compiles clean but throws "Can't find variable" at
    // runtime - which is why this button did nothing on every platform.
    const totalRAM = systemInfo?.total_memory_gb || 8;
    const freeRAM = systemInfo
      ? Math.max(1, systemInfo.total_memory_gb - systemInfo.used_memory_gb)
      : null;
    const lines = [
      `Your Own AI ${store.appVersion || 'unknown version'}`,
      `OS: ${systemInfo ? `${systemInfo.os_name} ${systemInfo.os_version}` : 'unknown'}`,
      `CPU: ${systemInfo?.cpu_brand || 'unknown'} (${systemInfo?.cpu_count ?? '?'} cores)`,
      `Memory: ${totalRAM.toFixed(1)}GB total${freeRAM !== null ? `, ${freeRAM.toFixed(1)}GB free` : ''}`,
      `Graphics: ${
        systemInfo?.gpu_name
          ? systemInfo.gpu_integrated
            ? `${systemInfo.gpu_name} (integrated, shares system memory)`
            : `${systemInfo.gpu_name}${systemInfo.total_vram_gb ? ` (${systemInfo.total_vram_gb.toFixed(1)}GB VRAM)` : ''}`
          : 'none detected'
      }`,
    ];
    if (store.engineBackend) lines.push(`Engine: ${store.engineBackend}`);
    lines.push(
      `Downloaded models (${store.downloadedModels.length}): ${
        store.downloadedModels.map((m) => m.name).join(', ') || 'none'
      }`,
    );
    try {
      // Rust-side copy: navigator.clipboard is denied by Windows WebView2,
      // which made this button silently do nothing there.
      const { copyText } = await import('../utils/clipboard');
      await copyText(lines.join('\n'));
      store.systemInfoCopied = true;
      setTimeout(() => (store.systemInfoCopied = false), 2000);
    } catch (e) {
      console.warn('[ModelDownloader] copy failed:', e);
    }
  });

  // Pause/resume a downloaded model. Paused models stay on disk but are hidden
  // from the AI model picker (Your AIs). Mirrors pausing an AI.
  const handleTogglePause$ = $((filename: string) => {
    const paused = !store.pausedModels.includes(filename);
    setModelPaused(filename, paused);
    store.pausedModels = paused
      ? [...store.pausedModels, filename]
      : store.pausedModels.filter((n) => n !== filename);
  });

  // Post-download setup: load model, update AIs if first model, show success
  const finalizeDownload = $(async (familyId: string, filename: string, displayName: string, isFirstModel: boolean) => {
    if (finalizeInFlight.has(filename)) return;
    finalizeInFlight.add(filename);
    try {
    await loadModels();

    if (isFirstModel) {
      try {
        await updateAllAisWithFirstModel(filename);
        await refreshUserAis();
      } catch (e) {
        console.error('[ModelDownloader] Failed to update AIs:', e);
      }
    }

    try {
      const { loadModelBounded } = await import('../utils/loadModelBounded');
      await loadModelBounded({ filename, withVision: false, reason: "post-download" });
    } catch (e) {
      console.error('[ModelDownloader] Failed to load model:', e);
    }

    store.successMessage = displayName;
    if (isMedicalSpecialist(filename) && !medicalPromptDone()) {
      const current = getMedicalModel() ?? (await ensureMedicalModel());
      if (!current || !isMedicalSpecialist(current)) {
        store.medicalOffer = filename;
      } else {
        setMedicalPromptDone();
      }
    }
    localStorage.setItem('completedModelDownload', JSON.stringify({ modelName: displayName, timestamp: Date.now() }));
    forgetActiveDownload(familyId);
    const { [familyId]: _finished, ...stillRunning } = store.downloads;
    store.downloads = stillRunning;

    setTimeout(() => { store.successMessage = null; }, 10000);
    } finally {
      finalizeInFlight.delete(filename);
    }
  });

  // On mount: check for completed or in-progress downloads
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ cleanup }) => {
    // 1. Show recent completion banner (user navigated away and came back after download finished)
    const completedDownload = localStorage.getItem('completedModelDownload');
    if (completedDownload) {
      try {
        const info = JSON.parse(completedDownload);
        if (Date.now() - info.timestamp < 300000) {
          store.successMessage = info.modelName;
          setTimeout(() => { store.successMessage = null; }, 10000);
        }
        localStorage.removeItem('completedModelDownload');
      } catch {
        localStorage.removeItem('completedModelDownload');
      }
    }

    // 2. Reattach to downloads still in progress (user navigated away
    //    mid-download). Stale records (older than 2 hours) are dropped.
    const activeMap = readActiveDownloads();
    const actives: ActiveDownload[] = [];
    for (const a of Object.values(activeMap)) {
      if (Date.now() - a.startedAt > 2 * 60 * 60 * 1000) forgetActiveDownload(a.familyId);
      else actives.push(a);
    }
    if (actives.length === 0) return;

    // Re-show each card's downloading state
    const reattached: typeof store.downloads = {};
    for (const a of actives) {
      reattached[a.familyId] = { progress: null, stage: a.stage === 'vision' ? 'vision' : 'model' };
    }
    store.downloads = { ...store.downloads, ...reattached };
    const byFile = new Map(actives.map((a) => [a.filename, a]));

    // Progress events are per file: route each to its card
    const unlistenProgress = listen<{ filename: string; downloaded: number; total: number; percent: number }>(
      'model-download-progress',
      (event) => {
        const a = byFile.get(event.payload.filename);
        if (!a) return;
        const current = store.downloads[a.familyId];
        if (!current) return;
        store.downloads = { ...store.downloads, [a.familyId]: { ...current, progress: event.payload } };
      }
    );

    // Completion: finalize with the MODEL file - during the vision stage
    // `filename` is the projector, which must not be loaded as a chat model.
    const unlistenComplete = listen<{ filename: string }>(
      'model-download-complete',
      (event) => {
        const a = byFile.get(event.payload.filename);
        if (a) finalizeDownload(a.familyId, a.modelFilename ?? a.filename, a.familyName, a.isFirstModel);
      }
    );

    // Files that already landed while we were away. isModelDownloaded, not
    // listModels: projectors are filtered out of the model list, so a
    // finished vision stage would never match.
    for (const a of actives) {
      modelManager.isModelDownloaded(a.filename).then((alreadyDone) => {
        if (alreadyDone) finalizeDownload(a.familyId, a.modelFilename ?? a.filename, a.familyName, a.isFirstModel);
      });
    }

    cleanup(() => {
      unlistenProgress.then(fn => fn());
      unlistenComplete.then(fn => fn());
    });
  });

  // Scroll success banner into view
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(({ track }) => {
    track(() => store.successMessage);
    if (store.successMessage && successBannerRef.value) {
      successBannerRef.value.scrollIntoView({
        behavior: 'smooth',
        block: 'start',
      });
    }
  });

  const handleDownload$ = $(async (familyId: string) => {
    const variant = store.selectedVariants[familyId];
    const family = modelFamilies.find((f) => f.id === familyId);
    if (!variant || !family) return;

    // Some publishers require accepting their terms before download (the
    // terms pass through to the user - see the family's `license`). Ask once
    // per license id; agreement is stored and covers sibling variants.
    if (
      family.license &&
      localStorage.getItem(`licenseAccepted:${family.license.id}`) !== 'true'
    ) {
      store.licensePrompt = familyId;
      return;
    }

    // "First model" = nothing usable on disk and nothing else in flight -
    // only the first download of a fresh install sets every AI's model.
    const othersInFlight = Object.keys(store.downloads).length > 0;
    store.downloads = { ...store.downloads, [familyId]: { progress: null, stage: 'model' } };
    store.error = null;
    store.successMessage = null;

    const modelsBeforeDownload = await modelManager.listModels();
    const isFirstModel = modelsBeforeDownload.length === 0 && !othersInFlight;
    const displayName = `${family.name} ${variant.parameterCount}`;

    // Persist active download so it survives page navigation
    const activeDownload: ActiveDownload = {
      familyId,
      familyName: displayName,
      filename: variant.filename,
      isFirstModel,
      startedAt: Date.now(),
    };
    rememberActiveDownload(activeDownload);

    try {
      await modelManager.downloadModel(variant.downloadUrl, variant.filename, (progress) => {
        const current = store.downloads[familyId];
        store.downloads = { ...store.downloads, [familyId]: { stage: current?.stage ?? 'model', progress } };
      });

      // A vision model is only half-installed without its projector (the "eyes").
      // Pull the matching one too — paired by the same filename-prefix rule the
      // conductor uses — so "download a vision model" actually enables vision.
      {
        const proj = projectorFor(family, variant);
        if (proj && !(await modelManager.isModelDownloaded(proj.filename))) {
          // Make the second file visible as its own stage - a fresh 0% bar
          // under the same name reads as a stalled or repeated download.
          store.downloads = { ...store.downloads, [familyId]: { progress: null, stage: 'vision' } };
          rememberActiveDownload({
            ...activeDownload,
            stage: 'vision',
            filename: proj.filename,
            modelFilename: variant.filename,
          } satisfies ActiveDownload);
          await modelManager.downloadModel(proj.downloadUrl, proj.filename, (progress) => {
            store.downloads = { ...store.downloads, [familyId]: { stage: 'vision', progress } };
          });
        }
      }

      await finalizeDownload(familyId, variant.filename, displayName, isFirstModel);
    } catch (error) {
      console.error('Download failed:', error);
      store.error = getUserFriendlyErrorMessage(error);
      forgetActiveDownload(familyId);
      const { [familyId]: _failed, ...stillRunning } = store.downloads;
      store.downloads = stillRunning;
    }
  });

  const handleCustomModelDownload$ = $(async (downloadUrl: string, filename: string) => {
    const othersInFlight = Object.keys(store.downloads).length > 0;
    store.downloads = { ...store.downloads, custom: { progress: null, stage: 'model' } };
    store.error = null;
    store.successMessage = null;

    const modelsBeforeDownload = await modelManager.listModels();
    const isFirstModel = modelsBeforeDownload.length === 0 && !othersInFlight;
    const displayName = formatModelDisplayName(filename);

    const activeDownload: ActiveDownload = {
      familyId: 'custom',
      familyName: displayName,
      filename,
      isFirstModel,
      startedAt: Date.now(),
    };
    rememberActiveDownload(activeDownload);

    try {
      await modelManager.downloadModel(downloadUrl, filename, (progress) => {
        store.downloads = { ...store.downloads, custom: { stage: 'model', progress } };
      });

      store.customModelModalOpen = false;
      await finalizeDownload('custom', filename, displayName, isFirstModel);
    } catch (error) {
      console.error('Custom model download failed:', error);
      store.error = getUserFriendlyErrorMessage(error);
      forgetActiveDownload('custom');
      const { custom: _failed, ...stillRunning } = store.downloads;
      store.downloads = stillRunning;
    }
  });

  const agreeLicense = $(async () => {
    const familyId = store.licensePrompt;
    const family = familyId ? modelFamilies.find((f) => f.id === familyId) : null;
    if (!familyId || !family?.license) return;
    localStorage.setItem(`licenseAccepted:${family.license.id}`, 'true');
    store.licensePrompt = null;
    await handleDownload$(familyId);
  });

  const openLicenseUrl = $(async (url: string) => {
    const { openUrl } = await import('@tauri-apps/plugin-opener');
    await openUrl(url);
  });

  const handleDeleteClick$ = $((filename: string) => {
    const displayName = formatModelDisplayName(filename);
    store.modelToDelete = { filename, displayName };
    store.deleteModalOpen = true;
  });

  const handleDeleteConfirm$ = $(async () => {
    if (!store.modelToDelete) return;

    store.isDeleting = true;
    try {
      const deletedFilename = store.modelToDelete.filename;

      const affectedAis = aiData.userDefinedAis.filter(
        (ai) => ai.model === deletedFilename
      );
      const affectedAiNames = affectedAis.map((ai) => ai.name);

      await modelManager.deleteModel(deletedFilename);
      await loadModels();

      const storedModel = localStorage.getItem('currentModel');
      if (storedModel === deletedFilename) {
        localStorage.removeItem('currentModel');
        window.dispatchEvent(
          new CustomEvent('modelDeleted', { detail: { filename: deletedFilename } })
        );
      }

      const remainingModels = store.downloadedModels.filter((m) => m.name !== deletedFilename);
      const fallbackModel = remainingModels[0]?.name || null;

      if (fallbackModel && affectedAis.length > 0) {
        for (const ai of affectedAis) {
          await updateCustomAi(ai.id, { model: fallbackModel });
        }

        const aiList =
          affectedAiNames.length <= 3
            ? affectedAiNames.join(', ')
            : `${affectedAiNames.slice(0, 3).join(', ')} and ${affectedAiNames.length - 3} more`;

        store.successMessage = `Model deleted successfully. Updated ${affectedAiNames.length} AI${affectedAiNames.length === 1 ? '' : 's'} (${aiList}) to use ${formatModelDisplayName(fallbackModel)}.`;
      } else if (affectedAis.length > 0 && !fallbackModel) {
        for (const ai of affectedAis) {
          await updateCustomAi(ai.id, { model: '' });
        }
        store.successMessage = `Model deleted successfully. ${affectedAiNames.length} AI${affectedAiNames.length === 1 ? ' has' : 's have'} been reset. Please download a new model.`;
      } else {
        store.successMessage = `Model deleted successfully.`;
      }

      if (affectedAiNames.length > 0) {
        setTimeout(() => {
          store.successMessage = null;
        }, 10000);
      }

      store.deleteModalOpen = false;
      store.modelToDelete = null;
    } catch (error) {
      console.error('Delete failed:', error);
      store.error = 'Failed to delete model: ' + error;
    } finally {
      store.isDeleting = false;
    }
  });

  const handleDeleteCancel$ = $(() => {
    store.deleteModalOpen = false;
    store.modelToDelete = null;
  });

  const totalRAM = systemInfo?.total_memory_gb || 8;
  // Integrated graphics share system RAM - size them as CPU, not as a card.
  // An unusable GPU (safe mode / device verdict) sizes as CPU too.
  const totalVRAM = store.gpuUnusable
    ? null
    : systemInfo?.gpu_integrated ? null : (systemInfo?.total_vram_gb || null);
  const freeRAM = systemInfo ? Math.max(1, systemInfo.total_memory_gb - systemInfo.used_memory_gb) : null;
  // A damaged file is not a download - its card offers to fetch it again.
  const downloadedFilenames = new Set(
    store.downloadedModels.filter((m) => !m.damaged).map((m) => m.name),
  );
  // Task filters — the capability axis users actually shop by ("what's it for").
  // Size is handled separately by the runnable / "needs more memory" split below,
  // so these tabs don't need to repeat it. Order = most-shopped first.
  const TASK_FILTERS: Capability[] = ['coding', 'reasoning', 'agentic', 'writing', 'math'];
  // A family matches a tab by capability — except 'vision', which is a modality
  // (the model sees images), not a "good at" capability.
  const matchesTask = (f: ModelFamily, task: 'all' | Capability | 'vision') =>
    task === 'all' ? true
      : task === 'vision' ? getModality(f).in.includes('vision')
      : f.capabilities.includes(task);
  const SORT_OPTIONS = [
    { key: 'new', label: 'Newest' },
    { key: 'name-asc', label: 'A–Z' },
    { key: 'name-desc', label: 'Z–A' },
    { key: 'size-asc', label: 'Smallest' },
    { key: 'size-desc', label: 'Largest' },
  ] as const;

  // Find the single best recommended family + variant for this system.
  // Prefers models that fit fully in VRAM (fastest), then largest suitable.
  const bestPick = (() => {
    const candidates = modelFamilies
      .filter(f => f.recommended && getBestVariantForSystem(f, totalRAM, totalVRAM, freeRAM) !== null)
      .map(f => {
        const variant = getBestVariantForSystem(f, totalRAM, totalVRAM, freeRAM)!;
        const gpu = getGPUStatus(variant, totalVRAM);
        return { family: f, variant, gpu };
      });

    // Sort: full GPU first, then by size descending within each group
    candidates.sort((a, b) => {
      if (a.gpu.isFull !== b.gpu.isFull) return a.gpu.isFull ? -1 : 1;
      return b.variant.size - a.variant.size;
    });

    return candidates[0] || null;
  })();

  // "Best for this computer" - one honest pick per activity. Candidates
  // are fit-filtered FIRST (getBestVariantForSystem returns null when
  // nothing fits), so a small machine is never sold a model it can't
  // run; ranking then follows the catalog's own relevance data.
  const catRank: Record<string, number> = { quality: 0, balanced: 1, specialist: 2, fast: 3 };
  const pickFor = (
    match: (f: ModelFamily) => boolean,
    preferId?: string,
    catOrder?: Record<string, number>,
  ) => {
    const order = catOrder ?? catRank;
    const candidates = modelFamilies
      .filter(match)
      .map(f => ({ f, v: getBestVariantForSystem(f, totalRAM, totalVRAM, freeRAM) }))
      .filter((x): x is { f: ModelFamily; v: ModelVariant } => x.v !== null);
    candidates.sort((a, b) => {
      if (preferId) {
        const ap = a.f.id === preferId ? 0 : 1;
        const bp = b.f.id === preferId ? 0 : 1;
        if (ap !== bp) return ap - bp;
      }
      const ac = order[a.f.category] ?? 9;
      const bc = order[b.f.category] ?? 9;
      if (ac !== bc) return ac - bc;
      const ar = a.f.released ?? '', br = b.f.released ?? '';
      if (ar !== br) return ar > br ? -1 : 1;
      return a.f.name.localeCompare(b.f.name);
    });
    return candidates[0] ?? null;
  };
  const medicalPick =
    pickFor(f => f.capabilities.includes('medical'), 'medgemma',
      { specialist: 0, quality: 1, balanced: 2, fast: 3 }) ??
    pickFor(f => f.capabilities.includes('reasoning') || f.capabilities.includes('chat'));
  const medicalIsSpecialist = medicalPick !== null && medicalPick.f.capabilities.includes('medical');
  const activityPicks = [
    {
      key: 'coding',
      title: 'Codes with you',
      blurb: 'The strongest project agent that fits this machine.',
      pick: pickFor(f => f.capabilities.includes('agentic') || f.capabilities.includes('coding')),
    },
    {
      key: 'chat',
      title: 'Everyday chat',
      blurb: 'The best all-rounder for your memory.',
      pick: pickFor(f => f.capabilities.includes('chat')),
    },
    {
      key: 'vision',
      title: 'Sees images',
      blurb: 'Reads screenshots and documents you attach.',
      pick: pickFor(f => getModality(f).in.includes('vision')),
    },
    {
      key: 'medical',
      title: 'Health questions',
      blurb: medicalIsSpecialist
        ? 'A medical specialist - and health questions always stay on your device.'
        : 'The medical specialist needs more memory than this machine has - this is your strongest general model instead.',
      pick: medicalPick,
    },
  ].filter(a => a.pick !== null);

  // One delegated handler for the panel (Qwik closures inside .map are
  // unreliable - same data-attr pattern as the variant picker below).
  const panelAction$ = $(async (e: Event) => {
    const el = (e.target as HTMLElement).closest('[data-pick-action]') as HTMLElement | null;
    if (!el) return;
    const action = el.getAttribute('data-pick-action');
    const familyId = el.getAttribute('data-pick-family') ?? '';
    const filename = el.getAttribute('data-pick-file') ?? '';
    if (action === 'download') {
      const family = modelFamilies.find(f => f.id === familyId);
      const variant = family?.variants.find(v => v.filename === filename);
      if (!family || !variant) return;
      store.selectedVariants[familyId] = variant;
      await handleDownload$(familyId);
    } else if (action === 'set-medical') {
      await setMedicalModel(filename);
      setMedicalPromptDone();
      store.medicalChoice = filename;
    }
  });

  // Families for the current tab, in the user's chosen sort order. 'Size' uses the
  // smallest variant (the floor to run it). 'Newest' floats New-tagged models up,
  // then alphabetical. The runnable / "needs more memory" split happens after.
  const familyMinSize = (f: ModelFamily) => Math.min(...f.variants.map(v => v.size));
  const visibleFamilies = modelFamilies
    .filter(f => matchesTask(f, store.selectedTask))
    .sort((a, b) => {
      switch (store.sortBy) {
        case 'name-asc': return a.name.localeCompare(b.name);
        case 'name-desc': return b.name.localeCompare(a.name);
        case 'size-asc': return familyMinSize(a) - familyMinSize(b);
        case 'size-desc': return familyMinSize(b) - familyMinSize(a);
        case 'new':
        default: {
          // Newest release first (ISO dates compare lexically); undated last, then A–Z.
          const ad = a.released ?? '', bd = b.released ?? '';
          if (ad !== bd) return ad > bd ? -1 : 1;
          return a.name.localeCompare(b.name);
        }
      }
    });

  // Split into what this machine can run vs. what needs more memory. The latter
  // is grouped under a collapsible divider (in-place), not just greyed at the
  // bottom — so the action and its effect sit in the same spot.
  const runnableFamilies = visibleFamilies.filter(f => isFamilyRunnable(f, totalRAM, totalVRAM, freeRAM));
  const tooBigFamilies = visibleFamilies.filter(f => !isFamilyRunnable(f, totalRAM, totalVRAM, freeRAM));


  /** Render a single model card */
  // Variant picker writes through data-attrs + closest(), NOT an inline closure:
  // Qwik's `$()` closures inside `.map()` don't reliably capture loop variables
  // (here NESTED maps — family × variant), so an inline handler could write the
  // wrong/stale variant and make the card look frozen. See qwik-pitfalls.md
  // "Event handling in `.map()` loops".
  const selectVariant$ = $((e: Event) => {
    const li = (e.target as HTMLElement).closest('[data-variant-file]') as HTMLElement | null;
    if (!li) return;
    const familyId = li.dataset.familyId;
    const file = li.dataset.variantFile;
    if (!familyId || !file) return;
    const family = modelFamilies.find((f) => f.id === familyId);
    const variant = family?.variants.find((v) => v.filename === file);
    if (!variant) return;
    store.selectedVariants = { ...store.selectedVariants, [familyId]: variant };
    store.openDropdowns = { ...store.openDropdowns, [familyId]: false };
  });

  // NB: `selectedVariant` is passed IN (read from the store at the .map call site in
  // JSX), NOT read from the store here. Qwik only tracks reactive reads that happen
  // inside a JSX expression; a read inside this nested helper isn't tracked, so the
  // card wouldn't re-render on a variant pick. Reading it at the call site fixes that.
  const renderModelCard = (family: ModelFamily, selectedVariant: ModelVariant | undefined) => {
    if (!selectedVariant) return null;

    const isDownloaded = downloadedFilenames.has(selectedVariant.filename);
    const download = store.downloads[family.id];
    const isDownloading = !!download;
    const projector = projectorFor(family, selectedVariant);
    const isSuitable = isVariantSuitable(selectedVariant, totalRAM, totalVRAM, freeRAM);
    const runMode = getRunMode(selectedVariant, totalRAM, totalVRAM, freeRAM);
    const isExpanded = store.expandedDetails[family.id];

    // "Best for you" only shows when the selected variant matches the recommended one
    const isBestPick = bestPick !== null
      && bestPick.family.id === family.id
      && bestPick.variant.filename === selectedVariant.filename;

    return (
      <div
        key={family.id}
        class={`generic-container rounded-2xl overflow-hidden flex flex-col justify-between transition-all hover:shadow-2xl transform hover:-translate-y-1 ${
          !isSuitable ? 'opacity-75' : ''
        }`}
      >
        {/* One consistent vertical rhythm for the whole card body (space-y) instead of
            ad-hoc per-element margins, which drifted and looked squished. */}
        <div class="p-5 space-y-3.5">
          {/* Header: name on its own line, badges wrapping on a row below — so a long
              name never squishes the badges or pushes them off the card. */}
          <div>
            <h3 class="text-lg font-semibold text-[var(--text-primary)] leading-tight">{family.name}</h3>
            <div class="flex flex-wrap gap-1 mt-2.5">
              {!isSuitable && (
                <span class="px-2 py-0.5 bg-amber-500/15 text-amber-700 dark:text-amber-400 border border-amber-500/40 text-[10px] rounded-full font-semibold whitespace-nowrap">
                  {totalVRAM && totalVRAM > 0
                    ? "Too big for your GPU"
                    : systemInfo?.gpu_integrated
                      ? "Too big for your memory"
                      : `Needs ${selectedVariant.minRAM}GB RAM`}
                </span>
              )}
              {isSuitable && runMode === 'gpu' && (
                <span class="px-2 py-0.5 bg-emerald-500/15 text-emerald-700 dark:text-emerald-400 border border-emerald-500/40 text-[10px] rounded-full font-semibold whitespace-nowrap">
                  Your GPU
                </span>
              )}
              {isSuitable && runMode === 'moe-split' && (
                <span
                  title="Bigger than your graphics card's memory. The model's less-used parts stay in main memory while the rest runs on the card - fast for its size."
                  class="px-2 py-0.5 bg-emerald-500/15 text-emerald-700 dark:text-emerald-400 border border-emerald-500/40 text-[10px] rounded-full font-semibold whitespace-nowrap"
                >
                  Runs here - split with main memory
                </span>
              )}
              {isSuitable && runMode === 'cpu' && systemInfo?.gpu_integrated && (
                <span
                  title="Your graphics share system memory, so this model runs on the processor"
                  class="px-2 py-0.5 bg-sky-500/15 text-sky-700 dark:text-sky-400 border border-sky-500/40 text-[10px] rounded-full font-semibold whitespace-nowrap"
                >
                  Integrated graphics
                </span>
              )}
              {isBestPick && (
                <span class="px-2 py-0.5 bg-[var(--bg-button-primary)] text-[var(--text-button-primary)] text-[10px] rounded-full font-semibold whitespace-nowrap">
                  Best for you
                </span>
              )}
              {family.traits.map(trait => {
                const info = traitInfo[trait];
                if (!info) return null;
                return (
                  <span
                    key={trait}
                    title={info.description}
                    class={`px-1.5 py-0.5 ${info.color} text-white text-[10px] rounded-full font-semibold whitespace-nowrap`}
                  >
                    {info.label}
                  </span>
                );
              })}
            </div>
          </div>

          {/* Description — full, not clamped (curated + short; truncating them read as broken).
              min-h keeps cards in a row roughly aligned when descriptions differ in length. */}
          <p class="text-sm text-[var(--text-secondary)] min-h-[2.5rem]">
            {family.description}
          </p>

          {/* Provenance - who made the weights, who packaged them, what a
              derivative is based on. Records deserve to know their maker. */}
          {family.maker && (
            <p class="text-xs text-[var(--text-muted)] -mt-1">
              {family.community
                ? `Community build by ${family.maker}${family.derivedFrom ? ` - ${family.derivedFrom.toLowerCase().charAt(0) + family.derivedFrom.slice(1)}` : ''}`
                : `Official ${family.maker} weights${family.quantizedBy ? `, packaged by ${family.quantizedBy}` : ''}${family.derivedFrom ? ` - ${family.derivedFrom.toLowerCase().charAt(0) + family.derivedFrom.slice(1)}` : ''}`}
            </p>
          )}

          {/* Size variant dropdown */}
          {family.variants.length > 1 && (
            <div>
              <LiquidMetalBorder borderRadius="9999px">
                <div class="relative">
                  <button
                    type="button"
                    class="relative w-full cursor-default rounded-full bg-[var(--bg-input)] py-2 pl-3 pr-10 text-left text-[var(--text-primary)] focus:outline-none disabled:opacity-50 gradient-border-target text-sm"
                    disabled={isDownloading}
                    onClick$={() => {
                      store.openDropdowns = {
                        ...store.openDropdowns,
                        [family.id]: !store.openDropdowns[family.id],
                      };
                    }}
                  >
                    <span class="block truncate">
                      {selectedVariant.parameterCount} ({selectedVariant.size}GB download)
                    </span>
                    <span class="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-3">
                      <LuChevronDown class="h-4 w-4 text-[var(--text-muted)]" aria-hidden="true" />
                    </span>
                  </button>
                  {store.openDropdowns[family.id] && (
                    <>
                      <div class="fixed inset-0 z-[5]" onClick$={() => {
                        store.openDropdowns = { ...store.openDropdowns, [family.id]: false };
                      }} />
                      <ul class="absolute z-10 mt-1 max-h-60 w-full overflow-auto rounded-2xl bg-[var(--bg-card)] py-1 shadow-lg ring-1 ring-black ring-opacity-5 focus:outline-none text-sm">
                        {family.variants.map((variant, idx) => {
                          const variantSuitable = isVariantSuitable(variant, totalRAM, totalVRAM, freeRAM);
                          return (
                            <li
                              key={idx}
                              data-family-id={family.id}
                              data-variant-file={variant.filename}
                              class={`relative cursor-default select-none py-2 pl-10 pr-4 hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-primary)] ${
                                selectedVariant.filename === variant.filename
                                  ? 'bg-[var(--bg-dropdown-hover)] text-[var(--text-primary)]'
                                  : 'text-[var(--text-dropdown)]'
                              } ${!variantSuitable ? 'opacity-40' : ''}`}
                              onClick$={selectVariant$}
                            >
                              <span
                                class={`block truncate ${
                                  selectedVariant.filename === variant.filename ? 'font-medium' : 'font-normal'
                                }`}
                              >
                                {variant.parameterCount} ({variant.size}GB)
                                {!variantSuitable && ' — needs more RAM'}
                              </span>
                              {selectedVariant.filename === variant.filename && (
                                <span class="absolute inset-y-0 left-0 flex items-center pl-3 text-[var(--text-link)]">
                                  <LuCheck class="h-4 w-4" aria-hidden="true" />
                                </span>
                              )}
                            </li>
                          );
                        })}
                      </ul>
                    </>
                  )}
                </div>
              </LiquidMetalBorder>
            </div>
          )}

          {/* Capability chips — what it's good at (also the future routing signal) */}
          <div class="flex flex-wrap gap-1">
            {family.capabilities.slice(0, 4).map((cap) => (
              <span
                key={cap}
                class="px-2 py-0.5 bg-[var(--bg-dropdown)] border border-[var(--border-subtle)] rounded text-xs text-[var(--text-primary)]"
              >
                {capabilityInfo[cap].label}
              </span>
            ))}
            {(selectedVariant.contextWindow || family.contextWindow) && (
              <span
                class="px-2 py-0.5 bg-[var(--bg-main)] border border-[var(--border-subtle)] rounded text-xs text-[var(--text-muted)]"
                title="Model's trained context window. Your Own AI picks the actual runtime context from your graphics card and memory - downloaded models show it as 'runs at'."
              >
                {formatContext(selectedVariant.contextWindow || family.contextWindow!)} context
              </span>
            )}
          </div>

          {/* Expandable technical details — button + panel grouped so they stay tight
              (one rhythm unit; the panel hugs its toggle rather than floating away). */}
          <div>
          <button
            type="button"
            class="flex items-center gap-1 text-xs text-[var(--text-muted)] hover:text-[var(--text-secondary)] transition-colors"
            onClick$={() => {
              store.expandedDetails = {
                ...store.expandedDetails,
                [family.id]: !isExpanded,
              };
            }}
          >
            <LuInfo class="w-3 h-3" />
            {isExpanded ? 'Hide details' : 'Technical details'}
          </button>

          {isExpanded && (
            <div class="mt-2 space-y-1 text-xs text-[var(--text-muted)] bg-[var(--bg-main)] rounded-lg p-3">
              <div class="flex justify-between">
                <span>Download size</span>
                <span class="font-medium text-[var(--text-secondary)]">{selectedVariant.size}GB</span>
              </div>
              <div class="flex justify-between">
                <span>Parameters</span>
                <span class="font-medium text-[var(--text-secondary)]">{selectedVariant.parameterCount}</span>
              </div>
              {(selectedVariant.contextWindow || family.contextWindow) && (
                <div class="flex justify-between">
                  <span>Context window</span>
                  <span class="font-medium text-[var(--text-secondary)]">{formatContext(selectedVariant.contextWindow || family.contextWindow!)} tokens</span>
                </div>
              )}
              <div class="flex justify-between">
                <span>Quantization</span>
                <span class="font-medium text-[var(--text-secondary)]">{selectedVariant.quantization}</span>
              </div>
              <div class="flex justify-between">
                <span>Min RAM</span>
                <span class="font-medium text-[var(--text-secondary)]">{selectedVariant.minRAM}GB</span>
              </div>
            </div>
          )}
          </div>
        </div>

        {/* Footer: download / status */}
        <div class="px-5 py-3 mt-auto">
          {isDownloaded ? (
            <div class="flex justify-center">
              <div class="flex items-center gap-2 text-green-600 dark:text-green-400 text-sm font-medium">
                <LuCheck class="w-4 h-4" />
                Downloaded
              </div>
            </div>
          ) : isDownloading ? (
            <div>
              <div class="flex items-center gap-2 mb-2 justify-center">
                <div class="w-4 h-4 border-2 border-[var(--text-primary)] border-t-transparent rounded-full animate-spin" />
                <span class="text-sm text-[var(--text-primary)]">
                  {downloadLabel(download, !!projector)}
                </span>
              </div>
              {download.progress && (
                <div>
                  <div class="w-full bg-[var(--border-subtle)] rounded-full h-2 overflow-hidden mb-1">
                    <div
                      class="bg-blue-600 h-full transition-all duration-300"
                      style={{ width: `${download.progress.percent}%` }}
                    />
                  </div>
                  <p class="text-xs text-[var(--text-muted)] text-center">
                    {modelManager.formatModelSize(download.progress.downloaded)}
                    {download.progress.total > 0
                      ? ` of ${modelManager.formatModelSize(download.progress.total)}`
                      : ''}
                  </p>
                </div>
              )}
              {download.stage === 'vision' && (
                <p class="text-xs text-[var(--text-muted)] text-center mt-1">
                  Model downloaded - now fetching the second, smaller file that
                  lets it read images.
                </p>
              )}
            </div>
          ) : (
            <div class="flex justify-end">
              <LiquidMetalButton
                onClick$={() => handleDownload$(family.id)}
                disabled={(!isSuitable && !store.betaFitOverride) || isDownloading}
                class="flex items-center gap-2 px-4 py-2 text-sm transition-colors disabled:opacity-50 disabled:cursor-not-allowed"
              >
                <LuHardDriveDownload class="w-4 h-4" />
                {projector
                  ? `Download (${selectedVariant.size} GB + ${projector.size} GB vision)`
                  : `Download (${selectedVariant.size} GB)`}
              </LiquidMetalButton>
            </div>
          )}

          {!isSuitable && (
            <p class="text-xs text-[var(--text-muted)] text-center mt-2">
              Needs {selectedVariant.minRAM}GB RAM minimum
            </p>
          )}
        </div>
      </div>
    );
  };

  return (
    <div class="max-w-7xl mx-auto px-6">
      {/* Help tip — privacy (verifiable, not trust-based) */}
      <Callout
        intent="success"
        title="Private, and verifiable"
        id="offline-private"
        iconPath="M16.5 10.5V6.75a4.5 4.5 0 10-9 0v3.75m-.75 11.25h10.5a2.25 2.25 0 002.25-2.25v-6.75a2.25 2.25 0 00-2.25-2.25H6.75a2.25 2.25 0 00-2.25 2.25v6.75a2.25 2.25 0 002.25 2.25z"
        class="mb-6"
      >
        Offline models run entirely on your device, and Your Own AI is open
        source — so this isn't "trust us." Anyone can read the code and confirm
        your conversations never leave your computer. No account or internet
        needed once a model's downloaded.
      </Callout>

      {/* Success Banner */}
      {store.successMessage && (
        <div
          ref={successBannerRef}
          class="mb-4 p-4 bg-green-50 dark:bg-green-900/30 border border-green-200 dark:border-green-700 rounded-lg flex items-start justify-between"
        >
          <div class="flex-1">
            {store.successMessage.includes('deleted') ? (
              <p class="text-sm font-medium text-green-800 dark:text-green-200">
                {store.successMessage}
              </p>
            ) : (
              <>
                <p class="text-sm font-medium text-green-800 dark:text-green-200">
                  <strong>{store.successMessage}</strong> has been downloaded and is ready to use!
                </p>
                <p class="text-sm text-green-700 dark:text-green-300 mt-1">
                  To use this model, go to the <strong>Your AIs</strong> page and edit the AI
                  you'd like to use it with.
                </p>
              </>
            )}
          </div>
          <button
            onClick$={() => { store.successMessage = null; }}
            class="ml-4 text-green-600 dark:text-green-400 hover:text-green-800 dark:hover:text-green-200 transition-colors"
            aria-label="Dismiss"
          >
            <LuX class="w-5 h-5" />
          </button>
        </div>
      )}

      {/* Error Display */}
      {store.error && (
        <div class="mb-4 p-4 bg-red-100 dark:bg-red-900 border border-red-200 dark:border-red-700 rounded-lg">
          <p class="text-sm text-red-700 dark:text-red-300">{store.error}</p>
        </div>
      )}

      {/* Best for this computer - the decision most people came to make,
          answered first: one fit-checked pick per activity. */}
      {activityPicks.length > 0 && (
        <div class="mb-8">
          <div class="flex items-baseline justify-between mb-4 border-b border-[var(--border-subtle)] pb-2">
            <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela">
              Best for this computer
            </h2>
            <span class="text-xs text-[var(--text-muted)]">
              {totalRAM.toFixed(0)} GB memory{totalVRAM ? ` · ${totalVRAM.toFixed(0)} GB graphics` : ''}
            </span>
          </div>
          <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-4 gap-4" onClick$={panelAction$}>
            {activityPicks.map((a) => {
              const { f, v } = a.pick!;
              const downloaded = downloadedFilenames.has(v.filename);
              const pickDownload = store.downloads[f.id];
              const inFlight = !!pickDownload;
              const isHealthChoice = store.medicalChoice === v.filename;
              return (
                <div
                  key={a.key}
                  class="generic-container rounded-2xl p-4 flex flex-col gap-2"
                >
                  <p class="text-[10px] font-semibold uppercase tracking-wider text-[var(--text-muted)]">
                    {a.title}
                  </p>
                  <p class="text-base font-semibold text-[var(--text-primary)] leading-tight">
                    {f.name} {v.parameterCount}
                  </p>
                  <p class="text-xs text-[var(--text-secondary)] flex-1">
                    {a.blurb} {v.size} GB.
                  </p>
                  {store.loadingModels && (
                    <p class="flex items-center justify-center gap-2 text-xs text-[var(--text-muted)] py-1.5">
                      <span class="inline-block h-3 w-3 rounded-full border-2 border-[var(--border-subtle)] border-t-[var(--text-secondary)] animate-spin" />
                      Checking your models..
                    </p>
                  )}
                  {!store.loadingModels && !downloaded && !inFlight && (
                    <span
                      class="contents"
                      data-pick-action="download"
                      data-pick-family={f.id}
                      data-pick-file={v.filename}
                    >
                      <LiquidMetalButton class="w-full px-3 py-1.5 text-sm">
                        Download
                      </LiquidMetalButton>
                    </span>
                  )}
                  {inFlight && (
                    <p class="text-xs text-[var(--text-muted)] py-1.5 text-center">
                      {downloadLabel(pickDownload, !!projectorFor(f, v))}
                    </p>
                  )}
                  {!store.loadingModels && downloaded && a.key !== 'medical' && (
                    <p class="text-xs text-emerald-500 dark:text-emerald-400 py-1.5 text-center font-medium">
                      Downloaded ✓
                    </p>
                  )}
                  {!store.loadingModels && downloaded && a.key === 'medical' && (
                    isHealthChoice ? (
                      <p class="text-xs text-emerald-500 dark:text-emerald-400 py-1.5 text-center font-medium">
                        Your health model ✓
                      </p>
                    ) : (
                      <span
                        class="contents"
                        data-pick-action="set-medical"
                        data-pick-file={v.filename}
                      >
                        <LiquidMetalButton
                          variant="secondary"
                          class="w-full px-3 py-1.5 text-sm"
                        >
                          Use for health questions
                        </LiquidMetalButton>
                      </span>
                    )
                  )}
                </div>
              );
            })}
          </div>
        </div>
      )}

      {/* Downloaded Models */}
      {(store.loadingModels || store.downloadedModels.length > 0) && (
        <div class="mb-8">
          {/* Inventory, not a showroom: compact rows (the catalog below has
              the rich cards), collapsible header with the disk total. */}
          <button
            type="button"
            onClick$={() => (store.inventoryOpen = !store.inventoryOpen)}
            class="flex w-full items-center justify-between mb-4 border-b border-[var(--border-subtle)] pb-2 bg-transparent border-x-0 border-t-0 cursor-pointer text-left"
          >
            <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela">
              Downloaded Models
              {store.loadingModels
                ? ''
                : ` (${store.downloadedModels.length}${(() => {
                    const gb = store.downloadedModels.reduce((t, m) => {
                      const n = parseFloat(String(m.size));
                      return Number.isFinite(n) ? t + n : t;
                    }, 0);
                    return gb > 0 ? ` · ${gb.toFixed(1)} GB` : '';
                  })()})`}
            </h2>
            <LuChevronDown
              class={`h-5 w-5 text-[var(--text-muted)] transition-transform ${store.inventoryOpen ? '' : '-rotate-90'}`}
            />
          </button>
          {!store.loadingModels && store.inventoryOpen &&
            Object.values(store.modelFits).some((f) => f.moe_offload) && (
            <Callout intent="info" id="moe-split" class="mb-4">
              One of your models is bigger than your graphics memory. Your Own AI
              splits the work - the model's less-used parts stay in main memory -
              so replies stay quick.
            </Callout>
          )}
          {store.loadingModels ? (
            <div class="flex items-center justify-center gap-3 py-12 text-[var(--text-secondary)]">
              <div class="w-5 h-5 border-2 border-[var(--text-muted)] border-t-transparent rounded-full animate-spin" />
              <span class="text-sm">Loading your models…</span>
            </div>
          ) : store.inventoryOpen && (
          <div class="generic-container rounded-2xl divide-y divide-[var(--border-subtle)]">
            {store.downloadedModels.map((model) => {
              const catalogMatch = modelFamilies.reduce<{ family: ModelFamily; variant: ModelVariant } | null>((found, family) => {
                if (found) return found;
                const variant = family.variants.find(v => v.filename === model.name);
                return variant ? { family, variant } : null;
              }, null);
              const displayName = catalogMatch
                ? `${catalogMatch.family.name} ${catalogMatch.variant.parameterCount}`
                : formatModelDisplayName(model.name);
              const quantization = catalogMatch?.variant.quantization || model.quantization;
              const isPaused = store.pausedModels.includes(model.name);
              const fitInfo = store.modelFits[model.name];
              const fitBadge = model.damaged
                ? {
                    label: 'Damaged file',
                    cls: 'bg-red-500/10 border-red-500/25 text-red-400',
                    tip: `This file is incomplete or corrupted and can't be used (${model.damaged}). Delete it and download the model again.`,
                  }
                : fitInfo?.moe_offload
                ? {
                    label: 'Runs here - split',
                    cls: 'bg-emerald-500/10 border-emerald-500/25 text-emerald-400',
                    tip: "Bigger than your graphics card's memory. The model's less-used parts stay in main memory while the rest runs on the card - fast for its size.",
                  }
                : fitInfo
                ? {
                    green: {
                      label: 'Full speed',
                      cls: 'bg-emerald-500/10 border-emerald-500/25 text-emerald-400',
                      tip: 'Fits entirely in your graphics card’s memory.',
                    },
                    yellow: {
                      label: 'Runs slower',
                      cls: 'bg-amber-500/10 border-amber-500/25 text-amber-400',
                      tip: 'Larger than your graphics card’s free memory - part of it runs on the processor, so responses are slower.',
                    },
                    red: {
                      label: 'Too large',
                      cls: 'bg-red-500/10 border-red-500/25 text-red-400',
                      tip: 'Needs more memory than this machine has free.',
                    },
                  }[fitInfo.fit]
                : null;
              return (
                <div
                  key={model.name}
                  class={`flex items-center gap-3 px-4 py-2.5 ${isPaused ? 'opacity-60' : ''}`}
                >
                  <div class="min-w-0 flex-1">
                    <div class="flex items-center gap-2">
                      <span class="text-sm font-semibold text-[var(--text-primary)] truncate">
                        {displayName}
                      </span>
                      {isPaused && (
                        <span class="shrink-0 px-2 py-0.5 bg-[var(--bg-dropdown)] border border-[var(--border-subtle)] text-[var(--text-muted)] text-[10px] rounded-full font-semibold whitespace-nowrap">
                          Paused
                        </span>
                      )}
                      {fitBadge && (
                        <span
                          title={fitBadge.tip}
                          class={`shrink-0 px-2 py-0.5 border text-[10px] rounded-full font-semibold whitespace-nowrap ${fitBadge.cls}`}
                        >
                          {fitBadge.label}
                        </span>
                      )}
                    </div>
                    <p
                      class="text-xs text-[var(--text-muted)] truncate"
                      title="Trained context = what the model was built to handle. 'Runs at' = the context Your Own AI starts it with on this machine."
                    >
                      {model.damaged
                        ? `${model.size} · incomplete or corrupted - delete it and download again`
                        : `${quantization} · ${model.size}`}
                      {!model.damaged && fitInfo && fitInfo.context_runtime > 0
                        ? ` · runs at ${formatContext(fitInfo.context_runtime)}${fitInfo.agent_template_ok ? ' · works in projects' : ''}`
                        : ''}
                      {!model.damaged && store.modelSpeeds[model.name]
                        ? ` · ~${Math.round(store.modelSpeeds[model.name])} tok/s measured`
                        : ''}
                    </p>
                  </div>
                  {!model.damaged && (
                  <LiquidMetalButton
                    variant="secondary"
                    onClick$={() => handleTogglePause$(model.name)}
                    title={isPaused ? 'Resume - offer this model again when you choose a model for an AI, and let automatic routing pick it' : 'Pause - hide this model wherever you choose a model for an AI, and keep automatic routing from picking it. It stays downloaded, and any AI already set to it keeps it.'}
                    class="p-2 shrink-0 transition-colors"
                  >
                    {isPaused ? (
                      <LuPlayCircle class="w-[18px] h-[18px]" />
                    ) : (
                      <LuPauseCircle class="w-[18px] h-[18px]" />
                    )}
                  </LiquidMetalButton>
                  )}
                  <LiquidMetalButton
                    variant="danger"
                    onClick$={() => handleDeleteClick$(model.name)}
                    class="p-2 shrink-0 transition-colors"
                  >
                    <LuTrash2 class="w-4 h-4" />
                  </LiquidMetalButton>
                </div>
              );
            })}
          </div>
          )}
        </div>
      )}

      {/* Model Catalog — tab bar + filtered grid */}
      <div class="mb-8">
        <h2 class="text-2xl font-bold text-[var(--text-primary)] font-varela mb-4 border-b border-[var(--border-subtle)] pb-2">
          Available Offline Models
        </h2>

        {/* Task filters + sort control. flex-wrap: below ~900px the sort
            control drops to its own line instead of crowding the tab strip. */}
        <div class="flex flex-wrap items-center justify-between gap-4 mb-6 sticky top-0 z-[5] bg-[var(--bg-main)] pt-2 -mt-2">
          <div class="flex gap-1 overflow-x-auto pb-1">
          {([
            { key: 'all' as const, label: 'All' },
            ...TASK_FILTERS.map(c => ({ key: c, label: capabilityInfo[c].label })),
            { key: 'vision' as const, label: 'Vision' },
          ]).map((tab) => {
            const isActive = store.selectedTask === tab.key;
            const count = tab.key === 'all'
              ? modelFamilies.length
              : modelFamilies.filter(f => matchesTask(f, tab.key)).length;
            return (
              <button
                key={tab.key}
                type="button"
                onClick$={() => { store.selectedTask = tab.key as any; }}
                class={`px-4 py-2 rounded-full text-sm font-medium whitespace-nowrap transition-all ${
                  isActive
                    ? 'bg-[var(--bg-button-primary)] text-[var(--text-button-primary)] shadow-md'
                    : 'bg-[var(--bg-card)] text-[var(--text-secondary)] border border-[var(--border-subtle)] hover:bg-[var(--bg-dropdown-hover)] hover:text-[var(--text-primary)]'
                }`}
              >
                {tab.label}
                <span class={`ml-1.5 text-xs ${isActive ? 'opacity-80' : 'opacity-50'}`}>
                  {count}
                </span>
              </button>
            );
          })}
          </div>

          {/* Sort — custom dropdown (a native <select> popup is GTK-themed on
              webkit and ignores our light/dark vars); right-aligned in the row. */}
          <div class="flex items-center gap-2 shrink-0">
            <label class="text-sm text-[var(--text-muted)] whitespace-nowrap">Sort</label>
            <div class="relative">
              <button
                type="button"
                onClick$={() => { store.sortOpen = !store.sortOpen; }}
                class="flex items-center justify-between gap-2 min-w-[7rem] px-3 py-2 rounded-full text-sm font-medium bg-[var(--bg-card)] text-[var(--text-secondary)] border border-[var(--border-subtle)] hover:text-[var(--text-primary)] focus:outline-none"
              >
                <span>{SORT_OPTIONS.find(o => o.key === store.sortBy)?.label}</span>
                <LuChevronDown class="w-4 h-4 text-[var(--text-muted)] flex-shrink-0" />
              </button>
              {store.sortOpen && (
                <>
                  <div class="fixed inset-0 z-40" onClick$={() => { store.sortOpen = false; }} />
                  <div class="absolute right-0 top-full mt-1 min-w-[8rem] z-50 rounded-lg bg-[var(--bg-dropdown)] border border-[var(--border-subtle)] shadow-xl py-1">
                    {SORT_OPTIONS.map(o => (
                      <button
                        key={o.key}
                        type="button"
                        onClick$={() => { store.sortBy = o.key; store.sortOpen = false; }}
                        class={`block w-full text-left px-3 py-1.5 text-sm whitespace-nowrap hover:bg-[var(--bg-card)] transition-colors ${
                          o.key === store.sortBy
                            ? 'text-[var(--text-primary)] font-medium'
                            : 'text-[var(--text-secondary)]'
                        }`}
                      >
                        {o.label}
                      </button>
                    ))}
                  </div>
                </>
              )}
            </div>
          </div>
        </div>

        {/* Model grid — what you can run, then a collapsible "needs more memory" group */}
        <div class="grid grid-cols-1 sm:grid-cols-2 lg:grid-cols-3 gap-6">
          {runnableFamilies.map((family) => (
            <div class="contents" key={`${family.id}:${store.selectedVariants[family.id]?.filename ?? ''}`}>
              {renderModelCard(family, store.selectedVariants[family.id])}
            </div>
          ))}

          {tooBigFamilies.length > 0 && (
            <button
              type="button"
              onClick$={() => { store.showTooBig = !store.showTooBig; }}
              class="col-span-full flex items-center gap-3 mt-2 group"
            >
              <div class="h-px flex-1 bg-[var(--border-subtle)]" />
              <span class="text-xs font-medium text-[var(--text-muted)] group-hover:text-[var(--text-secondary)] whitespace-nowrap flex items-center gap-1.5 transition-colors">
                <LuChevronDown class={`w-3.5 h-3.5 transition-transform ${store.showTooBig ? '' : '-rotate-90'}`} />
                {store.showTooBig
                  ? 'Needs more memory than this machine has'
                  : `${tooBigFamilies.length} model${tooBigFamilies.length === 1 ? '' : 's'} need more memory — show`}
              </span>
              <div class="h-px flex-1 bg-[var(--border-subtle)]" />
            </button>
          )}

          {store.showTooBig && store.isBetaBuild && (
            <label class="col-span-full flex items-start gap-2.5 rounded-lg border border-amber-700/50 bg-amber-900/15 px-3.5 py-2.5 cursor-pointer">
              <input
                type="checkbox"
                checked={store.betaFitOverride}
                onChange$={(_, el) => {
                  store.betaFitOverride = el.checked;
                  localStorage.setItem('beta-fit-override', el.checked ? 'on' : 'off');
                }}
                class="mt-0.5 accent-amber-500"
              />
              <span class="text-xs text-amber-200/90 leading-relaxed">
                <span class="font-semibold">Beta testing:</span> allow
                downloading models beyond this hardware. They run partly on
                the processor - expect slow replies, and loading can fail.
                Auto model picks ignore these; you choose them yourself.
              </span>
            </label>
          )}

          {store.showTooBig && tooBigFamilies.map((family) => (
            <div class="contents" key={`${family.id}:${store.selectedVariants[family.id]?.filename ?? ''}`}>
              {renderModelCard(family, store.selectedVariants[family.id])}
            </div>
          ))}
        </div>
      </div>

      {/* Custom Model Section */}
      <div class="mt-8 generic-container p-6 rounded-2xl text-center">
        <p class="text-[var(--text-secondary)] mb-4">Want a different model?</p>
        <LiquidMetalButton
          onClick$={() => { store.customModelModalOpen = true; }}
          class="inline-flex items-center gap-2 px-6 py-3 font-medium transition-all hover:scale-105"
        >
          <LuPlus class="w-5 h-5" />
          Add Custom Model from Hugging Face
        </LiquidMetalButton>
      </div>

      {/* System Information - everything model-fit depends on, plus a
          privacy-safe copy for support (the models folder path stays OUT of
          the copy: it contains the user's account name). */}
      <div class="mt-8 generic-container p-6 rounded-2xl">
        <div class="mb-3 flex items-center justify-between gap-3">
          <h3 class="font-semibold text-[var(--text-primary)] text-base">
            System Information
          </h3>
          <LiquidMetalButton
            variant="secondary"
            onClick$={copySystemInfo}
            title="Copy these details for support - your models folder path and account name are not included"
            class="px-3 py-1.5 text-xs"
          >
            {store.systemInfoCopied ? (
              <LuCheck class="w-3.5 h-3.5 text-emerald-500" />
            ) : (
              <LuCopy class="w-3.5 h-3.5" />
            )}
            {store.systemInfoCopied ? "Copied" : "Copy for support"}
          </LiquidMetalButton>
        </div>
        <div class="text-sm text-[var(--text-secondary)] space-y-2">
          <div class="flex items-center justify-between">
            <span class="text-[var(--text-muted)]">App version:</span>
            <span class="font-semibold">{store.appVersion || "…"}</span>
          </div>
          <div class="flex items-center justify-between">
            <span class="text-[var(--text-muted)]">Operating system:</span>
            <span class="font-semibold">
              {systemInfo ? `${systemInfo.os_name} ${systemInfo.os_version}` : "…"}
            </span>
          </div>
          <div class="flex items-center justify-between">
            <span class="text-[var(--text-muted)]">Processor:</span>
            <span class="font-semibold">
              {systemInfo?.cpu_brand || "Unknown"}
              {systemInfo ? ` · ${systemInfo.cpu_count} cores` : ""}
            </span>
          </div>
          <div class="flex items-center justify-between">
            <span class="text-[var(--text-muted)]">Memory:</span>
            <span class="font-semibold">
              {totalRAM.toFixed(1)}GB total
              {freeRAM !== null ? ` · ${freeRAM.toFixed(1)}GB free now` : ""}
            </span>
          </div>
          <div class="flex items-center justify-between">
            <span class="text-[var(--text-muted)]">Graphics:</span>
            <span class="font-semibold">
              {systemInfo?.gpu_name
                ? systemInfo.gpu_integrated
                  ? `${systemInfo.gpu_name} · integrated, shares system memory`
                  : `${systemInfo.gpu_name}${systemInfo.total_vram_gb ? ` · ${systemInfo.total_vram_gb.toFixed(1)}GB VRAM` : ""}`
                : "None detected · models run on the processor"}
            </span>
          </div>
          {store.engineBackend && (
            <div class="flex items-center justify-between">
              <span class="text-[var(--text-muted)]">Engine:</span>
              <span class="font-semibold">{store.engineBackend}</span>
            </div>
          )}
          <div class="flex items-center justify-between">
            <span class="text-[var(--text-muted)]">Downloaded models:</span>
            <span class="font-semibold">{store.downloadedModels.length}</span>
          </div>
          <div class="flex flex-col gap-1">
            <span class="text-[var(--text-muted)]">
              Models directory
              <span class="ml-1 text-[10px]">(shown here, never included in the copy)</span>
            </span>
            <code class="text-xs bg-[var(--bg-dropdown)] px-2 py-1 rounded border border-[var(--border-subtle)] break-all">
              {store.modelsDirectory}
            </code>
          </div>
          <p class="pt-1 text-xs text-[var(--text-muted)]">
            Something not working?{" "}
            <a
              href="/settings/#settings-diagnostics"
              class="text-[var(--text-link)] hover:underline"
            >
              Save a diagnostic report in Settings
            </a>
            .
          </p>
        </div>
      </div>


      {/* Delete Confirmation Modal */}
      <DeleteModelModal
        isOpen={store.deleteModalOpen}
        onClose={handleDeleteCancel$}
        onConfirm={handleDeleteConfirm$}
        modelName={store.modelToDelete?.displayName || ''}
        isDeleting={store.isDeleting}
      />

      {/* Custom Model Modal */}
      <CustomModelModal
        isOpen={store.customModelModalOpen}
        onClose$={$(() => { store.customModelModalOpen = false; })}
        onDownload$={handleCustomModelDownload$}
        isDownloading={!!store.downloads['custom']}
        downloadProgress={store.downloads['custom']?.progress ?? null}
      />

      {/* Publisher-terms agreement (required pass-through, e.g. HAI-DEF).
          Shown once per license id, at the moment of download. */}
      {store.licensePrompt && (() => {
        const fam = modelFamilies.find((f) => f.id === store.licensePrompt);
        if (!fam?.license) return null;
        const lic = fam.license;
        return (
          <div
            class="fixed inset-0 z-50 flex items-center justify-center p-4 bg-black/60"
            onClick$={(e, el) => {
              if (e.target === el) store.licensePrompt = null;
            }}
          >
            <div
              class="bg-[var(--bg-header-footer)] rounded-xl shadow-2xl w-full max-w-md p-6"
              onClick$={(e) => e.stopPropagation()}
            >
              <h3 class="text-lg font-semibold text-[var(--text-primary)] mb-1">
                {fam.name} has its own terms
              </h3>
              <p class="text-xs text-[var(--text-muted)] mb-3">{lic.notice}</p>
              <ul class="space-y-2 mb-4">
                {lic.points.map((pt, i) => (
                  <li key={i} class="text-sm text-[var(--text-secondary)] flex gap-2">
                    <span class="text-[var(--text-muted)] shrink-0">•</span>
                    {pt}
                  </li>
                ))}
              </ul>
              <button
                type="button"
                onClick$={() => openLicenseUrl(lic.url)}
                class="text-sm text-[var(--text-link)] hover:underline mb-5 block"
              >
                Read the full {lic.name}
              </button>
              <div class="flex flex-col-reverse sm:flex-row sm:justify-end gap-3">
                <LiquidMetalButton
                  variant="secondary"
                  onClick$={() => (store.licensePrompt = null)}
                  class="px-4 py-2 text-sm"
                >
                  Cancel
                </LiquidMetalButton>
                <LiquidMetalButton onClick$={agreeLicense} class="px-4 py-2 text-sm">
                  Agree and download
                </LiquidMetalButton>
              </div>
            </div>
          </div>
        );
      })()}
      <ConfirmModal
        isOpen={store.medicalOffer !== null}
        title="Use it for health questions?"
        message="You've installed a medical model. Health questions always stay on your device - would you like this model to answer them from now on? You can change this any time in Settings > Routing."
        confirmLabel="Yes, use the medical model"
        cancelLabel="Keep my current model"
        onConfirm$={async () => {
          const chosen = store.medicalOffer;
          store.medicalOffer = null;
          setMedicalPromptDone();
          if (chosen) {
            await setMedicalModel(chosen);
            store.medicalChoice = chosen;
          }
        }}
        onCancel$={async () => {
          store.medicalOffer = null;
          setMedicalPromptDone();
          // An explicit "keep" pins the current NON-specialist so the
          // specialist does not win by ranking anyway.
          await pinCurrentNonSpecialist();
        }}
      />
    </div>
  );
});

export default ModelDownloader;
