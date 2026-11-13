#!/usr/bin/env bash
# One identity on two devices, live (see specs/devices/devices.e2e.ts).
# Needs: the Vault's debug TEST binary (flowsta-vault/scripts/build-test-binary.sh),
# the e2e build of this app, the machine's models, the installed Vault
# CLOSED (a test instance exits at once otherwise), and no app instance up.
# Starts two FRESH test Vaults (md-e, md-f), creates an identity on the
# first, adds the second with the recovery phrase, then runs phase a on the
# first and phase b on the second. Never touches the installed Vault.
# DEVICES_VAULT_WRAP: a command the two test Vaults start under (e.g. their
# own memory scope: `systemd-run --user --scope -q -p MemoryMax=4G`), so a
# Vault conductor that grows fails the run, not the desktop.
# DEVICES_TWINS=1: after the four phases, the doubled-starters phases e and f.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE/.."
VAULT="$(cd "$HERE/../../flowsta-vault" && pwd)"
LOGS="${DEVICES_LOGS:-/tmp/yoai-devices-$$}"; mkdir -p "$LOGS"
ROOT="${DEVICES_ROOT:-$LOGS/profiles}"; A="$ROOT/A"; B="$ROOT/B"; rm -rf "$A" "$B"; mkdir -p "$A" "$B"
API="${FLOWSTA_API_URL:-https://auth-api-staging.flowsta.com}"
CID="$(grep -A1 'YOAI_HOLOCHAIN_CLIENT_ID: &str' src-tauri/src/flowsta.rs | grep -o '"flowsta_app_[^"]*"' | tr -d '"' | head -1)"
export DEVICES_NAME="${DEVICES_NAME:-Synced $(date +%H%M)}"
# DEVICES_B_TREE: the app tree that drives device B (default: this one). A
# worktree of an earlier release, built there with `npm run e2e:build`, makes
# this a mixed-version run: A on this build, B on that release (a computer
# on the earlier release added later). DEVICES_A_TREE below is the other way.
B_TREE="$(cd "${DEVICES_B_TREE:-$HERE/..}" && pwd)"
if [ "$B_TREE" != "$(pwd)" ]; then
  echo "== device B runs from $B_TREE"
  # B drives ITS release's app with THIS spec (an earlier release's copy has
  # no phase d and does not continue the conversation in phase c); the spec
  # is told B is the earlier release, whose drawer does not re-read while
  # open, so there phase c reopens it on each look as the person would.
  cp "$HERE/specs/devices/devices.e2e.ts" "$B_TREE/e2e/specs/devices/devices.e2e.ts"
  export DEVICES_B_EARLIER=1
fi
# The staging test material for B's rendezvous launches, read from this
# tree's Vault: B's launcher looks for it beside ITS tree (../../flowsta-vault),
# which a worktree elsewhere does not have (sed exits 2 under set -e and B
# never starts). Only those two launches get it: set for every launch,
# the app would take it to production's rendezvous.
B_AUTH="$(sed -n 's/^FLOWSTA_AUTH_MATERIAL=\([^ \\]*\).*/\1/p' "$VAULT/scripts/run-test-instance.sh" | head -1)"
[ -n "$B_AUTH" ] || { echo "no FLOWSTA_AUTH_MATERIAL in $VAULT/scripts/run-test-instance.sh"; exit 1; }
# DEVICES_A_TREE: the same for device A (default: this one). A on an
# earlier release and B on this build is the household that already had a
# computer when the new one joined. A from another tree gets the staging
# material for its rendezvous launches too, as B does.
A_TREE="$(cd "${DEVICES_A_TREE:-$HERE/..}" && pwd)"
a_auth=()
if [ "$A_TREE" != "$(pwd)" ]; then
  echo "== device A runs from $A_TREE"
  cp "$HERE/specs/devices/devices.e2e.ts" "$A_TREE/e2e/specs/devices/devices.e2e.ts"
  a_auth=(FLOWSTA_AUTH_MATERIAL="$B_AUTH")
  export DEVICES_A_EARLIER=1
fi

killtests() { for pid in $(pgrep -x flowsta-vault); do case "$(readlink /proc/$pid/exe 2>/dev/null)" in *target/debug*) kill "$pid" 2>/dev/null || true;; esac; done; }
answering() { for p in 27777 27778 27779; do curl -s -m 2 "http://127.0.0.1:$p/dev/status" | grep -q '"harness":true' && echo "$p"; done; }
waitnew() { for _ in $(seq 1 36); do for p in $(answering); do [ "$p" != "${1:-}" ] && { echo "$p"; return 0; }; done; sleep 5; done; return 1; }
dev() { curl -s -m 60 -X POST "http://127.0.0.1:$1/dev/devices" -H 'content-type: application/json' -H 'origin: https://ourtest.flowsta.com' -d "$2"; }
trap 'killtests; sleep 2; killtests' EXIT

