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
import { modelFamilies, getBestFamilyForRAM, getBestVariantForSystem, type ModelVariant } from '../data/recommended-models';
import { modelManager, type DownloadProgress } from '../utils/modelManager';
import { invoke } from '@tauri-apps/api/core';

interface RecommendedModel {
  familyId: string;
  familyName: string;
  variant: ModelVariant;
  reason: string;
  hasGPU: boolean;
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
function getRecommendedModel(systemInfo: SystemInfo | null): RecommendedModel {
  const totalRAM = systemInfo?.total_memory_gb || 4;
  const totalVRAM = systemInfo?.total_vram_gb || null;
  const hasGPU = !!systemInfo?.gpu_name && (totalVRAM || 0) > 0;
  const gpuName = systemInfo?.gpu_name || 'GPU';

  if (hasGPU && totalVRAM) {
    // Find the largest model that fits entirely in VRAM (skip specialist models for first-run)
    const candidates: { family: typeof modelFamilies[0]; variant: ModelVariant }[] = [];

    for (const family of modelFamilies) {
      if (family.category === 'specialist') continue;
      for (const variant of family.variants) {
        if (variant.size <= totalVRAM) {
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
      return {
        familyId: best.family.id,
        familyName: best.family.name,
        variant: best.variant,
        reason: `Your GPU (${gpuName}) has ${totalVRAM.toFixed(1)}GB VRAM — ${best.family.name} ${best.variant.parameterCount} fits entirely on GPU for maximum speed!`,
        hasGPU: true,
      };
    }
  }

  // No GPU or nothing fits in VRAM — find best model for CPU
  // Pick the largest model from recommended families that fits in RAM with overhead
  const bestFamily = getBestFamilyForRAM(totalRAM, null); // null VRAM = CPU-only calc

  if (bestFamily) {
    const bestVariant = getBestVariantForSystem(bestFamily, totalRAM, null);
    if (bestVariant) {
      return {
        familyId: bestFamily.id,
        familyName: bestFamily.name,
        variant: bestVariant,
        reason: `With ${totalRAM.toFixed(0)}GB RAM, ${bestFamily.name} ${bestVariant.parameterCount} is optimised for your system — fast and efficient on CPU!`,
        hasGPU: false,
      };
    }
  }

  // Final fallback: smallest available model from a recommended family
  const fallbackFamily = modelFamilies.find((f) => f.recommended) || modelFamilies[0];
  const fallbackVariant = fallbackFamily.variants[0];

  return {
    familyId: fallbackFamily.id,
    familyName: fallbackFamily.name,
    variant: fallbackVariant,
    reason: 'Fast and efficient on your CPU — perfect for getting started with private AI conversations!',
    hasGPU: false,
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

    const recommendedModel = getRecommendedModel(systemInfo);

    // Reset state when modal opens
    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(({ track }) => {
      track(() => isOpen);
      if (isOpen) {
        error.value = null;
        downloadProgress.value = null;
      }
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

        // Load the model in the background
        try {
          console.log('[WelcomeModal] Loading model in background:', recommendedModel.variant.filename);
          await invoke('load_model', { filename: recommendedModel.variant.filename, withVision: false, reason: "welcome" });
        } catch (loadError) {
          console.error('[WelcomeModal] Failed to load model:', loadError);
        }

        // Notify parent that model was downloaded
        onModelDownloaded$(recommendedModel.variant.filename);

        // Auto-close modal after successful download
        onClose$();
      } catch (err) {
        console.error('[WelcomeModal] Download failed:', err);
        error.value = getUserFriendlyErrorMessage(err);
      } finally {
        isDownloading.value = false;
        downloadProgress.value = null;
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
                    Downloading {modelDisplayName}...
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

                <button
                  onClick$={() => (window.location.href = '/setup')}
                  class="w-full py-3 text-[var(--text-secondary)] hover:text-[var(--text-primary)] transition-colors text-sm"
                >
                  Or browse all available models →
                </button>
              </div>
            )}
          </div>
        </div>
      </div>
    );
  }
);

export default WelcomeModal;
