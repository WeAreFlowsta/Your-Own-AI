#!/usr/bin/env bash
# A first launch of the INSTALLED package on this machine, in a scratch
# home, so the welcome flow shows and the person's own profile, models and
# identity are not touched. Downloads are real and land in the scratch home.
#   e2e/webkit/local-repro.sh [intel|nvidia]
# nvidia: NVIDIA draws the window (PRIME render offload; check with
# nvidia-smi that the app's processes are listed). Lock the Vault first so
# the scratch app starts on a local profile. Delete the scratch home after.
set -euo pipefail
MODE="${1:-intel}"
APP=/usr/bin/app
pgrep -x app >/dev/null && { echo "Your Own AI is already running - close it first"; exit 1; }
SCRATCH="${REPRO_HOME:-/home/$(id -un)/yoai-webkit-repro}"
mkdir -p "$SCRATCH"
export HOME="$SCRATCH" XDG_CONFIG_HOME="$SCRATCH/.config" XDG_DATA_HOME="$SCRATCH/.local/share" XDG_CACHE_HOME="$SCRATCH/.cache"
if [ "$MODE" = nvidia ]; then
  export __NV_PRIME_RENDER_OFFLOAD=1 __GLX_VENDOR_LIBRARY_NAME=nvidia
  export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/10_nvidia.json
fi
echo "session: ${XDG_SESSION_TYPE:-?}  mode: $MODE  WebKitGTK $(dpkg-query -W -f='${Version}' libwebkit2gtk-4.1-0)"
echo "scratch home: $SCRATCH"
exec "$APP"
