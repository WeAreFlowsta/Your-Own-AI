import {
  component$,
  useSignal,
  useVisibleTask$,
  $,
  type QRL,
  noSerialize,
  type NoSerialize,
} from '@builder.io/qwik';
import { LuX, LuCheck } from '@qwikest/icons/lucide';
import LiquidMetalButton from './LiquidMetalButton';
import Cropper from 'cropperjs';
import 'cropperjs/dist/cropper.css';

interface ImageCropModalProps {
  show: boolean;
  onHide$: QRL<() => void>;
  imageSrc: string | null;
  onCropComplete$: QRL<(croppedImageBlob: Blob) => void>;
}

export const ImageCropModal = component$<ImageCropModalProps>(
  ({ show, onHide$, imageSrc, onCropComplete$ }) => {
    const imgRef = useSignal<HTMLImageElement>();
    const cropperRef = useSignal<NoSerialize<Cropper>>();
    const error = useSignal<string | null>(null);

    // Initialize cropperjs when image loads and modal is shown
    // eslint-disable-next-line qwik/no-use-visible-task
    useVisibleTask$(({ track, cleanup }) => {
      track(() => show);
      track(() => imageSrc);

      if (!show || !imageSrc) return;

      // Wait for the image element to be rendered
      const timer = setTimeout(() => {
        const imgEl = imgRef.value;
        if (!imgEl) return;

        const cropper = new Cropper(imgEl, {
          aspectRatio: 1,
          viewMode: 1,
          autoCropArea: 0.9,
          responsive: true,
          guides: false,
          center: true,
          highlight: false,
          cropBoxMovable: true,
          cropBoxResizable: true,
          toggleDragModeOnDblclick: false,
        });

        cropperRef.value = noSerialize(cropper);
      }, 100);

      cleanup(() => {
        clearTimeout(timer);
        if (cropperRef.value) {
          cropperRef.value.destroy();
          cropperRef.value = undefined;
        }
      });
    });

    const handleCrop$ = $(async () => {
      error.value = null;
      const cropper = cropperRef.value;
      if (!cropper) {
        error.value = 'Could not crop image. Please try again.';
        return;
      }

      const canvas = cropper.getCroppedCanvas({
        width: 256,
        height: 256,
        imageSmoothingEnabled: true,
        imageSmoothingQuality: 'high',
      });

      if (!canvas) {
        error.value = 'Could not process image. Please try again.';
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
        <div
          class="bg-[var(--bg-header-footer)] p-6 md:p-8 rounded-xl shadow-2xl w-full max-w-lg transform transition-all duration-300 ease-in-out scale-95 relative my-8"
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
              <div class="flex justify-center bg-black/20 rounded-lg p-4 cropper-circle-overlay">
                <img
                  ref={imgRef}
                  alt="Crop me"
                  src={imageSrc}
                  style={{ maxHeight: '60vh', objectFit: 'contain' }}
                />
              </div>
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
              onClick$={handleCrop$}
              class="w-full sm:w-auto inline-flex justify-center items-center px-6 py-2.5 text-base font-medium focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-[var(--focus-ring)] transition-colors disabled:opacity-70"
            >
              <LuCheck class="w-[18px] h-[18px] mr-2" />
              Crop & Save
            </LiquidMetalButton>
          </div>
        </div>
      </div>
    );
  }
);
