import { component$, useSignal, useVisibleTask$, type QRL } from '@builder.io/qwik';
import { invoke } from '@tauri-apps/api/core';
import LiquidMetalButton from './LiquidMetalButton';

/**
 * Fine-tune one model on this computer (FINE_TUNE_PANEL layer 2). Empty
 * fields = the automatics decide, shown in the placeholder. Saving offers
 * the truth about when it applies: reloaded now when this model is the
 * loaded one, otherwise at its next load.
 */
interface ModelTuneDialogProps {
  model: string;
  /** The context the automatics start this model with (fit's number). */
  autoCtx?: number;
  /** MoE model: the automatics' expert-layers-on-CPU pick, when known. */
  isMoe: boolean;
  autoMoeN?: number | null;
  /** A speed-up draft file is registered for this model. */
  hasDraft: boolean;
  onClose$: QRL<() => void>;
}

export default component$<ModelTuneDialogProps>((props) => {
  const ctx = useSignal<string>('');
  const moeN = useSignal<string>('');
  const draftOff = useSignal(false);
  const note = useSignal('');
  const busy = useSignal(false);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    try {
      const t = await invoke<{ context?: number; moe_cpu_layers?: number; draft_off?: boolean }>(
        'tuning_get',
        { model: props.model },
      );
      ctx.value = t.context != null ? String(t.context) : '';
      moeN.value = t.moe_cpu_layers != null ? String(t.moe_cpu_layers) : '';
      draftOff.value = !!t.draft_off;
    } catch {
      /* fresh dialog */
    }
  });

  const inputClass =
    'mt-1 w-full bg-[var(--bg-input)] text-[var(--text-primary)] rounded-full px-4 py-2 text-sm border border-[var(--border-subtle)] focus:outline-none';

  return (
    <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
      <div class="w-full max-w-md rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-header-footer)] p-6 shadow-2xl">
        <h3 class="text-base font-semibold text-[var(--text-primary)]">Fine-tune {props.model.replace(/\.gguf$/, '')}</h3>
        <p class="mt-2 text-xs text-[var(--text-muted)]">
          For this computer only. Empty = automatic (the number in the box). A change applies when the
          model next loads - Save reloads it right away if it is running now.
        </p>
        <div class="mt-4 space-y-3">
          <label class="block text-xs text-[var(--text-secondary)]">
            Context size (tokens of reading room)
            <input
              type="number" min="4096" step="4096" value={ctx.value}
              placeholder={props.autoCtx ? `Auto (${props.autoCtx})` : 'Auto'}
              onInput$={(_, el) => { ctx.value = el.value; note.value = ''; }}
              class={inputClass}
            />
          </label>
          {ctx.value !== '' && props.autoCtx && Number(ctx.value) > props.autoCtx && (
            <p class="text-xs text-amber-500">
              More than the automatics predict fits on this machine - it may load slowly or fail; the
              model's trained limit still caps it.
            </p>
          )}
          {props.isMoe && (
            <label class="block text-xs text-[var(--text-secondary)]">
              Expert layers in main memory (0 = everything on the card)
              <input
                type="number" min="0" step="1" value={moeN.value}
                placeholder={props.autoMoeN != null ? `Auto (${props.autoMoeN})` : 'Auto'}
                onInput$={(_, el) => { moeN.value = el.value; note.value = ''; }}
                class={inputClass}
              />
            </label>
          )}
          {props.hasDraft && (
            <label class="flex items-center gap-2 text-xs text-[var(--text-secondary)]">
              <input
                type="checkbox" checked={draftOff.value}
                onChange$={(_, el) => { draftOff.value = el.checked; note.value = ''; }}
                class="h-4 w-4 rounded accent-[var(--text-link)]"
              />
              Leave the speed-up file out (measure the difference yourself)
            </label>
          )}
          {note.value && <p class="text-xs text-[var(--text-secondary)]">{note.value}</p>}
        </div>
        <div class="mt-5 flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
          <LiquidMetalButton variant="secondary" class="px-4 py-2 text-sm" onClick$={props.onClose$}>
            Close
          </LiquidMetalButton>
          <LiquidMetalButton
            class="px-4 py-2 text-sm"
            disabled={busy.value}
            onClick$={async () => {
              busy.value = true;
              try {
                const tuning: Record<string, unknown> = {};
                if (ctx.value !== '' && Number(ctx.value) > 0) tuning.context = Math.round(Number(ctx.value));
                if (moeN.value !== '' && Number(moeN.value) >= 0) tuning.moe_cpu_layers = Math.round(Number(moeN.value));
                if (draftOff.value) tuning.draft_off = true;
                await invoke('tuning_set', { model: props.model, tuning });
                const reloaded = await invoke<boolean>('tuning_apply_now', { model: props.model });
                note.value = reloaded
                  ? 'Reloaded with your settings.'
                  : Object.keys(tuning).length
                    ? 'Saved - applies when this model next loads.'
                    : 'Back to automatic - applies when this model next loads.';
              } catch (e) {
                note.value = `Could not save: ${e}`;
              } finally {
                busy.value = false;
              }
            }}
          >
            Save
          </LiquidMetalButton>
        </div>
      </div>
    </div>
  );
});
