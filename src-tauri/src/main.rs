// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // NOTE — do NOT set WEBKIT_DISABLE_DMABUF_RENDERER here. It was tried
    // (2026-06-11) as a fix for the dev-mode first-page-load issue, and on
    // webkit2gtk 2.52 + NVIDIA/Wayland it pushed WebKitGTK onto a fallback
    // render path that SEGFAULTS (null deref in libwebkit2gtk) under GPU
    // load. A/B tested on the affected machine: default DMABUF rendering
    // is stable; disabling it crashes. The actual first-page bug was the
    // Vite mid-hydration reload, fixed via optimizeDeps.include in
    // vite.config.ts.
    app_lib::run()
}
