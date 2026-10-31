#!/usr/bin/env bash
# A first launch of the INSTALLED package on this machine, in a scratch
# home, so the welcome flow shows and the person's own profile, models and
# identity are not touched. Downloads are real and land in the scratch home.
#   e2e/webkit/local-repro.sh
# It runs on whatever GPU drives the display. NVIDIA=1 puts WebKit on the
# NVIDIA card with NVIDIA's EGL (as on a desktop NVIDIA machine) with no
# reboot: the render device and the EGL vendor must match, or WebKit aborts
# at startup ("Could not create GBM EGL display"). FIX=1 adds the workaround
# (Mesa's EGL + WEBKIT_DMABUF_RENDERER_DISABLE_GBM=1), also on top of NVIDIA=1. Lock the Vault first
# so the scratch app starts on a local profile. Delete the scratch home after.
# GDB=1 runs it under gdb and prints the crashing stack and the library
# load addresses when it stops.
set -euo pipefail
APP=/usr/bin/app
pgrep -x app >/dev/null && { echo "Your Own AI is already running - close it first"; exit 1; }
SCRATCH="${REPRO_HOME:-/home/$(id -un)/yoai-webkit-repro}"
mkdir -p "$SCRATCH"
export HOME="$SCRATCH" XDG_CONFIG_HOME="$SCRATCH/.config" XDG_DATA_HOME="$SCRATCH/.local/share" XDG_CACHE_HOME="$SCRATCH/.cache"
echo "session: ${XDG_SESSION_TYPE:-?}  display GPU mode: $(prime-select query 2>/dev/null || echo ?)  WebKitGTK $(dpkg-query -W -f='${Version}' libwebkit2gtk-4.1-0)"
if [ "${NVIDIA:-}" = 1 ]; then
  export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/10_nvidia.json
  for d in /sys/class/drm/renderD*; do
    [ "$(basename "$(readlink -f "$d/device/driver")")" = nvidia ] && export WEBKIT_WEB_RENDER_DEVICE_FILE="/dev/dri/$(basename "$d")"
  done
  echo "WebKit on NVIDIA: ${WEBKIT_WEB_RENDER_DEVICE_FILE:-no NVIDIA render node found}"
fi
if [ "${FIX:-}" = 1 ]; then
  export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json WEBKIT_DMABUF_RENDERER_DISABLE_GBM=1
  echo "workaround on: Mesa's EGL + GBM off"
fi
echo "scratch home: $SCRATCH"
if [ "${GDB:-}" = 1 ]; then
  exec gdb -q -batch -iex 'set debuginfod enabled off' -ex 'set pagination off' \
    -ex 'handle SIGPIPE SIGUSR1 SIGUSR2 SIG34 SIG35 nostop noprint pass' \
    -ex run -ex 'bt 60' -ex 'info sharedlibrary' --args "$APP"
fi
exec "$APP"
