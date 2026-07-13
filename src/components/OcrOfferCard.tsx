import { component$, useStore, $, type QRL } from '@builder.io/qwik';
import { invoke } from '@tauri-apps/api/core';
import LiquidMetalButton from './LiquidMetalButton';
import { modelManager } from '../utils/modelManager';
import { OCR_MODELS } from '../data/recommended-models';
import type { AttachedFile } from '../types';

interface OcrOfferCardProps {
  file: { filePath: string; filename: string };
  /** Called with the attachable file once OCR produces text. */
  onAttach$: QRL<(f: AttachedFile) => void>;
  onCancel$: QRL<() => void>;
}

/**
 * Shown when a scanned PDF is attached but the optional OCR models aren't
 * installed. Downloads the two ocrs models (with progress), runs OCR on the
 * pending PDF, and hands back the extracted text to attach. All on-device.
 */
export const OcrOfferCard = component$<OcrOfferCardProps>((props) => {
  const store = useStore({
    phase: 'idle' as 'idle' | 'downloading' | 'reading' | 'error',
    percent: 0,
    error: '',
  });

  const start$ = $(async () => {
    store.error = '';
    store.percent = 0;
    store.phase = 'downloading';
    try {
      const files = OCR_MODELS.files ?? [
        { filename: OCR_MODELS.filename, downloadUrl: OCR_MODELS.downloadUrl },
      ];
      for (let i = 0; i < files.length; i++) {
        await modelManager.downloadModel(
          files[i].downloadUrl,
          files[i].filename,
          (p) => (store.percent = Math.round(((i + p.percent / 100) / files.length) * 100)),
        );
      }
      store.phase = 'reading';
      const text = await invoke<string>('ocr_scanned_pdf', { filePath: props.file.filePath });
      if (!text || !text.trim()) {
        store.phase = 'error';
        store.error = 'No readable text found in this scan.';
        return;
      }
      props.onAttach$({
        filename: props.file.filename,
        path: props.file.filePath,
        sizeBytes: new TextEncoder().encode(text).length,
        content: text,
        truncated: false,
        estimatedTokens: Math.ceil(text.length / 4),
      });
    } catch (e) {
      store.phase = 'error';
      store.error = e instanceof Error ? e.message : 'Something went wrong';
    }
  });

  return (
    <div class="bg-yellow-50 dark:bg-yellow-900/30 border border-yellow-200 dark:border-yellow-700 rounded-lg p-4">
      <p class="text-sm font-medium text-yellow-800 dark:text-yellow-200 mb-1">
        “{props.file.filename}” looks like a scanned document
      </p>

      {store.phase === 'idle' && (
        <>
          <p class="text-sm text-yellow-700 dark:text-yellow-300 mb-3">
            Add scanned-document reading (~30 MB, one time) so this PDF can be read,
            summarised, and source-grounded — all on your device.
          </p>
          <div class="flex items-center gap-3">
            <LiquidMetalButton onClick$={start$} class="px-4 py-2 text-sm">
              Add &amp; read it
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

      {store.phase === 'downloading' && (
        <div class="mt-2">
          <p class="text-sm text-yellow-700 dark:text-yellow-300 mb-2">Downloading OCR…</p>
          <div class="w-full h-2 rounded-full bg-yellow-200 dark:bg-yellow-800 overflow-hidden">
            <div
              class="h-full bg-yellow-500 dark:bg-yellow-400 transition-all duration-200"
              style={{ width: `${store.percent}%` }}
            />
          </div>
          <p class="text-xs text-yellow-700 dark:text-yellow-300 mt-1">{store.percent}%</p>
        </div>
      )}

      {store.phase === 'reading' && (
        <p class="text-sm text-yellow-700 dark:text-yellow-300 flex items-center gap-1.5">
          <span class="inline-block w-1.5 h-1.5 rounded-full bg-yellow-500 animate-pulse" />
          Reading the document…
        </p>
      )}

      {store.phase === 'error' && (
        <>
          <p class="text-sm text-red-600 dark:text-red-400 mb-3">{store.error}</p>
          <div class="flex items-center gap-3">
            <LiquidMetalButton onClick$={start$} class="px-4 py-2 text-sm">
              Try again
            </LiquidMetalButton>
            <button
              onClick$={props.onCancel$}
              class="text-sm text-yellow-700 dark:text-yellow-300 hover:underline"
            >
              Cancel
            </button>
          </div>
        </>
      )}
    </div>
  );
});
