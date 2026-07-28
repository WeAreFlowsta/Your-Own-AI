# Your Own AI

Private AI on your machine. No one in control but you.

Your Own AI is a free desktop app for macOS, Windows, and Linux that runs
AI models privately on your device. Your AIs, their personalities, their
memories of you, and every conversation live on your machine - with an
offline model, nothing you type ever leaves it. And because the app is
open source, that claim is verifiable, not promised.

**[Download](https://yourownai.net/download/)** ·
**[Website](https://yourownai.net)** ·
**[Documentation](https://docs.yourownai.net)**

## What it does

- **Runs locally.** Download open models (GGUF) from a curated catalog
  with hardware-fit guidance, and chat with no account and no internet
  after setup. GPU-accelerated out of the box (Vulkan on Linux and
  Windows, Metal on macOS), with an optional one-click CUDA engine for
  NVIDIA cards.
- **Characters that are yours.** Author AIs from 18 personalities with
  their own portraits, memories, and knowledge - then share them as
  single-file packs, cryptographically signed so anyone importing one can
  verify who made it. Eight free characters are on the
  [characters page](https://yourownai.net/uses/your-characters/).
- **Projects: agentic coding in chat.** Open a folder and your AI reads
  files, proposes edits as diffs, and runs commands - asking permission
  for every action, with every step recorded. Powered by
  [Your Own AI Build](https://github.com/WeAreFlowsta/Your-Own-AI-Build),
  a free add-on downloaded in one click.
- **Documents, with proof.** Attach files (scanned paper included, via
  on-device OCR) and check any answer claim-by-claim against the exact
  wording in the source with Verify sources.
- **Memory you can read.** A shared profile, each AI's own memory, and
  per-project memory - every entry visible, editable, and deletable.
  Nothing hidden.
- **Records you can prove.** Conversations are written into
  tamper-evident, cryptographically signed records (built on Holochain),
  private and encrypted on your device. Export any conversation, and
  optionally sign it with your Flowsta identity so anyone can verify it
  at [flowsta.com](https://flowsta.com).
- **Backed up, recoverable.** With a
  [Flowsta Vault](https://flowsta.com/vault) connected, everything backs
  up automatically - each conversation as its own compressed object, so
  no session is too large to protect - and restores to a new device.
- **Smart routing, in plain language.** Auto modes pick the right model
  per question by rules you can read: health questions stay on your
  device by policy, current events can use web search with cited
  sources, and every reply says which model answered and why. The mode
  name is the consent - "Auto - Offline Only" never touches the internet.
- **Online when you want it.** Frontier models and web search are
  available with a paid plan, through a relay that strips your identity
  and never stores or logs your messages. Never required, never default -
  and everything local stays free if you cancel.
- **An engine for other apps.** A local OpenAI-compatible endpoint serves
  your AIs - personality, memory, and records included - to editors,
  agent frameworks, and scripts. Or connect your own OpenAI-compatible
  server and its models join the picker.

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

Building the full app also bundles Holochain sidecar binaries and pdfium;
see `.github/workflows/build-app-installers.yml` for the complete recipe.

## License

AGPL-3.0-or-later — see [LICENSE](./LICENSE) and
[THIRD_PARTY_NOTICES](./THIRD_PARTY_NOTICES.md).
