#!/bin/bash
# The Holochain sidecars, for a dev build on this machine and for the release
# build (.github/workflows/build-release.yml runs this script), each checked
# against .github/holochain-sidecars.sha256:
#
#   0.6.1  conductor and key store from the official Holochain release
#   0.7.1  key store from the official Holochain release; conductor from
#          https://github.com/WeAreFlowsta/holochain-build, which builds the
#          official 0.7.1 source with the patches listed there
#
#   ./download-holochain-binaries.sh            this machine's target
#   ./download-holochain-binaries.sh <triple>   another target
#
# 0.6.1 lands as yourowai-<bin>-<triple>, 0.7.1 as yourowai-<bin>-07-<triple>
# (about 180 MB for the four files).

set -euo pipefail
cd "$(dirname "$0")"

T="${1:-}"
if [ -z "$T" ]; then
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64) T=x86_64-unknown-linux-gnu ;;
    Linux-aarch64) T=aarch64-unknown-linux-gnu ;;
    Darwin-arm64) T=aarch64-apple-darwin ;;
    Darwin-x86_64) T=x86_64-apple-darwin ;;
    MINGW*|MSYS*|CYGWIN*) T=x86_64-pc-windows-msvc ;;
    *) echo "Unknown machine $(uname -s)-$(uname -m): pass a target triple"; exit 1 ;;
  esac
fi
EXT=""
if [ "$T" = "x86_64-pc-windows-msvc" ]; then EXT=".exe"; fi

BINDIR="src-tauri/binaries"
mkdir -p "$BINDIR"
pinned() { awk -v k="$1" '$1 !~ /^#/ && $2 == k { print $1 }' .github/holochain-sidecars.sha256; }
sha() { if command -v sha256sum >/dev/null; then sha256sum "$1"; else shasum -a 256 "$1"; fi | cut -d' ' -f1; }

# <binary>:<suffix>:<release tag>. A tag with "-flowsta." is a release of
# WeAreFlowsta/holochain-build; any other is a release of holochain/holochain.
for ITEM in \
  "holochain::holochain-0.6.1" \
  "lair-keystore::holochain-0.6.1" \
  "holochain:-07:holochain-0.7.1-flowsta.4" \
  "lair-keystore:-07:holochain-0.7.1"; do
  BIN="${ITEM%%:*}"; REST="${ITEM#*:}"; SUFFIX="${REST%%:*}"; HC_RELEASE="${REST#*:}"
  case "$HC_RELEASE" in
    *-flowsta.*) REPO="WeAreFlowsta/holochain-build" ;;
    *) REPO="holochain/holochain" ;;
  esac
  BASE="https://github.com/${REPO}/releases/download/${HC_RELEASE}"
  {
    ASSET="${BIN}-${T}${EXT}"
    DEST="${BINDIR}/yourowai-${BIN}${SUFFIX}-${T}${EXT}"
    want=$(pinned "${HC_RELEASE}/${ASSET}")
    if [ -z "$want" ]; then echo "${HC_RELEASE}/${ASSET}: no pinned checksum"; exit 1; fi
    if [ -f "$DEST" ] && [ "$(sha "$DEST")" = "$want" ]; then
      echo "$DEST: already here"
      continue
    fi
    curl -fsSL --retry 5 --retry-delay 20 --retry-all-errors "${BASE}/${ASSET}" -o "$DEST.part"
    got=$(sha "$DEST.part")
    if [ "$got" != "$want" ]; then rm -f "$DEST.part"; echo "${HC_RELEASE}/${ASSET}: SHA-256 $got does not match the pinned $want"; exit 1; fi
    mv "$DEST.part" "$DEST"
    chmod +x "$DEST"
    echo "$DEST (${HC_RELEASE}, checksum verified)"
  }
done
