#!/usr/bin/env bash
# One computer with a Vault, through the update from the previous release
# and the 0.6 stop, then on the 0.7 line alone, live (specs/alone):
#   1. The previous release (MOVE_FROM_TREE, built there with
#      `npm run e2e:build`), linked to a fresh test Vault: the 0.6 line
#      alone; the chat spec writes conversations, and its backups go out.
#   2. This build: the 0.7 line starts beside it, the pass carries every
#      record and flips the marker.
#   3. This build again: a full pass, then the 0.6 line stops.
#   4. This build on the 0.7 line alone (ALONE_PHASE=alone): everything
#      listed, a new chat recorded, Back up now writes it all, search finds
#      old and new, an export imports once and not twice.
#   5. A reinstall under the same Vault (the profile wiped, its link kept):
#      the first start reads that no other device holds the identity's data,
#      starts on the 0.7 line alone, and the restore brings the backup back.
# Needs: the Vault's debug TEST binary (flowsta-vault/scripts/build-test-binary.sh),
# the e2e builds of this app and the previous release, the machine's models
# (YOAI_E2E_MODELS_DIR for a subset), the installed Vault CLOSED, no app up.
# ALONE_VAULT_WRAP: a command the test Vault starts under (its own memory
# scope). Logs and the home are kept in ALONE_LOGS (default ~/.cache/yoai-alone-live).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE/.."
VAULT="$(cd "$HERE/../../flowsta-vault" && pwd)"
FROM="$(cd "${MOVE_FROM_TREE:?the previous release tree (MOVE_FROM_TREE)}" && pwd)"
LOGS="${ALONE_LOGS:-$HOME/.cache/yoai-alone-live}"; rm -rf "$LOGS"; mkdir -p "$LOGS"
H="$LOGS/home"; mkdir -p "$H"
DATA="$H/.local/share/com.solar.yourowai"
API="${FLOWSTA_API_URL:-https://auth-api-staging.flowsta.com}"
CID="$(grep -A1 'YOAI_HOLOCHAIN_CLIENT_ID: &str' src-tauri/src/flowsta.rs | grep -o '"flowsta_app_[^"]*"' | tr -d '"' | head -1)"
fail() { echo "FAIL: $*" >&2; exit 1; }
# Nobody at the computer, as far as this build can tell (a debug build
# reads it; the previous release ignores it): the full pass and the stop
# run with a person at the computer. ALONE_REAL_IDLE=1 keeps the real one.
[ -n "${ALONE_REAL_IDLE:-}" ] || export YOAI_TEST_AWAY_SECS=600

killtests() { for pid in $(pgrep -x flowsta-vault); do case "$(readlink /proc/$pid/exe 2>/dev/null)" in *target/debug*) kill "$pid" 2>/dev/null || true;; esac; done; }
answering() { for p in 27777 27778 27779; do curl -s -m 2 "http://127.0.0.1:$p/dev/status" | grep -q '"harness":true' && echo "$p"; done; }
dev() { curl -s -m 60 -X POST "http://127.0.0.1:$1/dev/devices" -H 'content-type: application/json' -H 'origin: https://ourtest.flowsta.com' -d "$2"; }
trap 'killtests; sleep 2; killtests' EXIT

[ -z "$(pgrep -x 'app|your-own-ai')" ] || fail "an app instance is up"
[ -z "$(answering)" ] || fail "a test Vault is already answering - stop it first"
[ -x "$VAULT/src-tauri/target/debug/flowsta-vault" ] || fail "no Vault test binary (flowsta-vault/scripts/build-test-binary.sh)"
rm -rf "$HOME/.local/share/flowsta-vault-md-e"
(cd "$VAULT" && ${ALONE_VAULT_WRAP:-} dbus-run-session -- scripts/run-test-instance.sh md-e > "$LOGS/md-e.log" 2>&1 &)
P1=""; for _ in $(seq 1 36); do P1="$(answering | head -1 || true)"; [ -n "$P1" ] && break; sleep 5; done
[ -n "$P1" ] || fail "the test Vault never answered"
created="$(curl -s -m 120 -X POST "http://127.0.0.1:$P1/dev/setup-identity" -H 'content-type: application/json' \
  -d "{\"api_url\":\"$API\",\"password\":\"Alone-$(date +%s)!x\",\"email\":\"yoai-alone-$(date +%s)@example.com\",\"display_name\":\"Alone Test\"}")"