[ -z "$(answering)" ] || { echo "a test Vault is already answering - stop it first"; exit 1; }
# The Vault's debug test binary (a target clean removes it): build it when missing.
[ -x "$VAULT/src-tauri/target/debug/flowsta-vault" ] || { echo "== building the Vault test binary"; (cd "$VAULT" && scripts/build-test-binary.sh > "$LOGS/vault-test-build.log" 2>&1) || { echo "Vault test build failed"; tail -20 "$LOGS/vault-test-build.log"; exit 1; }; }
rm -rf "$HOME/.local/share/flowsta-vault-md-e" "$HOME/.local/share/flowsta-vault-md-f"
(cd "$VAULT" && ${DEVICES_VAULT_WRAP:-} dbus-run-session -- scripts/run-test-instance.sh md-e > "$LOGS/md-e.log" 2>&1 &)
P1="$(waitnew)" || { echo "md-e never answered"; exit 1; }
(cd "$VAULT" && ${DEVICES_VAULT_WRAP:-} dbus-run-session -- scripts/run-test-instance.sh md-f > "$LOGS/md-f.log" 2>&1 &)
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
(cd "$A_TREE" && YOAI_E2E_HOME="$A" YOAI_VAULT_PORT="$P1" DEVICES_PHASE=a node e2e/run.mjs models --spec e2e/specs/devices/devices.e2e.ts)
# The copy must be A's LAST manifest (its forced backup after the chat, the
# one with the rule), not one from its first start: a start writes one too,
# and it is newer than A_START. A's log says when the last was written (UTC).
last_written="$(sed -E 's/\x1b\[[0-9;]*m//g' "$A"/.local/share/com.solar.yourowai/logs/*.log 2>/dev/null | grep "manifest written" | tail -1 | sed -E 's/^\[([0-9-]+)\]\[([0-9:]+)\].*/\1 \2/' || true)"
# Five seconds of margin: the log line is stamped as the write finishes and
# the Vault's created_at can be a second earlier.
if [ -n "$last_written" ]; then A_START="$(( $(date -u -d "$last_written" +%s) - 5 ))"; fi
echo "== V2 fetches V1's copies (A's last manifest: ${last_written:-unknown})"
for _ in $(seq 1 150); do
  # A round right after a Vault restarted its lines (a memory restart) can
  # take longer than one call waits: the loop asks again.
  dev "$P2" '{"op":"round"}' >/dev/null || true
  held="$(dev "$P2" "{\"op\":\"backups\",\"client_id\":\"$CID\"}" || true)"
  newest="$(printf '%s' "$held" | python3 -c 'import sys,json; h=[x for x in json.load(sys.stdin).get("held",[]) if x.get("label")=="manifest" and x.get("from")]; print(max([x.get("created_at",0) for x in h] or [0]))' 2>/dev/null || echo 0)"
  [ "${newest:-0}" -ge "$A_START" ] && break
  sleep 4
