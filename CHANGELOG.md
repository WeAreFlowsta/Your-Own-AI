# Changelog

All notable changes to Your Own AI are documented here. The release workflow
extracts the entry matching the pushed tag into the GitHub release notes.

## [Unreleased]

### Auto permissions for the agent (off by default)

- **New: your AI can run ordinary project work without asking** - reading
  and searching, editing files inside the project folder, building,
  testing, routine git. Anything that reaches beyond the folder, cannot
  be undone, or that it isn't sure about still asks, and the card says
  why. Off unless you turn it on: Settings > Agent sets the default for
  new projects; the project chip in the header switches it for one
  project (remembered per folder, applied live).
- Judged on your machine: the routine list is a rule-based check that
  never calls a model. A separate switch lets the AI's model judge grey
  areas - for an online AI that sends the command to the provider, so it
  is its own choice.
- **Every decision is in your records** - actions allowed automatically
  show as receipts on the rail (with which judge allowed them), and each
  turn keeps a compact ledger of everything the agent was allowed to do,
  written into your records with the turn. Permission answers now carry
  the decision, its scope, when and how it was answered; the app's own
  automatic allow of project-memory reads is recorded too.

### Fixes

- The hero "Continue" line shows its state the moment you click - a
  spinner while it opens, and the records-warming message when your
  records are still loading after launch - instead of nothing for up
  to ten seconds on a slow machine.
- Online chat turns send a per-conversation cache key, so consecutive
  turns reuse the provider's cache and bill at the cached-input rate;
  Online Models shows that rate.

## [0.4.1] - 2026-08-16

### Offline models load on every Windows install

- **Fixed: on Windows installs outside the C: drive, the bundled engine
  could not start, so offline models never loaded** - the app reached
  "Starting server with model" and went silent. The engine was located
  through a path resolver that fails on custom-drive installs; it is now
  found relative to the app itself, the same way the record-keeping
  engines already were. Thanks to the tester whose patient diagnostic
  reports over three days led us to it.
- Graphics-card probes are bounded: a driver that hangs while enumerating
  devices no longer stalls every model load - after 15 seconds the load
  continues on the processor.
- Every way a model load can fail is now named in the diagnostic report,
  including a server that starts and produces no output at all.

### Deleting a conversation works for every AI

- **Fixed: deleting a conversation from an AI's records did nothing** for AIs
  created after the app first learned to delete - new AIs, AIs restored
  from your Vault, and every fresh install. The ability is now given to
  each AI's records the moment they are set up, existing ones are brought
  up to date on the next launch, and a delete that fails says so instead
  of closing quietly.

### Your records, warming up honestly

- After launch, the Memory page, the conversations drawer, and the home
  screen's "Continue last conversation" all show that your records are
  still warming up instead of an empty list, a false zero, or a dead
  click - and open the moment they are ready.
- Warmup copy says "ready", not "online" - these records never leave your
  device.

### Attachments

- Attached documents appear as file chips in your message, never as their
  extracted text - in regular chats and in project sessions alike.
- Attachments stay on your device unless "Send attachments to online
  models" is on. The setting now lives in Settings > Routing, because it
  decides both where the message goes and what may leave the machine, and
  the one setting governs regular chats and project sessions.

### Agent sessions, seen clearly

- Background work is named and visible: condensing working notes,
  extracting memories, and scripts still running in the background stay in
  the rail - even across a stopped and restarted turn.
- The rail names the real tool behind an agent's action ("Remember
  something for this project") instead of a generic "Use Tool".
- Agent sessions start faster: the online model list is cached for a
  minute instead of being fetched on every step.

### Routing and models

- A paused model is out of automatic routing's reach until you resume it,
  and the pause and resume tooltips say exactly that.
- Signed-in online accounts stay signed in as the service rotates their
  sign-in tokens.

### Under the hood

- Rust dependencies audited and updated with the record-keeping crates
  left byte-identical; the dependency lockfile is now committed so every
  build is reproducible.
- Front-end build tooling updated; the development build targets the same
  JavaScript level as the production build.

## [0.4.0] - 2026-08-15

### Qwen 3.8-27B, offline with vision

- Alibaba's newest open model joins the offline catalog - frontier-class
  coding, math, and reasoning with built-in thinking, Apache-2.0 licensed.
- Hybrid attention keeps long documents fast, with a 262K context window.
- An optional vision add-on lets it see images you attach.
- Best on 24GB-class graphics cards, or 32GB RAM on the processor.

### Your records now work on macOS

- Fixed: on Mac, the record-keeping engine crashed the first time it set
  up an AI's conversation records - every macOS install was silently
  affected, and new chats were never saved to your records.
