#!/bin/bash

# Download holochain + lair-keystore binaries for bundling with the app
# These are compiled by our GitHub Actions workflow (build-holochain-binaries.yml)

set -e

BINDIR="src-tauri/binaries"
mkdir -p "$BINDIR"

echo "📥 Downloading Holochain binaries..."
echo ""

# Get latest release from OUR repository
REPO="WeAreFlowsta/Your-Own-AI"

# Check for GitHub token (needed for private repos)
AUTH_HEADER=""
if [ -n "$GITHUB_TOKEN" ]; then
  echo "🔑 Using GITHUB_TOKEN for authentication"
  AUTH_HEADER="Authorization: token $GITHUB_TOKEN"
fi

# Auto-detect latest holochain release
echo "🔍 Finding latest Holochain release..."
RELEASE_TAG=$(curl -s ${AUTH_HEADER:+-H "$AUTH_HEADER"} "https://api.github.com/repos/${REPO}/releases" | grep '"tag_name"' | grep 'holochain-' | head -1 | sed -E 's/.*"([^"]+)".*/\1/')

if [ -z "$RELEASE_TAG" ]; then
  echo "❌ Error: Could not find Holochain release"
  echo "💡 For private repos, set GITHUB_TOKEN environment variable:"
  echo "   export GITHUB_TOKEN=your_github_token"
  echo "   ./download-holochain-binaries.sh"
  echo ""
  echo "💡 Or run the build-holochain-binaries workflow first in GitHub Actions"
  exit 1
fi

BASE_URL="https://github.com/${REPO}/releases/download/${RELEASE_TAG}"

echo "📦 Using release: ${RELEASE_TAG}"
echo ""

# Linux (x86_64)
echo "⬇️  Downloading Linux binaries..."
curl -L ${AUTH_HEADER:+-H "$AUTH_HEADER"} "${BASE_URL}/holochain-x86_64-unknown-linux-gnu" -o "${BINDIR}/yourowai-holochain-x86_64-unknown-linux-gnu"
curl -L ${AUTH_HEADER:+-H "$AUTH_HEADER"} "${BASE_URL}/lair-keystore-x86_64-unknown-linux-gnu" -o "${BINDIR}/yourowai-lair-keystore-x86_64-unknown-linux-gnu"
chmod +x "${BINDIR}/yourowai-holochain-x86_64-unknown-linux-gnu" "${BINDIR}/yourowai-lair-keystore-x86_64-unknown-linux-gnu"
echo "✅ Linux binaries ready"

# macOS (Apple Silicon - ARM64)
echo "⬇️  Downloading macOS ARM64 binaries..."
curl -L ${AUTH_HEADER:+-H "$AUTH_HEADER"} "${BASE_URL}/holochain-aarch64-apple-darwin" -o "${BINDIR}/yourowai-holochain-aarch64-apple-darwin"
curl -L ${AUTH_HEADER:+-H "$AUTH_HEADER"} "${BASE_URL}/lair-keystore-aarch64-apple-darwin" -o "${BINDIR}/yourowai-lair-keystore-aarch64-apple-darwin"
chmod +x "${BINDIR}/yourowai-holochain-aarch64-apple-darwin" "${BINDIR}/yourowai-lair-keystore-aarch64-apple-darwin"
echo "✅ macOS ARM64 binaries ready"

# macOS (Intel - x86_64)
echo "⬇️  Downloading macOS Intel binaries..."
curl -L ${AUTH_HEADER:+-H "$AUTH_HEADER"} "${BASE_URL}/holochain-x86_64-apple-darwin" -o "${BINDIR}/yourowai-holochain-x86_64-apple-darwin"
curl -L ${AUTH_HEADER:+-H "$AUTH_HEADER"} "${BASE_URL}/lair-keystore-x86_64-apple-darwin" -o "${BINDIR}/yourowai-lair-keystore-x86_64-apple-darwin"
chmod +x "${BINDIR}/yourowai-holochain-x86_64-apple-darwin" "${BINDIR}/yourowai-lair-keystore-x86_64-apple-darwin"
echo "✅ macOS Intel binaries ready"

# Windows (x86_64)
echo "⬇️  Downloading Windows binaries..."
curl -L ${AUTH_HEADER:+-H "$AUTH_HEADER"} "${BASE_URL}/holochain-x86_64-pc-windows-msvc.exe" -o "${BINDIR}/yourowai-holochain-x86_64-pc-windows-msvc.exe"
curl -L ${AUTH_HEADER:+-H "$AUTH_HEADER"} "${BASE_URL}/lair-keystore-x86_64-pc-windows-msvc.exe" -o "${BINDIR}/yourowai-lair-keystore-x86_64-pc-windows-msvc.exe"
echo "✅ Windows binaries ready"

echo ""
echo "✅ All Holochain binaries downloaded!"
echo ""
echo "📊 Binary sizes:"
ls -lh "${BINDIR}"/holochain-* "${BINDIR}"/lair-keystore-*
echo ""
echo "🎯 Binaries ready for bundling with Tauri app"
