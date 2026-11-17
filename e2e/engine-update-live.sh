#!/usr/bin/env bash
# The NVIDIA engine as an update's last step, live (specs/engine): a scratch
# profile holds an engine folder from "an earlier version"; the app, started
# with nothing pressed, fetches the pinned engine by itself, the first model
# load waits for it, and the model comes up on the NVIDIA engine.
# The download is this machine's own installed engine, zipped and served
# from 127.0.0.1 (the test build's YOAI_TEST_CUDA_ZIP_URL), so nothing large
# is fetched from the network. The person's engine folder is only read.
# Needs: Linux with an NVIDIA card and the engine installed in the person's
# profile at this app's pinned version, the e2e build, the machine's models
# (YOAI_E2E_MODELS_DIR for a subset), about 4 GB of free disk, no app up.
# Logs are kept in ENGINE_UPDATE_LOGS; the zip and the scratch engine go.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE/.."
LOGS="${ENGINE_UPDATE_LOGS:-$HOME/.cache/yoai-engine-update-live}"; rm -rf "$LOGS"; mkdir -p "$LOGS"
fail() { echo "FAIL: $*" >&2; exit 1; }
nap() { python3 -c "import time; time.sleep($1)"; }
[ -z "$(pgrep -x 'app|your-own-ai')" ] || fail "an app instance is up"
TAG="$(sed -n 's/^pub const LLAMA_ENGINE_TAG: &str = "llama-\(.*\)";.*/\1/p' src-tauri/src/engine.rs | head -1)"
[ -n "$TAG" ] || TAG="$(sed -n 's/^const LLAMA_ENGINE_TAG: &str = "llama-\(.*\)";.*/\1/p' src-tauri/src/engine.rs | head -1)"
[ -n "$TAG" ] || fail "could not read the pinned engine tag"
REAL="$HOME/.local/share/com.solar.yourowai/engines/cuda-$TAG"
[ -x "$REAL/llama-server" ] || { echo "== no NVIDIA engine at the pinned version ($TAG) on this machine; skipped"; exit 0; }
H="$LOGS/home"; DATA="$H/.local/share/com.solar.yourowai"
mkdir -p "$DATA/engines/cuda-b0000"
printf '#!/bin/sh\nexit 1\n' > "$DATA/engines/cuda-b0000/llama-server"; chmod +x "$DATA/engines/cuda-b0000/llama-server"

SERVE="$LOGS/serve"; mkdir -p "$SERVE"
ZIP="$SERVE/engine.zip"
echo "== zipping the installed engine ($TAG)"
( cd "$REAL" && zip -q -1 -r "$ZIP" . )
SHA="$(sha256sum "$ZIP" | cut -d' ' -f1)"
PORT=18473
python3 -m http.server "$PORT" --bind 127.0.0.1 --directory "$SERVE" > "$LOGS/server.out" 2>&1 &
SERVER=$!
# An engine process started from the scratch profile can outlive the session.
strays() { for p in $(pgrep -x llama-server || true); do case "$(readlink "/proc/$p/exe" 2>/dev/null)" in "$DATA"/*) echo "$p";; esac; done; }
cleanup() { kill "$SERVER" 2>/dev/null || true; for p in $(strays); do kill "$p" 2>/dev/null || true; done; rm -rf "$SERVE" "$DATA/engines"; }
trap cleanup EXIT
nap 1

echo "== one launch with an earlier engine in the profile; nothing is pressed"
YOAI_BLOCK_MODEL_DOWNLOADS=0 YOAI_TEST_CUDA_ZIP_URL="http://127.0.0.1:$PORT/engine.zip" YOAI_TEST_CUDA_ZIP_SHA256="$SHA" \
  YOAI_E2E_HOME="$H" node e2e/run.mjs models --spec e2e/specs/engine/engine-update.e2e.ts > "$LOGS/run.out" 2>&1 \
  || { grep -vE "window states|get_window_states|mock store" "$LOGS/run.out" | tail -30; fail "the spec failed"; }
cat "$DATA"/logs/*.log 2>/dev/null | sed -E 's/\x1b\[[0-9;]*m//g' > "$LOGS/app.log"
grep -oE "e2e: .*" "$LOGS/run.out" | cut -c1-260 | sed 's/^/   /'
grep -E "\[Engine\]|spawning chat server" "$LOGS/app.log" | sed -E 's/^\[[0-9-]+\]\[([0-9:]+)\]\[[^]]*\]\[[A-Z]+\] /   \1 /' | cut -c1-200 | head -14
grep -q "the model loads when the NVIDIA engine is in place" "$LOGS/app.log" || fail "the first load did not wait for the engine"
first_spawn="$(grep -m1 "spawning chat server" "$LOGS/app.log" || true)"
case "$first_spawn" in *"Cuda engine"*) ;; *) fail "the first model load was not on the NVIDIA engine: $first_spawn";; esac
grep -q "removed stale CUDA engine cuda-b0000" "$LOGS/app.log" || fail "the earlier engine was not removed"
echo "== the engine arrived by itself and the first load used it: passed (logs kept at $LOGS)"