- The cause was missing security entitlements on our bundled engine; it
  now ships with the same set Holochain's own desktop apps use, and
  records set up in seconds on first launch (confirmed on Apple Silicon).
- If your Mac ended up in "Running on CPU for stability" after
  force-quitting the broken app, one click on "Try GPU again" restores
  full speed - and macOS now tolerates force-quits without tripping that
  safety net.
- Huge thanks to the tester whose codesign-level bug report led us
  straight to it.

### Switch models where you are

- The model name on every AI card is now a switcher - click it to change
  that AI's model in place.
- A quiet chip beside the chat's Ask row shows the current model
  arrangement and switches it in one tap (Settings > Appearance can hide
  it for a bare chat).
- Same choices everywhere: automatic routing modes, your offline models
  with their fit dots, online models, or your own connected server.

### Honest on every graphics card

- When a graphics driver can't actually run models, the app now detects
  it, switches to your processor automatically, and says so plainly - with
  a driver-update hint when one would genuinely help. "Model too large" is
  no longer misused for driver failures.
- The optional NVIDIA CUDA engine is only offered on cards it supports.
- Model recommendations, fit badges, and context sizing all plan for the
  processor when the graphics card is out of play.
- Windows engine downloads are now code-signed.

### Under the hood

- Inference engine updated to llama.cpp b10435 - fixes Muse Glimmer
  occasionally losing a trailing tool call in agent work, plus sharper
  tool-call parsing for Qwen models.
- Automatic model picks no longer grab a bigger model while your current
  one is still loading - fit is judged as if the slot were free, so
  balanced routing stays balanced.
- On slow or busy machines, record setup now waits out the storage
  engine's warmup instead of showing "records couldn't be set up" too
  early.
- Diagnostic reports on macOS now include the actual crash cause from
  system crash records.

## [0.3.0] - 2026-08-13

### Muse Glimmer 30B, on your own hardware

- Meta's Muse Glimmer 30B joins the offline catalog - chat, image
  understanding, and real agent work in your project folders, running
  entirely on your device.
- Works on NVIDIA and AMD consumer graphics cards (optional
  high-performance NVIDIA engine available in-app).
- Inference engine updated to llama.cpp b10355.
- Reasoning strength adapts to the question - quick answers stay quick,
  hard problems get deep thinking.

### Bring your history

- Import your conversations from ChatGPT, Claude, and Perplexity exports.
- Import coding sessions from Claude Code, OpenCode, Codex, Cursor, and
  Aider - most auto-detected with one click.
- Adopted conversations keep their original dates, and your AIs remember
  what's in them.
- Your Memory page: filter learned facts by import, forget any batch in
  one click.

### Online models

- Grok 4.6 and Grok 4.6 (Web) available the day they launched - with
  support for the restored reasoning dial.
- DeepSeek V4 now serves its full 1M context; catalog shows every model's
  real context and release date.
- Online chats stream noticeably faster - connection reuse, lighter checks
  before the first token.
- Model cards show which shelf each model belongs to (chat, web search,
  coding) and sort correctly by newest.

### Smarter on your hardware

- Context size is now chosen from your graphics card and memory together -
  agent sessions get the room they need (fixes project work dying at 8K
  context on 32GB machines).
- Downloaded model cards show how each model fits this machine: full
  speed, runs slower, or too large - plus trained context vs what it runs
  at here.
- Auto model picks are fit-aware end to end: a model that struggles on
  your hardware hands off to one that runs at full speed, except while a
  project is open so your session's model stays warm.

### Diagnostics and reliability

- One-click diagnostic report (Settings > Help & diagnostics) - system,
  models, routing decisions, crash records; redacted, saved locally, never
  uploaded; also copyable straight to the clipboard.
- If the app ever gets stuck starting, the loading screen offers to save
  the same report - no working app required.
- The report lists every model file's health, so a damaged download can't
  hide.
- One app instance, enforced - a second launch or a leftover process can
  no longer break startup.
- Faster startup on machines where the record-keeping engine takes time
  to warm up.
- The NVIDIA engine and the Your Own AI Build agent both gained proper
  update flows for future releases.
- Copy buttons work reliably on Windows everywhere in the app.
- Windows installer is code-signed; an MSI package is now published
  alongside it.

## [0.2.0] - 2026-08-06

### Changed
- **Projects are dramatically faster.** Model metadata is now cached
  instead of re-read on every step - agent steps that took close to a
  minute on modest hardware now land in seconds, and opening a project
  is near-instant.
- **GPT-5.6 Sol drives project work by default**, and your own model
  picks from Settings now apply to project sessions too (they were
  silently ignored before).
