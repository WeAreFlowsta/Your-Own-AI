# Changelog

All notable changes to Your Own AI are documented here. The release workflow
extracts the entry matching the pushed tag into the GitHub release notes.

## [0.1.0-rc.2] - 2026-07-29

### Fixed
- The projects dropdown in the header no longer gets painted over by
  the AI selector row on the chat page - header menus now always stack
  above page content.

## [0.1.0-rc.1] - 2026-07-29

### Added
- The "How to connect" panel in External access links to step-by-step
  guides for Hermes Agent, OpenClaw, and other OpenAI-compatible apps
  at docs.yourownai.net.

### Changed
- Windows installers are now code signed - no more "Windows protected
  your PC" step during install. macOS builds remain signed and
  notarized as always.

## [0.1.0-beta.16] - 2026-07-29

### Added
- **Serve your AIs to your other devices.** A new toggle in Settings →
  External access opens the local endpoint to your network, minting an
  access key that other devices present as their API key - the same
  three-field setup every OpenAI-compatible app already uses, with Copy
  setup providing all three. Apps on this computer stay keyless, devices
  without the key are refused, and the key can be regenerated anytime.

## [0.1.0-beta.15] - 2026-07-28

### Added
- A "Reading ..." notice appears the moment any file is attached, so a
  large document never looks like nothing happened while it's read in.

### Changed
- The signed-export dialogs (Export AI and knowledge packs) now know
  whether Flowsta Vault is installed and unlocked, and guide you to the
  right next step for each case - including a direct link to get the
  Vault when it isn't installed. Exporting unsigned is always available.

## [0.1.0-beta.14] - 2026-07-27

### Fixed
- The monthly-allowance and sign-in notices now appear as proper cards in
  normal chat as well - the last remaining path that printed the raw error
  into the reply.

## [0.1.0-beta.13] - 2026-07-27

### Fixed
- Online-model billing and sign-in notices now appear as proper cards on
  every failure path during project work - one route was still printing
  the raw error into the chat.

## [0.1.0-beta.12] - 2026-07-27

### Changed
- The loaded-model indicator now sits first in the header, so the project
  chip and conversations stay together.

### Fixed
- Sign-in and plan notices now appear as proper cards during project work
  too - an online model needing attention mid-session no longer prints raw
  error text into the chat. Sending a new message clears the previous
  notice.

## [0.1.0-beta.11] - 2026-07-27

### Changed
- **A new app icon.** The mark in neutral chrome on a black tile, sized to
  be seen.
- **Conversation backups that grow with you.** With the newest Flowsta
  Vault, each conversation backs up as its own compressed object - there
  is no overall size budget, a single long session can never be too large
  to protect, and backup runs only re-send conversations with new
  messages. Older Vaults keep the single-snapshot backup they have today.
- The NVIDIA graphics-card speed tip now sits below the message field on
  the home page.
- Backups copy in Settings now describes the one recovery story: your
  key and conversations back up to your Flowsta Vault together.

## [0.1.0-beta.10] - 2026-07-27

### Added
- **Projects: agentic coding in chat.** Open a project folder and your AI
  works in it - reads files, makes edits, and runs commands, always with
  your permission. Every step is recorded in your private transcripts.
  Projects need Your Own AI Build, a free add-on downloaded with one
  click from inside the app (the project menu or the Components page).
- **See the work as it happens.** A working turn shows its whole story
  live: each step with a readable label and its real result, the model's
  thinking, file edits as colored diffs, and long-running commands
  streaming their output - full scrollback while they run, following the
  newest line until you scroll up. When the turn ends it folds into a
  single summary line you can reopen any time. A "Simple project view"
  setting trims the verbosity, never the liveness.
- **Project memory.** Each project keeps a memory all your AIs share:
  an AI can deliberately save a note while it works (you see exactly
  what it wants to save before allowing it), each session's takeaways
  are distilled when you finish, and you can edit every line yourself.
  Remember something "for this project" or "for this AI" - your choice.
- **Conversations pick up where they left off.** Reopening a project
  conversation restores what the AI knew, so it remembers the plans and
  commands from earlier instead of starting blank.
- **Cost-aware routing for projects.** Side tasks run on your device when
  your hardware is comfortably up to it, and a thrifty setting keeps
  whole project sessions on-device when they fit. The routing settings
  explain exactly what happens and when.

### Fixed
- A reply could occasionally go missing from your records when two
  saves landed at the same moment. Saving is now serialized per
  identity and retried, and long results are trimmed to fit instead of
  failing silently.

