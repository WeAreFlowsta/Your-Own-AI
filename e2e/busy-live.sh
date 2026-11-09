#!/usr/bin/env bash
# Background work on a busy computer, live (specs/busy): one fresh profile,
# no Vault. Another program takes about half the processor from launch; the
# folder check that runs by itself waits and says why, a reply still
# answers, and once the program stops the check runs by itself and reads the
# note added meanwhile.
# Nobody at the computer, as far as this build can tell (YOAI_TEST_AWAY_SECS),
# so the processor is the only thing holding the work.
# Needs: the e2e build of this app, the machine's models (YOAI_E2E_MODELS_DIR
# for a subset; it must hold the embedding model), no app up. Logs and the
# home are kept in BUSY_LOGS (default ~/.cache/yoai-busy-live).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE/.."
LOGS="${BUSY_LOGS:-$HOME/.cache/yoai-busy-live}"; rm -rf "$LOGS"; mkdir -p "$LOGS"
H="$LOGS/home"; mkdir -p "$H"
DATA="$H/.local/share/com.solar.yourowai"
fail() { echo "FAIL: $*" >&2; exit 1; }
# The spec stops its busy loops; this catches any a crashed run leaves.
trap 'pkill -f "yoai-busy-hog" 2>/dev/null || true' EXIT

[ -z "$(pgrep -x 'app|your-own-ai')" ] || fail "an app instance is up"
export YOAI_TEST_AWAY_SECS=600

echo "== one launch, a busy computer from the start"
YOAI_E2E_LONG=1 YOAI_E2E_HOME="$H" node e2e/run.mjs models --spec e2e/specs/busy/busy.e2e.ts > "$LOGS/run.out" 2>&1 \
  || { grep -vE "window states|get_window_states" "$LOGS/run.out" | tail -40; fail "the spec failed"; }
cat "$DATA"/logs/*.log 2>/dev/null | sed -E 's/\x1b\[[0-9;]*m//g' > "$LOGS/app.log"
grep -oE "e2e: .*" "$LOGS/run.out" | grep -v "model loaded" | sed 's/^/   /'
grep -E "\[machine\] .* waits: " "$LOGS/app.log" | sed -E 's/.*\[machine\] /   app: /' | sort | uniq -c
grep -q "\[machine\] folder indexing waits: other programs are busy" "$LOGS/app.log" \
  || fail "the app never logged the folder check waiting for the busy processor"
grep -E "passing|failing" "$LOGS/run.out" | tail -2
echo "PASS: background work waited for the busy computer and ran once it was free"
