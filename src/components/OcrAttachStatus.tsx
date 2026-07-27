import { component$, $, type Signal } from '@builder.io/qwik';
import type { AttachedFile } from '../types';
import { OcrOfferCard } from './OcrOfferCard';

interface OcrAttachStatusProps {
  attachedFiles: Signal<AttachedFile[]>;
  ocrProcessing: Signal<string | null>;
  ocrNeeded: Signal<{ filePath: string; filename: string } | null>;
  /** Filename being read into context (any attachment type). */
  reading?: Signal<string | null>;
}

/**
 * Scanned-PDF OCR status shown with the attachment shelf: a "reading…" notice
 * while OCR runs, or the download offer when the models aren't installed. Its own
 * small component (signals passed as props, single return, no early-return) so the
 * reads have a clean reactive scope — see Qwik gotchas #2 and #10.
 */
export const OcrAttachStatus = component$<OcrAttachStatusProps>((props) => {
  return (
    <>
      {props.reading?.value && !props.ocrProcessing.value && (
        <div class="mt-2 flex items-center gap-2.5 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-card)] px-3 py-2 text-xs text-[var(--text-secondary)]">
          <span class="inline-block w-2 h-2 rounded-full bg-[var(--bg-button-primary)] animate-pulse shrink-0" />
          Reading “{props.reading.value}”…
        </div>
      )}

      {props.ocrProcessing.value && (
        <div class="mt-2 flex items-center gap-2.5 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-card)] px-3 py-2 text-xs text-[var(--text-secondary)]">
          <span class="inline-block w-2 h-2 rounded-full bg-[var(--bg-button-primary)] animate-pulse shrink-0" />
          Reading “{props.ocrProcessing.value}” — scanned documents can take a moment.
        </div>
      )}

      {props.ocrNeeded.value && (
        <div class="mt-2">
          <OcrOfferCard
            file={props.ocrNeeded.value}
            onAttach$={$((f) => {
              props.attachedFiles.value = [...props.attachedFiles.value, f];
              props.ocrNeeded.value = null;
            })}
            onCancel$={$(() => {
              props.ocrNeeded.value = null;
            })}
          />
        </div>
      )}
    </>
  );
});
