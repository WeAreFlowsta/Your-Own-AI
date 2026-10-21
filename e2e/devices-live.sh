#!/usr/bin/env bash
# One identity on two devices, live (see specs/devices/devices.e2e.ts).
# Needs: the Vault's debug TEST binary (flowsta-vault/scripts/build-test-binary.sh),
# the e2e build of this app, the machine's models, the installed Vault
# CLOSED (a test instance exits at once otherwise), and no app instance up.
# Starts two FRESH test Vaults (md-e, md-f), creates an identity on the
# first, adds the second with the recovery phrase, then runs phase a on the
# first and phase b on the second. Never touches the installed Vault.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE/.."
VAULT="$(cd "$HERE/../../flowsta-vault" && pwd)"
LOGS="${DEVICES_LOGS:-/tmp/yoai-devices-$$}"; mkdir -p "$LOGS"
ROOT="${DEVICES_ROOT:-$LOGS/profiles}"; A="$ROOT/A"; B="$ROOT/B"; rm -rf "$A" "$B"; mkdir -p "$A" "$B"
API="${FLOWSTA_API_URL:-https://auth-api-staging.flowsta.com}"
CID="$(grep -A1 'YOAI_HOLOCHAIN_CLIENT_ID: &str' src-tauri/src/flowsta.rs | grep -o '"flowsta_app_[^"]*"' | tr -d '"' | head -1)"
export DEVICES_NAME="${DEVICES_NAME:-Synced $(date +%H%M)}"

killtests() { for pid in $(pgrep -x flowsta-vault); do case "$(readlink /proc/$pid/exe 2>/dev/null)" in *target/debug*) kill "$pid" 2>/dev/null || true;; esac; done; }
answering() { for p in 27777 27778 27779; do curl -s -m 2 "http://127.0.0.1:$p/dev/status" | grep -q '"harness":true' && echo "$p"; done; }
waitnew() { for _ in $(seq 1 36); do for p in $(answering); do [ "$p" != "${1:-}" ] && { echo "$p"; return 0; }; done; sleep 5; done; return 1; }
dev() { curl -s -m 60 -X POST "http://127.0.0.1:$1/dev/devices" -H 'content-type: application/json' -H 'origin: https://ourtest.flowsta.com' -d "$2"; }
trap 'killtests; sleep 2; killtests' EXIT

[ -z "$(answering)" ] || { echo "a test Vault is already answering - stop it first"; exit 1; }
# The Vault's debug test binary (a target clean removes it): build it when missing.
[ -x "$VAULT/src-tauri/target/debug/flowsta-vault" ] || { echo "== building the Vault test binary"; (cd "$VAULT" && scripts/build-test-binary.sh > "$LOGS/vault-test-build.log" 2>&1) || { echo "Vault test build failed"; tail -20 "$LOGS/vault-test-build.log"; exit 1; }; }
rm -rf "$HOME/.local/share/flowsta-vault-md-e" "$HOME/.local/share/flowsta-vault-md-f"
(cd "$VAULT" && dbus-run-session -- scripts/run-test-instance.sh md-e > "$LOGS/md-e.log" 2>&1 &)
P1="$(waitnew)" || { echo "md-e never answered"; exit 1; }
(cd "$VAULT" && dbus-run-session -- scripts/run-test-instance.sh md-f > "$LOGS/md-f.log" 2>&1 &)
P2="$(waitnew "$P1")" || { echo "md-f never answered"; exit 1; }
echo "== test Vaults: V1=$P1 V2=$P2 (logs $LOGS)"

PW="Devices-$(date +%s)!x"
created="$(curl -s -m 120 -X POST "http://127.0.0.1:$P1/dev/setup-identity" -H 'content-type: application/json' \
  -d "{\"api_url\":\"$API\",\"password\":\"$PW\",\"email\":\"yoai-devices-$(date +%s)@example.com\",\"display_name\":\"Devices Test\"}")"
PHRASE="$(printf '%s' "$created" | python3 -c 'import sys,json; print(json.load(sys.stdin).get("phrase",""))')"
[ -n "$PHRASE" ] || { echo "identity not created: $created"; exit 1; }
echo "== V1 holds the identity"
for _ in $(seq 1 60); do dev "$P1" '{"op":"list"}' | grep -q '"devices"' && break; sleep 4; done
door="$(dev "$P2" "{\"op\":\"phrase-door\",\"phrase\":\"$PHRASE\",\"password\":\"$PW\"}")"
printf '%s' "$door" | grep -q '"agent_pub_key"' || { echo "V2 did not join: $door"; exit 1; }
echo "== V2 joined with the phrase"
n=0
for _ in $(seq 1 60); do n="$(dev "$P2" '{"op":"list"}' | grep -c '"install_id"' || true)"; [ "${n:-0}" -ge 2 ] && break; sleep 4; done
echo "== V2 sees $n devices"

