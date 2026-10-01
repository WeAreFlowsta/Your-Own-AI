#!/usr/bin/env bash
# Launches the e2e build of Your Own AI in an ISOLATED profile for the UI
# tests (build-docs planning/UI_AUTOMATION.md). @wdio/tauri-service runs
# this instead of the binary, so the app never sees the person's own
# profile: HOME points at e2e/profile (wiped by `npm run e2e`), and on the
# dev box's Wayland desktop the window is an XWayland window so screen
# recorders (ffmpeg x11grab) and xdotool can see it.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
REAL_HOME="$HOME"
export HOME="${YOAI_E2E_HOME:-$HERE/profile}"
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_DATA_HOME="$HOME/.local/share"
export XDG_CACHE_HOME="$HOME/.cache"
mkdir -p "$HOME"
# Two launch modes (Eric 2026-10-01: "sometimes run it with the welcome
# screen and others using the installed models"):
#   fresh (default)        - empty profile: the welcome flow.
#   YOAI_E2E_WITH_MODELS=1 - the scratch profile's settings point at the
#                            person's REAL models folder (read as installed
#                            models; specs in this mode must never delete
#                            or download), so the chat flows run at once.
if [ -n "${YOAI_E2E_WITH_MODELS:-}" ]; then
  MODELS="${YOAI_E2E_MODELS_DIR:-$REAL_HOME/.local/share/com.solar.yourowai/models}"
  mkdir -p "$XDG_DATA_HOME/com.solar.yourowai"
  printf '{"modelsDir":"%s"}\n' "$MODELS" > "$XDG_DATA_HOME/com.solar.yourowai/settings.json"
fi
# YOAI_E2E_NATIVE_WAYLAND=1 keeps the native Wayland window (no recording tools see it).
# Engine scenarios (Eric 2026-10-01: "sometimes with cuda and sometimes
# without"):
#   YOAI_E2E_CPU_ONLY=1  - the app's own FLOWSTA_CPU_ONLY switch: every layer
#                          on the CPU, no GPU enumeration.
#   YOAI_E2E_WITH_CUDA=1 - the person's installed CUDA engine folder is linked
#                          into the scratch profile (never downloaded here).
if [ -n "${YOAI_E2E_CPU_ONLY:-}" ]; then export FLOWSTA_CPU_ONLY=1; fi
if [ -n "${YOAI_E2E_WITH_CUDA:-}" ]; then
  ENGINES="${YOAI_E2E_ENGINES_DIR:-$REAL_HOME/.local/share/com.solar.yourowai/engines}"
  if [ -d "$ENGINES" ]; then mkdir -p "$XDG_DATA_HOME/com.solar.yourowai"; ln -sfn "$ENGINES" "$XDG_DATA_HOME/com.solar.yourowai/engines"; fi
fi
if [ "${XDG_SESSION_TYPE:-}" = "wayland" ] && [ -z "${YOAI_E2E_NATIVE_WAYLAND:-}" ]; then export GDK_BACKEND=x11; fi
exec "$HERE/../src-tauri/target/debug/app" "$@"