printf '%s' "$created" | grep -q '"phrase"' || fail "identity not created: $created"
for _ in $(seq 1 60); do dev "$P1" '{"op":"list"}' | grep -q '"devices"' && break; sleep 4; done
echo "== the test Vault ($P1) holds a new identity"

# The profile starts linked (the app's client id is not registered on the
# staging API), as devices-live.sh does.
key="u$(python3 -c 'import os,base64; print(base64.urlsafe_b64encode(bytes([0x84,0x20,0x24])+os.urandom(36)).decode().rstrip("="))')"
vkey="$(curl -s -m 3 "http://127.0.0.1:$P1/status" | grep -o '"agent_pub_key":"[^"]*"' | cut -d'"' -f4 || true)"
[ -n "$vkey" ] || fail "the test Vault reports no identity key"
folder="$(python3 -c 'import sys,base64,hashlib; k=sys.argv[1][1:]; raw=base64.urlsafe_b64decode(k+"="*(-len(k)%4)); print(hashlib.sha256(raw).hexdigest()[:16])' "$vkey")"
link() {
  local prof
  for prof in "$DATA/profiles/$folder" "$DATA/profiles/local"; do
    mkdir -p "$prof"; printf '{"link_done":true,"app_link_key":"%s"}\n' "$key" > "$prof/flowsta-auth.json"
  done
  dev "$P1" "{\"op\":\"connect\",\"client_id\":\"$CID\",\"app_name\":\"Your Own AI (alone)\",\"origin\":\"yoai://app\",\"app_agent_pub_key\":\"$key\"}" >/dev/null
}
link

