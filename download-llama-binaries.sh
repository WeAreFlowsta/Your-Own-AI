#!/bin/bash

# Download llama-server binaries for bundling with the app
# These are compiled by our GitHub Actions from llama.cpp source
# Ensures static linking and broad model compatibility

set -e

BINDIR="src-tauri/bin"
mkdir -p "$BINDIR"

echo "📥 Downloading llama-server binaries..."
echo ""

# Get latest release from OUR repository
REPO="Your-Own-AI/App"

# Check for GitHub token (needed for private repos)
AUTH_HEADER=""
if [ -n "$GITHUB_TOKEN" ]; then
  echo "🔑 Using GITHUB_TOKEN for authentication"
  AUTH_HEADER="Authorization: token $GITHUB_TOKEN"
fi

# Auto-detect latest llama-server release
echo "🔍 Finding latest llama-server release..."
# The engine release the app is PINNED to (engine.rs), never "the newest
# llama- release": the two must agree, and app releases outnumber engine
# releases on the first page of the listing anyway.
RELEASE_TAG=$(sed -n 's/.*LLAMA_ENGINE_TAG: &str = "\([^"]*\)".*/\1/p' src-tauri/src/engine.rs)
if [ -z "$RELEASE_TAG" ]; then
  RELEASE_TAG=$(curl -s ${AUTH_HEADER:+-H "$AUTH_HEADER"} "https://api.github.com/repos/${REPO}/releases?per_page=100" | grep '"tag_name"' | grep 'llama-' | head -1 | sed -E 's/.*"([^"]+)".*/\1/')
fi

if [ -z "$RELEASE_TAG" ]; then
  echo "❌ Error: Could not find llama-server release"
  echo "💡 For private repos, set GITHUB_TOKEN environment variable:"
  echo "   export GITHUB_TOKEN=your_github_token"
  echo "   ./download-llama-binaries.sh"
  exit 1
fi

BASE_URL="https://github.com/${REPO}/releases/download/${RELEASE_TAG}"

echo "📦 Using release: ${RELEASE_TAG}"
echo ""

# Linux (x86_64)
echo "⬇️  Downloading Linux binary..."
curl -L ${AUTH_HEADER:+-H "$AUTH_HEADER"} "${BASE_URL}/llama-server-x86_64-unknown-linux-gnu" -o "${BINDIR}/llama-server-x86_64-unknown-linux-gnu"
chmod +x "${BINDIR}/llama-server-x86_64-unknown-linux-gnu"
echo "✅ Linux binary ready"

# macOS (Apple Silicon - ARM64)
echo "⬇️  Downloading macOS ARM64 binary..."
curl -L ${AUTH_HEADER:+-H "$AUTH_HEADER"} "${BASE_URL}/llama-server-aarch64-apple-darwin" -o "${BINDIR}/llama-server-aarch64-apple-darwin"
chmod +x "${BINDIR}/llama-server-aarch64-apple-darwin"
echo "✅ macOS ARM64 binary ready"

# macOS (Intel - x86_64)
echo "⬇️  Downloading macOS Intel binary..."
curl -L ${AUTH_HEADER:+-H "$AUTH_HEADER"} "${BASE_URL}/llama-server-x86_64-apple-darwin" -o "${BINDIR}/llama-server-x86_64-apple-darwin"
chmod +x "${BINDIR}/llama-server-x86_64-apple-darwin"
echo "✅ macOS Intel binary ready"

# Windows (x86_64)
echo "⬇️  Downloading Windows binary..."
curl -L ${AUTH_HEADER:+-H "$AUTH_HEADER"} "${BASE_URL}/llama-server-x86_64-pc-windows-msvc.exe" -o "${BINDIR}/llama-server-x86_64-pc-windows-msvc.exe"
echo "✅ Windows binary ready"

echo ""
echo "✅ All binaries downloaded!"
echo ""
echo "📊 Binary sizes:"
ls -lh "${BINDIR}"/llama-server-*
echo ""
echo "🎯 Binaries ready for bundling with Tauri app"
