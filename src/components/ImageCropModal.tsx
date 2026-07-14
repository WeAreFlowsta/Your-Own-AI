import {
  component$,
  useSignal,
  $,
  type QRL,
  noSerialize,
  type NoSerialize,
} from '@builder.io/qwik';
import { LuX, LuCheck } from '@qwikest/icons/lucide';
import LiquidMetalButton from './LiquidMetalButton';
import ImageCropper from './ImageCropper';

interface ImageCropModalProps {
  show: boolean;
  onHide$: QRL<() => void>;
  imageSrc: string | null;
  onCropComplete$: QRL<(croppedImageBlob: Blob) => void>;
}

export const ImageCropModal = component$<ImageCropModalProps>(
  ({ show, onHide$, imageSrc, onCropComplete$ }) => {
    const error = useSignal<string | null>(null);
    // ImageCropper exports a fresh canvas on every crop change; we keep the
    // latest one and only commit it when the user hits Crop & Save.
    const latestCanvas = useSignal<NoSerialize<HTMLCanvasElement>>();

    const handleCropChange$ = $((canvas: HTMLCanvasElement) => {
      latestCanvas.value = noSerialize(canvas);
      error.value = null;
    });

    const handleSave$ = $(() => {
      const canvas = latestCanvas.value;
      if (!canvas) {
        error.value = 'Could not crop image. Please try again.';
        return;
      }
      canvas.toBlob(
        (blob) => {
          if (blob) {
            onCropComplete$(blob);
            onHide$();
          } else {
            error.value = 'Failed to create cropped image. Please try another image.';
          }
        },
        'image/jpeg',
        0.95
      );
    });

    if (!show) return null;

    return (
      <div
        class="fixed inset-0 bg-black/60 flex items-start justify-center p-4 z-[100] transition-opacity duration-300 ease-in-out overflow-y-auto"
        onClick$={(e, el) => {
          // e.currentTarget is null in Qwik's async handlers — use the element arg.
          if (e.target === el) {
            onHide$();
          }
        }}
      >
        {/* No transform/scale on this dialog: cropperjs v2's bounds check
            mixes getBoundingClientRect (scaled) with canvas coords (unscaled),
            so a scaled ancestor makes it reject the maximized selection. */}
        <div
          class="bg-[var(--bg-header-footer)] p-6 md:p-8 rounded-xl shadow-2xl w-full max-w-lg relative my-8"
          onClick$={(e) => e.stopPropagation()}
        >
          <div class="flex justify-between items-center mb-6">
            <h2 class="text-2xl font-semibold text-[var(--text-primary)] font-varela">
              Crop Your AI Thumbnail
            </h2>
            <button
              onClick$={onHide$}
              class="p-2 rounded-full text-[var(--text-muted)] hover:bg-[var(--bg-dropdown-hover)] transition-colors"
              aria-label="Close modal"
            >
              <LuX class="w-6 h-6" />
            </button>
          </div>

          <div class="space-y-4">
            {error.value && (
              <div class="p-3 bg-red-50 dark:bg-red-900/30 rounded-lg border border-red-200 dark:border-red-700/50">
                <p class="text-sm text-red-700 dark:text-red-300">{error.value}</p>
              </div>
            )}
            {imageSrc && (
              <ImageCropper
                imageSrc={imageSrc}
                cropShape="square"
                outputSize={256}
                onCropComplete$={handleCropChange$}
              />
            )}
          </div>

          <div class="flex flex-col-reverse sm:flex-row sm:justify-end sm:space-x-3 pt-6">
            <LiquidMetalButton
              variant="secondary"
              onClick$={onHide$}
              class="mt-3 sm:mt-0 w-full sm:w-auto inline-flex justify-center px-6 py-2.5 text-base font-medium focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-[var(--focus-ring)] transition-colors"
            >
              Cancel
            </LiquidMetalButton>
            <LiquidMetalButton
              onClick$={handleSave$}
              class="w-full sm:w-auto inline-flex justify-center items-center px-6 py-2.5 text-base font-medium focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-[var(--focus-ring)] transition-colors disabled:opacity-70"
            >
              <LuCheck class="w-[18px] h-[18px] mr-2" />
              Crop &amp; Save
            </LiquidMetalButton>
          </div>
        </div>
      </div>
    );
  }
);
