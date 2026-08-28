/**
 * Welcome Modal (Qwik) - First-run experience
 *
 * Shows when no offline models are downloaded.
 * Recommends a single model based on system specs.
 * Dismissible but encourages download.
 */

import {
  component$,
  useSignal,
  useVisibleTask$,
  $,
  type QRL,
} from '@builder.io/qwik';
import { LuHardDriveDownload, LuAlertTriangle, LuZap } from '@qwikest/icons/lucide';
import LiquidMetalButton from './LiquidMetalButton';
import type { SystemInfo } from './ModelDownloader';
import { modelFamilies, getBestFamilyForRAM, getBestVariantForSystem, getRunMode, type ModelVariant } from '../data/recommended-models';
import { modelManager, type DownloadProgress } from '../utils/modelManager';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

interface RecommendedModel {
  familyId: string;
  familyName: string;
  variant: ModelVariant;
  reason: string;
  hasGPU: boolean;
  /** The smallest model we ship is still a stretch for this machine - say so. */
  tight?: boolean;
}

interface WelcomeModalProps {
  isOpen: boolean;
  onClose$: QRL<() => void>;
  systemInfo: SystemInfo | null;
  onModelDownloaded$: QRL<(filename: string) => void>;
}

/**
 * Get the recommended model for first-time setup.
 * Strategy: pick the best model that fits entirely in VRAM for maximum speed.
 * Falls back to best CPU-capable model if no GPU or nothing fits in VRAM.
 */
function getRecommendedModel(
  systemInfo: SystemInfo | null,
  gpuUnusable = false,
): RecommendedModel {
  const totalRAM = systemInfo?.total_memory_gb || 4;
  // Integrated graphics share system RAM - recommend as CPU, not as a card.
  // gpuUnusable: the card exists but cannot run models (safe mode, or the
  // engine's own device verdict) - sizing must plan for the processor, or
  // the welcome pick would be "fast on your GPU" and then crawl on CPU.
  const totalVRAM =
    gpuUnusable || systemInfo?.gpu_integrated
      ? null
      : systemInfo?.total_vram_gb || null;
  const freeRAM = systemInfo ? Math.max(1, systemInfo.total_memory_gb - systemInfo.used_memory_gb) : null;
  const hasGPU = !!systemInfo?.gpu_name && (totalVRAM || 0) > 0;
  const gpuName = systemInfo?.gpu_name || 'GPU';

  if (hasGPU && totalVRAM) {
    // Grade with getRunMode end to end - the same call the models page
    // makes, with the card's memory AND system RAM in the picture. This
    // branch used to check VRAM alone; a big card on a RAM-poor machine
    // was sold a model the system could not stage (silent first-run
    // crash, found in the field).
    const candidates: { family: typeof modelFamilies[0]; variant: ModelVariant }[] = [];

    for (const family of modelFamilies) {
      if (family.category === 'specialist') continue;
      for (const variant of family.variants) {
        if (getRunMode(variant, totalRAM, totalVRAM, freeRAM) === 'gpu') {
          candidates.push({ family, variant });
        }
      }
    }

    if (candidates.length > 0) {
      // Sort: recommended families first, then by largest size
      candidates.sort((a, b) => {
        if (a.family.recommended !== b.family.recommended) {
          return a.family.recommended ? -1 : 1;
        }
        return b.variant.size - a.variant.size;
      });

      const best = candidates[0];
      // Unified memory (Apple Silicon) reads oddly as "VRAM" - name it honestly.
      const isUnified = gpuName.toLowerCase().includes('apple');
      return {
        familyId: best.family.id,
        familyName: best.family.name,
        variant: best.variant,
        reason: isUnified
          ? `Your ${gpuName} runs models in its shared memory - ${best.family.name} ${best.variant.parameterCount} fits comfortably with room for the system.`
          : `Your GPU (${gpuName}) has ${totalVRAM.toFixed(1)}GB VRAM - ${best.family.name} ${best.variant.parameterCount} fits entirely on GPU for maximum speed!`,
        hasGPU: true,
      };
    }
  }

  // No GPU or nothing fits in VRAM - find best model for CPU
  // Pick the largest model from recommended families that fits in RAM with overhead
  const bestFamily = getBestFamilyForRAM(totalRAM, null, freeRAM); // null VRAM = CPU-only calc

  if (bestFamily) {
    const bestVariant = getBestVariantForSystem(bestFamily, totalRAM, null, freeRAM);
    if (bestVariant) {
      return {
        familyId: bestFamily.id,
        familyName: bestFamily.name,
        variant: bestVariant,
        reason: systemInfo?.gpu_integrated
          ? `Your graphics share system memory, so compact models are the quick ones here. ${bestFamily.name} ${bestVariant.parameterCount} runs on your processor: good for everyday chat, slower on long answers. Everything stays private and free.`
          : `With ${totalRAM.toFixed(0)}GB RAM, ${bestFamily.name} ${bestVariant.parameterCount} runs on your processor: good for everyday chat, slower on long answers. Everything stays private and free.`,
        hasGPU: false,
      };
    }
  }

  // Final fallback: smallest available model from a recommended family.
  // (Sort explicitly - the variants array is NOT size-ordered; taking [0]
  // handed an 8GB MacBook Air a 16.8GB model.)
  const fallbackFamily = modelFamilies.find((f) => f.recommended) || modelFamilies[0];
  const fallbackVariant = [...fallbackFamily.variants].sort((a, b) => a.size - b.size)[0];

  return {
    familyId: fallbackFamily.id,
    familyName: fallbackFamily.name,
    variant: fallbackVariant,
    reason: `Your computer has ${totalRAM.toFixed(0)}GB of memory, which is below what the smallest model we ship (${fallbackVariant.size}GB) needs to run well. It will download and it may answer, slowly, on your processor. Everything else in Your Own AI works as normal, and everything offline stays private and free.`,
    hasGPU: false,
    tight: true,
  };
}

