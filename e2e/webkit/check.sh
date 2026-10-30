#!/usr/bin/env bash
# Launch checks against the system WebKitGTK: the installed Your Own AI
# package and WebKitGTK's own MiniBrowser, under X11 (Xvfb) and Wayland
# (headless Weston). Each case runs for CHECK_SECONDS (default 60) with a
# fresh profile and its own session bus. Crashes are caught by
# systemd-coredump; backtraces.sh reads them afterwards.
#
# Prints a results table (also to the GitHub step summary when there is
# one) and writes logs and X11 screenshots to the output folder. Exits 1
# when any case crashed.
#
# Usage: e2e/webkit/check.sh <output dir>
set -u
OUT="${1:?usage: check.sh <output dir>}"; mkdir -p "$OUT"
HERE="$(cd "$(dirname "$0")" && pwd)"
SECS="${CHECK_SECONDS:-60}"
APP="$(dpkg -L your-own-ai 2>/dev/null | grep -m1 '^/usr/bin/')"
MB="$(ls /usr/lib/*/webkit2gtk-4.1/MiniBrowser 2>/dev/null | head -1)"
PAGE="file://$HERE/view-transition.html"
[ -n "$APP" ] || { echo "the your-own-ai package is not installed"; exit 2; }
[ -n "$MB" ] || { echo "MiniBrowser not found"; exit 2; }

export XDG_RUNTIME_DIR="$(mktemp -d)"; chmod 700 "$XDG_RUNTIME_DIR"

# --- displays ---------------------------------------------------------------
Xvfb :99 -screen 0 1920x1080x24 -nolisten tcp >"$OUT/xvfb.log" 2>&1 &
for _ in $(seq 1 50); do [ -e /tmp/.X11-unix/X99 ] && break; sleep 0.1; done
DISPLAY=:99 glxinfo -B >"$OUT/glxinfo.txt" 2>&1 || true

WAYLAND=""
for renderer in gl pixman; do
  weston --backend=headless --renderer="$renderer" --socket=wl-check \
    --width=1920 --height=1080 --idle-time=0 >"$OUT/weston-$renderer.log" 2>&1 &
  wpid=$!
  for _ in $(seq 1 30); do
    [ -S "$XDG_RUNTIME_DIR/wl-check" ] && { WAYLAND="$renderer"; break 2; }
    kill -0 "$wpid" 2>/dev/null || break
    sleep 0.2
  done
  kill "$wpid" 2>/dev/null
done
echo "X11: Xvfb :99 ($(grep -m1 'OpenGL renderer' "$OUT/glxinfo.txt" | cut -d: -f2- | xargs))"
echo "Wayland: ${WAYLAND:+headless Weston, $WAYLAND renderer}${WAYLAND:-not available}"

# --- one case -----------------------------------------------------------------
ROWS=()
CRASHED=0
run_case() { # name display(x11|wayland) app|minibrowser [url] [VAR=value ...]
  local name=$1 disp=$2 kind=$3; shift 3
  local cmd=("$APP")
  if [ "$kind" = minibrowser ]; then cmd=("$MB" "$1"); shift; fi
  if [ "$disp" = wayland ] && [ -z "$WAYLAND" ]; then
    ROWS+=("| $name | $disp | skipped: no Wayland compositor | |"); return
  fi
  local denv=(-u WAYLAND_DISPLAY DISPLAY=:99 GDK_BACKEND=x11)
  [ "$disp" = wayland ] && denv=(-u DISPLAY WAYLAND_DISPLAY=wl-check GDK_BACKEND=wayland)
  local home; home="$(mktemp -d)"
  local start; start=$(date +%s)
  (
    cd "$home" || exit 1
    ulimit -c unlimited
    exec setsid dbus-run-session -- env "${denv[@]}" "$@" \
      HOME="$home" XDG_CONFIG_HOME="$home/.config" XDG_DATA_HOME="$home/.local/share" \
      XDG_CACHE_HOME="$home/.cache" "${cmd[@]}"
  ) >"$OUT/$name.log" 2>&1 &
  local pid=$! t=0 result="running after ${SECS}s"
  while [ "$t" -lt "$SECS" ]; do
    sleep 1; t=$((t + 1))
    if [ "$disp" = x11 ] && { [ "$t" -eq 10 ] || [ "$t" -eq 30 ] || [ "$t" -eq $((SECS - 5)) ]; }; then
      import -display :99 -window root "$OUT/$name-${t}s.png" 2>/dev/null || true
    fi
    if ! kill -0 "$pid" 2>/dev/null; then
      wait "$pid"; result="exited after ${t}s (status $?)"; break
    fi
  done
  kill -TERM -- "-$pid" 2>/dev/null; sleep 3; kill -KILL -- "-$pid" 2>/dev/null; wait "$pid" 2>/dev/null
  # Anything that left the session (sidecars, web processes) goes too, so
  # one case cannot leave state for the next.
  pkill -KILL -x app 2>/dev/null; pkill -KILL -f 'yourowai-|llama-server|MiniBrowser|WebKit[A-Za-z]*Process' 2>/dev/null
  sleep 2 # let systemd-coredump finish writing
  local crashes
  crashes=$(sudo coredumpctl --json=short --no-pager list --since="@$start" 2>/dev/null \
    | jq -r '[.[] | "\(.exe | split("/") | last) signal \(.sig)"] | join(", ")' 2>/dev/null)
  [ -n "$crashes" ] && CRASHED=$((CRASHED + 1))
  for f in "$home"/.local/share/*/logs/*.log; do
    [ -f "$f" ] && cp "$f" "$OUT/$name-app-$(basename "$f" | tr ' ' '_')"
  done
  ROWS+=("| $name | $disp | $result | ${crashes:-none} |")
  echo "$name: $result; crashes: ${crashes:-none}"
}

# --- the cases ----------------------------------------------------------------
run_case app-x11            x11     app
run_case app-x11-scale2     x11     app GDK_SCALE=2
run_case app-wayland        wayland app
run_case minibrowser-blank-x11      x11     minibrowser about:blank
run_case minibrowser-vt-x11         x11     minibrowser "$PAGE"
run_case minibrowser-novt-x11       x11     minibrowser "$PAGE?vt=0"
run_case minibrowser-blank-wayland  wayland minibrowser about:blank
run_case minibrowser-vt-wayland     wayland minibrowser "$PAGE"
run_case minibrowser-novt-wayland   wayland minibrowser "$PAGE?vt=0"

# --- report -------------------------------------------------------------------
{
  echo "### WebKitGTK launch check: $(lsb_release -ds 2>/dev/null)"
  echo
  echo "WebKitGTK $(dpkg-query -W -f='${Version}' libwebkit2gtk-4.1-0 2>/dev/null), $(dpkg-query -W -f='your-own-ai ${Version}' your-own-ai 2>/dev/null), ${SECS}s per case"
  echo
  echo "| Case | Display | Result | Crashes |"
  echo "|---|---|---|---|"
  printf '%s\n' "${ROWS[@]}"
} | tee "$OUT/summary.md" >> "${GITHUB_STEP_SUMMARY:-/dev/null}"
cat "$OUT/summary.md"
[ "$CRASHED" -eq 0 ]
