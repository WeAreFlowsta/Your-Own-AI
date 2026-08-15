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

/**
 * Mirror the paused set into the tauri store so the ROUTER (Rust) honors
 * it: a paused model must not be auto-picked either - "hide it from me"
 * and "don't hand it to me" are the same intent. Best-effort, like the
 * other routing-pref mirrors; the router reads `pausedModels` per pick.
 */
export async function mirrorPausedModels(): Promise<void> {
  try {
    const { Store } = await import('@tauri-apps/plugin-store');
    const store = await Store.load('settings.json');
    await store.set('pausedModels', [...getPausedModels()]);
    await store.save();
  } catch {
    /* mirror is best-effort */
  }
}

/** Pause/resume a model. Fires `modelPauseChanged` so open views can refresh. */
export function setModelPaused(id: string, paused: boolean): void {
  const set = getPausedModels();
  if (paused) set.add(id);
  else set.delete(id);
  localStorage.setItem(PAUSED_MODELS_KEY, JSON.stringify([...set]));
  void mirrorPausedModels();
  window.dispatchEvent(
    new CustomEvent('modelPauseChanged', { detail: { id, paused } })
  );
}
