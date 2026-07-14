import { component$, useSignal, useVisibleTask$, type QRL } from "@builder.io/qwik";

interface ImageCropperProps {
  imageSrc: string;
  /** "circle" for profile pics, "square" for thumbnails */
  cropShape?: "circle" | "square";
  /** Output size in pixels (default 300 for profile, 128 for thumbnails) */
  outputSize?: number;
  /** Called with the cropped canvas whenever the crop changes */
  onCropComplete$: QRL<(canvas: HTMLCanvasElement) => void>;
}

/**
 * ImageCropper Component - powered by cropperjs v2
 *
 * Provides drag, zoom, pinch-zoom, and mouse wheel zoom.
 * Circle crop for profile pictures, square crop for thumbnails.
 */
export default component$<ImageCropperProps>((props) => {
  const containerRef = useSignal<HTMLDivElement>();
  const imgRef = useSignal<HTMLImageElement>();
  // Aspect ratio of the source image (width / height) - drives the container
  // size so there's no transparent letterbox space that the user could crop into.
  const imageAspect = useSignal<number>(1);

  const shape = props.cropShape || "circle";
  const outputSize = props.outputSize || (shape === "circle" ? 300 : 128);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    const img = imgRef.value;
    const container = containerRef.value;
    if (!img || !container) return;

    // If the image already loaded before we got here, prime imageAspect
    // synchronously so the container starts with the correct ratio (avoids
    // one-frame of letterbox before the `onLoad` handler updates the signal).
    if (img.complete && img.naturalWidth && img.naturalHeight) {
      imageAspect.value = img.naturalWidth / img.naturalHeight;
    }

    // Dynamic import to avoid SSR issues
    const CropperModule = await import("cropperjs");
    const Cropper = CropperModule.default;

    // cropperjs v2 uses shadow DOM - no external CSS needed

    const cropper = new Cropper(img, {
      container,
    });

    // Configure after init - cropperjs v2 elements are custom elements
    // that appear asynchronously in the DOM
    const configTimer = setInterval(() => {
      const selection = cropper.getCropperSelection();
      if (selection) {
        selection.aspectRatio = 1;
        selection.initialAspectRatio = 1;
        selection.initialCoverage = 0.95;
        clearInterval(configTimer);

        // Size the cropper-canvas to match the image's aspect ratio exactly,
        // within the available parent width and a 500px height cap. CSS
        // aspect-ratio alone fights with `width: 100%` + `max-height` (width
        // wins, aspect is violated) producing letterbox bars. Compute in JS.
        const cropperCanvas = cropper.getCropperCanvas() as HTMLElement | null;
        const MAX_HEIGHT = 500;
        const resizeCanvasToImage = (aspect: number) => {
          if (!cropperCanvas || !container) return;
          // Use parent-of-container as the width cap (the modal inner width).
          // container.offsetWidth itself would be whatever we set last time.
          const parent = container.parentElement;
          const maxW = parent?.offsetWidth ?? 464;
          let w = maxW;
          let h = w / aspect;
          if (h > MAX_HEIGHT) {
            h = MAX_HEIGHT;
            w = h * aspect;
          }
          cropperCanvas.style.width = `${w}px`;
          cropperCanvas.style.height = `${h}px`;
          // Shrink the outer container to match so the cropper sits centered
          // in the modal instead of left-aligned with empty space on the right.
          container.style.width = `${w}px`;
          container.style.height = `${h}px`;
        };
        if (cropperCanvas) {
          resizeCanvasToImage(imageAspect.value);
          cropperCanvas.removeAttribute("background");
        }

        // Set up change listener - whenever crop changes, export the result.
        // Guard against non-square selections: cropperjs briefly emits them
        // during init before aspectRatio=1 takes effect; $toCanvas then
        // returns a canvas matching the selection's aspect (e.g. 300x51),
        // which the server's sharp cover-crop blows up into a zoomed blob.
        selection.addEventListener("change", async (e: Event) => {
          try {
            // cropperjs emits the change event BEFORE writing this.width/height,
            // so read from event.detail which has the pending new dimensions.
            const detail = (e as CustomEvent<{ x: number; y: number; width: number; height: number }>).detail;
            const w = detail?.width ?? selection.width;
            const h = detail?.height ?? selection.height;
            if (w !== h || w === 0) return;

            // Reject any pending move/resize that would push the selection
            // outside the cropper-image's visible bounds. Without this the
            // user can drag the selection into the empty canvas corners
            // that appear once the image is panned.
            if (cropperCanvas) {
              const ci = cropper.getCropperImage() as HTMLElement | null;
              const imgRect = ci?.getBoundingClientRect();
              if (imgRect && imgRect.width > 0 && imgRect.height > 0) {
                const canvasRect = cropperCanvas.getBoundingClientRect();
                const minX = imgRect.left - canvasRect.left;
                const minY = imgRect.top - canvasRect.top;
                const maxX = imgRect.right - canvasRect.left;
                const maxY = imgRect.bottom - canvasRect.top;
                const tol = 1;
                const x = detail?.x ?? selection.x;
                const y = detail?.y ?? selection.y;
                if (
                  x < minX - tol ||
                  y < minY - tol ||
                  x + w > maxX + tol ||
                  y + h > maxY + tol
                ) {
                  e.preventDefault();
                  return;
                }
              }
            }

            // $toCanvas reads this.width/this.height synchronously in its
            // Promise executor. cropperjs's $change fires the event BEFORE
            // writing those fields - so wait one microtask for state to
            // commit, otherwise $toCanvas uses stale (pre-change) dimensions.
            await Promise.resolve();
            const canvas = await selection.$toCanvas({
              width: outputSize,
              height: outputSize,
            });
            if (canvas.width !== canvas.height) return;
            props.onCropComplete$(canvas);
          } catch { /* crop not ready yet */ }
        });

        // Explicitly place a centered square selection against the CURRENT
        // cropper-canvas dimensions, using $change (which fires the change
        // event our handler listens to).
        //
        // Why not $reset / $initSelection: both keep this.x/this.y from the
        // mount-time init when the canvas hadn't laid out yet - producing a
        // zoomed top-left crop. Explicit $change with fresh coords overrides.
        //
        // Why the retry: cropper-canvas is a custom element that gets its
        // final size after ResizeObserver + shadow DOM layout. offsetWidth
        // can be 0 for a few frames.
        let retries = 0;
        const forceCenteredCrop = () => {
          const canvasEl = cropper.getCropperCanvas() as HTMLElement | null;
          const w = canvasEl?.offsetWidth ?? 0;
          const h = canvasEl?.offsetHeight ?? 0;
          if (w === 0 || h === 0) {
            if (retries++ < 30) requestAnimationFrame(forceCenteredCrop);
            return;
          }
          const size = Math.min(w, h) * 0.95;
          const x = (w - size) / 2;
          const y = (h - size) / 2;
          selection.$change(x, y, size, size);
        };

        const cropperImage = cropper.getCropperImage();
        if (cropperImage) {
          // Floor scale at fit so users can't shrink the image below the
          // canvas (which would create empty corners). Wheel/pinch zoom-in
          // is still allowed for precision crops.
          (cropperImage as HTMLElement).setAttribute("min-scale", "1");
          cropperImage.$ready((image: HTMLImageElement) => {
            // Re-read the real aspect from cropperImage now that it's loaded,
            // in case the <img> onLoad hadn't fired before we first sized
            // the cropper-canvas. Keeps the canvas tightly shaped to the
            // image and eliminates letterbox on load-race.
            if (image.naturalWidth && image.naturalHeight) {
              const a = image.naturalWidth / image.naturalHeight;
              resizeCanvasToImage(a);
              imageAspect.value = a;
            }
            cropperImage.$center("contain");
            requestAnimationFrame(forceCenteredCrop);
          });
        } else {
          requestAnimationFrame(forceCenteredCrop);
        }

        // If the cropper-canvas changes size later (because <img onLoad>
        // updated imageAspect → container aspect-ratio reflowed), re-center
        // the image inside it. Otherwise the image keeps its stale transform
        // and shows letterbox bars around the edges.
        if (typeof ResizeObserver !== "undefined" && cropperCanvas) {
          let firstResize = true;
          const ro = new ResizeObserver(() => {
            if (firstResize) { firstResize = false; return; }
            const ci = cropper.getCropperImage();
            if (ci) ci.$center("contain");
            requestAnimationFrame(forceCenteredCrop);
          });
          ro.observe(cropperCanvas as Element);
          cleanup(() => ro.disconnect());
        }
      }
    }, 100);

    cleanup(() => {
      clearInterval(configTimer);
      cropper.destroy();
    });
  });

  return (
    <div class="space-y-4">
      <div
        ref={containerRef}
        class="relative rounded-lg overflow-hidden mx-auto"
      >
        <img
          ref={imgRef}
          src={props.imageSrc}
          alt="Crop preview"
          crossOrigin="anonymous"
          onLoad$={(_, el) => {
            if (el.naturalWidth && el.naturalHeight) {
              imageAspect.value = el.naturalWidth / el.naturalHeight;
            }
          }}
          style={{ display: "block", maxWidth: "100%", maxHeight: "100%" }}
        />
      </div>

      <p class="text-xs text-[var(--text-muted)] text-center">
        Drag to position • Drag corners or sides to resize • Scroll or pinch to zoom in
      </p>
    </div>
  );
});
