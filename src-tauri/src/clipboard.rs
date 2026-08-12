//! One reliable copy-to-clipboard path for the frontend.
//!
//! `navigator.clipboard.writeText` is denied by Windows WebView2 unless the
//! host grants the clipboard permission, so buttons built on it silently did
//! nothing there (first reported: the offline-models system-info copy).
//! Copying on the Rust side talks to the OS clipboard directly and works the
//! same on every platform.

use tauri::AppHandle;

#[tauri::command]
pub fn copy_text(app: AppHandle, text: String) -> Result<(), String> {
    use tauri_plugin_clipboard_manager::ClipboardExt;
    app.clipboard()
        .write_text(text)
        .map_err(|e| format!("Could not copy to the clipboard: {e}"))
}
