#!/usr/bin/env bash
# Launches the e2e build of Your Own AI in an ISOLATED profile for the UI
# tests (build-docs planning/UI_AUTOMATION.md). @wdio/tauri-service runs
# this instead of the binary, so the app never sees the person's own
# profile: HOME points at e2e/profile (wiped by `npm run e2e`), and on the
# dev box's Wayland desktop the window is an XWayland window so screen
# recorders (ffmpeg x11grab) and xdotool can see it.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
export HOME="${YOAI_E2E_HOME:-$HERE/profile}"
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_DATA_HOME="$HOME/.local/share"
export XDG_CACHE_HOME="$HOME/.cache"
mkdir -p "$HOME"
# YOAI_E2E_NATIVE_WAYLAND=1 keeps the native Wayland window (no recording tools see it).
if [ "${XDG_SESSION_TYPE:-}" = "wayland" ] && [ -z "${YOAI_E2E_NATIVE_WAYLAND:-}" ]; then export GDK_BACKEND=x11; fi
exec "$HERE/../src-tauri/target/debug/app" "$@"
