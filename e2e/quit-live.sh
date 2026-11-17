#!/usr/bin/env bash
# A quit, live, on Linux: the app is started in a scratch home on the real
# desktop session, asked to quit the way the system asks (SIGTERM, the clean
# path), and the quit is read back:
#   - the log says the page's processes were ended and the run closed cleanly;
#   - no page process outlives the app;
#   - nothing printed a heap abort ("corrupted ...", "free(): ...").
# QUIT_ROUNDS quits (default 5). Needs: this app's e2e build, no app up.
# Logs and the home are kept in QUIT_LOGS.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE/.."
LOGS="${QUIT_LOGS:-$HOME/.cache/yoai-quit-live}"; rm -rf "$LOGS"; mkdir -p "$LOGS"
ROUNDS="${QUIT_ROUNDS:-5}"
fail() { echo "FAIL: $*" >&2; exit 1; }
nap() { python3 -c "import time; time.sleep($1)"; }
[ "$(uname -s)" = Linux ] || { echo "== a Linux check; skipped here"; exit 0; }
[ -z "$(pgrep -x 'app|your-own-ai')" ] || fail "an app instance is up"
BIN_REAL="$(readlink -f src-tauri/target/debug/your-own-ai)"
ours() { for pid in $(pgrep -x your-own-ai); do [ "$(readlink -f "/proc/$pid/exe" 2>/dev/null)" = "$BIN_REAL" ] && echo "$pid"; done; }
# By the program file: the name in the process table is cut at 15 letters.
pages_of() { for p in $(pgrep -P "$1" || true); do case "$(readlink "/proc/$p/exe" 2>/dev/null)" in */WebKitWebProcess) echo "$p";; esac; done; }
for i in $(seq 1 "$ROUNDS"); do
  OUT="$LOGS/quit-$i.out"
  YOAI_E2E_NATIVE_WAYLAND=1 YOAI_E2E_HOME="$LOGS/home" YOAI_VAULT_PORT=27990 e2e/launch-app.sh > "$OUT" 2>&1 &
  app=""; pages=""
  for _ in $(seq 1 60); do
    app="$(ours | head -1)"
    if [ -n "$app" ]; then pages="$(pages_of "$app")"; [ -n "$pages" ] && break; fi
    nap 0.5
  done
  [ -n "$app" ] && [ -n "$pages" ] || fail "round $i: the app or its page process did not start (see $OUT)"
  nap 8
  pages="$(pages_of "$app")"
  kill -TERM "$app"
  for _ in $(seq 1 80); do [ -d "/proc/$app" ] || break; nap 0.5; done
  [ -d "/proc/$app" ] && fail "round $i: the app did not quit in 40 s"
  nap 2
  for p in $pages; do [ -d "/proc/$p" ] && fail "round $i: page process $p outlived the app"; done
  grep -q "\[exit\] ended [0-9]* page process" "$OUT" || fail "round $i: the log does not say the page's processes were ended"
  grep -q "\[exit\] closed cleanly" "$OUT" || fail "round $i: the run did not close cleanly"
  if grep -qE "corrupted|free\(\)|double free|malloc\(\)" "$OUT"; then fail "round $i: a heap abort was printed: $(grep -E 'corrupted|free\(\)|double free|malloc\(\)' "$OUT" | head -1)"; fi
  echo "   round $i: $(grep -o '\[exit\] ended [0-9]* page process[^ ]*' "$OUT" | head -1); closed cleanly"
done
echo "== $ROUNDS quits: passed (logs kept at $LOGS)"
