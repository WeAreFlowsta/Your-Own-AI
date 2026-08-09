#!/usr/bin/env bash
# Build ONLY the coordinator zome wasm and stage it as an app resource.
#
# Deliberately never rebuilds the integrity wasm or repacks the .dna/.happ:
# the DNA hash covers the integrity zome, and a byte-different integrity
# build (new compiler, new profile) would change the hash and orphan every
# existing install onto a fresh, invisible cell. The committed
# workdir/transcript_integrity.wasm and the .happ stay untouched forever;
# coordinator updates reach cells (new AND existing installs) through the
# app's startup update_coordinators sweep, which reads the wasm this script
# stages into src-tauri/resources/.
set -euo pipefail
cd "$(dirname "$0")"

# getrandom's wasm32-unknown-unknown build needs an explicit backend; the
# "custom" backend defers to the Holochain host's randomness at runtime.
RUSTFLAGS='--cfg getrandom_backend="custom"' \
  cargo build --release --target wasm32-unknown-unknown -p transcript_coordinator

SRC=target/wasm32-unknown-unknown/release/transcript_coordinator.wasm
OUT=../../src-tauri/resources/transcript_coordinator.wasm
cp "$SRC" "$OUT"
echo "Staged coordinator wasm: $(du -h "$OUT" | cut -f1) -> $OUT"
echo "Remember: bump COORDINATOR_VERSION in src-tauri/src/dna.rs so the"
echo "startup sweep pushes it to installed cells."