# The app's client id is not registered on the staging API, so each test
# Vault takes the app's link through the dev `connect` op and the profile
# starts linked (what the app checks on launch) - as island-live.sh does.
prelink() {  # <home> <port> <label>
  local home="$1" port="$2" label="$3"
  local key; key="u$(python3 -c 'import os,base64; print(base64.urlsafe_b64encode(bytes([0x84,0x20,0x24])+os.urandom(36)).decode().rstrip("="))')"
  local vkey folder; vkey="$(curl -s -m 3 "http://127.0.0.1:$port/status" | grep -o '"agent_pub_key":"[^"]*"' | cut -d'"' -f4)"
  folder="$(python3 -c 'import sys,base64,hashlib; k=sys.argv[1][1:]; raw=base64.urlsafe_b64decode(k+"="*(-len(k)%4)); print(hashlib.sha256(raw).hexdigest()[:16])' "$vkey")"
  local prof
  for prof in "$home/.local/share/com.solar.yourowai/profiles/$folder" "$home/.local/share/com.solar.yourowai/profiles/local"; do
    mkdir -p "$prof"; printf '{"link_done":true,"app_link_key":"%s"}\n' "$key" > "$prof/flowsta-auth.json"
  done
  dev "$port" "{\"op\":\"connect\",\"client_id\":\"$CID\",\"app_name\":\"Your Own AI ($label)\",\"origin\":\"yoai://app\",\"app_agent_pub_key\":\"$key\"}" >/dev/null
  echo "prelinked $label on $port"
}
prelink "$A" "$P1" A
prelink "$B" "$P2" B

A_START="$(date +%s)"
echo "== phase a: device A renames an AI to '$DEVICES_NAME' and backs up (Vault $P1)"
YOAI_E2E_HOME="$A" YOAI_VAULT_PORT="$P1" DEVICES_PHASE=a node e2e/run.mjs models --spec e2e/specs/devices/devices.e2e.ts
echo "== V2 fetches V1's copies"
for _ in $(seq 1 150); do
  dev "$P2" '{"op":"round"}' >/dev/null
  held="$(dev "$P2" "{\"op\":\"backups\",\"client_id\":\"$CID\"}")"
  # The copy must be the manifest A wrote AFTER its chat, not the one from its first start.
  newest="$(printf '%s' "$held" | python3 -c 'import sys,json; h=[x for x in json.load(sys.stdin).get("held",[]) if x.get("label")=="manifest" and x.get("from")]; print(max([x.get("created_at",0) for x in h] or [0]))' 2>/dev/null || echo 0)"
  [ "${newest:-0}" -ge "$A_START" ] && break
  sleep 4
done
[ "${newest:-0}" -ge "$A_START" ] || { echo "V2 never received A's new manifest (newest copy $newest < $A_START): $held"; exit 1; }
echo "== V2 holds A's manifest"
echo "== phase b: device B signs in fresh on Vault $P2"
YOAI_E2E_HOME="$B" YOAI_VAULT_PORT="$P2" DEVICES_PHASE=b node e2e/run.mjs models --spec e2e/specs/devices/devices.e2e.ts
echo "== phase c: both devices online - A unattended on the default ports, B driven on shifted ports (Vaults $P1 / $P2)"
# A runs unattended on the rendezvous with its WebDriver on another port, so B's test driver (4445) is B's.
# A holds the RECORDS, not a chat: it gets an empty models folder, so it never loads a model - two
# instances each with a model exceeded 16 GB and the kernel killed the editor (2026-10-13).
mkdir -p "$LOGS/no-models"
YOAI_E2E_HOME="$A" YOAI_VAULT_PORT="$P1" YOAI_E2E_WITH_MODELS=1 YOAI_E2E_MODELS_DIR="$LOGS/no-models" YOAI_E2E_WITH_RENDEZVOUS=1 TAURI_WEBDRIVER_PORT=4446 e2e/launch-app.sh > "$LOGS/device-a-phase-c.log" 2>&1 &
A_PID=$!
killA() { for pid in $(pgrep -x app); do case "$(readlink /proc/$pid/exe 2>/dev/null)" in *"Your Own AI App"*target/debug*) kill "$pid" 2>/dev/null || true;; esac; done; }
trap 'killA; killtests; sleep 2; killtests' EXIT
for _ in $(seq 1 60); do curl -s -m 2 http://127.0.0.1:11435/health | grep -q '"ok"' && break; sleep 2; done
curl -s -m 2 http://127.0.0.1:11435/health | grep -q '"ok"' || { echo "device A never came up for phase c"; exit 1; }
echo "== device A is up (pid $A_PID); driving B"
YOAI_E2E_HOME="$B" YOAI_VAULT_PORT="$P2" YOAI_PORT_OFFSET=100 YOAI_E2E_WITH_RENDEZVOUS=1 DEVICES_PHASE=c node e2e/run.mjs models --spec e2e/specs/devices/devices.e2e.ts
killA
echo "== all three phases passed; profiles kept at $ROOT"