- **Model choices respect your hardware honestly.** Recommendations
  count the memory that's actually free, integrated graphics is sized
  as what it is (with a truthful "Integrated graphics" card badge), and
  automatic picks prefer a model that runs comfortably over a smarter
  one that barely loads.
- **Privacy-first routing means what it says.** With the online lean
  set to privacy-first, hard questions stay on your device and only
  genuine live-web needs go online.
- Gemma 4 downloads use Google's official builds carrying their July
  refresh (better tool use, wider vision); the larger variants now
  show their true 256K context. Existing Gemma downloads keep working -
  re-download to get the refresh.
- The app opens at a roomier default size, the Your AIs cards say
  "Edit" where before there was only an icon, and every
  project-readiness notice says exactly what to look for offline: a
  model marked "Agentic" on the Offline Models page.

### Added
- **See your plan usage in Settings** - spend against your monthly
  allowance, with an optional live ticker in the header (off by
  default).
- **System Information with "Copy for support"** - everything that
  determines model fit, copyable in one click, with nothing personal
  included.
- **Project sessions warn before they can't work**: opening a folder
  with an offline-only AI now tells you up front if no downloaded
  model can drive project work, or if the capable one won't fit
  comfortably on your hardware.
- **Only one copy of the app runs at a time** - launching it again
  focuses the open window instead of starting a second instance that
  would fight the first over your models and graphics memory.
- Older log files are kept when the log rotates, so a problem's
  history survives an app restart.

### Fixed
- **Editing your selected AI now applies immediately.** Before, an
  open chat kept the AI's previous settings until you switched away -
  including its online/offline mode, which could route a message
  online after you had chosen Offline Only. Fixed, and this class of
  routing promise is now verified automatically across nearly two
  thousand setting combinations before each release.
- A model that crashed the app while loading is never automatically
  loaded again on the next start - no more crash loops that required
  deleting files by hand.
- A model too slow to load says so clearly and isn't retried that
  session, instead of failing with a raw error code.
- Models whose chat format can't support project work are no longer
  offered for it, whatever their name suggests.
- The "permission needed" button scrolls to the permission card
  instead of past it.

## [0.1.1] - 2026-08-04

### Fixed
- **Offline models load for every Windows user name.** On Windows accounts
  whose user folder contains non-ASCII characters (u with umlaut, Turkish
  letters, and friends), every offline model failed to load with a
  misleading "too large for your computer's memory" message. Model files
  are now opened in a way that works for every profile path, and a file
  that genuinely cannot be opened says so instead of blaming memory.
- **Restoring your Vault key works reliably on Windows.** Adopting the key
  from your Flowsta Vault could leave the app without a working
  conversation engine until reinstall. The restore now stops everything
  cleanly first, verifies the old state is fully released before switching
  keys, and aborts safely - changing nothing - when it cannot.
- Factory reset and app exit fully stop the background conversation engine
  on Windows.

### Added
- **Backups tell you when they are waiting.** If a backup attempt is held
  or fails - for example your Vault was just restored and wants its export
  imported first - the Flowsta Account section says so in plain words,
  with what to do next. Silence no longer looks like success.
- **Key recovery reads as one story.** When your Vault holds a different
  conversation key than this device, Backups & recovery walks you through
  it as two labeled steps: restore the key, then restore the
  conversations.

## [0.1.0] - 2026-07-29

The first stable release of Your Own AI - private AI on your machine.
No one in control but you.

- **Yours, offline.** Author AIs with their own personalities, portraits,
  memories, and knowledge. Chat with open models from the built-in
  catalog, GPU accelerated, with no account and nothing leaving your
  device.
- **Documents, with proof.** Attach files - scanned paper included, read
  on-device - and check any answer claim by claim against the exact
  wording in the source.
- **Projects.** Open a folder and your AI reads files, proposes edits,
  and runs commands, asking permission for every action - powered by the
  free Your Own AI Build add-on.
- **Memory you can read.** A shared profile, per-AI memories, and project
  memory - every entry visible, editable, and deletable.
- **Records you can prove.** Conversations are written into
  tamper-evident, signed records on your device. Export any
  conversation, and optionally sign it with your Flowsta identity.
- **Backed up, recoverable.** With Flowsta Vault connected, every
  conversation, AI, and memory backs up automatically and restores on a
  new device.
- **Share your AIs.** Export any AI as a signed pack others can import
  and verify - eight free characters are on yourownai.net.
- **Online when you want it.** Optional paid plans add frontier models
  and web search through a relay that strips your identity and never
  stores your messages. Everything local stays free, always.
- **An engine for other apps.** A local OpenAI-compatible endpoint
  serves your AIs - personality, memory, and records included - to
  editors, agent frameworks, and scripts, on this computer or your
  network.
- Installers are code signed on Windows and signed and notarized on
  macOS.

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
