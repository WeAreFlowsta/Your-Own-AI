import { component$, useSignal, useVisibleTask$, type QRL } from '@builder.io/qwik';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

interface TuneResult {
  ctx: number;
  moe_cpu_layers?: number | null;
  draft: boolean;
  kv_q8?: boolean;
  load_secs: number;
  pp_tps: number;
  gen_tps: number;
  failed?: string | null;
}
import LiquidMetalButton from './LiquidMetalButton';
import TuneSlider from './TuneSlider';

/**
 * Fine-tune one model on this computer (FINE_TUNE_PANEL layer 2). Empty
 * fields = the automatics decide, shown in the placeholder. Saving offers
 * the truth about when it applies: reloaded now when this model is the
 * loaded one, otherwise at its next load.
 */
interface ModelTuneDialogProps {
  model: string;
  /** The model's trained context limit (caps the slider). */
  maxCtx?: number;
  /** MoE: total expert-carrying layers (caps the split slider). */
  nLayers?: number;
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
  const ctx = useSignal<number | null>(null);
  const moeN = useSignal<number | null>(null);
  const draftOff = useSignal(false);
  /** KV cache precision: 'auto' | 'f16' (standard) | 'q8_0' (compact). */
  const kv = useSignal<'auto' | 'f16' | 'q8_0'>('auto');
  const note = useSignal('');
  const busy = useSignal(false);
  const results = useSignal<TuneResult[]>([]);
  const tuning = useSignal<{ done: number; total: number; current: string } | null>(null);
  const sliderPos = useSignal(0);

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async () => {
    try {
      const t = await invoke<{ context?: number; moe_cpu_layers?: number; draft_off?: boolean; kv_cache?: string }>(
        'tuning_get',
        { model: props.model },
      );
      ctx.value = t.context ?? null;
      moeN.value = t.moe_cpu_layers ?? null;
      draftOff.value = !!t.draft_off;
      kv.value = t.kv_cache === 'q8_0' ? 'q8_0' : t.kv_cache === 'f16' ? 'f16' : 'auto';
    } catch {
      /* fresh dialog */
    }
    try {
      const p = await invoke<{ results: TuneResult[] } | null>('tune_profiles_get', { model: props.model });
      if (p?.results) {
        results.value = p.results;
        // Open on the saved pick, not the leftmost: find the position whose
        // context matches the saved (or automatic) value.
        const ok = p.results.filter((r) => !r.failed);
        const byCtx = [...new Map(ok.map((r) => [r.ctx, r])).keys()].sort((a, b) => a - b);
        const current = ctx.value ?? props.autoCtx;
        if (current != null) {
          let best = 0;
          byCtx.forEach((c, i) => { if (Math.abs(c - current) < Math.abs(byCtx[best] - current)) best = i; });
          sliderPos.value = best;
        }
      }
    } catch { /* no profile yet */ }
  });

  // Tune-run progress (chats pause while it runs; every number measured here).
  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    const un = await listen<{ model: string; done: number; total: number; current: string }>('tune-run', (e) => {
      if (e.payload.model !== props.model) return;
      tuning.value = e.payload.done >= e.payload.total ? null : e.payload;
    });
    cleanup(() => un());
  });


  return (
    <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
      <div class="w-full max-w-md max-h-[calc(100vh-2rem)] overflow-y-auto rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-header-footer)] p-6 shadow-2xl">
        <h3 class="text-base font-semibold text-[var(--text-primary)]">Fine-tune {props.model.replace(/\.gguf$/, '')}</h3>
        <p class="mt-2 text-xs text-[var(--text-muted)]">
          For this computer only. Automatic unless you move a slider - Auto puts a row back. A change
          applies when the model next loads; Save reloads it right away if it is running now.
        </p>
        <div class="mt-4 space-y-4">
          <TuneSlider label="Context size (tokens of reading room)" value={ctx.value}
            autoLabel={props.autoCtx ? `Auto (${props.autoCtx})` : 'Auto'} autoValue={props.autoCtx}
            ticks={[4096, 8192, 16384, 32768, 65536, 131072].filter((c) => !props.maxCtx || c <= props.maxCtx)}
            unit="tokens"
            onChange$={(v) => { ctx.value = v; note.value = ''; }} />
          {ctx.value != null && props.autoCtx && ctx.value > props.autoCtx && (
            <p class="text-xs text-amber-500">
              More than the automatics predict fits on this machine - it may load slowly or fail; the
              model's trained limit still caps it.
            </p>
          )}
          {props.isMoe && moeN.value != null && props.autoMoeN != null && props.autoMoeN > 0 && moeN.value < props.autoMoeN && (
            <p class="text-xs text-amber-500">
              Fewer expert layers in main memory than the automatics pick ({props.autoMoeN}) - the rest
              must fit on the card, and may not.
            </p>
          )}
          {props.isMoe && (
            <TuneSlider label="Expert layers in main memory (0 = everything on the card)" value={moeN.value}
              autoLabel={props.autoMoeN != null ? `Auto (${props.autoMoeN})` : 'Auto'}
              autoValue={props.autoMoeN ?? 0}
              min={0} max={props.nLayers || 64} step={1} unit="layers"
              onChange$={(v) => { moeN.value = v; note.value = ''; }} />
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
          <div>
            <p class="text-xs text-[var(--text-secondary)]">Context memory (KV cache)</p>
            <div class="mt-1 flex gap-1">
              {(['auto', 'f16', 'q8_0'] as const).map((opt) => (
                <button
                  key={opt}
                  type="button"
                  onClick$={() => { kv.value = opt; note.value = ''; }}
                  class={`rounded-full border px-2.5 py-0.5 text-xs transition-colors ${
                    kv.value === opt
                      ? 'border-[var(--text-link)] text-[var(--text-primary)]'
                      : 'border-[var(--border-subtle)] text-[var(--text-muted)] hover:text-[var(--text-secondary)]'
                  }`}
                >
                  {opt === 'auto' ? 'Auto' : opt === 'f16' ? 'Standard' : 'Compact'}
                </button>
              ))}
            </div>
            <p class="mt-1 text-[11px] text-[var(--text-muted)]">
              Compact halves the memory the context takes, so more fits on the card. Auto uses it only
              once Tune on this computer has measured it clean for this model here.
            </p>
          </div>
          {note.value && <p class="text-xs text-[var(--text-secondary)]">{note.value}</p>}
        </div>
        <div class="mt-4 border-t border-[var(--border-subtle)] pt-3">
          <p class="text-xs text-[var(--text-muted)]">
            Tune on this computer measures a handful of setups on your own hardware - context sizes,
            the expert split, the speed-up file, the compact cache. It takes a few minutes, loads the model repeatedly,
            and chats pause while it runs. Every number below was measured here, not promised.
          </p>
          {tuning.value ? (
            <div class="mt-2 flex items-center gap-3 text-xs text-[var(--text-secondary)]">
              <span class="inline-block h-3 w-3 rounded-full border-2 border-[var(--border-subtle)] border-t-[var(--text-secondary)] animate-spin" />
              Measuring {tuning.value.done + 1} of {tuning.value.total}: {tuning.value.current}
              <button type="button" class="text-[var(--text-link)] hover:underline"
                onClick$={async () => { try { await invoke('tune_cancel'); } catch { /* already done */ } }}>
                Stop
              </button>
            </div>
          ) : (
            <button
              type="button"
              class="mt-2 text-xs text-[var(--text-link)] hover:underline"
              onClick$={async () => {
                note.value = '';
                tuning.value = { done: 0, total: 1, current: 'starting' };
                try {
                  const p = await invoke<{ results: TuneResult[] }>('tune_run', { model: props.model });
                  results.value = p.results;
                } catch (e) {
                  note.value = `Tune run failed: ${e}`;
                } finally {
                  tuning.value = null;
                }
              }}
            >
              {results.value.length ? 'Measure again' : 'Tune on this computer'}
            </button>
          )}
          {(() => {
            // What the measurements decided for Auto, said plainly: the
            // compact cache against its standard twin, and a smaller
            // expert split that ran faster than the picker's.
            const ok = results.value.filter((r) => !r.failed && r.gen_tps > 0);
            const lines: string[] = [];
            const compact = ok.find((r) => r.kv_q8);
            if (compact) {
              const twin = ok.find((r) => !r.kv_q8 && r.ctx === compact.ctx && r.moe_cpu_layers === compact.moe_cpu_layers && r.draft === compact.draft);
              if (twin) {
                const pct = Math.round(((compact.gen_tps - twin.gen_tps) / twin.gen_tps) * 100);
                lines.push(
                  compact.gen_tps >= 0.95 * twin.gen_tps
                    ? `Compact cache measured at ${compact.ctx}: ~${Math.round(compact.gen_tps)} tok/s, ${pct === 0 ? 'the same as' : pct > 0 ? `${pct}% faster than` : `${-pct}% slower than`} standard - Auto uses it.`
                    : `Compact cache measured at ${compact.ctx}: ${-pct}% slower than standard here - Auto keeps standard.`,
                );
              }
            } else if (results.value.some((r) => r.kv_q8 && r.failed)) {
              lines.push('The compact cache did not load here - Auto keeps standard.');
            }
            if (props.isMoe && props.autoMoeN != null && props.autoCtx) {
              const atCtx = ok.filter((r) => r.ctx === props.autoCtx);
              const picker = atCtx.find((r) => r.moe_cpu_layers === props.autoMoeN);
              if (picker) {
                const faster = atCtx
                  .filter((r) => r.moe_cpu_layers != null && r.moe_cpu_layers < (props.autoMoeN as number) && r.gen_tps >= 1.05 * picker.gen_tps)
                  .sort((a, b) => b.gen_tps - a.gen_tps)[0];
                if (faster) lines.push(`${faster.moe_cpu_layers} expert layers in RAM measured ~${Math.round(faster.gen_tps)} tok/s against ~${Math.round(picker.gen_tps)} for the automatic ${props.autoMoeN} - Auto uses the faster split.`);
              }
            }
            return lines.length ? (
              <ul class="mt-2 space-y-1">
                {lines.map((l) => <li key={l} class="text-xs text-[var(--text-secondary)]">{l}</li>)}
              </ul>
            ) : null;
          })()}
          {(() => {
            const ok = results.value.filter((r) => !r.failed);
            const byCtx = new Map<number, TuneResult>();
            for (const r of ok) {
              const b = byCtx.get(r.ctx);
              if (!b || r.gen_tps > b.gen_tps) byCtx.set(r.ctx, r);
            }
            const positions = [...byCtx.values()].sort((a, b) => a.ctx - b.ctx);
            if (positions.length < 2) return null;
            const pos = positions[Math.min(sliderPos.value, positions.length - 1)];
            return (
              <div class="mt-3">
                <div class="flex items-center justify-between text-[10px] uppercase tracking-wider text-[var(--text-muted)]">
                  <span>Speed</span><span>Room</span>
                </div>
                <input
                  type="range" min={0} max={positions.length - 1} step={1} value={sliderPos.value}
                  class="w-full appearance-none h-1.5 rounded-full bg-[var(--border-subtle)] cursor-pointer [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:w-3.5 [&::-webkit-slider-thumb]:h-3.5 [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-[var(--text-link)] [&::-moz-range-thumb]:w-3.5 [&::-moz-range-thumb]:h-3.5 [&::-moz-range-thumb]:rounded-full [&::-moz-range-thumb]:bg-[var(--text-link)] [&::-moz-range-thumb]:border-none"
                  onInput$={(_, el) => {
                    sliderPos.value = Number(el.value);
                    const p = positions[Number(el.value)];
                    if (!p) return;
                    ctx.value = p.ctx;
                    if (p.moe_cpu_layers != null) moeN.value = p.moe_cpu_layers;
                    draftOff.value = !p.draft && props.hasDraft;
                    kv.value = p.kv_q8 ? 'q8_0' : 'auto';
                    note.value = 'Pick filled in above - Save to keep it.';
                  }}
                />
                <p class="text-xs text-[var(--text-secondary)]">
                  {pos.ctx} context · ~{Math.round(pos.gen_tps)} tok/s (reads at ~{Math.round(pos.pp_tps)})
                  {pos.moe_cpu_layers != null ? pos.moe_cpu_layers === 0 ? ' · all on the card' : ` · ${pos.moe_cpu_layers} expert layers in RAM` : ''}
                  {props.hasDraft ? (pos.draft ? ' · speed-up on' : ' · speed-up off') : ''}
                  {pos.kv_q8 ? ' · compact cache' : ''}
                </p>
              </div>
            );
          })()}
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
                if (ctx.value != null && ctx.value > 0) tuning.context = Math.round(ctx.value);
                if (moeN.value != null && moeN.value >= 0) tuning.moe_cpu_layers = Math.round(moeN.value);
                if (draftOff.value) tuning.draft_off = true;
                if (kv.value !== 'auto') tuning.kv_cache = kv.value;
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
