import { component$, useStore, useVisibleTask$, $, type QRL } from '@builder.io/qwik';
import LiquidMetalButton from './LiquidMetalButton';
import { modelManager } from '../utils/modelManager';
import { modelFamilies, VISION_PROJECTORS, isVariantSuitable } from '../data/recommended-models';
import { refreshCatalogModes } from '../utils/modelCache';
import type { VisionPlanFile } from '../contexts/VisionDownloadContext';

interface VisionDownloadCardProps {
  /** Begin the download (handed to the app-level manager, which shows progress in
   *  the global indicator and survives navigation). */
  onStart$: QRL<(files: VisionPlanFile[], visionModel: string) => void>;
  onCancel$: QRL<() => void>;
  /** System specs so we recommend the right Gemma size (E4B if it fits, else E2B). */
  ram?: number;
  vram?: number | null;
}

/**
 * Inline "get vision" card: works out the smallest download that gives this
 * device a working offline vision model (a matching projector, plus the Gemma 4
 * E2B base only if no vision-capable base is present), then hands it to the
 * app-level download manager so progress shows everywhere and survives leaving
 * this page. Everything stays local — this is the on-device add-on, not a cloud call.
 */
export const VisionDownloadCard = component$<VisionDownloadCardProps>((props) => {
  const store = useStore({
    phase: 'planning' as 'planning' | 'idle' | 'error',
    files: [] as VisionPlanFile[],
    visionModel: '',
    error: null as string | null,
  });

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    try {
      const gemma = modelFamilies.find((f) => f.id === 'gemma-4');
      const e2b = gemma?.variants.find((v) => v.parameterCount === 'E2B');
      const e4b = gemma?.variants.find((v) => v.parameterCount === 'E4B');
      const e2bProj = VISION_PROJECTORS.find((p) => p.id === 'gemma-4-e2b-vision');
      const e4bProj = VISION_PROJECTORS.find((p) => p.id === 'gemma-4-e4b-vision');

      // Pick the right Gemma size for this machine: use E4B if the user already
      // has it, OR if their hardware can comfortably run it; otherwise the lighter
      // E2B (the safe floor that runs on anything). They can always upgrade later.
      const hasE4B = e4b ? await modelManager.isModelDownloaded(e4b.filename) : false;
      // Graded by the app's one grader, never by this card's own arithmetic.
      const modes = await refreshCatalogModes().catch(() => null);
      const e4bFits = !!e4b && !!modes && isVariantSuitable(e4b, modes);
      const useE4B = hasE4B || e4bFits;
      const base = useE4B ? e4b! : e2b!;
      const proj = useE4B ? e4bProj! : e2bProj!;
      store.visionModel = base.filename;

      const files: VisionPlanFile[] = [];
      if (!(await modelManager.isModelDownloaded(base.filename))) {
        files.push({
          url: base.downloadUrl,
          filename: base.filename,
          label: `Gemma 4 ${base.parameterCount} model`,
          size: base.size,
        });
      }
      if (!(await modelManager.isModelDownloaded(proj.filename))) {
        files.push({
          url: proj.downloadUrl,
          filename: proj.filename,
          label: proj.name,
          size: proj.size,
        });
      }

      if (files.length === 0) {
        // Already present (race) — start anyway; the manager no-ops finished files.
        props.onStart$([], store.visionModel);
        return;
      }
      store.files = files;
      store.phase = 'idle';
    } catch (e) {
      store.error = e instanceof Error ? e.message : String(e);
      store.phase = 'error';
    }
  });

  const totalGb = store.files.reduce((s, f) => s + f.size, 0);

  return (
    <div class="bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-700 rounded-lg p-4">
      <p class="text-sm font-medium text-yellow-800 dark:text-yellow-200 mb-1">
        This image needs a vision model
      </p>

      {store.phase === 'planning' && (
        <p class="text-sm text-yellow-700 dark:text-yellow-300">Checking what you already have…</p>
      )}

      {store.phase === 'error' && (
        <p class="text-sm text-red-600 dark:text-red-400">Couldn't prepare the download: {store.error}</p>
      )}

      {store.phase === 'idle' && (
        <>
          <p class="text-sm text-yellow-700 dark:text-yellow-300 mb-3">
            Add a small vision model so this AI can see images you attach — it runs
            entirely on your device. {store.files.length > 1 ? 'Downloads' : 'Download'}{' '}
            ~{totalGb.toFixed(1)} GB:
          </p>
          <ul class="text-xs text-yellow-700 dark:text-yellow-300 mb-4 space-y-0.5 list-disc list-inside">
            {store.files.map((f) => (
              <li key={f.filename}>
                {f.label} · {f.size.toFixed(1)} GB
              </li>
            ))}
          </ul>
          <p class="text-xs text-yellow-700/80 dark:text-yellow-300/80 mb-3">
            You can keep using the app while it downloads — progress shows in the corner.
          </p>
          <div class="flex items-center gap-3">
            <LiquidMetalButton
              onClick$={$(() => props.onStart$(store.files, store.visionModel))}
              class="px-4 py-2 text-sm"
            >
              Download &amp; continue
            </LiquidMetalButton>
            <button
              onClick$={props.onCancel$}
              class="text-sm text-yellow-700 dark:text-yellow-300 hover:underline"
            >
              Not now
            </button>
          </div>
        </>
      )}
    </div>
  );
});
