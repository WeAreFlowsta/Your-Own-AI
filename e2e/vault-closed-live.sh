#!/usr/bin/env bash
# "Restart to update" with Flowsta Vault not running, then an install that
# fails (specs/locked/vault-closed.e2e.ts), in locked-live.sh's setup: a test
# Vault with a new identity and a profile linked to it. The spec closes the
# Vault itself. Logs in LOCKED_LOGS (default ~/.cache/yoai-locked-live).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
LOGS="${LOCKED_LOGS:-$HOME/.cache/yoai-locked-live}"
LOCKED_SPEC=e2e/specs/locked/vault-closed.e2e.ts YOAI_TEST_UPDATE_INSTALL_FAILS=1 "$HERE/locked-live.sh"
grep -q "\[update\] not installed yet: no Vault backup could be made (vault_unavailable)" "$LOGS/app.log" || { echo "FAIL: the closed Vault did not hold the update" >&2; exit 1; }
grep -q "\[update\] the install failed (the test build was asked to fail it)" "$LOGS/app.log" || { echo "FAIL: the install was never made to fail" >&2; exit 1; }
echo "== the Vault closed, then a failed install: passed (logs kept at $LOGS)"
