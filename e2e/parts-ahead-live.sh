#!/usr/bin/env bash
# The parts a new version needs, fetched ahead of the update, live
# (specs/engine/parts-ahead): two launches of one scratch profile.
#   1. "The version doing the update": the profile holds an earlier Projects
#      helper and an earlier NVIDIA engine. The app is handed a manifest's
#      parts (the test build's YOAI_TEST_PARTS_AHEAD) and fetches both ahead:
#      the helper beside the installed one, the engine into its own folder.
#   2. "The new version": the engine it pins is marked as fetched ahead. At
#      start it puts both in place, removes the earlier ones, and the model
#      comes up on the NVIDIA engine. Downloads are switched off in this
#      launch: nothing may be fetched.
# The engine download is this machine's installed engine, zipped and served
# from 127.0.0.1; the helper is the real pinned release (about 70 MB).
# Needs: Linux, an NVIDIA card with the engine installed in the person's
# profile at the pinned version (only read), a copy of the helper program
# (PARTS_AHEAD_HELPER, default ~/.cache/yoai-build-043/your-own-ai-build),
# the e2e build, the machine's models (YOAI_E2E_MODELS_DIR for a subset),
# about 6 GB of free disk, no app up. Logs are kept in PARTS_AHEAD_LOGS.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE/.."
LOGS="${PARTS_AHEAD_LOGS:-$HOME/.cache/yoai-parts-ahead-live}"; rm -rf "$LOGS"; mkdir -p "$LOGS"
fail() { echo "FAIL: $*" >&2; exit 1; }
nap() { python3 -c "import time; time.sleep($1)"; }
[ -z "$(pgrep -x 'app|your-own-ai')" ] || fail "an app instance is up"
TAG="$(sed -n 's/^pub const LLAMA_ENGINE_TAG: &str = "llama-\(.*\)";.*/\1/p' src-tauri/src/engine.rs | head -1)"
[ -n "$TAG" ] || fail "could not read the pinned engine tag"
REAL="$HOME/.local/share/com.solar.yourowai/engines/cuda-$TAG"
HELPER="${PARTS_AHEAD_HELPER:-$HOME/.cache/yoai-build-043/your-own-ai-build}"
[ -x "$REAL/llama-server" ] || { echo "== no NVIDIA engine at the pinned version ($TAG) on this machine; skipped"; exit 0; }
[ -x "$HELPER" ] || { echo "== no copy of the helper program at $HELPER; skipped"; exit 0; }
H="$LOGS/home"; DATA="$H/.local/share/com.solar.yourowai"
mkdir -p "$DATA/engines/cuda-b0000" "$DATA/build"
printf '#!/bin/sh\nexit 1\n' > "$DATA/engines/cuda-b0000/llama-server"; chmod +x "$DATA/engines/cuda-b0000/llama-server"
cp "$HELPER" "$DATA/build/your-own-ai-build"; echo "0.4.2" > "$DATA/build/VERSION"

