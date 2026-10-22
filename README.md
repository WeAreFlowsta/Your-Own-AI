# Your Own AI

Private AI on your machine. No one in control but you.

Your Own AI is a free desktop app for macOS, Windows, and Linux that runs
AI models privately on your computer. Your AIs, their personalities, their
memory of you, and every conversation live on your own devices. With an
offline model, nothing you type ever leaves the machine. And because the
app is open source, that claim is verifiable, not promised.

**[Download](https://yourownai.net/download/)** ·
**[Models](https://yourownai.net/models/)** ·
**[How it compares](https://yourownai.net/features/)** ·
**[Documentation](https://docs.yourownai.net)** ·
**[Releases](https://github.com/WeAreFlowsta/Your-Own-AI/releases)**

## What it does

- **Runs locally, on the machine you already have.** Download open
  models (GGUF) from a curated catalog with a fit check for your
  hardware, and chat with no account or internet for local use. Models
  larger than your graphics card still run: a mixture-of-experts model
  keeps its rarely used parts in main memory and the rest on the card,
  and the catalog carries one for every size of machine, from 8 GB
  laptops to big-memory workstations. GPU acceleration out of the box
  (Vulkan on Linux and Windows, Metal on macOS), an optional one-click
  CUDA engine for NVIDIA cards, and models can live on any drive.
- **Your AIs on every computer you own.** Sign in with the same free
  [Flowsta Vault](https://flowsta.com/vault/) on a second computer and
  your AIs, their pictures, your memory and your conversations arrive
  there, peer to peer between your own devices and encrypted with your
  own key. A conversation started on one computer appears on the other;
  a delete on one removes it from both. No server keeps a copy. See
  [Your other devices](https://docs.yourownai.net/data/devices).
- **Memory that knows what it is keeping.** Rules you set, facts about
  you, decisions, and what each work session left behind are kept as
  separate, dated entries. Say "from now on, keep it short" and every AI
  follows it. While the app is idle, a local model tidies your notes and
  shows you every change, with Undo. Each AI also has a memory of its
  own, and what every AI knows about you lives in one place you can read,
  edit, or forget. All of it stays on your devices.
- **Replies that keep going.** Ask, go anywhere in the app, and the
  answer is there when you come back. An activity tray lists what is
  still running and lets you answer an agent's question from any page.
- **Routing that measures instead of assumes.** Auto modes pick the model
  for each question by how models actually run on your computer, not a
  spec sheet. Every reply says which model answered, where, and why.
  Health questions stay on your device by policy, and the mode name is
  the consent: "Auto - Offline Only" never sends a question to an online
  model.
- **The web, per AI.** Each AI has its own switch for reading web pages,
  done on your computer. An agent asks before reading an address you did
  not give it.
- **Characters that are yours.** Author AIs from fifteen bundled
  personalities with their own portraits, memories, and knowledge, for
  personal or work use, then share them as single-file packs,
  cryptographically signed so anyone importing one can verify who made
  it. Free characters are on the
  [characters page](https://yourownai.net/uses/your-characters/).
- **Projects: agentic coding in chat.** Open a folder and your AI reads
  files, proposes edits as diffs, and runs commands, with a permission
  level you choose and every step recorded. A project keeps its own
  memory, the same on every computer. Powered by
  [Your Own AI Build](https://github.com/WeAreFlowsta/Your-Own-AI-Build),
  a free add-on downloaded in one click.
- **Tools and skills.** Your AIs can read and write your Obsidian or
  Logseq notes, keep a folder of documents in sync, and take on skills
  and tools as signed add-ons.
- **Documents and images, with proof.** Attach files (scanned paper
  included, via on-device OCR) and images (read by a vision model on your
  device). Check any answer claim by claim against the exact wording in
  the source with Verify sources.
- **Records you can prove.** Conversations are written into
  tamper-evident, cryptographically signed records (built on Holochain),
  private and encrypted on your device, from the moment you send. Export
  any conversation, and optionally sign it with your Flowsta identity so
  anyone can verify it at [flowsta.com](https://flowsta.com).
- **Online when you want it.** Frontier models from several providers and
  web search are an optional paid plan, signed in with your Flowsta
  identity, through a relay that strips your identity and never stores
  your messages. You choose which providers may answer and see where
  each is based; when one is busy, another takes the question; Settings
  shows where every answer came from and what it cost. Never required,
  never the default, and everything local stays free if you cancel.
- **An engine for other apps.** A local OpenAI-compatible endpoint serves
  your AIs, personality, memory, and records included, to editors, agent
  frameworks, and scripts. Or connect your own OpenAI-compatible server
  and its models join the picker.
- **A truth matrix.** Settings can check the app's own claims against
  your computer: every model loads at the size it promised, answers in
  plain words, and routes as the rules say. The report names nothing
  about you and is safe to share when asking for help.

## Requirements

- macOS 13.3 or later on Apple Silicon, macOS 12 or later on Intel (where
  offline models run on the processor), Windows 10/11 (64-bit), or a 64-bit
  Linux desktop (`.deb` or `.rpm`).
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
- llama.cpp (local inference), with EmbeddingGemma-300M as the memory
  component
- Holochain (signed, encrypted records; the private network between your
  own devices)
- [Your Own AI Build](https://github.com/WeAreFlowsta/Your-Own-AI-Build)
  (the agent behind Projects)

## Development

```bash
npm install
./download-llama-binaries.sh       # the inference engine sidecars
./download-holochain-binaries.sh   # the records sidecars
npm run tauri dev
```

`npm run build` runs the type check and the source checks before the
bundles. Rust tests run with `cargo test --lib` in `src-tauri/`; the UI
suites drive the real window (`npm run e2e`, `npm run e2e:models`, see
`e2e/README.md`). Releases are tagged `vX.Y.Z` (betas `vX.Y.Z-beta.N`)
and their notes come from [CHANGELOG.md](./CHANGELOG.md); see
`.github/workflows/build-release.yml` for the complete recipe.

## Feedback

Found a problem? Open an issue with the diagnostics file from Settings.
It carries the app log and your hardware summary, and nothing personal.
Betas go to a small group of testers first; every stable release is what
came out of their reports.

## License

AGPL-3.0-or-later - see [LICENSE](./LICENSE) and
[THIRD_PARTY_NOTICES](./THIRD_PARTY_NOTICES.md).
