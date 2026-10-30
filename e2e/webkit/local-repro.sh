#!/usr/bin/env bash
# A first launch of the INSTALLED package on this machine, in a scratch
# home, so the welcome flow shows and the person's own profile, models and
# identity are not touched. Downloads are real and land in the scratch home.
#   e2e/webkit/local-repro.sh
# It runs on whatever GPU drives the display. To test an NVIDIA laptop the
# way a desktop NVIDIA machine runs, switch the display to NVIDIA
# (`sudo prime-select nvidia`, reboot); forcing NVIDIA's EGL with the PRIME
# offload variables instead makes WebKit abort at startup ("Could not
# create GBM EGL display") - not a real configuration. Lock the Vault first
# so the scratch app starts on a local profile. Delete the scratch home after.
set -euo pipefail
APP=/usr/bin/app
pgrep -x app >/dev/null && { echo "Your Own AI is already running - close it first"; exit 1; }
SCRATCH="${REPRO_HOME:-/home/$(id -un)/yoai-webkit-repro}"
mkdir -p "$SCRATCH"
export HOME="$SCRATCH" XDG_CONFIG_HOME="$SCRATCH/.config" XDG_DATA_HOME="$SCRATCH/.local/share" XDG_CACHE_HOME="$SCRATCH/.cache"
echo "session: ${XDG_SESSION_TYPE:-?}  display GPU: $(prime-select query 2>/dev/null || echo ?)  WebKitGTK $(dpkg-query -W -f='${Version}' libwebkit2gtk-4.1-0)"
echo "scratch home: $SCRATCH"
exec "$APP"
