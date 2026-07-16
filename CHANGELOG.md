# Changelog

All notable changes to Your Own AI are documented here. The release workflow
extracts the entry matching the pushed tag into the GitHub release notes.

## [0.1.0-beta.5] - 2026-07-16

### Changed
- **Your first model no longer gets pinned to your AIs.** On a fresh install,
  AIs are set to "Auto - Offline Only", so they automatically use the best
  model you have as your collection grows, instead of staying on the starter
  model forever.
- **Smoother, lighter streaming.** Replies render with far less overhead
  while generating - most noticeable on long reports with fast graphics
  cards - and switching models mid-conversation starts the reply about half
  a second sooner.

## [0.1.0-beta.4] - 2026-07-16

### Added
- **Give your AIs documents.** Editing an AI now has a Knowledge tab: add
  files (PDF, Word, Excel, text, code) and the AI reads and remembers them,
  drawing on the relevant parts in any conversation. Original files can be
  moved or deleted afterward - the AI keeps its own copy. A large document
  costs nothing extra per message: only the pieces that matter are used.
  Documents also show on the AI's memory page, where they can be added too.
- **A cleaner edit-AI dialog.** Editing an AI now uses a wider layout with
  sections - Basics, Behaviour, Details, Knowledge, Appearance - instead of
  one long scroll. Creating a new AI stays quick and simple.
- **The NVIDIA engine is offered on the home screen.** If your graphics card
  qualifies and the high-performance engine isn't installed, a dismissible
  tip lets you download it right there - no trip to Settings.

### Changed
- **Attached files now show as chips in the message box** - name, size, and
  a remove button - instead of a list below it. Images appear as thumbnails
  in the same row. A context-usage note appears only when attachments start
  to crowd the model's memory.

## [0.1.0-beta.3] - 2026-07-16

The inference engine now runs on clean Windows and macOS machines - beta 2's
"model couldn't be loaded" errors were the engine failing to start at all.

### Fixed
- **Windows: the engine no longer needs Microsoft's C++ runtime installed.**
  On machines with an older Visual C++ redistributable the engine crashed
  instantly and the app misread it as "model too large". The runtime is now
  built into the engine itself, so nothing on the machine matters.
- **macOS: the engine no longer depends on Homebrew.** It was linked against
  a Homebrew OpenSSL library that only exists on build machines, so it died
  on launch on real Macs. It now has no such dependency.
- **Honest errors when a model fails to load.** "Too large for your graphics
  card" is now only shown when the engine actually ran out of memory; an
  engine crash says so instead of sending you hunting for smaller models.

### Changed
- **New app icon.** The mark now sits on a dark gradient tile so it reads
  clearly on any desktop, light or dark, at every size.

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
