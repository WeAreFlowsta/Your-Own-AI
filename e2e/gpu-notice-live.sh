#!/usr/bin/env bash
# "Running on CPU for stability", live (specs/models/gpu-notice.e2e.ts): the
# record of earlier exits is seeded before each launch in a scratch home, and
# the debug build's safety ladder is switched on (FLOWSTA_GPU_SAFEMODE=1).
#   1. Two exits while NO model was loading onto the graphics card: no notice.
#   2. Two exits while a model WAS loading onto it: the notice, then "Try GPU
#      again" clears it.
# Needs: this app's e2e build, the machine's models (YOAI_E2E_MODELS_DIR for
# a subset), no app up. Logs and the home are kept in GPU_NOTICE_LOGS.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE/.."
LOGS="${GPU_NOTICE_LOGS:-$HOME/.cache/yoai-gpu-notice-live}"; rm -rf "$LOGS"; mkdir -p "$LOGS"
fail() { echo "FAIL: $*" >&2; exit 1; }
[ -z "$(pgrep -x 'app|your-own-ai')" ] || fail "an app instance is up"
run() {  # <phase> <a model was loading: true|false>
  local phase="$1" loading="$2" H="$LOGS/home-$1"
  mkdir -p "$H/.local/share/com.solar.yourowai"
  # One earlier strike on record, and a run that never ended cleanly.
  printf '{"pending":true,"crashes":1,"forced_cpu":false,"backend":"bundled","pending_since":%s,"gpu_started":true,"gpu_loading":%s}\n' "$(date +%s)" "$loading" > "$H/.local/share/com.solar.yourowai/gpu-safety.json"
  echo "== exits $phase a model load"
  FLOWSTA_GPU_SAFEMODE=1 GPU_NOTICE_PHASE="$phase" YOAI_E2E_HOME="$H" node e2e/run.mjs models --spec e2e/specs/models/gpu-notice.e2e.ts > "$LOGS/$phase.out" 2>&1 || { grep -vE "window states|get_window_states" "$LOGS/$phase.out" | tail -30; fail "exits $phase a model load: the spec failed"; }
  grep -oE "e2e: gpu safe mode .*" "$LOGS/$phase.out" | cut -c1-260 | sed 's/^/   /'
  grep -qE "1 passing" "$LOGS/$phase.out" || fail "exits $phase a model load: the spec did not run"
  sed -E 's/\x1b\[[0-9;]*m//g' "$H"/.local/share/com.solar.yourowai/logs/*.log | grep -E "\[GPU\]" | sed -E 's/^\[[0-9-]+\]\[([0-9:]+)\]\[[^]]*\]\[[A-Z]+\] /   \1 /' | head -6
}
run outside false
grep -q "the app closed while no model was loading onto the GPU" "$LOGS/home-outside"/.local/share/com.solar.yourowai/logs/*.log || fail "the exits outside a load were not set aside"
run during true
echo "== the GPU notice: passed (logs kept at $LOGS)"
