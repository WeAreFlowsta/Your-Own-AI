#!/bin/bash
# Build the Your Own AI transcript DNA v2 (Holochain 0.7.1).
#
# v2 is a new DNA hash, so a new network beside v1's: records carry their
# ids inside the encrypted plaintext, and links carry the keyed hash of
# their record's id.
#
# The app bundles this pack as src-tauri/resources/yourown_ai_transcript_v2_happ.happ.
# Once a release has shipped it, never rebuild it: a new build is a new DNA
# hash, so a new network. The sweettests in tests/ load it from workdir/.
#
# Prerequisites:
#   - hc CLI 0.7.1 (cargo install holochain_cli --version 0.7.1), reachable
#     as `hc-0.7` or named in $HC
#   - Rust 1.94 with the wasm32 target (rust-toolchain.toml pins it)

set -e
cd "$(dirname "$0")"

if ! command -v ${HC:-hc-0.7} &> /dev/null; then
    echo "Error: Holochain CLI 0.7.1 (hc-0.7, or \$HC) not found"
    echo "Install with: cargo install holochain_cli --version 0.7.1"
    exit 1
fi

echo "Building the transcript zomes..."
RUSTFLAGS='--cfg getrandom_backend="custom"' CARGO_TARGET_DIR=target \
    cargo build --release --target wasm32-unknown-unknown

cp target/wasm32-unknown-unknown/release/transcript_integrity.wasm workdir/
cp target/wasm32-unknown-unknown/release/transcript_coordinator.wasm workdir/

${HC:-hc-0.7} dna pack workdir
${HC:-hc-0.7} app pack workdir

echo "Built: workdir/yourown_ai_transcript_v2.dna and workdir/yourown_ai_transcript_v2_happ.happ"
sha256sum workdir/yourown_ai_transcript_v2_happ.happ