done
[ "${newest:-0}" -ge "$A_START" ] || { echo "V2 never received A's new manifest (newest copy $newest < $A_START): $held"; exit 1; }
echo "== V2 holds A's manifest"
echo "== phase b: device B signs in fresh on Vault $P2"
(cd "$B_TREE" && YOAI_E2E_HOME="$B" YOAI_VAULT_PORT="$P2" DEVICES_PHASE=b node e2e/run.mjs models --spec e2e/specs/devices/devices.e2e.ts)
echo "== phase c: both devices online - A unattended on the default ports, B driven on shifted ports (Vaults $P1 / $P2)"
# A runs unattended on the rendezvous with its WebDriver on another port, so B's test driver (4445) is B's.
# A holds the RECORDS, not a chat: it gets an empty models folder, so it never loads a model - two
# instances each with a model exceeded 16 GB and the kernel killed the editor (2026-10-13).
mkdir -p "$LOGS/no-models"
env "${a_auth[@]}" YOAI_E2E_HOME="$A" YOAI_VAULT_PORT="$P1" YOAI_E2E_WITH_MODELS=1 YOAI_E2E_MODELS_DIR="$LOGS/no-models" YOAI_E2E_WITH_RENDEZVOUS=1 TAURI_WEBDRIVER_PORT=4446 "$A_TREE/e2e/launch-app.sh" > "$LOGS/device-a-phase-c.log" 2>&1 &
A_PID=$!
killA() { for pid in $(pgrep -x 'app|your-own-ai'); do case "$(readlink /proc/$pid/exe 2>/dev/null)" in "$A_TREE"/src-tauri/target/debug/*) kill "$pid" 2>/dev/null || true;; esac; done; }
killB() { for pid in $(pgrep -x 'app|your-own-ai'); do case "$(readlink /proc/$pid/exe 2>/dev/null)" in "$B_TREE"/src-tauri/target/debug/*) kill "$pid" 2>/dev/null || true;; esac; done; }
gone() { for _ in $(seq 1 30); do [ -z "$(pgrep -x 'app|your-own-ai')" ] && return 0; sleep 2; done; return 1; }
trap 'killA; killB; killtests; sleep 2; killtests' EXIT
for _ in $(seq 1 60); do curl -s -m 2 http://127.0.0.1:11435/health | grep -q '"ok"' && break; sleep 2; done
curl -s -m 2 http://127.0.0.1:11435/health | grep -q '"ok"' || { echo "device A never came up for phase c"; exit 1; }
echo "== device A is up (pid $A_PID); driving B"
# Phase c waits up to six minutes for A's record to reach B through the
# rendezvous (10-22: it arrived at 5 min and the five-minute harness limit
# had already fired) - the long limit applies.
(cd "$B_TREE" && FLOWSTA_AUTH_MATERIAL="$B_AUTH" YOAI_E2E_LONG=1 YOAI_E2E_HOME="$B" YOAI_VAULT_PORT="$P2" YOAI_PORT_OFFSET=100 YOAI_E2E_WITH_RENDEZVOUS=1 DEVICES_PHASE=c node e2e/run.mjs models --spec e2e/specs/devices/devices.e2e.ts)
killA; gone || { echo "device A did not stop after phase c"; exit 1; }

echo "== phase d: B unattended on the default ports, A driven on shifted ports: B's reply arrives on A"
FLOWSTA_AUTH_MATERIAL="$B_AUTH" YOAI_E2E_HOME="$B" YOAI_VAULT_PORT="$P2" YOAI_E2E_WITH_MODELS=1 YOAI_E2E_MODELS_DIR="$LOGS/no-models" YOAI_E2E_WITH_RENDEZVOUS=1 TAURI_WEBDRIVER_PORT=4446 "$B_TREE/e2e/launch-app.sh" > "$LOGS/device-b-phase-d.log" 2>&1 &
for _ in $(seq 1 60); do curl -s -m 2 http://127.0.0.1:11435/health | grep -q '"ok"' && break; sleep 2; done
curl -s -m 2 http://127.0.0.1:11435/health | grep -q '"ok"' || { echo "device B never came up for phase d"; exit 1; }
(cd "$A_TREE" && env "${a_auth[@]}" YOAI_E2E_LONG=1 YOAI_E2E_HOME="$A" YOAI_VAULT_PORT="$P1" YOAI_PORT_OFFSET=100 YOAI_E2E_WITH_RENDEZVOUS=1 DEVICES_PHASE=d node e2e/run.mjs models --spec e2e/specs/devices/devices.e2e.ts)
killB; gone || true

# Each device's move to the 0.7 line, from its own log (a device on a release
# before the move has none).
for d in A B; do
  home="$ROOT/$d"; logs="$home/.local/share/com.solar.yourowai/logs"
  # Under pipefail a release before the move (no marker, no pass line) must not end the run.
  marker="$(ls "$home"/.local/share/com.solar.yourowai/profiles/*/moved-07 2>/dev/null | head -1 || true)"
  echo "== device $d: marker $([ -n "$marker" ] && echo present || echo "absent (a release before the move has none)"); passes:"
  { cat "$logs"/*.log 2>/dev/null | sed -E 's/\x1b\[[0-9;]*m//g' | grep -E '\[0\.7\] pass: ' | sed -E 's/.*\[0\.7\] /   /' | head -6; } || true
done
echo "== all four phases passed; profiles kept at $ROOT"

# DEVICES_TWINS=1: the household whose two computers each hold both copies of
# a starter (linked before a joining device's starters folded into the
# identity's). With both apps stopped, each stored list is given what such
# a household has: on B, its own starter founded on itself again, and a copy
# of A's; on A, a copy of B's. Then specs/devices/twins.e2e.ts, phases e and f.
[ "${DEVICES_TWINS:-}" = 1 ] || exit 0
TWINS_NAME="${TWINS_NAME:-Teresa}"
planted="$(python3 - "$A" "$B" "$TWINS_NAME" <<'PY'
import glob, json, sys
a_home, b_home, name = sys.argv[1:4]
def load(home):
    for path in glob.glob(home + "/.local/share/com.solar.yourowai/profiles/*/ai-data.json"):
        d = json.load(open(path))
        raw = d.get("custom-ais")
        ais = json.loads(raw) if isinstance(raw, str) else (raw or [])
        if any(x.get("name") == name and x.get("agentPubKey") for x in ais):
            return path, d, ais, isinstance(raw, str)
    raise SystemExit("no stored list holds " + name + " under " + home)
def save(path, d, ais, as_text):
    d["custom-ais"] = json.dumps(ais) if as_text else ais
    json.dump(d, open(path, "w"))
KEEP = ("name", "description", "baseArchetypeId", "systemPrompt", "emoji", "askBlurb", "status", "lengthDisposition", "defaultMode", "useEmojis")
pa, da, aa, ta = load(a_home)
pb, db, ab, tb = load(b_home)
mine_a = next(x for x in aa if x.get("name") == name)
mine_b = next(x for x in ab if x.get("name") == name)
fa = mine_a.get("foundingKey") or mine_a["agentPubKey"]
kb = mine_b["agentPubKey"]
if mine_b.get("foundingKey") != fa:
    raise SystemExit("B's %s is not the identity's (founded %s, A's is %s)" % (name, mine_b.get("foundingKey"), fa))
copy = lambda src, founding: dict({k: src[k] for k in KEEP if k in src}, id=founding, foundingKey=founding, model="auto:offline")
mine_b["foundingKey"] = kb
ab.append(copy(mine_a, fa))
aa.append(copy(mine_b, kb))
save(pa, da, aa, ta); save(pb, db, ab, tb)
print(min(fa, kb), kb)
PY
)"
read -r TWINS_LEADER TWINS_OWN_AGENT <<< "$planted"
[ -n "${TWINS_OWN_AGENT:-}" ] || { echo "the doubled starters could not be written into the stored lists"; exit 1; }
export TWINS_NAME TWINS_LEADER TWINS_OWN_AGENT TWINS_STAMP="$(date +%s)"
echo "== twins: both stored lists hold two ${TWINS_NAME}s; ${TWINS_LEADER:0:14} should lead on both"

