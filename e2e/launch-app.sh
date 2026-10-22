#!/usr/bin/env bash
# Launches the e2e build of Your Own AI in an ISOLATED profile for the UI
# tests (see e2e/README.md). @wdio/tauri-service runs
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
# A test launch never downloads a model, whatever a spec or the app does.
export YOAI_BLOCK_MODEL_DOWNLOADS=1
# Two launch modes:
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
# Engine scenarios:
#   YOAI_E2E_CPU_ONLY=1  - the app's own FLOWSTA_CPU_ONLY switch: every layer
#                          on the CPU, no GPU enumeration.
#   YOAI_E2E_WITH_CUDA=1 - the person's installed CUDA engine folder is linked
#                          into the scratch profile (never downloaded here).
if [ -n "${YOAI_E2E_CPU_ONLY:-}" ]; then export FLOWSTA_CPU_ONLY=1; fi
if [ -n "${YOAI_E2E_WITH_CUDA:-}" ]; then
  ENGINES="${YOAI_E2E_ENGINES_DIR:-$REAL_HOME/.local/share/com.solar.yourowai/engines}"
  if [ -d "$ENGINES" ]; then mkdir -p "$XDG_DATA_HOME/com.solar.yourowai"; ln -sfn "$ENGINES" "$XDG_DATA_HOME/com.solar.yourowai/engines"; fi
fi
# YOAI_E2E_ONLINE=1 - online models through the STAGING proxy (test plans
# only, never a person's), signed in as the e2e test identity the online
# specs bring (~/.config/yoai-e2e/staging-identity.json). The Vault port is
# pinned to one where nothing answers, so the installed Vault - another
# identity - can never sign the test session out.
if [ -n "${YOAI_E2E_ONLINE:-}" ]; then
  export YOAI_PROXY_URL="${YOAI_PROXY_URL:-https://yoai-model-proxy-staging-386500392150.us-central1.run.app}"
  export YOAI_VAULT_PORT="${YOAI_VAULT_PORT:-27990}"
fi
# YOAI_E2E_WITH_RENDEZVOUS=1 - the conductor meets the person's other devices
# through the STAGING rendezvous (the app reads these at run time), with the
# Vault's staging test material; without it the conductor stays alone.
if [ -n "${YOAI_E2E_WITH_RENDEZVOUS:-}" ]; then
  export FLOWSTA_BOOTSTRAP_URL=https://bootstrap-staging.flowsta.com
  export FLOWSTA_SIGNAL_URL=wss://bootstrap-staging.flowsta.com
  if [ -z "${FLOWSTA_AUTH_MATERIAL:-}" ]; then
    FLOWSTA_AUTH_MATERIAL="$(sed -n 's/^FLOWSTA_AUTH_MATERIAL=\([^ \\]*\).*/\1/p' "$HERE/../../flowsta-vault/scripts/run-test-instance.sh" 2>/dev/null | head -1)"
    export FLOWSTA_AUTH_MATERIAL
  fi
fi
if [ "${XDG_SESSION_TYPE:-}" = "wayland" ] && [ -z "${YOAI_E2E_NATIVE_WAYLAND:-}" ]; then export GDK_BACKEND=x11; fi
# One instance per spec. The app hands a second launch off to a running
# instance (single-instance), so a previous spec's app that is still
# shutting down would answer this spec's driver with its own stale state
# (10-22: three specs in a row saw "no model loaded" after a spec that ran
# a matrix leg - its instance outlived the session end). Wait for any e2e
# instance of THIS binary to go; after 20 s, end it by pid. Only the e2e
# build's own path is matched, never an installed app.
BIN="$HERE/../src-tauri/target/debug/app"
BIN_REAL="$(readlink -f "$BIN")"
leftover() { for pid in $(pgrep -x app); do [ "$(readlink -f "/proc/$pid/exe" 2>/dev/null)" = "$BIN_REAL" ] && echo "$pid"; done; }
# The two-device harness (devices-live.sh phase c) runs a second instance
# on shifted ports beside an unattended first one on purpose: the guard
# stands down for an offset launch, or it would end device A.
if [ -z "${YOAI_PORT_OFFSET:-}" ]; then
  for _ in $(seq 1 40); do [ -z "$(leftover)" ] && break; sleep 0.5; done
  for pid in $(leftover); do echo "e2e: ending leftover app instance $pid" >&2; kill -9 "$pid" 2>/dev/null || true; done
fi
exec "$BIN" "$@"
