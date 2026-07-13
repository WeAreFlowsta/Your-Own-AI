#!/usr/bin/env bash
# Fetch the bundled pdfium library (renders scanned PDF pages for OCR) from
# bblanchon/pdfium-binaries (Apache-2.0) into src-tauri/resources/pdfium/, named
# as pdfium-render expects (libpdfium.so / libpdfium.dylib / pdfium.dll).
#
# Run once per platform before building. In CI, run on each runner.
#   ./scripts/fetch-pdfium.sh                # auto-detect this host
#   ./scripts/fetch-pdfium.sh linux-x64      # or force a target
set -euo pipefail

REL="https://github.com/bblanchon/pdfium-binaries/releases/latest/download"
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

mkdir -p "$DEST"
tmp="$(mktemp -d)"
echo "Fetching pdfium ($target)…"
curl -fsSL "$REL/pdfium-$target.tgz" -o "$tmp/pdfium.tgz"
tar -xzf "$tmp/pdfium.tgz" -C "$tmp" "$inner"
cp "$tmp/$inner" "$DEST/$out"
rm -rf "$tmp"
echo "pdfium → $DEST/$out"