echo "== phase e: A unattended, B driven: B stands them as one and writes under its own copy"
env "${a_auth[@]}" YOAI_E2E_HOME="$A" YOAI_VAULT_PORT="$P1" YOAI_E2E_WITH_MODELS=1 YOAI_E2E_MODELS_DIR="$LOGS/no-models" YOAI_E2E_WITH_RENDEZVOUS=1 TAURI_WEBDRIVER_PORT=4446 "$A_TREE/e2e/launch-app.sh" > "$LOGS/device-a-phase-e.log" 2>&1 &
for _ in $(seq 1 60); do curl -s -m 2 http://127.0.0.1:11435/health | grep -q '"ok"' && break; sleep 2; done
curl -s -m 2 http://127.0.0.1:11435/health | grep -q '"ok"' || { echo "device A never came up for phase e"; exit 1; }
(cd "$B_TREE" && FLOWSTA_AUTH_MATERIAL="$B_AUTH" YOAI_E2E_LONG=1 YOAI_E2E_HOME="$B" YOAI_VAULT_PORT="$P2" YOAI_PORT_OFFSET=100 YOAI_E2E_WITH_RENDEZVOUS=1 TWINS_PHASE=e node e2e/run.mjs models --spec e2e/specs/devices/twins.e2e.ts)
killA; gone || { echo "device A did not stop after phase e"; exit 1; }

echo "== phase f: B unattended, A driven: A stands them as one, and lists what B wrote under the other copy"
FLOWSTA_AUTH_MATERIAL="$B_AUTH" YOAI_E2E_HOME="$B" YOAI_VAULT_PORT="$P2" YOAI_E2E_WITH_MODELS=1 YOAI_E2E_MODELS_DIR="$LOGS/no-models" YOAI_E2E_WITH_RENDEZVOUS=1 TAURI_WEBDRIVER_PORT=4446 "$B_TREE/e2e/launch-app.sh" > "$LOGS/device-b-phase-f.log" 2>&1 &
for _ in $(seq 1 60); do curl -s -m 2 http://127.0.0.1:11435/health | grep -q '"ok"' && break; sleep 2; done
curl -s -m 2 http://127.0.0.1:11435/health | grep -q '"ok"' || { echo "device B never came up for phase f"; exit 1; }
(cd "$A_TREE" && env "${a_auth[@]}" YOAI_E2E_LONG=1 YOAI_E2E_HOME="$A" YOAI_VAULT_PORT="$P1" YOAI_PORT_OFFSET=100 YOAI_E2E_WITH_RENDEZVOUS=1 TWINS_PHASE=f node e2e/run.mjs models --spec e2e/specs/devices/twins.e2e.ts)
killB; gone || true
echo "== doubled starters: one AI on both devices, the same one; B's conversation under its own copy shows on A"
