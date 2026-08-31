/**
 * Per-AI generation settings (FINE_TUNE_PANEL.md, layer 3).
 *
 * Resolution, per field: this AI's own value -> the global override ->
 * the model default (today: the app's constants; a catalog of maker-
 * recommended values can slot in as the base layer later). Applied live
 * per request - never a reload. Only overridden fields go on the wire;
 * the Rust side keeps its constants as the fallback, so an empty layer
 * changes nothing.
 */
export interface SamplingOverrides {
  temperature?: number;
  topP?: number;
  minP?: number;
  repeatPenalty?: number;
}

/** The app's constants (llm.rs) - the "model default" layer for now. */
export const SAMPLING_DEFAULTS = { temperature: 0.7, topP: 0.9, minP: 0.05, repeatPenalty: 1.1 };

export const SAMPLING_BOUNDS = {
  temperature: { min: 0, max: 2, step: 0.05 },
  topP: { min: 0.05, max: 1, step: 0.01 },
  minP: { min: 0, max: 0.5, step: 0.01 },
  repeatPenalty: { min: 1, max: 1.5, step: 0.01 },
} as const;

const GLOBAL_KEY = "globalSampling";

/** The machine-wide override layer (set from the fine-tune panel). */
export function globalSampling(): SamplingOverrides {
  try {
    if (typeof localStorage === "undefined") return {};
    const raw = localStorage.getItem(GLOBAL_KEY);
    const parsed = raw ? (JSON.parse(raw) as unknown) : null;
    return parsed && typeof parsed === "object" ? (parsed as SamplingOverrides) : {};
  } catch {
    return {};
  }
}

export function setGlobalSampling(s: SamplingOverrides): void {
  try {
    const clean = Object.fromEntries(Object.entries(s).filter(([, v]) => typeof v === "number"));
    if (Object.keys(clean).length) localStorage.setItem(GLOBAL_KEY, JSON.stringify(clean));
    else localStorage.removeItem(GLOBAL_KEY);
  } catch {
    /* a convenience layer; never throw into a save */
  }
}

/** Placeholder text for an empty field: names the layer that answers. */
export function samplingPlaceholder(field: keyof SamplingOverrides): string {
  const g = globalSampling()[field];
  if (typeof g === "number") return `Global (${g})`;
  return `Model default (${SAMPLING_DEFAULTS[field]})`;
}

/** The wire shape: only overridden fields, snake_case for the Rust side. */
export function wireSampling(
  ai: { sampling?: SamplingOverrides } | undefined | null,
): { temperature?: number; top_p?: number; min_p?: number; repeat_penalty?: number } | undefined {
  const g = globalSampling();
  const own = ai?.sampling ?? {};
  const pick = (f: keyof SamplingOverrides) => (typeof own[f] === "number" ? own[f] : g[f]);
  const out: Record<string, number> = {};
  const t = pick("temperature"); if (typeof t === "number") out.temperature = t;
  const p = pick("topP"); if (typeof p === "number") out.top_p = p;
  const m = pick("minP"); if (typeof m === "number") out.min_p = m;
  const r = pick("repeatPenalty"); if (typeof r === "number") out.repeat_penalty = r;
  return Object.keys(out).length ? out : undefined;
}
