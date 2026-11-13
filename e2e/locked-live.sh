#!/usr/bin/env bash
# "Restart to update" with the Vault locked, live (specs/locked): this build
# linked to a fresh test Vault; the Vault is locked (still syncing), then
# locked with syncing stopped, and the update's backup step runs each time.
# Needs: the Vault's debug TEST binary (flowsta-vault/scripts/build-test-binary.sh),
# this app's e2e build, the machine's models (YOAI_E2E_MODELS_DIR for a
# subset), the installed Vault CLOSED, no app up.
# LOCKED_SPEC: another spec to run in this setup (backup-big-live.sh).
# LOCKED_VAULT_WRAP: a command the test Vault starts under. Logs and the
# home are kept in LOCKED_LOGS (default ~/.cache/yoai-locked-live).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE/.."
VAULT="$(cd "$HERE/../../flowsta-vault" && pwd)"
LOGS="${LOCKED_LOGS:-$HOME/.cache/yoai-locked-live}"; rm -rf "$LOGS"; mkdir -p "$LOGS"
H="$LOGS/home"; mkdir -p "$H"
DATA="$H/.local/share/com.solar.yourowai"
API="${FLOWSTA_API_URL:-https://auth-api-staging.flowsta.com}"
CID="$(grep -A1 'YOAI_HOLOCHAIN_CLIENT_ID: &str' src-tauri/src/flowsta.rs | grep -o '"flowsta_app_[^"]*"' | tr -d '"' | head -1)"
fail() { echo "FAIL: $*" >&2; exit 1; }

killtests() { for pid in $(pgrep -x flowsta-vault); do case "$(readlink /proc/$pid/exe 2>/dev/null)" in *target/debug*) kill "$pid" 2>/dev/null || true;; esac; done; }
answering() { for p in 27777 27778 27779; do curl -s -m 2 "http://127.0.0.1:$p/dev/status" | grep -q '"harness":true' && echo "$p"; done; }
dev() { curl -s -m 60 -X POST "http://127.0.0.1:$1/dev/devices" -H 'content-type: application/json' -H 'origin: https://ourtest.flowsta.com' -d "$2"; }
trap 'killtests; sleep 2; killtests' EXIT

[ -z "$(pgrep -x 'app|your-own-ai')" ] || fail "an app instance is up"
[ -z "$(answering)" ] || fail "a test Vault is already answering - stop it first"
[ -x "$VAULT/src-tauri/target/debug/flowsta-vault" ] || fail "no Vault test binary (flowsta-vault/scripts/build-test-binary.sh)"
rm -rf "$HOME/.local/share/flowsta-vault-md-e"
(cd "$VAULT" && ${LOCKED_VAULT_WRAP:-} dbus-run-session -- scripts/run-test-instance.sh md-e > "$LOGS/md-e.log" 2>&1 &)
P1=""; for _ in $(seq 1 36); do P1="$(answering | head -1 || true)"; [ -n "$P1" ] && break; sleep 5; done
[ -n "$P1" ] || fail "the test Vault never answered"
created="$(curl -s -m 120 -X POST "http://127.0.0.1:$P1/dev/setup-identity" -H 'content-type: application/json' \
  -d "{\"api_url\":\"$API\",\"password\":\"Locked-$(date +%s)!x\",\"email\":\"yoai-locked-$(date +%s)@example.com\",\"display_name\":\"Locked Test\"}")"
printf '%s' "$created" | grep -q '"phrase"' || fail "identity not created: $created"
for _ in $(seq 1 60); do dev "$P1" '{"op":"list"}' | grep -q '"devices"' && break; sleep 4; done
echo "== the test Vault ($P1) holds a new identity"

# The profile starts linked (the app's client id is not registered on the
# staging API), as devices-live.sh does.
key="u$(python3 -c 'import os,base64; print(base64.urlsafe_b64encode(bytes([0x84,0x20,0x24])+os.urandom(36)).decode().rstrip("="))')"
vkey="$(curl -s -m 3 "http://127.0.0.1:$P1/status" | grep -o '"agent_pub_key":"[^"]*"' | cut -d'"' -f4 || true)"
[ -n "$vkey" ] || fail "the test Vault reports no identity key"
folder="$(python3 -c 'import sys,base64,hashlib; k=sys.argv[1][1:]; raw=base64.urlsafe_b64decode(k+"="*(-len(k)%4)); print(hashlib.sha256(raw).hexdigest()[:16])' "$vkey")"
for prof in "$DATA/profiles/$folder" "$DATA/profiles/local"; do
  mkdir -p "$prof"; printf '{"link_done":true,"app_link_key":"%s"}\n' "$key" > "$prof/flowsta-auth.json"
done
dev "$P1" "{\"op\":\"connect\",\"client_id\":\"$CID\",\"app_name\":\"Your Own AI (locked)\",\"origin\":\"yoai://app\",\"app_agent_pub_key\":\"$key\"}" >/dev/null

YOAI_TEST_UPDATE_BACKUP_ONLY=1 YOAI_TEST_BACKUP_HOLD_MS=4000 YOAI_E2E_LONG=1 YOAI_E2E_HOME="$H" YOAI_VAULT_PORT="$P1" node e2e/run.mjs models --spec "${LOCKED_SPEC:-e2e/specs/locked/locked.e2e.ts}" > "$LOGS/launch.out" 2>&1 || { grep -vE "window states|get_window_states" "$LOGS/launch.out" | tail -40; fail "the spec failed"; }
sed -E 's/\x1b\[[0-9;]*m//g' "$DATA"/logs/*.log > "$LOGS/app.log"
grep -oE "e2e: .*" "$LOGS/launch.out" | cut -c1-300 | sed 's/^/   /'
# Another spec against the same linked profile and test Vault (LOCKED_SPEC):
# its own script reads the log.
[ -z "${LOCKED_SPEC:-}" ] || { echo "== ${LOCKED_SPEC}: the spec passed (logs kept at $LOGS)"; exit 0; }
grep -E "\[update\]" "$LOGS/app.log" | sed -E 's/^\[[0-9-]+\]\[([0-9:]+)\]\[[^]]*\]\[[A-Z]+\] /   \1 /'
grep -q '"conversations":[1-9]' <<< "$(grep -o 'e2e: locked backup {.*' "$LOGS/launch.out")" || fail "the backup made while the Vault was locked holds no conversation"
[ "$(grep -c "\[update\] a Vault backup is running: the install waits for it" "$LOGS/app.log")" -ge 1 ] || fail "the update never waited for a backup that was already running"
grep -q "\[update\] the Vault backup is current" "$LOGS/app.log" || fail "a backup that had just finished was repeated"
grep -q "\[update\] not installed yet: no Vault backup could be made (vault_locked)" "$LOGS/app.log" || fail "the stopped Vault did not hold the update"
grep -q "\[update\] no Vault backup before the install (vault_locked): the person said to go on without one" "$LOGS/app.log" || fail "\"Update without a backup\" did not go on without one"
echo "== Restart to update with the Vault locked: passed (logs kept at $LOGS)"
