#!/usr/bin/env bash
# The move with no Vault, live: three launches of the chat spec in one
# scratch home, each checked from the app's own log.
#   1. A new install: the 0.6 line alone, records written there.
#   2. The 0.7 line starts beside it; the first pass carries every record
#      (each kind complete) and flips the profile's marker.
#   3. A later start: every record already carried is found, only what
#      launch 2's chat wrote is copied, and nothing fails.
# Needs: the e2e build of this app, the machine's models (YOAI_E2E_MODELS_DIR
# for a subset), no Vault answering and no app instance up. The scratch home
# is kept for reading afterwards (MOVE_HOME, default ~/.cache/yoai-move-live).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE/.."
H="${MOVE_HOME:-$HOME/.cache/yoai-move-live}"
rm -rf "$H"; mkdir -p "$H"
DATA="$H/.local/share/com.solar.yourowai"
fail() { echo "FAIL: $*" >&2; exit 1; }

[ -z "$(pgrep -x 'app|your-own-ai')" ] || fail "an app instance is up"
for p in 27777 27778 27779; do
  ! curl -s -m 2 "http://127.0.0.1:$p/status" >/dev/null 2>&1 || fail "a Vault answers on $p: this run is the no-Vault case"
done

seen=0
launch() {  # <n>: one launch of the chat spec; its log lines go to launch-<n>.log
  echo "== launch $1"
  YOAI_E2E_HOME="$H" node e2e/run.mjs models --spec e2e/specs/models/chat.e2e.ts > "$H/launch-$1.out" 2>&1 || { tail -30 "$H/launch-$1.out"; fail "launch $1: the chat spec failed"; }
  local total; total="$(cat "$DATA"/logs/*.log 2>/dev/null | wc -l)"
  cat "$DATA"/logs/*.log | sed -E 's/\x1b\[[0-9;]*m//g' | tail -n "$((total - seen))" > "$H/launch-$1.log"
  seen="$total"
  grep -E "passing|failing" "$H/launch-$1.out" | tail -2
}

# The first pass line of a launch, every kind complete and nothing failed.
first_pass() {  # <n>
  local line; line="$(grep -m1 -E '\[0\.7\] pass: ' "$H/launch-$1.log" || true)"
  [ -n "$line" ] || fail "launch $1: no pass ran"
  echo "   launch $1: ${line#*\[0.7\] }" >&2
  ! grep -q " incomplete" <<< "$line" || fail "launch $1: a kind is incomplete"
  # "<kinds>[; flipped][; <error>] in <n> ms"
  local head="${line% in * ms}"; head="${head/; flipped/}"
  ! grep -q "; " <<< "$head" || fail "launch $1: the pass ended with an error: ${head#*; }"
  python3 - "$line" <<'PY' || fail "launch $1: a kind's records are not all on 0.7"
import re, sys
kinds = re.findall(r"([a-z ]+) (\d+)/(\d+) \(\+(\d+)\)", sys.argv[1].split("pass: ", 1)[1])
assert kinds and all(int(a) == int(b) for _, a, b, _ in kinds), kinds
PY
  printf '%s' "$line"
}

launch 1
grep -q "no 0.6 key store yet: the move begins at the next start" "$H/launch-1.log" || fail "launch 1: a new install did not start on the 0.6 line alone"
! grep -qE '\[0\.7\] pass: ' "$H/launch-1.log" || fail "launch 1: a pass ran on a new install"
PROFILE="$(dirname "$(find "$DATA/profiles" -maxdepth 2 -name 'lair' -type d | head -1)")"
[ -d "$PROFILE" ] || fail "launch 1: no profile with a key store"
[ ! -e "$PROFILE/moved-07" ] || fail "launch 1: the marker is there before any move"
echo "   new install on 0.6 alone; profile $(basename "$PROFILE")"

launch 2
p2="$(first_pass 2)"
grep -q "; flipped" <<< "$p2" || fail "launch 2: the first full pass did not flip the marker"
[ -e "$PROFILE/moved-07" ] || fail "launch 2: no marker in the profile"
conv2="$(sed -E 's/.*conversations ([0-9]+)\/.*/\1/' <<< "$p2")"
[ "${conv2:-0}" -ge 1 ] || fail "launch 2: no conversation carried"

launch 3
p3="$(first_pass 3)"
written3="$(sed -E 's/.*conversations [0-9]+\/[0-9]+ \(\+([0-9]+)\).*/\1/' <<< "$p3")"
total3="$(sed -E 's/.*conversations [0-9]+\/([0-9]+).*/\1/' <<< "$p3")"
[ "$total3" -ge "$conv2" ] || fail "launch 3: fewer conversations than launch 2 carried ($total3 < $conv2)"
[ "$written3" -le $((total3 - conv2 + 1)) ] || fail "launch 3: re-copied conversations launch 2 had carried (+$written3 of $total3)"
! grep -qE '\]\[(WARN|ERROR)\] \[0\.7\]' "$H/launch-3.log" || { grep -E '\]\[(WARN|ERROR)\] \[0\.7\]' "$H/launch-3.log" | head -5 >&2; fail "launch 3: warnings on the 0.7 line"; }
echo "== the move with no Vault: passed (home kept at $H)"
