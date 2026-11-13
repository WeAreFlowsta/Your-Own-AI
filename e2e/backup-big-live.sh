#!/usr/bin/env bash
# A Vault backup of an AI with a thousand conversations (specs/locked/
# backup-big): which passes ask its cell for the list, and how long each
# pass takes. Runs in locked-live.sh's setup (this build linked to a fresh
# test Vault). YOAI_E2E_BIG: how many conversations (1000).
# Logs and the home are kept in BIG_LOGS (default ~/.cache/yoai-backup-big-live).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
LOGS="${BIG_LOGS:-$HOME/.cache/yoai-backup-big-live}"
fail() { echo "FAIL: $*" >&2; exit 1; }
LOCKED_LOGS="$LOGS" LOCKED_SPEC=e2e/specs/locked/backup-big.e2e.ts "$HERE/locked-live.sh"
LOG="$LOGS/app.log"
grep -oE "e2e: big .*" "$LOGS/launch.out" | cut -c1-260 | sed 's/^/   /'
BIGAPP="$(grep -oE "\[escrow\] read 1[0-9]{3,} conversation\(s\) of [^ ]+" "$LOG" | head -1 | awk '{print $NF}')"
[ -n "$BIGAPP" ] || fail "no pass read the big AI's conversations"
echo "   the big AI's cell: $BIGAPP"
grep -E "\[escrow\] (read [0-9]+ conversation\(s\) of $BIGAPP|$BIGAPP: nothing written since it was last listed)" "$LOG" | sed -E 's/^\[[0-9-]+\]\[([0-9:]+)\]\[[^]]*\]\[[A-Z]+\] /   \1 /' | cut -c1-200
listed="$(grep -cE "\[escrow\] read [0-9]+ conversation\(s\) of $BIGAPP" "$LOG" || true)"
stood="$(grep -cE "\[escrow\] $BIGAPP: nothing written since it was last listed" "$LOG" || true)"
echo "   the big AI was listed by $listed pass(es) and stood as backed up in $stood"
# Listed by the first pass and the fourth (plus any pass the app ran by itself
# while the conversations were being written); stood in the second, third and fifth.
[ "$stood" -ge 3 ] || fail "the big AI was listed again by a pass with nothing written to it ($stood of at least 3 passes skipped it)"
[ "$listed" -ge 2 ] || fail "the big AI was not listed again after a write to it"
echo "== A backup of a long-used AI: passed (logs kept at $LOGS)"
