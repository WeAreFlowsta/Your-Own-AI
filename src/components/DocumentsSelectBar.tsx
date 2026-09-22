import { component$, useSignal, $, type QRL, type Signal } from '@builder.io/qwik';
import ConfirmModal from './ConfirmModal';

/**
 * Choosing several documents at once: select all shown, remove the chosen
 * ones after one confirmation. Shown above a list of two or more; the list
 * owns the `selected` ids and the removal of one document.
 */
export const DocumentsSelectBar = component$<{
  docIds: string[];
  selected: Signal<string[]>;
  onRemoveOne$: QRL<(docId: string) => Promise<void> | void>;
  onDone$?: QRL<() => void>;
}>((props) => {
  const confirm = useSignal(false);
  const busy = useSignal(false);
  const all = props.docIds.length > 0 && props.docIds.every((id) => props.selected.value.includes(id));
  const n = props.selected.value.length;
  if (props.docIds.length < 2) return null;
  return (
    <div class="flex flex-wrap items-center justify-between gap-2 text-xs text-[var(--text-muted)]">
      <label class="inline-flex items-center gap-2 cursor-pointer select-none">
        <input
          type="checkbox"
          checked={all}
          onChange$={() => {
            props.selected.value = all ? [] : [...props.docIds];
            confirm.value = false;
          }}
        />
        <span>{all ? `All ${props.docIds.length} chosen` : `Select all ${props.docIds.length}`}</span>
      </label>
      {n > 0 && !confirm.value && (
        <span class="inline-flex items-center gap-2">
          <button type="button" onClick$={() => { props.selected.value = []; }} class="px-2.5 py-1 rounded-full border border-[var(--border-subtle)] text-[var(--text-secondary)] hover:text-[var(--text-primary)]">
            Clear
          </button>
          <button type="button" onClick$={() => { confirm.value = true; }} class="px-3 py-1 rounded-full border border-red-500/40 text-red-400 hover:bg-red-500/10">
            Remove {n} chosen
          </button>
        </span>
      )}
      <ConfirmModal
        isOpen={confirm.value}
        title={n === 1 ? 'Remove this document?' : `Remove these ${n} documents?`}
        message={`${n === 1 ? 'It leaves' : 'They leave'} this AI's documents, and ${n === 1 ? 'its card goes' : 'their cards go'} too. The ${n === 1 ? 'file itself stays' : 'files themselves stay'} where ${n === 1 ? 'it is' : 'they are'}.`}
        confirmLabel="Remove"
        cancelLabel="Keep"
        variant="danger"
        busy={busy.value}
        onConfirm$={$(async () => {
          busy.value = true;
          try {
            for (const id of [...props.selected.value]) await props.onRemoveOne$(id);
            props.selected.value = [];
          } finally {
            busy.value = false;
            confirm.value = false;
            await props.onDone$?.();
          }
        })}
        onCancel$={$(() => { confirm.value = false; })}
      />
    </div>
  );
});

export default DocumentsSelectBar;
