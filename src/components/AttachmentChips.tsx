import { component$, $, type Signal } from '@builder.io/qwik';
import { LuX, LuFile } from '@qwikest/icons/lucide';
import type { AttachedFile, AttachedImage } from '../types';

/**
 * Attached files/images as chips INSIDE the input field (rendered by
 * ContentEditor above the text line - visually in-field without putting
 * anything inside the contenteditable itself, which Qwik reconciliation
 * and caret handling both punish; see the ContentEditor action-chip notes).
 *
 * Replaces the old FileContextShelf box below the form. The per-file
 * traffic light lives on as the file icon's color; full path + token
 * estimate move to the tooltip; the total-budget line only appears once
 * the total turns amber - quiet while everything comfortably fits.
 */
interface AttachmentChipsProps {
  files: Signal<AttachedFile[]>;
  images?: Signal<AttachedImage[]>;
  contextWindowSize: number;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** Traffic light per file: share of the context window it eats alone. */
function fileStatus(tokens: number, contextWindow: number): 'green' | 'amber' | 'red' {
  const ratio = tokens / contextWindow;
  if (ratio < 0.15) return 'green';
  if (ratio < 0.4) return 'amber';
  return 'red';
}

function totalStatusOf(totalTokens: number, contextWindow: number): 'green' | 'amber' | 'red' {
  const ratio = totalTokens / contextWindow;
  if (ratio < 0.5) return 'green';
  if (ratio < 0.75) return 'amber';
  return 'red';
}

const iconColors = {
  green: 'text-[var(--text-muted)]',
  amber: 'text-amber-500',
  red: 'text-red-500',
};

const totalTextColors = {
  green: 'text-[var(--text-muted)]',
  amber: 'text-amber-600 dark:text-amber-400',
  red: 'text-red-600 dark:text-red-400',
};

export const AttachmentChips = component$<AttachmentChipsProps>((props) => {
  const files = props.files.value;
  const images = props.images?.value ?? [];
  if (files.length === 0 && images.length === 0) return null;

  const totalTokens = files.reduce((sum, f) => sum + f.estimatedTokens, 0);
  const totalStatus = totalStatusOf(totalTokens, props.contextWindowSize);
  const itemCount = files.length + images.length;

  const removeFile = $((path: string) => {
    props.files.value = props.files.value.filter((f) => f.path !== path);
  });

  const removeImage = $((idx: number) => {
    if (!props.images) return;
    props.images.value = props.images.value.filter((_, i) => i !== idx);
  });

  const clearAll = $(() => {
    props.files.value = [];
    if (props.images) props.images.value = [];
  });

  return (
    <div class="px-2 pt-1.5 pb-0.5">
      <div class="flex flex-wrap items-center gap-1.5">
        {images.map((img, idx) => (
          <span key={`img-${idx}`} class="relative shrink-0" title={img.filename}>
            <img
              src={img.dataUrl}
              alt={img.filename}
              width={32}
              height={32}
              class="w-8 h-8 object-cover rounded-lg border border-[var(--border-subtle)]"
            />
            <button
              type="button"
              onClick$={() => removeImage(idx)}
              title="Remove image"
              class="absolute -top-1.5 -right-1.5 bg-[var(--bg-dropdown)] rounded-full border border-[var(--border-subtle)] text-[var(--text-muted)] hover:text-red-500 transition-colors"
            >
              <LuX class="w-3 h-3" />
            </button>
          </span>
        ))}

        {files.map((file) => {
          const status = fileStatus(file.estimatedTokens, props.contextWindowSize);
          return (
            <span
              key={file.path}
              class="flex items-center gap-1.5 max-w-[14rem] rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-dropdown)] pl-2 pr-1 py-1"
              title={`${file.path} · ~${file.estimatedTokens.toLocaleString()} tokens${
                file.truncated ? ' · large file, read partially' : ''
              }`}
            >
              <LuFile class={`w-3.5 h-3.5 shrink-0 ${iconColors[status]}`} />
              <span class="text-xs text-[var(--text-primary)] truncate">{file.filename}</span>
              <span class="text-[10px] text-[var(--text-muted)] shrink-0">
                {formatSize(file.sizeBytes)}
              </span>
              <button
                type="button"
                onClick$={() => removeFile(file.path)}
                title="Remove file"
                class="shrink-0 rounded-full p-0.5 text-[var(--text-muted)] hover:text-red-500 transition-colors"
              >
                <LuX class="w-3 h-3" />
              </button>
            </span>
          );
        })}

        {itemCount >= 3 && (
          <button
            type="button"
            onClick$={clearAll}
            class="text-[10px] text-[var(--text-muted)] hover:text-[var(--text-primary)] transition-colors px-1"
          >
            Clear all
          </button>
        )}
      </div>

      {/* Budget line: silent while green, honest once the total bites. */}
      {files.length > 0 && totalStatus !== 'green' && (
        <div class={`text-right text-[10px] mt-0.5 ${totalTextColors[totalStatus]}`}>
          ~{totalTokens.toLocaleString()} / {props.contextWindowSize.toLocaleString()} tokens
        </div>
      )}
    </div>
  );
});
