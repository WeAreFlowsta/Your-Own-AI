#!/usr/bin/env bash
# The Vault on an identity Your Own AI was never connected under, live
# (specs/locked/another-identity.e2e.ts): this build linked to a fresh test
# Vault as identity A; then the Vault is locked, a second identity B is made
# in it (so it holds B, unlocked), and the app starts again. Reports which
# profile each start opens and what the page says, and checks that A's
# profile is left as it was.
# Needs what locked-live.sh needs. Logs and the home in ANOTHER_LOGS
# (default ~/.cache/yoai-another-live).
set -euo pipefail
PW="Another-$(date +%s)!x"
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE/.."
VAULT="$(cd "$HERE/../../flowsta-vault" && pwd)"
LOGS="${ANOTHER_LOGS:-$HOME/.cache/yoai-another-live}"; rm -rf "$LOGS"; mkdir -p "$LOGS"
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
(cd "$VAULT" && ${ANOTHER_VAULT_WRAP:-} dbus-run-session -- scripts/run-test-instance.sh md-e > "$LOGS/md-e.log" 2>&1 &)
P1=""; for _ in $(seq 1 36); do P1="$(answering | head -1 || true)"; [ -n "$P1" ] && break; sleep 5; done
[ -n "$P1" ] || fail "the test Vault never answered"
created="$(curl -s -m 120 -X POST "http://127.0.0.1:$P1/dev/setup-identity" -H 'content-type: application/json' \
  -d "{\"api_url\":\"$API\",\"password\":\"$PW\",\"email\":\"yoai-another-a-$(date +%s)@example.com\",\"display_name\":\"Locked Test\"}")"
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
dev "$P1" "{\"op\":\"connect\",\"client_id\":\"$CID\",\"app_name\":\"Your Own AI (another)\",\"origin\":\"yoai://app\",\"app_agent_pub_key\":\"$key\"}" >/dev/null

run() { # phase name
  ANOTHER_PHASE="$1" YOAI_E2E_HOME="$H" YOAI_VAULT_PORT="$P1" node e2e/run.mjs models --spec e2e/specs/locked/another-identity.e2e.ts > "$LOGS/launch-$1.out" 2>&1 || { tail -20 "$LOGS/launch-$1.out"; fail "the $1 phase's spec failed"; }
  grep -oE "e2e: .*" "$LOGS/launch-$1.out" | cut -c1-400 | sed 's/^/   /'
}
echo "== phase a: the Vault holds A, the identity this install is linked under"
run a
ROOT_A="$(grep -o 'e2e: profile [^ ]*' "$LOGS/launch-a.out" | tail -1 | awk '{print $3}')"
[ -n "$ROOT_A" ] || fail "phase a did not say which profile was open"
before="$(ls "$DATA/profiles/$ROOT_A" | sort | tr '\n' ' ')"

curl -s -m 30 -X POST "http://127.0.0.1:$P1/dev/lock" >/dev/null; sleep 5
made="$(curl -s -m 300 -X POST "http://127.0.0.1:$P1/dev/setup-identity" -H 'content-type: application/json' \
  -d "{\"api_url\":\"$API\",\"password\":\"$PW\",\"email\":\"yoai-another-b-$(date +%s)@example.com\",\"display_name\":\"Another B\"}")"
printf '%s' "$made" | grep -q '"agent_pub_key"' || fail "the second identity was not made: $(printf '%s' "$made" | cut -c1-200)"
sleep 20
echo "== the Vault now holds B; to this app's origin it says: $(curl -s -m 5 -H 'origin: yoai://app' "http://127.0.0.1:$P1/status" | python3 -c 'import sys,json; d=json.load(sys.stdin); print("unlocked", d.get("unlocked"), "identity", "named" if d.get("agent_pub_key") else "not named")'); with no origin: $(curl -s -m 5 "http://127.0.0.1:$P1/status" | python3 -c 'import sys,json; d=json.load(sys.stdin); print("named" if d.get("agent_pub_key") else "not named")')"
echo "== phase b: the Vault holds B, which this install was never connected under"
run b
ROOT_B="$(grep -o 'e2e: profile [^ ]*' "$LOGS/launch-b.out" | tail -1 | awk '{print $3}')"
after="$(ls "$DATA/profiles/$ROOT_A" | sort | tr '\n' ' ')"
echo "== profiles: with A '$ROOT_A', with B '$ROOT_B'; on disk: $(ls "$DATA/profiles" | tr '\n' ' ')"
[ "$before" = "$after" ] || fail "A's profile changed while the Vault held B (before: $before; after: $after)"
[ -f "$DATA/profiles/$ROOT_A/flowsta-auth.json" ] || fail "A's link file is gone"
if [ "$ROOT_A" = "$ROOT_B" ]; then echo "== RESULT: the app opened A's profile while the Vault held B (see what the page said above)"; else echo "== RESULT: the app opened another profile for the identity the Vault holds; A's is as it was"; fi
echo "== logs kept at $LOGS"
