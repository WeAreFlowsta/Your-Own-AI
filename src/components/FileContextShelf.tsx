import { component$, $, type Signal } from '@builder.io/qwik';
import { LuX, LuFile } from '@qwikest/icons/lucide';
import type { AttachedFile, AttachedImage } from '../types';

interface FileContextShelfProps {
  files: Signal<AttachedFile[]>;
  images?: Signal<AttachedImage[]>;
  contextWindowSize: number;
}

function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

/** Traffic light: green < 25%, amber < 75%, red >= 75% of context window */
function getFileStatus(tokens: number, contextWindow: number): 'green' | 'amber' | 'red' {
  const ratio = tokens / contextWindow;
  if (ratio < 0.15) return 'green';
  if (ratio < 0.4) return 'amber';
  return 'red';
}

function getTotalStatus(totalTokens: number, contextWindow: number): 'green' | 'amber' | 'red' {
  const ratio = totalTokens / contextWindow;
  if (ratio < 0.5) return 'green';
  if (ratio < 0.75) return 'amber';
  return 'red';
}

const statusColors = {
  green: 'bg-green-500',
  amber: 'bg-amber-500',
  red: 'bg-red-500',
};

const statusTextColors = {
  green: 'text-[var(--text-muted)]',
  amber: 'text-amber-600 dark:text-amber-400',
  red: 'text-red-600 dark:text-red-400',
};

export const FileContextShelf = component$<FileContextShelfProps>((props) => {
  const files = props.files.value;
  const images = props.images?.value ?? [];
  if (files.length === 0 && images.length === 0) return null;

  const totalTokens = files.reduce((sum, f) => sum + f.estimatedTokens, 0);
  const totalStatus = getTotalStatus(totalTokens, props.contextWindowSize);
  const itemCount = files.length + images.length;

  const removeFile = $((path: string) => {
    props.files.value = props.files.value.filter(f => f.path !== path);
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
    <div class="mt-3 rounded-xl border border-[var(--border-subtle)] bg-[var(--bg-card)] overflow-hidden">
      {/* Header */}
      <div class="flex items-center justify-between px-3 py-1.5 border-b border-[var(--border-subtle)]">
        <span class="text-xs font-medium text-[var(--text-secondary)]">
          Context ({itemCount} {itemCount === 1 ? 'item' : 'items'})
        </span>
        <button
          type="button"
          onClick$={clearAll}
          class="text-[var(--text-muted)] hover:text-[var(--text-primary)] transition-colors"
          title="Remove all files"
        >
          <LuX class="w-3.5 h-3.5" />
        </button>
      </div>

      {/* File list */}
      <div class="px-3 py-1.5 space-y-1">
        {files.map((file) => {
          const status = getFileStatus(file.estimatedTokens, props.contextWindowSize);
          return (
            <div key={file.path} class="flex items-center gap-2 group">
              {/* Traffic light dot */}
              <span class={`w-2 h-2 rounded-full shrink-0 ${statusColors[status]}`} />
              {/* File icon + name */}
              <LuFile class="w-3.5 h-3.5 text-[var(--text-muted)] shrink-0" />
              <span class="text-xs text-[var(--text-primary)] truncate flex-1" title={file.path}>
                {file.filename}
              </span>
              {/* Size */}
              <span class="text-[10px] text-[var(--text-muted)] shrink-0">
                {formatSize(file.sizeBytes)}
              </span>
              {/* Remove */}
              <button
                type="button"
                onClick$={() => removeFile(file.path)}
                class="text-[var(--text-muted)] hover:text-red-500 transition-colors opacity-0 group-hover:opacity-100 shrink-0"
                title="Remove file"
              >
                <LuX class="w-3 h-3" />
              </button>
            </div>
          );
        })}
      </div>

      {/* Image thumbnails (vision turns) */}
      {images.length > 0 && (
        <div class="px-3 py-2 flex flex-wrap gap-2 border-t border-[var(--border-subtle)]">
          {images.map((img, idx) => (
            <div key={idx} class="relative group">
              <img
                src={img.dataUrl}
                alt={img.filename}
                width={48}
                height={48}
                class="w-12 h-12 object-cover rounded-md border border-[var(--border-subtle)]"
              />
              <button
                type="button"
                onClick$={() => removeImage(idx)}
                title="Remove image"
                class="absolute -top-1.5 -right-1.5 bg-[var(--bg-card)] rounded-full border border-[var(--border-subtle)] text-[var(--text-muted)] hover:text-red-500 opacity-0 group-hover:opacity-100 transition-opacity"
              >
                <LuX class="w-3 h-3" />
              </button>
            </div>
          ))}
        </div>
      )}

      {/* Token budget footer (text attachments) */}
      {files.length > 0 && (
        <div class={`px-3 py-1 border-t border-[var(--border-subtle)] text-right ${statusTextColors[totalStatus]}`}>
          <span class="text-[10px]">
            ~{totalTokens.toLocaleString()} / {props.contextWindowSize.toLocaleString()} tokens
          </span>
        </div>
      )}
    </div>
  );
});
