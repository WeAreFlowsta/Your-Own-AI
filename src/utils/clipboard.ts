import { invoke } from "@tauri-apps/api/core";

/**
 * Copy text to the system clipboard via the Rust side.
 *
 * `navigator.clipboard.writeText` is denied by Windows WebView2 (no host
 * permission), so every copy button routes through this instead. Throws on
 * failure so callers can show their own feedback.
 */
export async function copyText(text: string): Promise<void> {
  await invoke("copy_text", { text });
}
