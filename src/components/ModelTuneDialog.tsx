import { component$, useSignal, useComputed$, useVisibleTask$, $, type QRL } from '@builder.io/qwik';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import LiquidMetalButton from './LiquidMetalButton';
import { LuSave, LuLoader2, LuChevronRight } from '@qwikest/icons/lucide';
import TuneSlider from './TuneSlider';

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

/**
 * Fine-tune one model on this computer (FINE_TUNE_PANEL layer 2).
 *
 * Redesigned 2026-09-09 (Eric: "measure on this computer needs to be the
 * main deal"): the dialog opens on what the model runs at NOW and where
 * that came from, then the measurement as the main event - one sentence
 * that states the cost, one primary button, and afterwards a slider from
 * Faster answers to More room with a card that translates the numbers
 * (pages in view, words a second) and one line on the trade-off. The
 * sliders live in a "Set it yourself" drawer, closed by default. Saving
 * says when it applies: reloaded now when this model is the loaded one,
 * otherwise at its next load.
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

type Kv = 'auto' | 'f16' | 'q8_0';

/** About 650 tokens to a page of prose; about three words to four tokens. */
const TOKENS_PER_PAGE = 650;
const WORDS_PER_TOKEN = 0.75;

function kTokens(n: number): string {
  return n >= 1024 ? `${Math.round(n / 1024)}K` : `${n}`;
}

