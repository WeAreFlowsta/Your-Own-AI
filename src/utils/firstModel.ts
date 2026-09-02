/**
 * First model - the graded first-run pick and the "first model in flight"
 * state shared by the welcome wizard, the chat and the root notice.
 *
 * The pick uses the app's one grader (Rust grade_catalog via CatalogModes):
 * the same verdict the models page shows, never a RAM-only guess.
 *
 * While the first download runs the app stays usable: the chat keeps a
 * question and answers it the moment the model lands. That state lives in
 * localStorage (survives a page change and an app restart - the engine
 * resumes the .part) and is announced through window events.
 */

import type { SystemInfo } from '../components/ModelDownloader';
import { modelFamilies, getBestFamilyForRAM, getBestVariantForSystem, getRunMode, type ModelVariant, type CatalogModes } from '../data/recommended-models';

export interface RecommendedModel {
  familyId: string;
  familyName: string;
  variant: ModelVariant;
  reason: string;
  hasGPU: boolean;
  /** The smallest model we ship is still a stretch for this machine - say so. */
  tight?: boolean;
  /** The catalog has not been graded yet - show a checking state, never a guess. */
  pending?: boolean;
}


/**
 * Get the recommended model for first-time setup.
 * Strategy: pick the best model that fits entirely in VRAM for maximum speed.
 * Falls back to best CPU-capable model if no GPU or nothing fits in VRAM.
 */
export function getRecommendedModel(
  systemInfo: SystemInfo | null,
  gpuUnusable = false,
  modes: CatalogModes | null = null,
): RecommendedModel {
  const totalRAM = systemInfo?.total_memory_gb || 4;
  if (!modes) {
    // Graded by the app's one grader (Rust) - until it answers there is
    // no honest pick, only a placeholder the button cannot act on.
    const family = modelFamilies.find((f) => f.recommended) || modelFamilies[0];
    const variant = [...family.variants].sort((a, b) => a.size - b.size)[0];
    return {
      familyId: family.id,
      familyName: family.name,
      variant,
      reason: 'Checking which model fits this computer…',
      hasGPU: false,
      pending: true,
    };
  }
  // Integrated graphics share system RAM - recommend as CPU, not as a card.
  // gpuUnusable: the card exists but cannot run models (safe mode, or the
  // engine's own device verdict) - sizing must plan for the processor, or
  // the welcome pick would be "fast on your GPU" and then crawl on CPU.
  const totalVRAM =
    gpuUnusable || systemInfo?.gpu_integrated
      ? null
      : systemInfo?.total_vram_gb || null;
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
        if (getRunMode(variant, modes) === 'gpu') {
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
  const bestFamily = getBestFamilyForRAM(modes);

  if (bestFamily) {
    const bestVariant = getBestVariantForSystem(bestFamily, modes);
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
export function getUserFriendlyErrorMessage(error: unknown): string {
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

/** localStorage key: JSON { filename, label } of the first download in flight. */
export const FIRST_MODEL_KEY = 'firstModelDownloading';
/** window event: the in-flight state changed (marked or cleared). */
export const FIRST_MODEL_CHANGED = 'firstModelChanged';
/** window event: the first model is downloaded, loaded and assigned.
 *  detail = { filename, label }. */
export const FIRST_MODEL_READY = 'firstModelReady';

export interface FirstModelInFlight {
  filename: string;
  /** Display name, e.g. "Gemma 4 E2B" */
  label: string;
}

export function firstModelInFlight(): FirstModelInFlight | null {
  try {
    const raw = localStorage.getItem(FIRST_MODEL_KEY);
    if (!raw) return null;
    const v = JSON.parse(raw);
    if (v && typeof v.filename === 'string' && typeof v.label === 'string') return v;
  } catch {
    /* unreadable = not in flight */
  }
  return null;
}

export function markFirstModelInFlight(filename: string, label: string): void {
  try {
    localStorage.setItem(FIRST_MODEL_KEY, JSON.stringify({ filename, label }));
  } catch { /* storage off - the wizard still works, the chat just cannot wait */ }
  window.dispatchEvent(new CustomEvent(FIRST_MODEL_CHANGED));
}

export function clearFirstModelInFlight(): void {
  try { localStorage.removeItem(FIRST_MODEL_KEY); } catch { /* nothing to clear */ }
  window.dispatchEvent(new CustomEvent(FIRST_MODEL_CHANGED));
}

/** What the chat shows in the message field while the first model downloads. */
export function firstModelWaitingPlaceholder(label: string, percent: number | null): string {
  return percent === null
    ? `Waiting for your first model - ${label} is downloading..`
    : `Waiting for your first model - ${label} ${percent}%..`;
}

/** sessionStorage key: the question held while the first model downloads.
 *  The chat page's state dies on navigation (Settings for the CUDA offer,
 *  the models page); the held question must not die with it. Images are
 *  not carried (a first-run question with an image is rare, and the bubble
 *  would be the size of the image). */
export const FIRST_MODEL_HELD_KEY = 'firstModelHeldTurn';

export interface HeldTurn {
  userInput: string;
  chatAction: string | null;
  fileContext?: string;
  aiId: string;
  aiLabel: string;
  aiImageUrl?: string;
}

export function heldTurn(): HeldTurn | null {
  try {
    const raw = sessionStorage.getItem(FIRST_MODEL_HELD_KEY);
    if (!raw) return null;
    const v = JSON.parse(raw);
    if (v && typeof v.userInput === 'string' && typeof v.aiId === 'string') return v;
  } catch {
    /* unreadable = nothing held */
  }
  return null;
}

export function setHeldTurn(t: HeldTurn): void {
  try { sessionStorage.setItem(FIRST_MODEL_HELD_KEY, JSON.stringify(t)); } catch { /* storage off */ }
}

export function clearHeldTurn(): void {
  try { sessionStorage.removeItem(FIRST_MODEL_HELD_KEY); } catch { /* nothing to clear */ }
}
