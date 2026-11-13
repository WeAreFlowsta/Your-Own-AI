#!/usr/bin/env bash
# After a restore, a join or a move: no conductor under the given profile
# folders may have said that a chain forked or that a write found the chain
# head moved. On Holochain 0.7.1 a cell's own chain and the network's copies
# share one store, so a second chain started under a key that already has
# one breaks that device's writes ("source chain head has moved", "FOREIGN
# KEY constraint failed" in the publish workflow) and, by design, is
# answered with a chain-fork warrant.
# Usage: e2e/no-fork-check.sh <profile home> [<profile home> ...]
# Prints each hit with its file and count; exits 1 on any.
set -u
bad=0
for home in "$@"; do
  while IFS= read -r -d '' log; do
    for what in "head has moved" "FOREIGN KEY constraint failed" "chain fork" "ChainFork"; do
      n="$(sed -E 's/\x1b\[[0-9;]*m//g' "$log" | grep -c -- "$what" || true)"
      if [ "${n:-0}" -gt 0 ]; then
        echo "   NO-FORK CHECK: \"$what\" x $n in ${log#"$home"/}"
        bad=1
      fi
    done
  done < <(find "$home" \( -name 'holochain-stderr*.log' -o -path '*/com.solar.yourowai/logs/*.log' \) -print0 2>/dev/null)
done
[ "$bad" -eq 0 ] && echo "   no fork, no moved chain head, no failed publish in $# profile(s)"
exit "$bad"
