import { invoke } from "@tauri-apps/api/core";

/**
 * The model the engine is really serving right now, or null.
 *
 * The header's model chip must come from here, never from
 * localStorage("currentModel") - that is only the remembered pick, and
 * painting it green put a loaded-looking chip over a server with no model
 * (field 09-03).
 */
export async function loadedModelNow(): Promise<string | null> {
  try {
    const m = await invoke<string | null>("get_current_model");
    if (!m) return null;
    return (await invoke<boolean>("is_llama_server_ready")) ? m : null;
  } catch {
    return null;
  }
}
