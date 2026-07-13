# Your Own AI

An offline-first AI desktop app. Your AIs, their personalities, their memory
of you, and every conversation live on your device. No one in control but you.

- **Runs locally.** Download open models (GGUF) and chat with no account, no
  internet after setup. GPU-accelerated out of the box (Vulkan on Linux and
  Windows, Metal on Apple Silicon).
- **AIs that know you.** Customizable personalities with persistent memory -
  facts you tell them, things they learn, knowledge you author for them -
  encrypted on your device.
- **Records you can prove.** Conversations are recorded to a tamper-evident,
  cryptographically signed store (built on Holochain), private and encrypted
  on your device. Exports can be signed with your Flowsta identity so anyone
  can verify them.
- **Backed up, recoverable.** With a [Flowsta Vault](https://flowsta.com/vault)
  connected, your whole AI world backs up automatically and restores to a new
  device with one click.
- **Extendable, by consent.** Optional add-ons download only when you choose
  them: memory recall, scanned-document OCR, vision, an on-device utility
  model, and hardware-specific engines (a CUDA engine for NVIDIA cards).
  Connect your own OpenAI-compatible server and its models join the picker.
- **Online when you want it.** Frontier cloud models are available through a
  privacy-preserving proxy with a paid plan - never required, never default.
- **An engine for other apps.** A local OpenAI-compatible inference endpoint
  lets your other tools use your AIs and models.

## Stack

- Tauri v2 (Rust backend)
- Qwik (TypeScript frontend)
- Tailwind CSS
- llama.cpp (local inference)
- Holochain (signed, encrypted conversation records)

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