## [0.1.0-beta.9] - 2026-07-17

### Added
- **Live web-search progress.** While an online search model researches,
  the status line shows each step - "Searching the web (3)..." and the
  actual queries it ran - instead of a silent wait. Deep research turns
  used to look frozen for 30-60 seconds while working perfectly.

### Changed
- **Online questions start much sooner.** The pre-checks that decide how a
  message is handled now run alongside routing instead of before it, and
  they no longer wait behind a model that's still loading. The worst case
  - an online question right after launch - used to stall for many
  seconds before anything happened.
- **Honest status lines.** The model-loading indicator only appears when
  your question is actually waiting on a model load, and it names the
  model. A background warm-up no longer badges an unrelated reply with a
  bare loading icon.
- **Thinking text reads properly.** The thinking box and the Thoughts
  view both render formatting the same way - no more doubled paragraph
  gaps in one and raw markdown in the other - and headings inside
  reasoning render small instead of shouting.
- Questions about upcoming fixtures and betting odds now count as
  needing current information and go to the web.

### Fixed
- A search model's reasoning could print a stray sentence (or more) into
  the reply when it kept thinking between searches. Reasoning now always
  lands in the thinking box. (Server-side - this also repairs older
  betas.)

## [0.1.0-beta.8] - 2026-07-17

### Fixed
- **Everyday questions no longer route to your medical model.** Health
  detection is now measured and precise: a question must genuinely read as
  being about your health, not merely resemble one. Previously questions
  like "whats the latest news" could be kept off the web and answered by
  the medical model.
- **A medical model can't take over general chat.** Specialist models only
  answer the questions they're for; once a health turn is done, the next
  ordinary question switches back to your general model instead of
  sticking with the specialist.
- **MedGemma's reasoning now shows in the thinking box** instead of
  printing inside the reply (which used to start with a stray "thought").
- **Hard questions sent online answer again.** The online service rejected
  a request setting used with the newest models; fixed on our servers, so
  this also repairs older betas.

### Added
- **Colors and gradients in the thumbnail gallery** - ten solid colors and
  ten soft radial gradients for a clean, professional look. The gallery is
  now grouped as Colors, Gradients, People, and Characters (the default
  AIs' portraits live in People and Characters now, not a separate group).

## [0.1.0-beta.7] - 2026-07-17

### Added
- **Kimi models join the online catalog.** Kimi K3 - Moonshot AI's new
  flagship with deep reasoning, a huge context window, and image
  understanding - plus the remarkably low-cost Kimi K2.6 and the dedicated
  Kimi K2.7 Code.
- **A better Online Models page.** Now matches the Offline Models layout:
  filter tabs (All, Chat, Web search, Coding) with model counts over a
  single grid, and a sort control - newest, name, or price. Each card shows
  which categories the model belongs to, and an "Auto pick" badge marks the
  models automatic routing already uses on your behalf.

### Changed
- A model can now appear in more than one category - Kimi K3 and GPT-5.6
  Sol show under both Chat and Coding, where they belong.

## [0.1.0-beta.6] - 2026-07-17

### Added
- **Medical models.** MedGemma 4B and 27B - Google's health-tuned models,
  both able to read images - join the model catalog, with a GPT-OSS 120B
  variant for high-memory machines. Medical models download after a short
  agree-to-the-publisher's-terms step, shown right in the download flow.
- **Health questions stay on your device.** In the online-and-offline Auto
  mode, a question about your health never auto-routes to an online model:
  it stays local and prefers your medical model if you have one - photos of
  skin, X-rays, and scans included. The "Model" note under the reply tells
  you when this happened.
- **"Remember this" everywhere.** Save any reply, highlighted passage, or
  transcript entry into memory: a Remember button under each reply, a
  floating chip when you highlight text in a reply, and a button under each
  entry on the memory page.
- **Remember is reversible, and you choose where saves go.** Every Remember
  button is a toggle - click "Remembered" to forget it again, even after a
  restart. A new Settings - Memory section picks the destination for each
  kind of save (kept to that AI only, or shared notes every AI can draw on)
  and gathers the automatic-learning switch.

### Fixed
- Downloading a vision model now says what's happening when it fetches the
  second file (the small projector that lets the model read images) instead
  of showing a second unlabeled progress bar.
- Image questions no longer stall when the preferred vision model is too
  large for your hardware - they fall back to an image-capable model that
  fits.
- Dropdowns opened near the bottom of the edit-AI dialog now scroll into
  view instead of being clipped.

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
