# Changelog

All notable changes to Your Own AI are documented here. The release workflow
extracts the entry matching the pushed tag into the GitHub release notes.

## [0.1.0-beta.2] - 2026-07-16

Fixes from the first day of beta testing on Windows and low-memory Macs.

### Fixed
- **Windows: the app can actually run models now.** The bundled inference
  engine linked two OpenSSL libraries that aren't present on a normal
  Windows machine, so it crashed on launch with missing-DLL errors. The
  engine is rebuilt with no OpenSSL dependency, and the build now fails if
  that dependency ever sneaks back in.
- **Windows: no more stray terminal windows.** The background services the
  app starts (key store, conversation store, inference engine) had visible
  console windows; they are now hidden automatically.
- **Windows: closing and reopening the app works.** Background services are
  now tied to the app's lifetime, so quitting can't leave orphans behind
  that block the next launch from recording conversations.
- **Small-memory machines get honest model recommendations.** On an 8GB
  MacBook Air the welcome screen recommended a 16.8GB model and the models
  page showed nothing as runnable. Apple Silicon's shared memory is now
  detected as the GPU budget, the memory reserve scales with machine size,
  and the first-run recommendation uses the same fit math as the models
  page.

### Changed
- Beta builds are not Windows code-signed (SmartScreen will ask once);
  release candidates and stable releases are signed as usual.

## [0.1.0-beta.1] - 2026-07-15

First public beta.

### Highlights
- Chat with AI models running entirely on your machine - a curated catalog of
  open models sized from 2B to 32B, with hardware-aware recommendations so you
  only see what your computer can actually run.
- Auto model routing: let the app pick the best model per question, offline
  only or a mix of offline and online, with your choices for which online
  model handles current-info and hard questions.
- Online models (with a plan): the GPT-5.6 family, Grok 4.5, Grok Build, and
  Sonar - including live web search with cited sources.
- Every reply shows which model answered and why (the Model button under each
  answer), and can be redone on your device or online with one click.
- Persistent memory: your AIs remember facts you teach them and past
  conversations, stored encrypted on your machine.
- Attachments and vision: images, documents, and OCR for scanned PDFs.
- Conversation history is kept as an append-only encrypted transcript, with
  signed Markdown export.
- GPU acceleration out of the box (Vulkan on Linux and Windows, Metal on
  macOS), an optional high-performance CUDA engine download for NVIDIA cards,
  and the option to connect your own inference server.
- Back up and restore everything through your Flowsta Vault.

### Known beta limitations
- No auto-update yet: install new betas manually.
- Windows CUDA engine is built but not yet verified on hardware.
