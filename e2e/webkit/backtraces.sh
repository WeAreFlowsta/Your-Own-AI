#!/usr/bin/env bash
# A backtrace for every crash systemd-coredump caught on this machine,
# with debug symbols from Ubuntu's debuginfod server. Writes one file per
# crash to <output dir>/backtraces/ and prints the crashing thread's
# frames.
#
# Usage: e2e/webkit/backtraces.sh <output dir>
set -u
OUT="${1:?usage: backtraces.sh <output dir>}"; mkdir -p "$OUT/backtraces"
export DEBUGINFOD_URLS="${DEBUGINFOD_URLS:-https://debuginfod.ubuntu.com}"
sudo coredumpctl --no-pager list >"$OUT/backtraces/coredumpctl-list.txt" 2>&1
sudo coredumpctl --json=short --no-pager list 2>/dev/null | jq -r '.[] | "\(.pid) \(.exe)"' |
while read -r pid exe; do
  name="$(basename "$exe").$pid"
  core="$(mktemp)"
  if ! sudo coredumpctl -q dump "$pid" -o "$core" >/dev/null 2>&1; then
    echo "no core stored for $exe ($pid)" | tee "$OUT/backtraces/$name.txt"; continue
  fi
  sudo -E timeout 1200 gdb -batch -iex 'set debuginfod enabled on' \
    -ex 'bt 60' -ex 'thread apply all bt 25' -ex 'info sharedlibrary' \
    "$exe" "$core" >"$OUT/backtraces/$name.txt" 2>&1
  sudo rm -f "$core"
  echo "== $exe ($pid)"
  grep -m 30 '^#' "$OUT/backtraces/$name.txt"
done
true
