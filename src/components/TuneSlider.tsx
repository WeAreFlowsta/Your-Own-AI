import { component$, type QRL } from '@builder.io/qwik';

/**
 * One fine-tune row: a slider with the value (or the automatic default)
 * spelled out under it, and an Auto button that restores the default.
 * `value` null = automatic: the slider rests at `autoValue` and the line
 * shows `autoLabel`; the first drag sets a number. `ticks` makes the
 * slider step over discrete positions (context rungs) instead of a range.
 */
interface TuneSliderProps {
  label: string;
  value: number | null;
  autoLabel: string;
  autoValue?: number;
  min?: number;
  max?: number;
  step?: number;
  ticks?: number[];
  /** Text after the number ("tokens", "layers"...). */
  unit?: string;
  onChange$: QRL<(v: number | null) => void>;
}

export default component$<TuneSliderProps>((props) => {
  const ticks = props.ticks && props.ticks.length > 1 ? props.ticks : null;
  const lo = ticks ? 0 : (props.min ?? 0);
  const hi = ticks ? ticks.length - 1 : (props.max ?? 1);
  const st = ticks ? 1 : (props.step ?? 1);
  const auto = props.autoValue ?? (ticks ? ticks[0] : lo);
  const shown = props.value ?? auto;
  const pos = ticks
    ? (() => {
        let best = 0;
        ticks.forEach((t, i) => { if (Math.abs(t - shown) < Math.abs(ticks[best] - shown)) best = i; });
        return best;
      })()
    : shown;
  return (
    <div class="text-xs text-[var(--text-secondary)]">
      <div class="flex items-center justify-between gap-2">
        <span>{props.label}</span>
        {props.value != null && (
          <button
            type="button"
            onClick$={() => props.onChange$(null)}
            class="text-[var(--text-link)] hover:underline"
            title="Back to automatic"
          >
            Auto
          </button>
        )}
      </div>
      <input
        type="range" min={lo} max={hi} step={st} value={pos}
        class={`mt-1 w-full accent-[var(--text-link)] ${props.value == null ? 'opacity-60' : ''}`}
        onInput$={(_, el) => {
          const raw = Number(el.value);
          props.onChange$(ticks ? ticks[Math.max(0, Math.min(ticks.length - 1, raw))] : raw);
        }}
      />
      <p class={props.value != null ? 'text-[var(--text-primary)]' : 'text-[var(--text-muted)]'}>
        {props.value != null ? `${props.value}${props.unit ? ` ${props.unit}` : ''}` : props.autoLabel}
      </p>
    </div>
  );
});
