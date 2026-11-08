#!/usr/bin/env bash
# The update from the previous release with no Vault, live: six launches
# of the chat spec in one scratch home, each checked from the app's own log.
#   1. The previous release (MOVE_FROM_TREE, a worktree of that release
#      built there with `npm run e2e:build`): the 0.6 line alone, records
#      written there.
#   2. This build: the 0.7 line starts beside it; the first pass carries
#      every record (each kind complete) and flips the profile's marker.
#      0.6 leads this session, so it runs on until the next start.
#   3. This build again: every record already carried is found and only
#      what launch 2's chat wrote is copied. With no other device the pass
#      carries every author's records, then the 0.6 line stops at the
#      watch's next round - when the computer is idle (a person at it holds
#      the full pass, as it should). The chat after the stop is recorded.
#   4. After the stop: the 0.7 line alone, every conversation still listed.
#   5. The previous release again, by hand: it writes to the 0.6 line alone.
#   6. This build: it notices, starts 0.6 beside 0.7 and carries that.
# Needs: the e2e builds of this app and of the previous release, the
# machine's models (YOAI_E2E_MODELS_DIR for a subset), no Vault answering
# and no app instance up. The scratch home is kept for reading afterwards
# (MOVE_HOME, default ~/.cache/yoai-move-live).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE/.."
FROM="$(cd "${MOVE_FROM_TREE:?the previous release tree (MOVE_FROM_TREE)}" && pwd)"
H="${MOVE_HOME:-$HOME/.cache/yoai-move-live}"
rm -rf "$H"; mkdir -p "$H"
DATA="$H/.local/share/com.solar.yourowai"
fail() { echo "FAIL: $*" >&2; exit 1; }
# Nobody at the computer, as far as this build can tell: the full pass and
# the stop run as on an idle computer (a debug build reads this; the
# previous release ignores it). MOVE_REAL_IDLE=1 leaves the real reading.
[ -n "${MOVE_REAL_IDLE:-}" ] || export YOAI_TEST_AWAY_SECS=600

[ -z "$(pgrep -x 'app|your-own-ai')" ] || fail "an app instance is up"
for p in 27777 27778 27779; do
  ! curl -s -m 2 "http://127.0.0.1:$p/status" >/dev/null 2>&1 || fail "a Vault answers on $p: this run is the no-Vault case"
done

seen=0
launch() {  # <n> <tree>: one launch of the chat spec; its log lines go to launch-<n>.log
  echo "== launch $1 ($(basename "$2"))"
  (cd "$2" && YOAI_E2E_HOME="$H" node e2e/run.mjs models --spec e2e/specs/models/chat.e2e.ts) > "$H/launch-$1.out" 2>&1 || { tail -30 "$H/launch-$1.out"; fail "launch $1: the chat spec failed"; }
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
total_of() { sed -E 's/.*conversations [0-9]+\/([0-9]+).*/\1/' <<< "$1"; }
written_of() { sed -E 's/.*conversations [0-9]+\/[0-9]+ \(\+([0-9]+)\).*/\1/' <<< "$1"; }
no_07_warnings() {  # <n>
  ! grep -qE '\]\[(WARN|ERROR)\] \[0\.7\]' "$H/launch-$1.log" || { grep -E '\]\[(WARN|ERROR)\] \[0\.7\]' "$H/launch-$1.log" | head -5 >&2; fail "launch $1: warnings on the 0.7 line"; }
}

launch 1 "$FROM"
PROFILE="$(dirname "$(find "$DATA/profiles" -maxdepth 2 -name 'lair' -type d | head -1)")"
[ -d "$PROFILE" ] || fail "launch 1: no profile with a key store"
[ ! -e "$PROFILE/moved-07" ] || fail "launch 1: the marker is there before any move"
[ ! -e "$PROFILE/conductor-07" ] || fail "launch 1: the previous release made a 0.7 conductor"
echo "   the previous release on 0.6 alone; profile $(basename "$PROFILE")"

stopped() { grep -q "\[0\.6\] stopped: no other device needs it" "$H/launch-$1.log"; }
# After the stop: 0.7 alone, and every AI still lists what was carried.
# Then going back by hand: the previous release on the moved profile writes
# to the 0.6 line alone, and this build, started again, notices and carries
# what it wrote.
after_stop() {  # <n> <conversations expected>
  local n="$1" back="$(( $1 + 1 ))" again="$(( $1 + 2 ))"
  YOAI_E2E_MIN_CONVERSATIONS="$2" launch "$n" "$(pwd)"
  grep -qE "\[lines\] 0\.7 leads$" "$H/launch-$n.log" || { grep -m2 "\[lines\]" "$H/launch-$n.log" >&2; fail "launch $n: the 0.6 line started again after the stop"; }
  grep -o "e2e: [0-9]* conversation(s) listed, [0-9]* expected" "$H/launch-$n.out" | sed "s/^/   launch $n: /" || true
  no_07_warnings "$n"
  launch "$back" "$FROM"
  echo "   launch $back: the previous release ran on the moved profile"
  launch "$again" "$(pwd)"
  grep -q "\[0\.6\] written since this device stopped it" "$H/launch-$again.log" || fail "launch $again: what the previous release wrote on 0.6 went unnoticed"
  grep -q "\[lines\] 0.7 leads, 0.6 joins" "$H/launch-$again.log" || { grep -m2 "\[lines\]" "$H/launch-$again.log" >&2; fail "launch $again: the 0.6 line did not start again to carry it"; }
  local p; p="$(first_pass "$again")"
  [ "$(written_of "$p")" -ge 1 ] || fail "launch $again: what the previous release wrote was not carried"
  no_07_warnings "$again"
  echo "== the move with no Vault, and going back by hand: passed (home kept at $H)"
  exit 0
}

launch 2 "$(pwd)"
grep -q "\[lines\] 0.6 leads, 0.7 joins" "$H/launch-2.log" || { grep -m2 "\[lines\]" "$H/launch-2.log" >&2; fail "launch 2: an unmoved profile did not start 0.6 with 0.7 beside it"; }
p2="$(first_pass 2)"
grep -q "; flipped" <<< "$p2" || fail "launch 2: the first full pass did not flip the marker"
[ -e "$PROFILE/moved-07" ] || fail "launch 2: no marker in the profile"
conv2="$(total_of "$p2")"
[ "${conv2:-0}" -ge 1 ] || fail "launch 2: no conversation carried"

launch 3 "$(pwd)"
grep -q "\[lines\] 0.7 leads, 0.6 joins" "$H/launch-3.log" || { grep -m2 "\[lines\]" "$H/launch-3.log" >&2; fail "launch 3: a moved profile did not lead with 0.7 while 0.6 still holds records"; }
p3="$(first_pass 3)"
total3="$(total_of "$p3")"; written3="$(written_of "$p3")"
[ "$total3" -ge "$conv2" ] || fail "launch 3: fewer conversations than launch 2 carried ($total3 < $conv2)"
[ "$written3" -le $((total3 - conv2 + 1)) ] || fail "launch 3: re-copied conversations launch 2 had carried (+$written3 of $total3)"
no_07_warnings 3
if ! stopped 3; then
  grep -E "passes carry|pass skipped|computer is in use|other programs are busy" "$H/launch-3.log" | head -5 >&2 || true
  echo "== the move with no Vault: launches 1-3 passed; the 0.6 line did not stop (the full pass waits for an idle computer). Home kept at $H"
  exit 3
fi
echo "   launch 3 stopped the 0.6 line"
after_stop 4 "$total3"
