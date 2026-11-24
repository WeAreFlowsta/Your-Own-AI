/**
 * Text that leaves the conversation it was written in (a digest handed to
 * a new session, a summary kept as memory) goes through the secret mask
 * first: a password, key or recovery phrase in it is replaced. The mask
 * itself is in the backend (`secret_mask`), one function for every exit.
 */
import { invoke } from "@tauri-apps/api/core";

/** `text` with its secrets hidden. If the mask cannot run, nothing is
 *  handed on: an empty text, never the unmasked one. */
export async function maskText(text: string): Promise<string> {
  if (!text) return "";
  try {
    return (await invoke<{ text: string; hidden: number }>("mask_text", { text })).text;
  } catch (e) {
    console.warn("[mask] could not run; the text is not handed on:", e);
    return "";
  }
}
