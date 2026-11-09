#!/usr/bin/env bash
# A new computer replaces one that is gone, live (specs/devices/replace.e2e.ts).
# One identity, two test Vaults, this build on both computers:
#   a  A chats and backs up; then it is never started again.
#   b  B joins with the recovery phrase on its own Vault: its restore is
#      refused while A is listed and names A; B stops waiting for A, and the
#      restore brings A's conversations into B's own records.
# Needs what devices-live.sh needs. REPLACE_VAULT_WRAP: a command the test
# Vaults start under (their own memory scope). Logs and profiles in
# REPLACE_LOGS (default ~/.cache/yoai-replace-live).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE/.."
THIS="$(pwd)"
VAULT="$(cd "$HERE/../../flowsta-vault" && pwd)"
LOGS="${REPLACE_LOGS:-$HOME/.cache/yoai-replace-live}"; rm -rf "$LOGS"; mkdir -p "$LOGS"
A="$LOGS/profiles/A"; B="$LOGS/profiles/B"; mkdir -p "$A" "$B"
API="${FLOWSTA_API_URL:-https://auth-api-staging.flowsta.com}"
CID="$(grep -A1 'YOAI_HOLOCHAIN_CLIENT_ID: &str' src-tauri/src/flowsta.rs | grep -o '"flowsta_app_[^"]*"' | tr -d '"' | head -1)"
export DEVICES_NAME="${DEVICES_NAME:-Synced $(date +%H%M)}"
fail() { echo "FAIL: $*" >&2; exit 1; }

killtests() { for pid in $(pgrep -x flowsta-vault); do case "$(readlink /proc/$pid/exe 2>/dev/null)" in *target/debug*) kill "$pid" 2>/dev/null || true;; esac; done; }
killapps() { for pid in $(pgrep -x 'app|your-own-ai'); do case "$(readlink /proc/$pid/exe 2>/dev/null)" in "$THIS"/src-tauri/target/debug/*) kill "$pid" 2>/dev/null || true;; esac; done; }
gone() { for _ in $(seq 1 30); do [ -z "$(pgrep -x 'app|your-own-ai')" ] && return 0; sleep 2; done; return 1; }
answering() { for p in 27777 27778 27779; do curl -s -m 2 "http://127.0.0.1:$p/dev/status" | grep -q '"harness":true' && echo "$p"; done; return 0; }
waitnew() { for _ in $(seq 1 36); do for p in $(answering); do [ "$p" != "${1:-}" ] && { echo "$p"; return 0; }; done; sleep 5; done; return 1; }
dev() { curl -s -m 60 -X POST "http://127.0.0.1:$1/dev/devices" -H 'content-type: application/json' -H 'origin: https://ourtest.flowsta.com' -d "$2"; }
trap 'killapps; killtests; sleep 2; killtests' EXIT

[ -z "$(pgrep -x 'app|your-own-ai')" ] || fail "an app instance is up"
[ -z "$(answering)" ] || fail "a test Vault is already answering - stop it first"
[ -x "$VAULT/src-tauri/target/debug/flowsta-vault" ] || fail "no Vault test binary (flowsta-vault/scripts/build-test-binary.sh)"
rm -rf "$HOME/.local/share/flowsta-vault-md-e" "$HOME/.local/share/flowsta-vault-md-f"
(cd "$VAULT" && ${REPLACE_VAULT_WRAP:-} dbus-run-session -- scripts/run-test-instance.sh md-e > "$LOGS/md-e.log" 2>&1 &)
P1="$(waitnew)" || fail "md-e never answered"
(cd "$VAULT" && ${REPLACE_VAULT_WRAP:-} dbus-run-session -- scripts/run-test-instance.sh md-f > "$LOGS/md-f.log" 2>&1 &)
P2="$(waitnew "$P1")" || fail "md-f never answered"
echo "== test Vaults: V1=$P1 V2=$P2"
PW="Replace-$(date +%s)!x"
created="$(curl -s -m 120 -X POST "http://127.0.0.1:$P1/dev/setup-identity" -H 'content-type: application/json' \
  -d "{\"api_url\":\"$API\",\"password\":\"$PW\",\"email\":\"yoai-replace-$(date +%s)@example.com\",\"display_name\":\"Replace Test\"}")"
PHRASE="$(printf '%s' "$created" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("phrase",""))')"
[ -n "$PHRASE" ] || fail "identity not created: $created"
for _ in $(seq 1 60); do dev "$P1" '{"op":"list"}' | grep -q '"devices"' && break; sleep 4; done
door="$(dev "$P2" "{\"op\":\"phrase-door\",\"phrase\":\"$PHRASE\",\"password\":\"$PW\"}")"
printf '%s' "$door" | grep -q '"agent_pub_key"' || fail "V2 did not join: $door"
echo "== V2 joined with the phrase"

prelink() {  # <home> <port> <label>
  local home="$1" port="$2" label="$3"
  local key; key="u$(python3 -c 'import os,base64; print(base64.urlsafe_b64encode(bytes([0x84,0x20,0x24])+os.urandom(36)).decode().rstrip("="))')"
  local vkey folder; vkey="$(curl -s -m 3 "http://127.0.0.1:$port/status" | grep -o '"agent_pub_key":"[^"]*"' | cut -d'"' -f4 || true)"
  [ -n "$vkey" ] || fail "Vault $port reports no identity key"
  folder="$(python3 -c 'import sys,base64,hashlib; k=sys.argv[1][1:]; raw=base64.urlsafe_b64decode(k+"="*(-len(k)%4)); print(hashlib.sha256(raw).hexdigest()[:16])' "$vkey")"
  local prof
  for prof in "$home/.local/share/com.solar.yourowai/profiles/$folder" "$home/.local/share/com.solar.yourowai/profiles/local"; do
    mkdir -p "$prof"; printf '{"link_done":true,"app_link_key":"%s"}\n' "$key" > "$prof/flowsta-auth.json"
  done
  dev "$port" "{\"op\":\"connect\",\"client_id\":\"$CID\",\"app_name\":\"Your Own AI ($label)\",\"origin\":\"yoai://app\",\"app_agent_pub_key\":\"$key\"}" >/dev/null
}
prelink "$A" "$P1" A
prelink "$B" "$P2" B

run() {  # <label> <tree> <home> <vault port> <spec> [env...]
  local label="$1" tree="$2" home="$3" port="$4" spec="$5"; shift 5
  echo "== $label ($(basename "$tree"))"
  (cd "$tree" && env "$@" YOAI_E2E_HOME="$home" YOAI_VAULT_PORT="$port" node e2e/run.mjs models --spec "$spec") > "$LOGS/$label.out" 2>&1 \
    || { grep -vE "window states|get_window_states" "$LOGS/$label.out" | tail -25; fail "$label: the spec failed"; }
  grep -oE "e2e: [^\[]{0,200}" "$LOGS/$label.out" | head -6 | sed 's/^/   /'
}
# A's whole log, ANSI stripped, in a file: grep -q on a pipe ends the reader
# early, and under pipefail that reads as a failed check.
alog() { sed -E 's/\x1b\[[0-9;]*m//g' "$A"/.local/share/com.solar.yourowai/logs/*.log > "$LOGS/a-all.log"; cat "$LOGS/a-all.log"; }
ahas() { alog > /dev/null; grep -qE "$1" "$LOGS/a-all.log"; }
vault_holds_newest() {  # <home of the device> <port of the Vault that must hold its last manifest>
  local home="$1" port="$2" last mark newest held
  last="$(sed -E 's/\x1b\[[0-9;]*m//g' "$home"/.local/share/com.solar.yourowai/logs/*.log 2>/dev/null | grep "manifest written" | tail -1 | sed -E 's/^\[([0-9-]+)\]\[([0-9:]+)\].*/\1 \2/' || true)"
  [ -n "$last" ] || fail "no manifest written in $home's log"
  # The log line is stamped as the write finishes; the Vault's created_at
  # can be a second earlier. Five seconds of margin (the copy before it is
  # minutes older).
  mark="$(( $(date -u -d "$last" +%s) - 5 ))"
  for _ in $(seq 1 150); do
    dev "$P2" '{"op":"round"}' >/dev/null || true
    dev "$P1" '{"op":"round"}' >/dev/null || true
    held="$(dev "$port" "{\"op\":\"backups\",\"client_id\":\"$CID\"}" || true)"
    newest="$(printf '%s' "$held" | python3 -c 'import sys,json; h=[x for x in json.load(sys.stdin).get("held",[]) if x.get("label")=="manifest" and x.get("from")]; print(max([x.get("created_at",0) for x in h] or [0]))' 2>/dev/null || echo 0)"
    [ "${newest:-0}" -ge "$mark" ] && return 0
    sleep 4
  done
  fail "Vault $port never received the manifest of $last"
}

run a "$THIS" "$A" "$P1" e2e/specs/devices/replace.e2e.ts REPLACE_PHASE=a
vault_holds_newest "$A" "$P2"
# The conversation objects follow the manifest in the same rounds: a few more.
for _ in $(seq 1 5); do dev "$P2" '{"op":"round"}' >/dev/null || true; sleep 4; done
echo "   B's Vault holds A's backup; A is not started again"
run b "$THIS" "$B" "$P2" e2e/specs/devices/replace.e2e.ts REPLACE_PHASE=b
sed -E 's/\x1b\[[0-9;]*m//g' "$B"/.local/share/com.solar.yourowai/logs/*.log > "$LOGS/b-all.log"
grep -E "before the first start|\[lines\]|restoring the one held for|\[restore\] [0-9]+ conversation" "$LOGS/b-all.log" | head -6 | sed 's/^/   /' || true
echo "== a new computer replaced one that is gone: passed (logs kept at $LOGS)"