SERVE="$LOGS/serve"; mkdir -p "$SERVE"
ZIP="$SERVE/engine.zip"
echo "== zipping the installed engine ($TAG)"
( cd "$REAL" && zip -q -1 -r "$ZIP" . )
SHA="$(sha256sum "$ZIP" | cut -d' ' -f1)"
SIZE="$(stat -c %s "$ZIP")"
PORT=18474
python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$SERVE" > "$LOGS/server.out" 2>&1 &
SERVER=$!
strays() { for p in $(pgrep -x llama-server || true); do case "$(readlink "/proc/$p/exe" 2>/dev/null)" in "$DATA"/*) echo "$p";; esac; done; }
cleanup() { kill "$SERVER" 2>/dev/null || true; for p in $(strays); do kill "$p" 2>/dev/null || true; done; rm -rf "$SERVE" "$DATA/engines" "$DATA/build" "$DATA/build.next"; }
trap cleanup EXIT
nap 1

# The helper as this app's own parts.json names it for this platform.
HELPER_PART="$(python3 -c "
import json
for p in json.load(open('src-tauri/parts.json')):
    if p['name'] == 'projects-helper' and p['target'] == 'linux-x86_64': print(json.dumps(p))")"
PARTS="[$HELPER_PART,{\"name\":\"nvidia-engine\",\"target\":\"x86_64-unknown-linux-gnu\",\"version\":\"b99999\",\"size\":$SIZE,\"sha256\":\"$SHA\"}]"
applog() { cat "$DATA"/logs/*.log 2>/dev/null | sed -E 's/\x1b\[[0-9;]*m//g'; }
show() { grep -E "\[Engine\]|\[build\]|\[update\]|spawning chat server" "$1" | sed -E 's/^\[[0-9-]+\]\[([0-9:]+)\]\[[^]]*\]\[[A-Z]+\] /   \1 /' | cut -c1-200 | head -16; }

echo "== 1. the version doing the update: the named parts are fetched ahead"
PARTS_AHEAD_PHASE=before PARTS_AHEAD_DATA="$DATA" YOAI_TEST_PARTS_AHEAD="$PARTS" \
  YOAI_BLOCK_MODEL_DOWNLOADS=0 YOAI_TEST_CUDA_ZIP_URL="http://127.0.0.1:$PORT/engine.zip" YOAI_TEST_CUDA_ZIP_SHA256="$SHA" \
  YOAI_E2E_HOME="$H" node e2e/run.mjs models --spec e2e/specs/engine/parts-ahead.e2e.ts > "$LOGS/before.out" 2>&1 \
  || { grep -vE "window states|get_window_states|mock store" "$LOGS/before.out" | tail -30; applog > "$LOGS/before-app.log"; show "$LOGS/before-app.log"; fail "phase 1 failed"; }
applog > "$LOGS/before-app.log"; rm -f "$DATA"/logs/*.log
grep -oE "e2e: .*" "$LOGS/before.out" | cut -c1-200 | sed 's/^/   /'
show "$LOGS/before-app.log"
grep -q "Your Own AI Build 0.4.3 is in place for the update\|is in place for the update" "$LOGS/before-app.log" || fail "the log does not say a part was fetched ahead"
for p in $(strays); do kill "$p" 2>/dev/null || true; done

# The new version pins the engine that is now installed: say it came ahead.
[ -x "$DATA/engines/cuda-$TAG/llama-server" ] || fail "this version's engine is not installed after phase 1"
: > "$DATA/engines/cuda-$TAG/fetched-ahead"

echo "== 2. the new version: both are put in place at start, nothing is downloaded"
PARTS_AHEAD_PHASE=after PARTS_AHEAD_DATA="$DATA" PARTS_AHEAD_ENGINE="$TAG" YOAI_BLOCK_MODEL_DOWNLOADS=1 \
  YOAI_E2E_HOME="$H" node e2e/run.mjs models --spec e2e/specs/engine/parts-ahead.e2e.ts > "$LOGS/after.out" 2>&1 \
  || { grep -vE "window states|get_window_states|mock store" "$LOGS/after.out" | tail -30; applog > "$LOGS/after-app.log"; show "$LOGS/after-app.log"; fail "phase 2 failed"; }
applog > "$LOGS/after-app.log"
grep -oE "e2e: .*" "$LOGS/after.out" | cut -c1-200 | sed 's/^/   /'
show "$LOGS/after-app.log"
grep -q "\[build\] Your Own AI Build .* was fetched before the update: in place" "$LOGS/after-app.log" || fail "the helper was not put in place from the one fetched ahead"
grep -q "\[Engine\] the NVIDIA engine for this version was fetched before the update: in place" "$LOGS/after-app.log" || fail "the engine was not adopted"
if grep -qE "fetching .* now|fetching Your Own AI Build|downloading" "$LOGS/after-app.log"; then fail "the new version downloaded something: $(grep -E 'fetching .* now|fetching Your Own AI Build|downloading' "$LOGS/after-app.log" | head -1 | cut -c1-160)"; fi
first_spawn="$(grep -m1 "spawning chat server" "$LOGS/after-app.log" || true)"
case "$first_spawn" in *"Cuda engine"*) ;; *) fail "the first model load was not on the NVIDIA engine: $first_spawn";; esac
echo "== parts fetched ahead and put in place by the new version: passed (logs kept at $LOGS)"
