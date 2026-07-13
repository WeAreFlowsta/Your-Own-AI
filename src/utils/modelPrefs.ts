/**
 * Per-machine model preferences (localStorage).
 *
 * "Paused" models stay downloaded / available but are hidden from the AI model
 * picker — the model equivalent of pausing an AI on the Your AIs page. The set
 * is keyed by a generic model id so it works for BOTH offline models (the GGUF
 * filename) and online models (the `online:<id>` identifier), which is what
 * sets up the upcoming Online Models page to reuse the same pause/play.
 */

const PAUSED_MODELS_KEY = 'pausedModels';

/** Set of paused model ids (filenames and/or `online:` ids). */
export function getPausedModels(): Set<string> {
  try {
    const raw = localStorage.getItem(PAUSED_MODELS_KEY);
    return new Set<string>(raw ? JSON.parse(raw) : []);
  } catch {
    return new Set<string>();
  }
}

export function isModelPaused(id: string): boolean {
  return getPausedModels().has(id);
}

/** Pause/resume a model. Fires `modelPauseChanged` so open views can refresh. */
export function setModelPaused(id: string, paused: boolean): void {
  const set = getPausedModels();
  if (paused) set.add(id);
  else set.delete(id);
  localStorage.setItem(PAUSED_MODELS_KEY, JSON.stringify([...set]));
  window.dispatchEvent(
    new CustomEvent('modelPauseChanged', { detail: { id, paused } })
  );
}
