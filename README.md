# Your Own AI

Private AI on your machine. No one in control but you.

Your Own AI is a free desktop app for macOS, Windows, and Linux that runs
AI models privately on your device. Your AIs, their personalities, their
memories of you, and every conversation live on your machine - with an
offline model, nothing you type ever leaves it. And because the app is
open source, that claim is verifiable, not promised.

**[Download](https://yourownai.net/download/)** ·
**[Models](https://yourownai.net/models/)** ·
**[Website](https://yourownai.net)** ·
**[Documentation](https://docs.yourownai.net)**

## What it does

- **Runs locally, on the machine you already have.** Download open
  models (GGUF) from a curated catalog with a fit check for your
  hardware, and chat with no account or internet for local use. Models
  larger than your graphics card still run: a mixture-of-experts model
  keeps its rarely used parts in main memory and the rest on the card,
  and the catalog carries one for every size of machine, from 12 GB
  laptops to big-memory workstations. GPU acceleration out of the box
  (Vulkan on Linux and Windows, Metal on macOS), an optional one-click
  CUDA engine for NVIDIA cards, and models can live on any drive. Every
  model, offline and online, has its own page at
  [yourownai.net/models](https://yourownai.net/models/).
- **Memory that knows you as a whole.** Each AI has one memory - what you
  have given it and what it has learned with you - and everything your
  AIs know about you lives in one place. On top of it, your AIs keep a
  short summary of who you are, written on your own device by a local
  model and rewritten as things change. Every fact is yours to read, edit,
  or forget, traceable to the conversation it came from; if the summary
  is wrong, fix the fact and it rewrites itself. None of it leaves your
  machine.
- **Routing that measures instead of assumes.** Auto modes pick the model
  for each question by how models actually run on your computer - speed
  measured from real use, room for the turn, what the question needs -
  not a spec sheet. Online models follow the preferences you set. Every
  reply says which model answered and why, health questions stay on your
  device by policy, and the mode name is the consent: "Auto - Offline
  Only" never touches the internet.
- **Characters that are yours.** Author AIs from 18 personalities with
  their own portraits, memories, and knowledge - for personal or work
  use - then share them as single-file packs, cryptographically signed so
  anyone importing one can verify who made it. Eight free characters are
  on the [characters page](https://yourownai.net/uses/your-characters/).
- **Projects: agentic coding in chat.** Open a folder and your AI reads
  files, proposes edits as diffs, and runs commands - with a permission
  level you choose, and every step recorded. Powered by
  [Your Own AI Build](https://github.com/WeAreFlowsta/Your-Own-AI-Build),
  a free add-on downloaded in one click.
- **Documents and images, with proof.** Attach files (scanned paper
  included, via on-device OCR) and images (read by a vision model on your
  device). A long document gets the room it needs on your machine when
  your machine can give it, and an explicit offer when only an online
  model can. Check any answer claim-by-claim against the exact wording
  in the source with Verify sources.
- **Records you can prove.** Conversations are written into
  tamper-evident, cryptographically signed records (built on Holochain),
  private and encrypted on your device, from the moment you send - a
  stopped reply is kept and marked. Export any conversation, and
  optionally sign it with your Flowsta identity so anyone can verify it
  at [flowsta.com](https://flowsta.com).
- **Backed up, recoverable.** With a
  [Flowsta Vault](https://flowsta.com/vault) connected, everything backs
  up automatically - each conversation as its own compressed object, so
  no session is too large to protect - and restores to a new device.
- **Online when you want it.** Frontier models and web search are an
  optional paid service, signed in with your Flowsta identity, through a
  relay that strips your identity and never stores or logs your
  messages. Never required, never default - and everything local stays
  free if you cancel.
- **An engine for other apps.** A local OpenAI-compatible endpoint serves
  your AIs - personality, memory, and records included - to editors,
  agent frameworks, and scripts. Or connect your own OpenAI-compatible
  server and its models join the picker.

## Requirements

- macOS 10.15 or later (Apple Silicon or Intel), Windows 10/11 (64-bit), or
  a 64-bit Linux desktop (`.deb` or `.rpm`).
- 8 GB of memory runs the small models; 16 GB and a graphics card with
  6 GB or more opens up most of the catalog; 32 GB carries the large
  mixture-of-experts models with the graphics card doing part of the
  work. The [models pages](https://yourownai.net/models/) check any
  model against your machine before you download it, and the app grades
  every model the same way once installed.

## Stack

- Tauri v2 (Rust backend)
- Qwik (TypeScript frontend)
- Tailwind CSS
- llama.cpp (local inference)
- Holochain (signed, encrypted conversation records)
- [Your Own AI Build](https://github.com/WeAreFlowsta/Your-Own-AI-Build)
  (the agent behind Projects)

## Development

```bash
npm install
./download-llama-binaries.sh   # fetches the inference engine sidecars
npm run tauri dev
```

Rust tests run with `cargo test --lib` in `src-tauri/`. Building the full
app also bundles Holochain sidecar binaries and pdfium; see
`.github/workflows/build-release.yml` for the complete recipe. Releases
are tagged `vX.Y.Z` (betas `vX.Y.Z-beta.N`) and their notes come from
[CHANGELOG.md](./CHANGELOG.md).

## Feedback

Found a problem? Open an issue with the diagnostics file from Settings -
it carries the app log and your hardware summary, and nothing personal.
Betas go to a small group of testers first; every stable release is what
came out of their reports.

## License

AGPL-3.0-or-later - see [LICENSE](./LICENSE) and
[THIRD_PARTY_NOTICES](./THIRD_PARTY_NOTICES.md).
