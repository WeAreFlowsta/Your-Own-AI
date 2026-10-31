#!/usr/bin/env bash
# Fetch the bundled pdfium library (renders scanned PDF pages for OCR) from
# bblanchon/pdfium-binaries (Apache-2.0) into src-tauri/resources/pdfium/, named
# as pdfium-render expects (libpdfium.so / libpdfium.dylib / pdfium.dll).
#
# Run once per platform before building. In CI, run on each runner.
#   ./scripts/fetch-pdfium.sh                # auto-detect this host
#   ./scripts/fetch-pdfium.sh linux-x64      # or force a target
set -euo pipefail

# Pinned: the build Your Own AI 0.9.0 shipped. A bump changes the tag and all
# four SHA-256s together (each release lists them as the asset digests).
PDFIUM_TAG="chromium/8111"
REL="https://github.com/bblanchon/pdfium-binaries/releases/download/$PDFIUM_TAG"
DEST="$(cd "$(dirname "$0")/.." && pwd)/src-tauri/resources/pdfium"

target="${1:-}"
if [ -z "$target" ]; then
  case "$(uname -s)-$(uname -m)" in
    Linux-x86_64)   target="linux-x64" ;;
    Darwin-arm64)   target="mac-arm64" ;;
    Darwin-x86_64)  target="mac-x64" ;;
    *) echo "Unknown host; pass a target (linux-x64 | mac-arm64 | mac-x64 | win-x64)"; exit 1 ;;
  esac
fi

case "$target" in
  linux-*) inner="lib/libpdfium.so";    out="libpdfium.so" ;;
  mac-*)   inner="lib/libpdfium.dylib"; out="libpdfium.dylib" ;;
  win-*)   inner="bin/pdfium.dll";      out="pdfium.dll" ;;
  *) echo "Unknown target: $target"; exit 1 ;;
esac

case "$target" in
  linux-x64) want="bdc60126399f39fe768566a18d4be055a7b3681bc9e248e8a6656b4f6cae0aa2" ;;
  mac-arm64) want="46c674298c60123f1195c2a03c65ee08c4783c84874ce4f5b3a054abb3ecaf30" ;;
  mac-x64)   want="e7fece4693f65a7e47f84fa38099068d573d4dcbd46b1a3c8d2c26581c308466" ;;
  win-x64)   want="3452d11031050ec65e57f180215b57b9e6d465e3fbedafa8a77ad90885c7d23e" ;;
  *) echo "No pinned pdfium for $target"; exit 1 ;;
esac

mkdir -p "$DEST"
tmp="$(mktemp -d)"
echo "Fetching pdfium $PDFIUM_TAG ($target)…"
curl -fsSL "$REL/pdfium-$target.tgz" -o "$tmp/pdfium.tgz"
got="$(if command -v sha256sum >/dev/null; then sha256sum "$tmp/pdfium.tgz"; else shasum -a 256 "$tmp/pdfium.tgz"; fi | cut -d' ' -f1)"
[ "$got" = "$want" ] || { echo "pdfium-$target.tgz ($PDFIUM_TAG): SHA-256 $got does not match the pinned $want"; rm -rf "$tmp"; exit 1; }
tar -xzf "$tmp/pdfium.tgz" -C "$tmp" "$inner"
cp "$tmp/$inner" "$DEST/$out"
rm -rf "$tmp"
echo "pdfium → $DEST/$out"
