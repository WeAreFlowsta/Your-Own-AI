#!/usr/bin/env bash
# The online UI specs one at a time, each after the last one's app and engines are gone
# and 9 GB is free (run back to back, a spec's leftover embedding server kept the next
# spec's from starting: 11-18).   online-one-at-a-time.sh <out-dir> <spec> ...
set -u
O="$1"; shift; mkdir -p "$O"; cd "$(dirname "$0")/.." || exit 2
nap() { python3 -c "import time; time.sleep($1)"; }
echo "$(date +%H:%M) $(git branch --show-current) $(git log --oneline -1 | cut -c1-50)" | tee "$O/summary.txt"
for s in "$@"; do
  for i in $(seq 1 90); do
    avail=$(awk '/MemAvailable/ {print int($2/1048576)}' /proc/meminfo)
    if pgrep -x your-own-ai >/dev/null || pgrep -x app >/dev/null || pgrep -x llama-server >/dev/null || pgrep -x yourowai-llama- >/dev/null; then nap 10; continue; fi
    [ "$avail" -ge 9 ] && break; nap 10
  done
  t0=$(date +%s)
  YOAI_E2E_LONG=1 systemd-run --user --scope -q -p MemoryMax=8G -p MemorySwapMax=0 --collect node e2e/run.mjs online --spec "e2e/specs/online/$s.e2e.ts" > "$O/$s.out" 2>&1
  rc=$?; echo "$([ $rc = 0 ] && echo PASS || echo FAIL) $s ($(( $(date +%s) - t0 ))s, exit $rc)" | tee -a "$O/summary.txt"
done
echo "$(date +%H:%M) done" | tee -a "$O/summary.txt"
