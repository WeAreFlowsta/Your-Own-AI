#!/usr/bin/env bash
# The island merge, live, against a TEST Vault (see specs/island/island.e2e.ts).
# Needs: a test Vault on $YOAI_VAULT_PORT (default 27778) holding an unlocked
# identity (flowsta-vault/scripts/run-test-instance.sh + /dev/setup-identity),
# the e2e build, and the machine's models. Never touches the installed Vault.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE/.."
export YOAI_VAULT_PORT="${YOAI_VAULT_PORT:-27778}"
ROOT="${ISLAND_ROOT:-/tmp/yoai-island-$$}"
A="$ROOT/A"; B="$ROOT/B"
if [ -z "${SKIP_A:-}" ]; then rm -rf "$A"; fi
rm -rf "$B"; mkdir -p "$A" "$B"
CID="$(grep -A1 'YOAI_HOLOCHAIN_CLIENT_ID: &str' src-tauri/src/flowsta.rs | grep -o '"flowsta_app_[^"]*"' | tr -d '"' | head -1)"
# The app's client id is not registered on the test Vault's API, so the
# Vault would refuse /link-identity (app_not_found). A test Vault instead
# takes the link through its dev `connect` op, and the profile starts
# with link_done and the same link key - what the app checks on launch.
prelink() {  # <home dir> <label>
  local home="$1" label="$2"
  local key; key="u$(python3 -c 'import os,base64; print(base64.urlsafe_b64encode(bytes([0x84,0x20,0x24])+os.urandom(36)).decode().rstrip("="))')"
  # With an unlocked Vault present the app opens the identity's profile
  # folder (sha256 over the 39-byte key, first 16 hex) - pre-link there,
  # and in `local` for a launch that finds no Vault.
  local vkey folder; vkey="$(curl -s -m 3 "http://127.0.0.1:$YOAI_VAULT_PORT/status" | grep -o '"agent_pub_key":"[^"]*"' | cut -d'"' -f4)"
  folder="$(python3 -c 'import sys,base64,hashlib; k=sys.argv[1][1:]; raw=base64.urlsafe_b64decode(k+"="*(-len(k)%4)); print(hashlib.sha256(raw).hexdigest()[:16])' "$vkey")"
  local prof
  for prof in "$home/.local/share/com.solar.yourowai/profiles/$folder" "$home/.local/share/com.solar.yourowai/profiles/local"; do
    mkdir -p "$prof"
    printf '{"link_done":true,"app_link_key":"%s"}\n' "$key" > "$prof/flowsta-auth.json"
  done
  curl -s -m 10 -X POST "http://127.0.0.1:$YOAI_VAULT_PORT/dev/devices" -H 'content-type: application/json' -H 'origin: https://ourtest.flowsta.com' \
    -d "{\"op\":\"connect\",\"client_id\":\"$CID\",\"app_name\":\"Your Own AI ($label)\",\"origin\":\"yoai://app\",\"app_agent_pub_key\":\"$key\"}" >/dev/null
  echo "prelinked profile $label"
}
if [ -z "${SKIP_A:-}" ]; then
  prelink "$A" A
  echo "== phase a: profile A escrows the identity's material"
  YOAI_E2E_HOME="$A" ISLAND_PHASE=a node e2e/run.mjs models --spec e2e/specs/island/island.e2e.ts
fi
echo "== phase b0: profile B chats before it is linked"
YOAI_E2E_HOME="$B" ISLAND_PHASE=b0 node e2e/run.mjs models --spec e2e/specs/island/island.e2e.ts
prelink "$B" B
echo "== phase b1: profile B, linked now - the merge runs by itself and the app exits"
YOAI_E2E_HOME="$B" ISLAND_PHASE=b1 node e2e/run.mjs models --spec e2e/specs/island/island.e2e.ts || true
echo "== marker in B:"; ls "$B"/.local/share/com.solar.yourowai/profiles/*/island-* 2>/dev/null || echo "(no island files found)"
echo "== phase b2: profile B comes back"
YOAI_E2E_HOME="$B" ISLAND_PHASE=b2 node e2e/run.mjs models --spec e2e/specs/island/island.e2e.ts
echo "profiles kept at $ROOT"