# A pass's outcome line, not the lines that say where a running pass is. A
# launch that must see one (or the stop) waits for it: the pass waits while
# the chat runs.
PASS_DONE='\[0\.7\] pass: [a-z ]+ [0-9]+/[0-9]+ \(\+'
STOPPED='\[0\.6\] stopped: no other device needs it'
seen=0
launch() {  # <n> <tree> <spec> [env...]: one launch; its log lines go to launch-<n>.log
  local n="$1" tree="$2" spec="$3"; shift 3
  echo "== launch $n ($(basename "$tree"), $(basename "$spec"))"
  (cd "$tree" && env "$@" YOAI_E2E_HOME="$H" YOAI_VAULT_PORT="$P1" node e2e/run.mjs models --spec "$spec") > "$LOGS/launch-$n.out" 2>&1 || { grep -vE "window states|get_window_states" "$LOGS/launch-$n.out" | tail -30; fail "launch $n: the spec failed"; }
  local total; total="$(cat "$DATA"/logs/*.log 2>/dev/null | wc -l)"
  cat "$DATA"/logs/*.log | sed -E 's/\x1b\[[0-9;]*m//g' | tail -n "$((total - seen))" > "$LOGS/launch-$n.log"
  seen="$total"
  grep -E "passing|failing" "$LOGS/launch-$n.out" | tail -2
}
first_pass() {  # <n>: the first pass line, every kind complete
  local line; line="$(grep -m1 -E "$PASS_DONE" "$LOGS/launch-$1.log" || true)"
  [ -n "$line" ] || fail "launch $1: no pass ran"
  echo "   launch $1: ${line#*\[0.7\] }" >&2
  ! grep -q " incomplete" <<< "$line" || fail "launch $1: a kind is incomplete"
  printf '%s' "$line"
}
lines_said() { grep -m1 "\[lines\]" "$LOGS/launch-$1.log" | sed 's/.*\[lines\] //'; }

launch 1 "$FROM" e2e/specs/models/chat.e2e.ts
PROFILE="$(dirname "$(find "$DATA/profiles" -maxdepth 2 -name 'lair' -type d | head -1)")"
[ -d "$PROFILE" ] || fail "launch 1: no profile with a 0.6 key store"
[ ! -e "$PROFILE/conductor-07" ] || fail "launch 1: the previous release made a 0.7 conductor"
grep -q "incremental backup\|full backup" "$LOGS/launch-1.log" || echo "   launch 1: no backup line seen (the Vault may not have taken one yet)"
echo "   the previous release on 0.6 alone, linked; profile $(basename "$PROFILE")"

launch 2 "$(pwd)" e2e/specs/models/chat.e2e.ts YOAI_E2E_WAIT_LOG="$PASS_DONE"
[ "$(lines_said 2)" = "0.6 leads, 0.7 joins" ] || fail "launch 2: started '$(lines_said 2)', not 0.6 with 0.7 beside it"
p2="$(first_pass 2)"
grep -q "; flipped" <<< "$p2" || fail "launch 2: the first pass did not flip the marker"

launch 3 "$(pwd)" e2e/specs/models/chat.e2e.ts YOAI_E2E_WAIT_LOG="$STOPPED"
[ "$(lines_said 3)" = "0.7 leads, 0.6 joins" ] || fail "launch 3: started '$(lines_said 3)'"
p3="$(first_pass 3)"
grep -q "\[0\.6\] stopped: no other device needs it" "$LOGS/launch-3.log" || { grep -E "passes carry|pass skipped" "$LOGS/launch-3.log" | head -3 >&2; fail "launch 3: the 0.6 line did not stop"; }
total3="$(sed -E 's/.*conversations [0-9]+\/([0-9]+).*/\1/' <<< "$p3")"
echo "   launch 3 stopped the 0.6 line ($total3 conversation(s) on 0.6)"

launch 4 "$(pwd)" e2e/specs/alone/alone.e2e.ts ALONE_PHASE=alone ALONE_MIN_CONVERSATIONS="$total3"
[ "$(lines_said 4)" = "0.7 leads" ] || fail "launch 4: started '$(lines_said 4)', not on 0.7 alone"
grep -oE "e2e: (at the start|after the new chat|after the import): [0-9]+ conversation\(s\) listed, [0-9]+ expected|e2e: search: .*|e2e: import adopted .*" "$LOGS/launch-4.out" | sed 's/^/   launch 4: /'
backed="$(grep -o 'e2e: backup {.*' "$LOGS/launch-4.out" | grep -oE '"conversations":[0-9]+' | head -1 | cut -d: -f2 || true)"
[ -n "$backed" ] || fail "launch 4: no backup outcome"
! grep -qE '\]\[(WARN|ERROR)\] \[0\.7\]' "$LOGS/launch-4.log" || { grep -E '\]\[(WARN|ERROR)\] \[0\.7\]' "$LOGS/launch-4.log" | head -5 >&2; fail "launch 4: warnings on the 0.7 line"; }
echo "   launch 4: the backup holds $backed conversation(s)"

# A reinstall under the same Vault: the app's data goes, its link stays.
rm -rf "$DATA"
seen=0
link
launch 5 "$(pwd)" e2e/specs/alone/alone.e2e.ts ALONE_PHASE=restore ALONE_MIN_CONVERSATIONS="$backed"
[ "$(lines_said 5)" = "0.7 leads" ] || fail "launch 5: a reinstall on an identity that has moved started '$(lines_said 5)', not on 0.7 alone"
grep -q "before the first start: 0 other device(s) read from the Vault" "$LOGS/launch-5.log" || grep -m2 "before the first start" "$LOGS/launch-5.log" >&2
grep -oE "e2e: after the restore: [0-9]+ conversation\(s\) listed, [0-9]+ expected" "$LOGS/launch-5.out" | sed 's/^/   launch 5: /'
e2e/no-fork-check.sh "$H" || { echo "FAIL: a conductor reported a fork or a moved chain head (above)" >&2; exit 1; }
echo "== one computer with a Vault, the update, the stop and 0.7 alone: passed (logs kept at $LOGS)"
