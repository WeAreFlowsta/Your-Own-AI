#!/bin/bash
set -e

echo "Building Your Own AI transcript DNA v1..."

# Build zomes
echo "Building zomes..."
RUSTFLAGS='--cfg getrandom_backend="custom"' CARGO_TARGET_DIR=target cargo build --release --target wasm32-unknown-unknown

# Copy WASM to workdir
echo "Copying WASM files..."
cp target/wasm32-unknown-unknown/release/transcript_integrity.wasm workdir/
cp target/wasm32-unknown-unknown/release/transcript_coordinator.wasm workdir/

# Pack DNA
echo "Packing DNA..."
hc dna pack workdir

# Pack hApp
echo "Packing hApp..."
hc app pack workdir

echo ""
echo "Build complete!"
echo "  DNA: workdir/yourown_ai_transcript_v1.dna"
echo "  hApp: workdir/yourown_ai_transcript_v1_happ.happ"