export default component$<ModelTuneDialogProps>((props) => {
  const name = props.model.replace(/\.gguf$/, '');
  const ctx = useSignal<number | null>(null);
  const moeN = useSignal<number | null>(null);
  const draftOff = useSignal(false);
  const kv = useSignal<Kv>('auto');
  /** What was loaded from disk, to know whether anything changed. */
  const saved = useSignal<string>('');
  const note = useSignal('');
  const busy = useSignal(false);
  const results = useSignal<TuneResult[]>([]);
  const tuning = useSignal<{ done: number; total: number; current: string } | null>(null);
  const sliderPos = useSignal(0);
  const manualOpen = useSignal(false);

  const snapshot = $(() => JSON.stringify([ctx.value, moeN.value, draftOff.value, kv.value]));

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
    saved.value = await snapshot();
    // A person who set values by hand wants the drawer open.
    manualOpen.value = ctx.value != null || moeN.value != null || draftOff.value || kv.value !== 'auto';
    try {
      const p = await invoke<{ results: TuneResult[] } | null>('tune_profiles_get', { model: props.model });
      if (p?.results) {
        results.value = p.results;
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

  // eslint-disable-next-line qwik/no-use-visible-task
  useVisibleTask$(async ({ cleanup }) => {
    const un = await listen<{ model: string; done: number; total: number; current: string }>('tune-run', (e) => {
      if (e.payload.model !== props.model) return;
      tuning.value = e.payload.done >= e.payload.total ? null : e.payload;
    });
    cleanup(() => un());
  });

  /** The best measured setup per context, in context order - the slider's stops. */
  const positions = useComputed$(() => {
    const ok = results.value.filter((r) => !r.failed && r.gen_tps > 0);
    const byCtx = new Map<number, TuneResult>();
    for (const r of ok) {
      const b = byCtx.get(r.ctx);
      if (!b || r.gen_tps > b.gen_tps) byCtx.set(r.ctx, r);
    }
    return [...byCtx.values()].sort((a, b) => a.ctx - b.ctx);
  });

  /** What the model runs at now: the person's values over the automatics,
   *  with the measured speed for that setup when there is one. */
  const current = useComputed$(() => {
    const c = ctx.value ?? props.autoCtx ?? null;
    const m = props.isMoe ? (moeN.value ?? props.autoMoeN ?? null) : null;
    const k: Kv = kv.value;
    const ok = results.value.filter((r) => !r.failed && r.gen_tps > 0);
    const match = ok.find((r) => r.ctx === c && (m == null || r.moe_cpu_layers === m) && (k === 'auto' || r.kv_q8 === (k === 'q8_0')));
    const manual = ctx.value != null || moeN.value != null || draftOff.value || kv.value !== 'auto';
    const source: 'Automatic' | 'Measured here' | 'Set by you' = !manual ? 'Automatic' : match ? 'Measured here' : 'Set by you';
    return { ctx: c, moe: m, kv: k, tps: match?.gen_tps ?? null, source };
  });

  const dirty = useComputed$(() => saved.value !== '' && saved.value !== JSON.stringify([ctx.value, moeN.value, draftOff.value, kv.value]));

  const measure = $(async () => {
    note.value = '';
    tuning.value = { done: 0, total: 1, current: 'starting' };
    try {
      const p = await invoke<{ results: TuneResult[] }>('tune_run', { model: props.model });
      results.value = p.results;
    } catch (e) {
      note.value = `Measuring did not finish: ${e}`;
    } finally {
      tuning.value = null;
    }
  });

  const usePick = $((i: number) => {
    const p = positions.value[i];
    if (!p) return;
    ctx.value = p.ctx;
    if (p.moe_cpu_layers != null) moeN.value = p.moe_cpu_layers;
    draftOff.value = !p.draft && props.hasDraft;
    kv.value = p.kv_q8 ? 'q8_0' : 'auto';
    note.value = '';
  });

  const save = $(async () => {
    busy.value = true;
    try {
      const t: Record<string, unknown> = {};
      if (ctx.value != null && ctx.value > 0) t.context = Math.round(ctx.value);
      if (moeN.value != null && moeN.value >= 0) t.moe_cpu_layers = Math.round(moeN.value);
      if (draftOff.value) t.draft_off = true;
      if (kv.value !== 'auto') t.kv_cache = kv.value;
      await invoke('tuning_set', { model: props.model, tuning: t });
      const reloaded = await invoke<boolean>('tuning_apply_now', { model: props.model });
      saved.value = await snapshot();
      note.value = reloaded
        ? 'Reloaded with these settings.'
        : Object.keys(t).length
          ? 'Saved. Applies when this model next loads.'
          : 'Back to automatic. Applies when this model next loads.';
    } catch (e) {
      note.value = `Could not save: ${e}`;
    } finally {
      busy.value = false;
    }
  });

  // What the measurement decided for Auto, in one line each.
  const verdicts = useComputed$(() => {
    const ok = results.value.filter((r) => !r.failed && r.gen_tps > 0);
    const lines: string[] = [];
    const compact = ok.find((r) => r.kv_q8);
    if (compact) {
      const twin = ok.find((r) => !r.kv_q8 && r.ctx === compact.ctx && r.moe_cpu_layers === compact.moe_cpu_layers && r.draft === compact.draft);
      if (twin) {
        const pct = Math.round(((compact.gen_tps - twin.gen_tps) / twin.gen_tps) * 100);
        lines.push(
          compact.gen_tps >= 0.95 * twin.gen_tps
            ? `Compact cache: ${pct === 0 ? 'the same speed as' : pct > 0 ? `${pct}% faster than` : `${-pct}% slower than`} standard here, so Auto uses it.`
            : `Compact cache: ${-pct}% slower than standard here, so Auto keeps standard.`,
        );
      }
    } else if (results.value.some((r) => r.kv_q8 && r.failed)) {
      lines.push('Compact cache did not load here, so Auto keeps standard.');
    }
    if (props.isMoe && props.autoMoeN != null && props.autoCtx) {
      const atCtx = ok.filter((r) => r.ctx === props.autoCtx);
      const picker = atCtx.find((r) => r.moe_cpu_layers === props.autoMoeN);
      if (picker) {
        const faster = atCtx
          .filter((r) => r.moe_cpu_layers != null && r.moe_cpu_layers < (props.autoMoeN as number) && r.gen_tps >= 1.05 * picker.gen_tps)
          .sort((a, b) => b.gen_tps - a.gen_tps)[0];
        if (faster) lines.push(`Expert split: ${faster.moe_cpu_layers} layers in RAM ran ~${Math.round(faster.gen_tps)} tok/s against ~${Math.round(picker.gen_tps)} for the automatic ${props.autoMoeN}, so Auto uses ${faster.moe_cpu_layers}.`);
      }
    }
    return lines;
  });

  const pick = positions.value[Math.min(sliderPos.value, Math.max(0, positions.value.length - 1))];
  const autoIndex = props.autoCtx != null ? positions.value.findIndex((p) => p.ctx === props.autoCtx) : -1;
  const cur = current.value;
  const label = 'text-xs text-[var(--text-muted)]';

  return (
    <div class="fixed inset-0 z-50 flex items-center justify-center bg-black/50 p-4">
      <div class="w-full max-w-md max-h-[calc(100vh-2rem)] overflow-y-auto rounded-2xl border border-[var(--border-subtle)] bg-[var(--bg-header-footer)] p-6 shadow-2xl">
        <h3 class="text-base font-semibold text-[var(--text-primary)] break-all">{name}</h3>
        <p class="mt-1 text-sm text-[var(--text-secondary)]">Fine-tune how it runs on this computer.</p>

        {/* What it runs at now, and where that came from. */}
        <div class="mt-4 flex items-center justify-between gap-3 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-main)] px-3 py-2">
          <p class="text-xs text-[var(--text-primary)] leading-relaxed">
            {cur.ctx != null ? `${kTokens(cur.ctx)} context` : 'Automatic context'}
            {cur.tps != null ? ` · ${Math.round(cur.tps)} tok/s` : ''}
            {props.isMoe && cur.moe != null ? (cur.moe === 0 ? ' · all on the card' : ` · ${cur.moe} expert layers in RAM`) : ''}
            {cur.kv !== 'auto' ? (cur.kv === 'q8_0' ? ' · compact cache' : ' · standard cache') : ''}
          </p>
          <span
            class={`shrink-0 rounded-full border px-2 py-0.5 text-[11px] ${
              cur.source === 'Automatic'
                ? 'border-[var(--border-subtle)] text-[var(--text-muted)]'
                : 'border-[var(--text-link)] text-[var(--text-link)]'
            }`}
          >
            {cur.source}
          </span>
        </div>

        {/* Measure: the main event. */}
        <div class="mt-5">
          {tuning.value ? (
            <div class="flex items-center gap-3 text-sm text-[var(--text-secondary)]">
              <span class="inline-block h-3.5 w-3.5 rounded-full border-2 border-[var(--border-subtle)] border-t-[var(--text-secondary)] animate-spin" />
              Measuring {tuning.value.done + 1} of {tuning.value.total}: {tuning.value.current}
              <button
                type="button"
                class="ml-auto text-xs text-[var(--text-link)] hover:underline"
                onClick$={async () => { try { await invoke('tune_cancel'); } catch { /* already done */ } }}
              >
                Stop
              </button>
            </div>
          ) : positions.value.length < 2 ? (
            <>
              <p class="text-sm text-[var(--text-secondary)]">
                Loads it a few times with different setups and times each one. About three minutes,
                and chats pause while it runs.
              </p>
              <LiquidMetalButton class="mt-3 px-5 py-2 text-sm" onClick$={measure}>
                Measure on this computer
              </LiquidMetalButton>
            </>
          ) : (
            <>
              <div class="flex items-center justify-between text-[11px] uppercase tracking-wider text-[var(--text-muted)]">
                <span>Faster answers</span>
                <span>More room</span>
              </div>
              <input
                type="range"
                min={0}
                max={positions.value.length - 1}
                step={1}
                value={sliderPos.value}
                class="mt-1 w-full appearance-none h-1.5 rounded-full bg-[var(--border-subtle)] cursor-pointer [&::-webkit-slider-thumb]:appearance-none [&::-webkit-slider-thumb]:w-4 [&::-webkit-slider-thumb]:h-4 [&::-webkit-slider-thumb]:rounded-full [&::-webkit-slider-thumb]:bg-[var(--text-link)] [&::-moz-range-thumb]:w-4 [&::-moz-range-thumb]:h-4 [&::-moz-range-thumb]:rounded-full [&::-moz-range-thumb]:bg-[var(--text-link)] [&::-moz-range-thumb]:border-none"
                onInput$={(_, el) => {
                  sliderPos.value = Number(el.value);
                  usePick(Number(el.value));
                }}
              />
              <div class="mt-1 flex justify-between text-[10px] text-[var(--text-muted)]">
                {positions.value.map((p, i) => (
                  <span key={p.ctx} class={i === autoIndex ? 'text-[var(--text-link)]' : ''}>
                    {kTokens(p.ctx)}{i === autoIndex ? ' auto' : ''}
                  </span>
                ))}
              </div>
              {pick && (
                <div class="mt-3 rounded-lg border border-[var(--border-subtle)] bg-[var(--bg-main)] px-3 py-2 text-xs">
                  <p class="text-[var(--text-primary)]">
                    {kTokens(pick.ctx)} context, about {Math.max(1, Math.round(pick.ctx / TOKENS_PER_PAGE))} pages in view at once
                  </p>
                  <p class="text-[var(--text-primary)]">
                    {Math.round(pick.gen_tps)} tokens a second, about {Math.round(pick.gen_tps * WORDS_PER_TOKEN)} words
                  </p>
                  <p class="mt-1 text-[var(--text-muted)]">
                    Reads at ~{Math.round(pick.pp_tps)} tok/s
                    {pick.moe_cpu_layers != null ? (pick.moe_cpu_layers === 0 ? ' · all on the card' : ` · ${pick.moe_cpu_layers} expert layers in RAM`) : ''}
                    {props.hasDraft ? (pick.draft ? ' · speed-up on' : ' · speed-up off') : ''}
                    {pick.kv_q8 ? ' · compact cache' : ''}
                  </p>
                </div>
              )}
              <p class="mt-2 text-[11px] text-[var(--text-muted)]">
                More room lets the AI hold more of a long chat or document at once. Faster answers come from less room.
              </p>
              {verdicts.value.length > 0 && (
                <ul class="mt-2 space-y-1">
                  {verdicts.value.map((l) => <li key={l} class="text-[11px] text-[var(--text-muted)]">{l}</li>)}
                </ul>
              )}
              <button type="button" class="mt-2 text-xs text-[var(--text-link)] hover:underline" onClick$={measure}>
                Measure again
              </button>
            </>
          )}
        </div>

        {/* Set it yourself: the sliders, in a drawer. */}
        <div class="mt-5 border-t border-[var(--border-subtle)] pt-3">
          <button
            type="button"
            class="flex w-full items-center gap-1.5 text-sm text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
            aria-expanded={manualOpen.value}
            onClick$={() => (manualOpen.value = !manualOpen.value)}
          >
            <LuChevronRight class={`w-4 h-4 transition-transform ${manualOpen.value ? 'rotate-90' : ''}`} />
            Set it yourself
          </button>
          {manualOpen.value && (
            <div class="mt-3 space-y-4">
              <TuneSlider
                label="Context size"
                value={ctx.value}
                autoLabel={props.autoCtx ? `Auto (${kTokens(props.autoCtx)})` : 'Auto'}
                autoValue={props.autoCtx}
                ticks={[4096, 8192, 16384, 32768, 65536, 131072].filter((c) => !props.maxCtx || c <= props.maxCtx)}
                unit="tokens"
                onChange$={(v) => { ctx.value = v; note.value = ''; }}
              />
              {ctx.value != null && props.autoCtx && ctx.value > props.autoCtx && (
                <p class="text-xs text-amber-500">
                  More than fits by the automatics' measure. It may load slowly or fail.
                </p>
              )}
              {props.isMoe && (
                <TuneSlider
                  label="Expert layers in main memory"
                  value={moeN.value}
                  autoLabel={props.autoMoeN != null ? `Auto (${props.autoMoeN})` : 'Auto'}
                  autoValue={props.autoMoeN ?? 0}
                  min={0}
                  max={props.nLayers || 64}
                  step={1}
                  unit="layers"
                  onChange$={(v) => { moeN.value = v; note.value = ''; }}
                />
              )}
              {props.isMoe && moeN.value != null && props.autoMoeN != null && props.autoMoeN > 0 && moeN.value < props.autoMoeN && (
                <p class="text-xs text-amber-500">
                  Fewer layers in main memory than the automatics pick ({props.autoMoeN}). The rest must fit on the card.
                </p>
              )}
              <div>
                <p class={label} title="Compact stores the context at half the memory, so more fits on the card. Auto uses it only once a measurement here came back clean.">
                  Context memory
                </p>
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
              </div>
              {props.hasDraft && (
                <label class="flex items-center gap-2 text-xs text-[var(--text-secondary)]" title="A speed-up file drafts likely words ahead of the model. Leave it out to measure the difference yourself.">
                  <input
                    type="checkbox"
                    checked={draftOff.value}
                    onChange$={(_, el) => { draftOff.value = el.checked; note.value = ''; }}
                    class="h-4 w-4 rounded accent-[var(--text-link)]"
                  />
                  Leave the speed-up file out
                </label>
              )}
              {(ctx.value != null || moeN.value != null || draftOff.value || kv.value !== 'auto') && (
                <button
                  type="button"
                  class="text-xs text-[var(--text-link)] hover:underline"
                  onClick$={() => { ctx.value = null; moeN.value = null; draftOff.value = false; kv.value = 'auto'; note.value = ''; }}
                >
                  Back to automatic
                </button>
              )}
            </div>
          )}
        </div>

        {note.value && <p class="mt-3 text-xs text-[var(--text-secondary)]">{note.value}</p>}

        <div class="mt-5 flex flex-col-reverse gap-2 sm:flex-row sm:justify-end">
          <LiquidMetalButton
            variant="secondary"
            class="mt-3 sm:mt-0 w-full sm:w-auto inline-flex justify-center px-6 py-2.5 text-base font-medium disabled:opacity-70"
            disabled={busy.value}
            onClick$={props.onClose$}
          >
            Cancel
          </LiquidMetalButton>
          <LiquidMetalButton
            class="w-full sm:w-auto inline-flex justify-center items-center px-6 py-2.5 text-base font-medium disabled:opacity-70"
            disabled={busy.value || !dirty.value}
            onClick$={save}
          >
            {busy.value ? <LuLoader2 class="h-5 w-5 animate-spin mr-2" /> : <LuSave class="w-[18px] h-[18px] mr-2" />}
            Save Changes
          </LiquidMetalButton>
        </div>
      </div>
    </div>
  );
});
