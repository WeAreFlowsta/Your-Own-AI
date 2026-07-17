import { component$, type QRL } from '@builder.io/qwik';
import { LuX } from '@qwikest/icons/lucide';
import LiquidMetalButton from './LiquidMetalButton';
import {
  THUMBNAIL_GALLERY,
  GALLERY_GROUP_LABELS,
  type GalleryGroup,
  type GalleryThumb,
} from '../data/thumbnail-gallery';

interface ThumbnailGalleryModalProps {
  show: boolean;
  onHide$: QRL<() => void>;
  onSelect$: QRL<(thumb: GalleryThumb) => void>;
  /** Path of the gallery thumb currently in use, for the selected ring */
  selectedPath?: string | null;
  /** The current personality's default art, badged so it's easy to find */
  personalityPath?: string | null;
}

// Colors and Gradients lead: they're safe, neutral picks for any audience.
// Portrait and character art follows for those who want a face instead.
const GROUP_ORDER: GalleryGroup[] = ['colors', 'gradients', 'people', 'characters'];

export const ThumbnailGalleryModal = component$<ThumbnailGalleryModalProps>(
  ({ show, onHide$, onSelect$, selectedPath, personalityPath }) => {
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
          class="bg-[var(--bg-header-footer)] p-6 md:p-8 rounded-xl shadow-2xl w-full max-w-2xl transform transition-all duration-300 ease-in-out relative my-8"
          onClick$={(e) => e.stopPropagation()}
        >
          <div class="flex justify-between items-center mb-6">
            <h2 class="text-2xl font-semibold text-[var(--text-primary)] font-varela">
              Choose a Thumbnail
            </h2>
            <button
              onClick$={onHide$}
              class="p-2 rounded-full text-[var(--text-muted)] hover:bg-[var(--bg-dropdown-hover)] transition-colors"
              aria-label="Close modal"
            >
              <LuX class="w-6 h-6" />
            </button>
          </div>

          <div class="space-y-6">
            {GROUP_ORDER.map((group) => (
              <div key={group}>
                <p class="text-sm font-medium text-[var(--text-secondary)] mb-3">
                  {GALLERY_GROUP_LABELS[group]}
                </p>
                <div class="grid grid-cols-4 sm:grid-cols-5 md:grid-cols-6 gap-3">
                  {THUMBNAIL_GALLERY.filter((t) => t.group === group).map((thumb) => {
                    const isSelected = selectedPath === thumb.path;
                    const isPersonality = personalityPath === thumb.path;
                    return (
                      <button
                        key={thumb.id}
                        type="button"
                        title={
                          isPersonality ? `${thumb.name} (current personality)` : thumb.name
                        }
                        onClick$={() => onSelect$(thumb)}
                        class="group flex flex-col items-center space-y-1 focus:outline-none"
                      >
                        <span
                          class={`relative block w-14 h-14 sm:w-16 sm:h-16 rounded-full overflow-hidden border transition-all ${
                            isSelected
                              ? 'border-transparent ring-2 ring-[var(--focus-ring)]'
                              : 'border-[var(--border-subtle)] group-hover:ring-2 group-hover:ring-[var(--focus-ring)]'
                          }`}
                        >
                          <img
                            src={thumb.path}
                            alt={thumb.name}
                            width={64}
                            height={64}
                            loading="lazy"
                            class="w-full h-full object-cover object-center"
                          />
                          {isPersonality && (
                            <span class="absolute bottom-0 inset-x-0 bg-black/60 text-[9px] leading-3 text-white text-center py-0.5">
                              current
                            </span>
                          )}
                        </span>
                        <span class="text-[10px] leading-tight text-[var(--text-muted)] group-hover:text-[var(--text-secondary)] text-center truncate w-full">
                          {thumb.name}
                        </span>
                      </button>
                    );
                  })}
                </div>
              </div>
            ))}
          </div>

          <div class="flex justify-end pt-6">
            <LiquidMetalButton
              variant="secondary"
              onClick$={onHide$}
              class="w-full sm:w-auto inline-flex justify-center px-6 py-2.5 text-base font-medium focus:outline-none focus:ring-2 focus:ring-offset-2 focus:ring-[var(--focus-ring)] transition-colors"
            >
              Cancel
            </LiquidMetalButton>
          </div>
        </div>
      </div>
    );
  }
);