/**
 * Convert technical error messages to user-friendly messages
 */
function getUserFriendlyErrorMessage(error: unknown): string {
  const errorMessage = error instanceof Error ? error.message : String(error);

  if (errorMessage.includes('No space left on device') || errorMessage.includes('os error 28')) {
    return 'Not enough disk space. Please free up some space and try again.';
  }

  if (errorMessage.includes('Network error') || errorMessage.includes('Failed to fetch')) {
    return 'Network connection error. Please check your internet connection.';
  }

  if (errorMessage.includes('404') || errorMessage.includes('Not Found')) {
    return 'Model file not found. The download link may be broken.';
  }

  return errorMessage;
}

export const WelcomeModal = component$<WelcomeModalProps>(
  ({ isOpen, onClose$, systemInfo, onModelDownloaded$ }) => {
    const isDownloading = useSignal(false);
    const downloadProgress = useSignal<DownloadProgress | null>(null);
    const error = useSignal<string | null>(null);
    // The GPU may be present but unusable (safe mode / device verdict) -
    // the recommendation recomputes to CPU sizing once the status answers.
    const gpuUnusable = useSignal(false);

    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(async () => {
      try {
        const s = await invoke<{ active: boolean; device_unsupported?: string | null }>(
          'gpu_safe_mode_status',
        );
        gpuUnusable.value = s.active || !!s.device_unsupported;
      } catch {
        /* GPU sizing stands */
      }
    });

    const recommendedModel = getRecommendedModel(systemInfo, gpuUnusable.value);

    // A download that was interrupted (the app closed, the page changed)
    // continues by itself when the welcome comes back - never from zero.
    const resumed = useSignal(false);
    const finishFirstModel$ = $(async () => {
      // Load the model in the background
      try {
        console.log('[WelcomeModal] Loading model in background:', recommendedModel.variant.filename);
        await invoke('load_model', { filename: recommendedModel.variant.filename, withVision: false, reason: "welcome" });
      } catch (loadError) {
        console.error('[WelcomeModal] Failed to load model:', loadError);
      }
      onModelDownloaded$(recommendedModel.variant.filename);
      onClose$();
    });

    const handleDownload$ = $(async () => {
      isDownloading.value = true;
      error.value = null;
      downloadProgress.value = null;

      try {
        await modelManager.downloadModel(
          recommendedModel.variant.downloadUrl,
          recommendedModel.variant.filename,
          (progress) => {
            downloadProgress.value = progress;
          }
        );
        await finishFirstModel$();
      } catch (err) {
        console.error('[WelcomeModal] Download failed:', err);
        error.value = getUserFriendlyErrorMessage(err);
      } finally {
        isDownloading.value = false;
        downloadProgress.value = null;
      }
    });

    // Reset state when the modal opens - and pick the download back up: a
    // .part on disk resumes through the same call (the engine continues it
    // with a range request); a download the engine is STILL running (the
    // page changed mid-way) is attached to, never started twice.
    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(async ({ track, cleanup }) => {
      track(() => isOpen);
      if (!isOpen) return;
      error.value = null;
      downloadProgress.value = null;
      const filename = recommendedModel.variant.filename;
      const st = await modelManager.downloadStatus(filename);
      if (st.downloading) {
        resumed.value = true;
        isDownloading.value = true;
        if (st.total_bytes > 0) {
          downloadProgress.value = {
            filename,
            downloaded: st.downloaded_bytes,
            total: st.total_bytes,
            percent: Math.floor((st.downloaded_bytes / st.total_bytes) * 100),
          } as DownloadProgress;
        }
        const unProgress = await listen<DownloadProgress>('model-download-progress', (e) => {
          if (e.payload.filename === filename) downloadProgress.value = e.payload;
        });
        const unDone = await listen<{ filename: string }>('model-download-complete', async (e) => {
          if (e.payload.filename !== filename) return;
          try {
            await finishFirstModel$();
          } finally {
            isDownloading.value = false;
          }
        });
        cleanup(() => {
          unProgress();
          unDone();
        });
        return;
      }
      if (st.has_partial) {
        resumed.value = true;
        void handleDownload$();
      }
    });

    const modelDisplayName = `${recommendedModel.familyName} ${recommendedModel.variant.parameterCount}`;

    return (
      <div
        class="fixed inset-0 z-50 flex items-center justify-center p-4"
        style={{
          backgroundColor: 'rgba(0, 0, 0, 0.7)',
          backdropFilter: 'blur(16px) saturate(180%)',
          WebkitBackdropFilter: 'blur(16px) saturate(180%)',
        }}
      >
        <div
          class="bg-[var(--bg-header-footer)] rounded-2xl shadow-2xl w-full max-w-2xl relative overflow-hidden"
          onClick$={(e) => e.stopPropagation()}
        >

          {/* Content */}
          <div class="p-8 md:p-12">
            {/* Welcome Header */}
            <div class="text-center mb-8">
              <h1 class="text-4xl font-bold text-[var(--text-primary)] mb-2 font-varela">
                Welcome to Your Own AI
              </h1>
              <p class="text-base text-[var(--text-secondary)]">
                Let's Download your first Offline Model to get you started!!
              </p>
            </div>

            {/* Recommended Model Section */}
            <div class="bg-[var(--bg-card)] rounded-xl p-6 mb-6 border border-[var(--border-subtle)]">
              <div class="flex items-start gap-4 mb-4">
                {recommendedModel.hasGPU && (
                  <div class="flex-shrink-0 bg-yellow-500/20 rounded-full p-3">
                    <LuZap class="w-8 h-8 text-yellow-500" />
                  </div>
                )}
                <div class="flex-1">
                  <h2 class="text-2xl font-bold text-[var(--text-primary)] mb-2 font-varela">
                    Recommended for Your System
                  </h2>
                  <h3 class="text-xl font-semibold text-[var(--text-primary)] mb-2">
                    {modelDisplayName} ({recommendedModel.variant.size}GB)
                  </h3>
                  <p class="text-sm text-[var(--text-secondary)] leading-relaxed">
                    {recommendedModel.reason}
                  </p>
                  {/* CPU-class machines get one seed-planting sentence -
                      deliberately NO link and no second button: the
                      welcome's single job is the first offline download,
                      and nothing here may route away from it. */}
                  {recommendedModel.tight && (
                    <p class="mt-2 text-xs text-amber-500">
                      Expect slow answers on this machine. For fast ones, the optional plan adds GPT, Grok, Kimi, DeepSeek and more, running online at full speed on any computer - every price shown up front, in Settings after this download.
                    </p>
                  )}
                  {!recommendedModel.hasGPU && !recommendedModel.tight && (
                    <p class="mt-2 text-xs text-[var(--text-muted)]">
                      And whenever you want more: the optional plan adds GPT,
                      Grok, Kimi, DeepSeek and more, running online at full
                      speed on any machine, every price shown up front.
                      Everything offline stays free and private either way.
                    </p>
                  )}
                </div>
              </div>
            </div>

            {/* Error Display */}
            {error.value && (
              <div class="mb-6 p-4 bg-red-100 dark:bg-red-900/30 border border-red-200 dark:border-red-700 rounded-lg flex items-start gap-3">
                <LuAlertTriangle class="w-5 h-5 text-red-600 dark:text-red-400 flex-shrink-0 mt-0.5" />
                <div class="flex-1">
                  <p class="text-sm font-medium text-red-800 dark:text-red-200 mb-2">Download Failed</p>
                  <p class="text-sm text-red-700 dark:text-red-300">{error.value}</p>
                </div>
              </div>
            )}

            {/* Download Button or Progress */}
            {isDownloading.value ? (
              <div>
                <div class="flex items-center gap-3 mb-3 justify-center">
                  <div class="w-5 h-5 border-2 border-[var(--text-primary)] border-t-transparent rounded-full animate-spin" />
                  <span class="text-base font-medium text-[var(--text-primary)]">
                    {resumed.value ? `Continuing your download of ${modelDisplayName}...` : `Downloading ${modelDisplayName}...`}
                  </span>
                </div>
                {downloadProgress.value && (
                  <div>
                    <div class="w-full bg-[var(--border-subtle)] rounded-full h-3 overflow-hidden mb-2">
                      <div
                        class="bg-blue-600 h-full transition-all duration-300"
                        style={{ width: `${downloadProgress.value.percent}%` }}
                      />
                    </div>
                    <p class="text-sm text-[var(--text-muted)] text-center">
                      {downloadProgress.value.percent}% •{' '}
                      {modelManager.formatModelSize(downloadProgress.value.downloaded)} /{' '}
                      {modelManager.formatModelSize(downloadProgress.value.total)}
                    </p>
                  </div>
                )}
              </div>
            ) : (
              <div class="space-y-4">
                <LiquidMetalButton
                  onClick$={handleDownload$}
                  class="w-full py-4 font-semibold text-lg flex items-center justify-center gap-3"
                >
                  <LuHardDriveDownload class="w-6 h-6" />
                  Download {modelDisplayName}
                </LiquidMetalButton>

                {/* Returning users must learn their world is recoverable
                    BEFORE they start chatting - this modal is the only
                    first-run surface, and until this note existed nothing
                    in the app hinted at it. */}
                <p class="border-t border-[var(--border-subtle)] pt-3 text-xs text-[var(--text-muted)]">
                  Used Your Own AI before, with a Flowsta Vault backup? Sign
                  in and restore first - Settings → Your Flowsta Identity →
                  Restore conversations from Vault - so your AIs and
                  conversations come back before you start anything new here.
                </p>
              </div>
            )}
          </div>
        </div>
      </div>
    );
  }
);

export default WelcomeModal;
