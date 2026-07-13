# Third-party notices

Your Own AI stands on excellent open-source work. Bundled or downloaded
components and their licenses:

## Bundled with the app

- **llama.cpp** (`llama-server`) — MIT License. https://github.com/ggml-org/llama.cpp
- **Holochain** (`holochain`, `lair-keystore`) — Cryptographic Autonomy License 1.0. https://github.com/holochain/holochain
- **pdfium** (prebuilt binaries by bblanchon) — Apache License 2.0. https://github.com/bblanchon/pdfium-binaries

## Downloaded on demand (optional components)

- **ocrs text-detection and text-recognition models** — CC BY-SA 4.0,
  © Robert Knight. Downloaded from the author's distribution at
  https://ocrs-models.s3-accelerate.amazonaws.com/ when you enable
  scanned-document OCR. The ocrs engine itself (compiled into the app)
  is Apache-2.0 OR MIT. https://github.com/robertknight/ocrs
- **AI models** (GGUF chat models, embedding models, vision projectors)
  are downloaded from their publishers on Hugging Face under their own
  licenses, shown where applicable before download.

Full dependency license information for Rust crates and npm packages is
available from the standard manifests (`src-tauri/Cargo.toml`,
`package.json`).
