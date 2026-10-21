#!/usr/bin/env bash
# A true first launch by hand: the e2e build of the app in an EMPTY scratch
# home, so the welcome wizard shows (no model installed) and nothing in the
# person's own profile, models or identity is touched. Downloads are REAL
# here (no block switch). Keep the scratch home between relaunches while a
# download is in flight; check for finished files before deleting it.
#   npm run walkthrough                # scratch home: e2e/walkthrough-home
#   WALK_HOME=/some/dir npm run walkthrough
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
APP="$HERE/../src-tauri/target/debug/app"
[ -x "$APP" ] || { echo "no e2e build at $APP - run: npm run e2e:build"; exit 1; }
pgrep -x app >/dev/null && { echo "an app instance is already running (pgrep -x app) - close it first"; exit 1; }
export HOME="${WALK_HOME:-$HERE/walkthrough-home}"
export XDG_CONFIG_HOME="$HOME/.config" XDG_DATA_HOME="$HOME/.local/share" XDG_CACHE_HOME="$HOME/.cache"
mkdir -p "$HOME"
echo "scratch home: $HOME (downloads land here, for real)"
exec "$APP"
