/**
 * Recommended AI Models Catalog
 *
 * Curated list of models available for download from Hugging Face,
 * organized by family with multiple size variants. Each variant's flat
 * fields are its GGUF artifact (what the llama.cpp engines run); other
 * engine formats slot in per-variant via `artifacts` (see EngineFormat /
 * artifactFor) without touching this file's structure.
 *
 * Last updated: 2026-07-15
 */

/**
 * Engine formats — which artifact a variant provides per engine family.
 * Every engine today (the bundled Vulkan/Metal llama.cpp and the optional
 * CUDA build) runs `gguf`; future engines bring their own formats (MLX
 * quantized safetensors on Apple Silicon, HF safetensors for server-grade
 * backends). Only `gguf` exists in the catalog today.
 */
export type EngineFormat = 'gguf' | 'mlx' | 'safetensors';

/** One downloadable artifact of a model variant, in one engine format. */
export interface ModelArtifact {
  filename: string;
  downloadUrl: string;
  sizeGb: number;
  /** Source repo for repo-shaped formats (mlx / safetensors). */
  hfRepo?: string;
  quantization?: string;
}

export interface ModelVariant {
  parameterCount: string;  // e.g., "3B", "7B"
  // The flat fields below ARE the canonical GGUF artifact (authored inline
  // for every variant). Read them through `artifactFor(variant, format)` in
  // engine-aware code — never directly in NEW code — so adding a second
  // format is catalog data, not a refactor.
  size: number;            // GGUF download size in GB
  minRAM: number;          // Minimum RAM required in GB
  /** Context window when it differs from the family's (e.g. Gemma 4's
   *  larger variants carry 256K while E2B/E4B are 128K). */
  contextWindow?: number;
  downloadUrl: string;
  filename: string;
  quantization: string;    // e.g., "Q4_K_M"
  /** Additional per-format artifacts (mlx, safetensors) — additive; a
   *  variant without an entry for the active engine's format simply isn't
   *  offered on that engine. */
  artifacts?: Partial<Record<EngineFormat, ModelArtifact>>;
}

/**
 * The artifact a variant provides in the given format. GGUF derives from
 * the variant's canonical flat fields; other formats come from `artifacts`.
 * Returns null when the variant has nothing for that format.
 */
export function artifactFor(
  v: ModelVariant,
  format: EngineFormat = 'gguf',
): ModelArtifact | null {
  if (v.artifacts?.[format]) return v.artifacts[format] ?? null;
  if (format === 'gguf') {
    return {
      filename: v.filename,
      downloadUrl: v.downloadUrl,
      sizeGb: v.size,
      quantization: v.quantization,
    };
  }
  return null;
}

/**
 * Which artifact format the ACTIVE chat engine consumes. Both engines today
 * (bundled Vulkan/Metal, optional CUDA) are llama.cpp → gguf. When an
 * engine with a different model format lands (MLX, server backends), this
 * branches on the engine registry — and the download/selection UIs resolve
 * variants through `artifactFor(v, activeEngineFormat())`.
 */
export function activeEngineFormat(): EngineFormat {
  return 'gguf';
}

/**
 * Capability vocabulary — the controlled set of tasks a model is good at.
 * This is the routing signal (a future router maps task → capabilities → model)
 * AND drives the "good for" chips in the UI. One source of truth for both. Keep
 * this list small and curated; don't add freeform strings.
 */
export type Capability =
  | 'chat'
  | 'coding'
  | 'reasoning'
  | 'writing'
  | 'analysis'
  | 'math'
  | 'medical'
  | 'multilingual'
  | 'long-context'
  | 'agentic';

/**
 * Modality — input/output types a model handles. Everything is text→text today;
 * this axis is where vision / voice / image-gen / video-gen models slot in later
 * with no schema change (and where the page grows modality tabs).
 */
export type Modality = 'text' | 'vision' | 'voice' | 'image' | 'video';

/**
 * Traits — meaningful non-capability attributes worth a badge. Not "what it's
 * good at" (Capability) but "what kind of model it is".
 */
export type Trait = 'new' | 'thinking' | 'uncensored' | 'moe' | 'distilled';

/**
 * Role — what the model is FOR, which decides where it's managed in the UI.
 * `chat` (default) = a brain you converse with, browsed on the offline-models
 * page. Everything else is a capability dependency (downloaded on demand,
 * managed in Settings → Components), never shown in the chat picker.
 */
export type ModelRole = 'chat' | 'embedding' | 'voice-tts' | 'voice-stt' | 'vision-projector' | 'ocr' | 'utility';

export interface ModelFamily {
  id: string;              // e.g., "qwen-3.6"
  released?: string;       // ISO date the model became available (HF repo createdAt); drives the "Newest" sort
  contextWindow?: number;  // Trained max context in tokens (GGUF `context_length` via the HF API). Family-level — all variants share it. NB: YOAI loads at a RAM-capped runtime context, so this is the model's capability, not what it always runs at.
  name: string;            // Display name, e.g., "Qwen 3.6"
  description: string;     // One-line summary
  category: 'fast' | 'balanced' | 'quality' | 'specialist';
  recommended: boolean;    // Surfaced as a recommendation (welcome screen, "best for you")
  capabilities: Capability[]; // Routing signal + "good for" chips, most-relevant first
  traits: Trait[];         // Surfaced badges
  modality?: { in: Modality[]; out: Modality[] }; // Defaults to text→text (see getModality)
  /** Provenance, shown on the card. `maker` = who made the weights.
   *  `quantizedBy` = who packaged the UNMODIFIED weights we download
   *  (format conversion, not modification). `derivedFrom` = for
   *  derivatives: what this model is based on and how it differs.
   *  `community` marks third-party builds (vs the maker's own work). */
  maker?: string;
  quantizedBy?: string;
  derivedFrom?: string;
  community?: boolean;
  role?: ModelRole;        // Defaults to 'chat' (see getRole). Non-chat = a capability component.
  /** Some model publishers require users to accept terms before download
   *  (e.g. Google's Health AI Developer Foundations). When set, the download
   *  button shows an agreement step first; acceptance is stored per license
   *  id, so sibling variants/models under the same license ask only once.
   *  The `notice` line is the publisher's REQUIRED redistribution notice -
   *  show it verbatim. */
  license?: {
    id: string;
    name: string;
    url: string;
    notice: string;
    points: string[];
  };
  variants: ModelVariant[]; // Size options, sorted small to large
}

/** A downloadable capability model (embedding, voice, …) — the on-demand
 *  components that keep the base install lean. Simpler than a chat ModelFamily
 *  (no capabilities/traits/variants browsing); just one file to fetch. */
export interface CapabilityModel {
  id: string;
  name: string;
  role: ModelRole;
  description: string;
  filename: string;
  downloadUrl: string;
  size: number;       // GB (total across `files` if multi-file)
  /** Multi-file components (e.g. OCR needs two models). When set, the card
   *  installs/removes all of them; `filename`/`downloadUrl` mirror files[0]. */
  files?: { filename: string; downloadUrl: string }[];
  /** Filenames this component previously shipped under. THE CONVENTION:
   *  whenever `filename`/`files` change, move every replaced name in here.
   *  That is what turns the Components card into an "Update" offer for
   *  people who installed the old one (instead of repeating the
   *  first-install pitch) and lets the update delete the superseded file.
   *  Leave a name out and those users see "Download" for a thing they
   *  already downloaded, and keep both copies on disk. */
  previousFilenames?: string[];
}

/**
 * The embedding model behind memory recall. Defined here — NOT in `modelFamilies`
 * — so it downloads via Settings → Components and never appears in the chat
 * picker. bge-small-en-v1.5: small (384-dim, 512-ctx), runs in llama.cpp
 * `--embedding --pooling cls`. Chosen over nomic-embed after a calibration test
 * (2026-06-21): bge cleanly separates unrelated queries (max ~0.37) from real
 * matches (~0.58+), where nomic's scores clustered ~0.46–0.63 and no threshold
 * worked. Swappable (the index is rebuildable) — but a swap must also update the
 * Rust pooling flag (`ensure_embedding_server`) and the query/doc prefixes
 * (`embeddings.ts`). URL verified to resolve 2026-06-21 (67 MB).
 *
 * ⚠️ A swap is a MIGRATION, not just a new download: every per-AI memory
 * index holds THIS model's vectors, and recall silently degrades to noise
 * under a different one. Shipping a replacement means adding the old
 * filename to `previousFilenames` AND wiring a full re-embed (the rebuild
 * walker in transcriptMemory is the starting point).
 */
export const EMBEDDING_MODEL: CapabilityModel = {
  id: 'bge-small-en-v1.5',
  name: 'Memory & smart routing',
  role: 'embedding',
  description:
    'A small local model two features share: memory recall (each AI recalling relevant past conversations and notes) and smart routing (spotting when a question needs an online model). Runs on your device.',
  filename: 'bge-small-en-v1.5-f16.gguf',
  downloadUrl:
    'https://huggingface.co/CompendiumLabs/bge-small-en-v1.5-gguf/resolve/main/bge-small-en-v1.5-f16.gguf',
  size: 0.067, // ~67 MB
};

/**
 * The on-device GENERATIVE utility model behind fact extraction + report/code
 * mode detection. Optional (Settings → Components): when absent, those features
 * ride the active chat/online model; installing it keeps them fully on-device
 * even while chatting with online AIs (its own CPU server, llm.rs UTIL_PORT 8092).
 * Ministral-3B chosen by measurement (2026-06-28): it holds the extraction
 * precision (11/11) + classifier (14/15) where a 1.5B model collapses (6/11, 9/15).
 * Same GGUF as the Ministral-3 chat family — shared on disk if also a chat brain.
 */
export const UTILITY_MODEL: CapabilityModel = {
  id: 'ministral-3-utility',
  name: 'On-device extraction & smart modes',
  role: 'utility',
  description:
    'A small local model that learns your memory facts and spots when a question wants a report or code — kept fully on your device, even while chatting with online AIs.',
  filename: 'Ministral-3-3B-Instruct-2512-Q4_K_M.gguf',
  downloadUrl:
    'https://huggingface.co/unsloth/Ministral-3-3B-Instruct-2512-GGUF/resolve/main/Ministral-3-3B-Instruct-2512-Q4_K_M.gguf',
  size: 2.2,
};

/**
 * Multimodal projectors (mmproj) — the vision tower that lets an already-downloaded
 * chat model SEE attached images. Downloaded via Settings → Components, paired with
 * its model family at server start (`--mmproj`). A projector is a capability
 * dependency, never a chat model, so it's excluded from the model picker (see
 * `modelManager.listModels`). Gemma 4 E2B/E4B are natively multimodal — this is
 * their vision tower. URL verified to resolve 2026-06-25 (~990 MB, F16). The local
 * filename is namespaced (not the repo's bare `mmproj-F16.gguf`) so projectors for
 * different families never collide.
 */
export const VISION_PROJECTORS: CapabilityModel[] = [
  {
    id: 'qwen-3.8-vision',
    name: 'Qwen 3.8 vision',
    role: 'vision-projector',
    description:
      'Lets Qwen 3.8 see images you attach - screenshots, charts, documents. Needs the Qwen 3.8 model downloaded.',
    // Saved under OUR model-first name (upstream is mmproj-first):
    // key "qwen3.8-27b" prefixes the model filename for pairing.
    filename: 'Qwen3.8-27B-mmproj-F16.gguf',
    downloadUrl:
      'https://huggingface.co/unsloth/Qwen3.8-27B-GGUF/resolve/main/mmproj-F16.gguf',
    size: 0.93,
  },
  {
    id: 'ornith-1.5-vision',
    name: 'Ornith 1.5 vision',
    role: 'vision-projector',
    description:
      'Lets Ornith 1.5 35B see images you attach - screenshots, charts, documents. Needs the Ornith 1.5 35B model downloaded.',
    // Saved under OUR model-first name (upstream is mmproj-first):
    // key "ornith-1.5-35b" prefixes the model filename for pairing.
    filename: 'Ornith-1.5-35B-mmproj-BF16.gguf',
    downloadUrl:
      'https://huggingface.co/ornith-ai/Ornith-1.5-35B-A3B-GGUF/resolve/main/mmproj-Ornith-1.5-35B-BF16.gguf',
    size: 0.9,
  },
  {
    id: 'qwen-3.8-uncensored-vision',
    name: 'Qwen 3.8 Uncensored vision',
    role: 'vision-projector',
    description:
      'Lets Qwen 3.8 Uncensored see images you attach. Needs the Qwen 3.8 Uncensored model downloaded.',
    // Saved under OUR model-first mmproj name (upstream calls it "vision"):
    // key "qwen3.8-27b-uncensored" prefixes the model filename for pairing.
    filename: 'Qwen3.8-27B-Uncensored-mmproj-F16.gguf',
    downloadUrl:
      'https://huggingface.co/JonathanColetti/Qwen3.8-27B-Uncensored-GGUF/resolve/main/Qwen3.8-27B-Uncensored-vision-f16.gguf',
    size: 0.93,
  },
  {
    id: 'muse-glimmer-vision',
    name: 'Muse Glimmer vision',
    role: 'vision-projector',
    description:
      'Lets Muse Glimmer see images you attach — screenshots, charts, documents. Needs the Muse Glimmer model downloaded.',
    // Saved under OUR model-first name: the projector pairing matches by
    // "<key>-mmproj" prefixing the model filename; Unsloth's mmproj-first
    // upstream name would never pair.
    filename: 'Muse-Glimmer-30B-mmproj-Q8_0.gguf',
    downloadUrl:
      'https://huggingface.co/unsloth/Muse-Glimmer-30B-GGUF/resolve/main/mmproj-Muse-Glimmer-30B-Q8_0.gguf',
    size: 2.1,
  },
  {
    id: 'gemma-4-e2b-vision',
    name: 'Gemma 4 vision (E2B)',
    role: 'vision-projector',
    description:
      'Lets the Gemma 4 E2B model see images you attach — diagrams, screenshots, photos. The lighter option; needs the Gemma 4 E2B model downloaded.',
    filename: 'gemma-4-E2B-mmproj-F16.gguf',
    downloadUrl:
      'https://huggingface.co/unsloth/gemma-4-E2B-it-GGUF/resolve/main/mmproj-F16.gguf',
    size: 0.99, // ~986 MB
  },
  {
    id: 'gemma-4-e4b-vision',
    name: 'Gemma 4 vision (E4B)',
    role: 'vision-projector',
    description:
      'Lets the Gemma 4 E4B model see images you attach — diagrams, screenshots, photos. Needs the Gemma 4 E4B model downloaded.',
    filename: 'gemma-4-E4B-mmproj-F16.gguf',
    downloadUrl:
      'https://huggingface.co/google/gemma-4-E4B-it-qat-q4_0-gguf/resolve/main/gemma-4-E4B-it-mmproj.gguf',
    size: 0.99, // ~990 MB
  },
  {
    id: 'medgemma-4b-vision',
    name: 'MedGemma 4B vision',
    role: 'vision-projector',
    description:
      'Lets MedGemma 4B see medical images you attach - X-rays, skin photos, scans. Needs the MedGemma 4B model downloaded.',
    filename: 'medgemma-1.5-4b-it-mmproj-F16.gguf',
    downloadUrl:
      'https://huggingface.co/unsloth/medgemma-1.5-4b-it-GGUF/resolve/main/mmproj-F16.gguf',
    size: 0.79,
  },
  {
    id: 'medgemma-27b-vision',
    name: 'MedGemma 27B vision',
    role: 'vision-projector',
    description:
      'Lets MedGemma 27B see medical images you attach - X-rays, skin photos, scans. Needs the MedGemma 27B model downloaded.',
    filename: 'medgemma-27b-it-mmproj-F16.gguf',
    downloadUrl:
      'https://huggingface.co/unsloth/medgemma-27b-it-GGUF/resolve/main/mmproj-F16.gguf',
    size: 0.8,
  },
  {
    id: 'qwythos-9b-vision',
    name: 'Qwythos 9B vision',
    role: 'vision-projector',
    description:
      'Lets the Qwythos 9B model see images you attach — diagrams, screenshots, photos. Needs the Qwythos 9B model downloaded.',
    // Filename must prefix the model file so the conductor pairs them (find_projector_for).
    filename: 'Qwythos-9B-v2-mmproj-BF16.gguf',
    downloadUrl:
      'https://huggingface.co/empero-ai/Qwythos-9B-v2-GGUF/resolve/main/mmproj-Qwythos-9B-v2-BF16.gguf',
    size: 0.9, // ~920 MB
  },
];

/**
 * OCR for scanned PDFs. Two ocrs models (pure-Rust, RTen): a text-detection and a
 * text-recognition `.rten`. Downloaded on demand; pdfium (the page renderer) is
 * bundled with the app. Models are CC-BY-SA-4.0 — attributed in
 * THIRD_PARTY_NOTICES.md; we download (not redistribute) them. ~30 MB total (confirm exact sizes).
 */
export const OCR_MODELS: CapabilityModel = {
  id: 'ocrs-scanned-pdf',
  name: 'Scanned-document OCR',
  role: 'ocr',
  description:
    'Reads text from scanned or photographed PDFs, so they can be summarised, searched, and source-grounded like any other document. Runs entirely on your device.',
  filename: 'text-detection.rten',
  downloadUrl: 'https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten',
  size: 0.03, // ~30 MB total across both files
  files: [
    {
      filename: 'text-detection.rten',
      downloadUrl: 'https://ocrs-models.s3-accelerate.amazonaws.com/text-detection.rten',
    },
    {
      filename: 'text-recognition.rten',
      downloadUrl: 'https://ocrs-models.s3-accelerate.amazonaws.com/text-recognition.rten',
    },
  ],
};

export const modelFamilies: ModelFamily[] = [
  // ─── Recommended / Balanced ────────────────────────────────────────────
  {
    // Meta's agent-first local model (2026-08-10, Apache 2.0). EARLY
    // SUPPORT: llama.cpp landed the architecture the day it released;
    // treat field reports accordingly. Template verified tool-capable +
    // blessed (gguf.rs test); reasoning_strength dial wired in llm.rs.
    id: 'muse-glimmer',
    maker: 'Meta',
    quantizedBy: 'Unsloth',
    contextWindow: 131072,
    released: '2026-08-10',
    name: 'Muse Glimmer',
    description:
      'Meta\'s new agent-first model - built for reliable tool use, working in project folders, and recovering from its own mistakes. Sees images too. Brand-new: early support, expect rough edges.',
    category: 'quality',
    recommended: true,
    capabilities: ['agentic', 'coding', 'reasoning', 'chat'],
    traits: ['new'],
    modality: { in: ['text', 'vision'], out: ['text'] },
    variants: [
      {
        parameterCount: '30B',
        size: 12.4,
        minRAM: 16,
        downloadUrl:
          'https://huggingface.co/unsloth/Muse-Glimmer-30B-GGUF/resolve/main/Muse-Glimmer-30B-UD-Q2_K_XL.gguf',
        filename: 'Muse-Glimmer-30B-UD-Q2_K_XL.gguf',
        quantization: 'Q2_K_XL'
      },
      {
        parameterCount: '30B',
        size: 15.9,
        minRAM: 32,
        downloadUrl:
          'https://huggingface.co/unsloth/Muse-Glimmer-30B-GGUF/resolve/main/Muse-Glimmer-30B-UD-Q4_K_XL.gguf',
        filename: 'Muse-Glimmer-30B-UD-Q4_K_XL.gguf',
        quantization: 'Q4_K_XL'
      }
    ]
  },
  {
    id: 'qwen-3.8',
    maker: "Alibaba's Qwen team",
    quantizedBy: 'Unsloth',
    contextWindow: 262144,
    released: '2026-08-14',
    name: 'Qwen 3.8',
    description: 'Alibaba\'s newest. Frontier coding, math, and reasoning with built-in thinking; a hybrid attention design keeps long documents fast. The strongest model here for 24GB-class graphics cards.',
    category: 'quality',
    recommended: true,
    capabilities: ['coding', 'agentic', 'math', 'reasoning', 'multilingual', 'chat'],
    traits: ['new'],
    modality: { in: ['text', 'vision'], out: ['text'] },
    variants: [
      // The 3.8 open-weights generation is one dense 27B - no smaller
      // siblings exist (the only other 3.8 model is the 2.4T API flagship).
      // Apache-2.0, so no license gate. Only 16 of 64 layers are full
      // attention (rest linear), so the KV cache stays small even at
      // huge contexts.
      {
        parameterCount: '27B',
        size: 16.5,
        minRAM: 32,
        // Unsloth's Dynamic 3.0 requant renamed the file (the old
        // Q4_K_M URL 404s). Local filename stays the same - existing
        // installs, capability tiers, and routing key off it.
        downloadUrl: 'https://huggingface.co/unsloth/Qwen3.8-27B-GGUF/resolve/main/Qwen3.8-27B-UD-Q4_K_M.gguf',
        filename: 'Qwen3.8-27B-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      }
    ]
  },
  {
    id: 'qwen-3.6',
    maker: "Alibaba's Qwen team",
    quantizedBy: 'Unsloth',
    contextWindow: 262144,
    released: '2026-04-22',
    name: 'Qwen 3.6',
    description: 'Top-tier coding, math, and reasoning. The 35B MoE runs fast for its size — only 3B parameters active per token.',
    category: 'quality',
    recommended: true,
    capabilities: ['coding', 'agentic', 'math', 'reasoning', 'multilingual', 'chat'],
    traits: ['new', 'moe'],
    variants: [
      {
        parameterCount: '27B',
        size: 16.8,
        minRAM: 32,
        downloadUrl: 'https://huggingface.co/unsloth/Qwen3.6-27B-GGUF/resolve/main/Qwen3.6-27B-Q4_K_M.gguf',
        filename: 'Qwen3.6-27B-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      },
      {
        parameterCount: '35B-A3B (MoE)',
        size: 22.1,
        minRAM: 32,
        downloadUrl: 'https://huggingface.co/unsloth/Qwen3.6-35B-A3B-GGUF/resolve/main/Qwen3.6-35B-A3B-UD-Q4_K_M.gguf',
        filename: 'Qwen3.6-35B-A3B-UD-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      }
    ]
  },
  {
    id: 'gemma-4',
    maker: 'Google',
    quantizedBy: 'Google and Unsloth',
    contextWindow: 131072,
    released: '2026-04-01',
    name: 'Gemma 4',
    description: 'Google\'s latest open model. Strong writing, analysis, and instruction-following. E2B/E4B run on modest machines; the 26B is a fast MoE.',
    category: 'balanced',
    recommended: true,
    capabilities: ['writing', 'analysis', 'long-context', 'chat'],
    traits: ['new'],
    modality: { in: ['text', 'vision'], out: ['text'] },
    variants: [
      // Google's official QAT q4_0 builds (bf16-like quality at q4 size) -
      // these also carry Google's July 2026 refresh (tool-calling fixes,
      // wider vision resolution) that never got a version bump.
      // E2B stays on unsloth's compact Q4_K_M (rebuilt 2026-07-17, carries
      // the July refresh): the QAT build's extra ~0.25GB pushes it past
      // 4GB cards - and small machines are E2B's entire audience.
      {
        parameterCount: 'E2B',
        size: 3.1,
        minRAM: 8,
        downloadUrl: 'https://huggingface.co/unsloth/gemma-4-E2B-it-GGUF/resolve/main/gemma-4-E2B-it-Q4_K_M.gguf',
        filename: 'gemma-4-E2B-it-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      },
      {
        parameterCount: 'E4B',
        size: 5.2,
        minRAM: 16,
        downloadUrl: 'https://huggingface.co/google/gemma-4-E4B-it-qat-q4_0-gguf/resolve/main/gemma-4-E4B_q4_0-it.gguf',
        filename: 'gemma-4-E4B_q4_0-it.gguf',
        quantization: 'Q4_0 (QAT)'
      },
      {
        parameterCount: '12B',
        size: 7.0,
        minRAM: 16,
        contextWindow: 262144,
        downloadUrl: 'https://huggingface.co/google/gemma-4-12B-it-qat-q4_0-gguf/resolve/main/gemma-4-12b-it-qat-q4_0.gguf',
        filename: 'gemma-4-12b-it-qat-q4_0.gguf',
        quantization: 'Q4_0 (QAT)'
      },
      {
        parameterCount: '26B-A4B (MoE)',
        size: 14.5,
        minRAM: 32,
        contextWindow: 262144,
        downloadUrl: 'https://huggingface.co/google/gemma-4-26B-A4B-it-qat-q4_0-gguf/resolve/main/gemma-4-26B_q4_0-it.gguf',
        filename: 'gemma-4-26B_q4_0-it.gguf',
        quantization: 'Q4_0 (QAT)'
      },
      {
        parameterCount: '31B',
        size: 17.7,
        minRAM: 32,
        contextWindow: 262144,
        downloadUrl: 'https://huggingface.co/google/gemma-4-31B-it-qat-q4_0-gguf/resolve/main/gemma-4-31B_q4_0-it.gguf',
        filename: 'gemma-4-31B_q4_0-it.gguf',
        quantization: 'Q4_0 (QAT)'
      }
    ]
  },
  {
    id: 'ministral-3',
    maker: 'Mistral AI',
    quantizedBy: 'Unsloth',
    contextWindow: 262144,
    released: '2025-12-02',
    name: 'Ministral 3',
    description: 'Mistral\'s latest small model family. Fast inference, great for on-device use.',
    category: 'balanced',
    recommended: false,
    capabilities: ['chat', 'writing', 'analysis'],
    traits: ['new'],
    variants: [
      {
        parameterCount: '3B',
        size: 2.2,
        minRAM: 8,
        downloadUrl: 'https://huggingface.co/unsloth/Ministral-3-3B-Instruct-2512-GGUF/resolve/main/Ministral-3-3B-Instruct-2512-Q4_K_M.gguf',
        filename: 'Ministral-3-3B-Instruct-2512-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      },
      {
        parameterCount: '8B',
        size: 5.2,
        minRAM: 16,
        downloadUrl: 'https://huggingface.co/unsloth/Ministral-3-8B-Instruct-2512-GGUF/resolve/main/Ministral-3-8B-Instruct-2512-Q4_K_M.gguf',
        filename: 'Ministral-3-8B-Instruct-2512-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      },
      {
        parameterCount: '14B',
        size: 8.2,
        minRAM: 16,
        downloadUrl: 'https://huggingface.co/unsloth/Ministral-3-14B-Instruct-2512-GGUF/resolve/main/Ministral-3-14B-Instruct-2512-Q4_K_M.gguf',
        filename: 'Ministral-3-14B-Instruct-2512-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      }
    ]
  },

  // ─── Fast & Light ──────────────────────────────────────────────────────
  {
    id: 'phi-4-mini',
    maker: 'Microsoft',
    quantizedBy: 'Unsloth',
    contextWindow: 131072,
    released: '2025-02-28',
    name: 'Phi-4 Mini',
    description: 'Microsoft\'s compact model. Best option for machines with limited RAM.',
    category: 'fast',
    recommended: true,
    capabilities: ['chat', 'reasoning', 'coding'],
    traits: [],
    variants: [
      {
        parameterCount: '3.8B',
        size: 2.5,
        minRAM: 8,
        downloadUrl: 'https://huggingface.co/unsloth/Phi-4-mini-instruct-GGUF/resolve/main/Phi-4-mini-instruct-Q4_K_M.gguf',
        filename: 'Phi-4-mini-instruct-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      }
    ]
  },

  // ─── Quality / Reasoning ───────────────────────────────────────────────
  {
    id: 'gpt-oss',
    maker: 'OpenAI',
    quantizedBy: 'Unsloth',
    contextWindow: 131072,
    released: '2025-08-05',
    name: 'GPT-OSS (OpenAI)',
    description: 'OpenAI\'s open-weight model. Strong reasoning, agentic tasks, and function calling.',
    category: 'quality',
    recommended: false,
    capabilities: ['reasoning', 'agentic', 'coding', 'analysis'],
    traits: ['moe'],
    variants: [
      {
        parameterCount: '20B (3.6B active)',
        size: 11.6,
        minRAM: 16,
        downloadUrl: 'https://huggingface.co/unsloth/gpt-oss-20b-GGUF/resolve/main/gpt-oss-20b-Q4_K_M.gguf',
        filename: 'gpt-oss-20b-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      },
      {
        // The big-rig option: OpenAI's 120B MoE (only ~5B active per token, so
        // it generates at usable speed on high-RAM machines). Single-file
        // native-MXFP4 GGUF - the split Q4 files aren't supported by our
        // downloader. Among the strongest open models on health/knowledge
        // benchmarks; the fit system hides it from machines that can't run it.
        parameterCount: '120B (5.1B active)',
        size: 61,
        minRAM: 80,
        downloadUrl: 'https://huggingface.co/unsloth/gpt-oss-120b-GGUF/resolve/main/gpt-oss-120b-F16.gguf',
        filename: 'gpt-oss-120b-F16.gguf',
        quantization: 'MXFP4',
      }
    ]
  },
  {
    id: 'deepseek-r1',
    maker: 'DeepSeek',
    quantizedBy: 'Unsloth',
    derivedFrom: 'R1 reasoning distilled onto a Qwen3 8B base by DeepSeek themselves',
    contextWindow: 131072,
    released: '2025-05-29',
    name: 'DeepSeek R1 0528',
    description: 'Powerful chain-of-thought reasoning. Excels at complex problem solving, math, and logic.',
    category: 'quality',
    recommended: false,
    capabilities: ['reasoning', 'math', 'coding'],
    traits: ['thinking'],
    variants: [
      {
        parameterCount: '8B',
        size: 5.0,
        minRAM: 16,
        downloadUrl: 'https://huggingface.co/unsloth/DeepSeek-R1-0528-Qwen3-8B-GGUF/resolve/main/DeepSeek-R1-0528-Qwen3-8B-Q4_K_M.gguf',
        filename: 'DeepSeek-R1-0528-Qwen3-8B-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      }
    ]
  },

  {
    id: 'devstral-small-2',
    maker: 'Mistral AI',
    quantizedBy: 'Unsloth',
    contextWindow: 393216,
    released: '2025-12-10',
    name: 'Devstral Small 2',
    description: 'Mistral\'s agentic coding model. Tops open models on SWE-bench at its size — purpose-built for software engineering and code agents.',
    category: 'quality',
    recommended: false,
    capabilities: ['coding', 'agentic'],
    traits: ['new'],
    variants: [
      {
        parameterCount: '24B',
        size: 14.3,
        minRAM: 16,
        downloadUrl: 'https://huggingface.co/unsloth/Devstral-Small-2-24B-Instruct-2512-GGUF/resolve/main/Devstral-Small-2-24B-Instruct-2512-Q4_K_M.gguf',
        filename: 'Devstral-Small-2-24B-Instruct-2512-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      }
    ]
  },
  {
    id: 'ornith-1',
    maker: 'DeepReinforce',
    derivedFrom: 'Built on Qwen 3.5',
    contextWindow: 262144,
    released: '2026-08-20',
    name: 'Ornith 1.5',
    description: 'DeepReinforce\'s agentic coding model, self-improved with RL - 1.5 extends the loop to generating its own training tasks. State-of-the-art among open coders at its size, purpose-built for terminal coding agents and tool use. The 9B runs on modest machines; the 35B adds vision.',
    category: 'quality',
    recommended: false,
    capabilities: ['coding', 'agentic', 'reasoning'],
    traits: ['new', 'thinking'],
    variants: [
      {
        parameterCount: '9B',
        size: 5.6,
        minRAM: 16,
        downloadUrl: 'https://huggingface.co/ornith-ai/Ornith-1.5-9B-GGUF/resolve/main/Ornith-1.5-9B-Q4_K_M.gguf',
        filename: 'Ornith-1.5-9B-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      },
      {
        parameterCount: '35B',
        size: 21.7,
        minRAM: 32,
        downloadUrl: 'https://huggingface.co/ornith-ai/Ornith-1.5-35B-A3B-GGUF/resolve/main/Ornith-1.5-35B-Q4_K_M.gguf',
        filename: 'Ornith-1.5-35B-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      }
    ]
  },
  {
    id: 'glm-4.7-flash',
    maker: 'Zhipu (Z.ai)',
    quantizedBy: 'Unsloth',
    contextWindow: 202752,
    released: '2026-01-20',
    name: 'GLM-4.7 Flash',
    description: 'Zhipu\'s fast GLM model — a strong all-rounder tuned for agentic coding, reasoning, and tool use. The lighter "Flash" tier of the GLM family; the 30B MoE runs only 3B parameters per token.',
    category: 'quality',
    recommended: false,
    capabilities: ['coding', 'agentic', 'reasoning'],
    traits: ['new', 'moe'],
    variants: [
      {
        parameterCount: '30B-A3B (MoE)',
        size: 18.3,
        minRAM: 32,
        downloadUrl: 'https://huggingface.co/unsloth/GLM-4.7-Flash-GGUF/resolve/main/GLM-4.7-Flash-Q4_K_M.gguf',
        filename: 'GLM-4.7-Flash-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      }
    ]
  },
  {
    id: 'qwen3-coder',
    maker: "Alibaba's Qwen team",
    quantizedBy: 'Unsloth',
    contextWindow: 262144,
    released: '2025-07-31',
    name: 'Qwen3-Coder',
    description: 'Alibaba\'s agentic coding model — a 30B MoE with only 3B active per token. Tuned for repository-scale work and tool use (Qwen Code, Cline).',
    category: 'quality',
    recommended: false,
    capabilities: ['coding', 'agentic', 'long-context'],
    traits: ['new', 'moe'],
    variants: [
      {
        parameterCount: '30B-A3B (MoE)',
        size: 18.6,
        minRAM: 32,
        downloadUrl: 'https://huggingface.co/unsloth/Qwen3-Coder-30B-A3B-Instruct-GGUF/resolve/main/Qwen3-Coder-30B-A3B-Instruct-Q4_K_M.gguf',
        filename: 'Qwen3-Coder-30B-A3B-Instruct-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      }
    ]
  },

  // ─── Specialist ────────────────────────────────────────────────────────
  {
    id: 'qwen-3.5-opus-distilled',
    maker: 'Jackrong',
    derivedFrom: 'Qwen 3.5 distilled with Claude Opus reasoning traces',
    community: true,
    contextWindow: 262144,
    released: '2026-03-18',
    name: 'Qwen 3.5 Opus Distilled',
    description: 'Qwen 3.5 distilled from Claude Opus reasoning traces. Enhanced chain-of-thought capabilities.',
    category: 'specialist',
    recommended: false,
    capabilities: ['reasoning', 'agentic', 'coding', 'analysis', 'writing'],
    traits: ['distilled', 'thinking'],
    variants: [
      {
        parameterCount: '4B',
        size: 2.7,
        minRAM: 8,
        downloadUrl: 'https://huggingface.co/Jackrong/Qwen3.5-4B-Claude-4.6-Opus-Reasoning-Distilled-v2-GGUF/resolve/main/Qwen3.5-4B.Q4_K_M.gguf',
        filename: 'Qwen3.5-4B-Opus-Distilled-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      },
      {
        parameterCount: '9B',
        size: 5.6,
        minRAM: 16,
        downloadUrl: 'https://huggingface.co/Jackrong/Qwen3.5-9B-Claude-4.6-Opus-Reasoning-Distilled-v2-GGUF/resolve/main/Qwen3.5-9B.Q4_K_M.gguf',
        filename: 'Qwen3.5-9B-Opus-Distilled-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      },
      {
        parameterCount: '27B',
        size: 16.5,
        minRAM: 32,
        downloadUrl: 'https://huggingface.co/Jackrong/Qwen3.5-27B-Claude-4.6-Opus-Reasoning-Distilled-v2-GGUF/resolve/main/Qwen3.5-27B.Q4_K_M.gguf',
        filename: 'Qwen3.5-27B-Opus-Distilled-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      }
    ]
  },
  {
    id: 'qwen-3.5-uncensored',
    maker: 'HauhauCS',
    derivedFrom: 'Qwen 3.5 with safety training removed',
    community: true,
    contextWindow: 262144,
    released: '2026-03-05',
    name: 'Qwen 3.5 Uncensored',
    description: 'Qwen 3.5 with safety guardrails removed. No content filtering or refusals.',
    category: 'specialist',
    recommended: false,
    // NOT agentic: this build's template hard-rejects agent-shaped
    // conversations (verified live) - the family name flatters it.
    capabilities: ['writing', 'chat', 'analysis'],
    traits: ['uncensored'],
    variants: [
      {
        parameterCount: '2B',
        size: 1.2,
        minRAM: 4,
        downloadUrl: 'https://huggingface.co/HauhauCS/Qwen3.5-2B-Uncensored-HauhauCS-Aggressive/resolve/main/Qwen3.5-2B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf',
        filename: 'Qwen3.5-2B-Uncensored-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      },
      {
        parameterCount: '4B',
        size: 2.6,
        minRAM: 8,
        downloadUrl: 'https://huggingface.co/HauhauCS/Qwen3.5-4B-Uncensored-HauhauCS-Aggressive/resolve/main/Qwen3.5-4B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf',
        filename: 'Qwen3.5-4B-Uncensored-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      },
      {
        parameterCount: '9B',
        size: 5.3,
        minRAM: 16,
        downloadUrl: 'https://huggingface.co/HauhauCS/Qwen3.5-9B-Uncensored-HauhauCS-Aggressive/resolve/main/Qwen3.5-9B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf',
        filename: 'Qwen3.5-9B-Uncensored-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      },
      {
        parameterCount: '27B',
        size: 16.0,
        minRAM: 32,
        downloadUrl: 'https://huggingface.co/HauhauCS/Qwen3.5-27B-Uncensored-HauhauCS-Aggressive/resolve/main/Qwen3.5-27B-Uncensored-HauhauCS-Aggressive-Q4_K_M.gguf',
        filename: 'Qwen3.5-27B-Uncensored-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      }
    ]
  },
  {
    id: 'medgemma',
    maker: 'Google',
    quantizedBy: 'Unsloth',
    contextWindow: 131072,
    released: '2026-01-14',
    name: 'MedGemma',
    description:
      "Google's open medical models - discuss your own health records, lab results, and medical images (X-rays, skin photos, scans) privately on your device. For understanding and preparing questions, not diagnosis.",
    category: 'specialist',
    recommended: false,
    capabilities: ['medical', 'analysis', 'reasoning'],
    traits: ['new'],
    modality: { in: ['text', 'vision'], out: ['text'] },
    license: {
      id: 'hai-def',
      name: 'Health AI Developer Foundations terms',
      url: 'https://developers.google.com/health-ai-developer-foundations/terms',
      notice:
        'HAI-DEF is provided under and subject to the Health AI Developer Foundations Terms of Use.',
      points: [
        'Not for clinical use: no diagnosing or treating patients. Use it to understand your own results and prepare questions for your doctor.',
        'Restricted uses in the Health AI Developer Foundations Prohibited Use Policy apply.',
        'These same terms pass to anyone you share the model with.',
      ],
    },
    variants: [
      {
        parameterCount: '4B (v1.5)',
        size: 2.4,
        minRAM: 8,
        downloadUrl:
          'https://huggingface.co/unsloth/medgemma-1.5-4b-it-GGUF/resolve/main/medgemma-1.5-4b-it-Q4_K_M.gguf',
        filename: 'medgemma-1.5-4b-it-Q4_K_M.gguf',
        quantization: 'Q4_K_M',
      },
      {
        parameterCount: '27B',
        size: 15.5,
        minRAM: 32,
        downloadUrl:
          'https://huggingface.co/unsloth/medgemma-27b-it-GGUF/resolve/main/medgemma-27b-it-Q4_K_M.gguf',
        filename: 'medgemma-27b-it-Q4_K_M.gguf',
        quantization: 'Q4_K_M',
      },
    ],
  },
  {
    id: 'qwen-3.8-distilled',
    maker: 'empero-ai',
    derivedFrom: 'Distilled from Qwen 3.8',
    community: true,
    contextWindow: 262144,
    released: '2026-08-16',
    name: 'Qwen 3.8 Distilled',
    description: "Empero's full-parameter distillations of the Qwen 3.8 flagship into small, fast sizes - a big knowledge jump over same-size models (the 9B scores near the giants on broad knowledge), with step-by-step reasoning. Text only.",
    category: 'specialist',
    recommended: false,
    capabilities: ['reasoning', 'analysis', 'writing', 'chat'],
    traits: ['new', 'distilled', 'thinking'],
    variants: [
      {
        parameterCount: '2B',
        size: 1.3,
        minRAM: 4,
        downloadUrl: 'https://huggingface.co/empero-ai/Qwen3.8-2B-GGUF/resolve/main/Qwen3.8-2B-Q4_K_M.gguf',
        filename: 'Qwen3.8-2B-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      },
      {
        parameterCount: '4B',
        size: 2.8,
        minRAM: 8,
        downloadUrl: 'https://huggingface.co/empero-ai/Qwen3.8-4B-GGUF/resolve/main/Qwen3.8-4B-Q4_K_M.gguf',
        filename: 'Qwen3.8-4B-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      },
      {
        parameterCount: '9B',
        size: 5.8,
        minRAM: 16,
        downloadUrl: 'https://huggingface.co/empero-ai/Qwen3.8-9B-GGUF/resolve/main/Qwen3.8-9B-Q4_K_M.gguf',
        filename: 'Qwen3.8-9B-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      }
    ]
  },
  {
    id: 'qwen-3.8-uncensored',
    maker: 'orcarouter',
    derivedFrom: 'Qwen 3.8 with safety training removed',
    community: true,
    contextWindow: 262144,
    released: '2026-08-16',
    name: 'Qwen 3.8 Uncensored',
    description: "The Qwen 3.8 flagship with refusal behavior substantially reduced (openly documented - reduced, not eliminated). Same frontier coding, math, and reasoning, and it can see images with its vision add-on. For 24GB-class graphics cards.",
    category: 'specialist',
    recommended: false,
    capabilities: ['coding', 'agentic', 'math', 'reasoning', 'multilingual', 'chat'],
    traits: ['new', 'uncensored', 'thinking'],
    modality: { in: ['text', 'vision'], out: ['text'] },
    variants: [
      {
        parameterCount: '27B',
        size: 16.5,
        minRAM: 32,
        downloadUrl: 'https://huggingface.co/JonathanColetti/Qwen3.8-27B-Uncensored-GGUF/resolve/main/Qwen3.8-27B-Uncensored-noMTP-Q4_K_M.gguf',
        filename: 'Qwen3.8-27B-Uncensored-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      }
    ]
  },
  {
    id: 'qwythos-9b',
    maker: 'empero-ai',
    derivedFrom: 'A Qwen 3.5-based merge',
    community: true,
    contextWindow: 1048576,
    released: '2026-07-12',
    name: 'Qwythos 9B',
    description: 'A community Qwen 3.5-based merge — uncensored and multimodal (it can see images you attach), with step-by-step reasoning and tool use. A creative, unfiltered generalist. v2 trains out the repetition loops of the original.',
    category: 'specialist',
    recommended: false,
    capabilities: ['reasoning', 'agentic', 'writing', 'long-context'],
    traits: ['new', 'uncensored', 'thinking'],
    modality: { in: ['text', 'vision'], out: ['text'] },
    variants: [
      {
        parameterCount: '9B',
        size: 5.4,
        minRAM: 16,
        downloadUrl: 'https://huggingface.co/empero-ai/Qwythos-9B-v2-GGUF/resolve/main/Qwythos-9B-v2-Q4_K_M.gguf',
        filename: 'Qwythos-9B-v2-Q4_K_M.gguf',
        quantization: 'Q4_K_M'
      }
    ]
  },
];

// ─── Category metadata for UI ──────────────────────────────────────────────
export const categoryInfo: Record<ModelFamily['category'], { label: string; description: string; order: number }> = {
  fast:       { label: 'Fast & Light',    description: 'Quick responses on any hardware',          order: 0 },
  balanced:   { label: 'Balanced',        description: 'Best all-round performance and quality',   order: 1 },
  quality:    { label: 'High Quality',    description: 'Maximum capability for complex tasks',     order: 2 },
  specialist: { label: 'Specialist',      description: 'Distilled, uncensored, and niche models',  order: 3 },
};

// ─── Capability metadata (the "good for" chips + future routing) ───────────
export const capabilityInfo: Record<Capability, { label: string }> = {
  chat:           { label: 'Chat' },
  coding:         { label: 'Coding' },
  reasoning:      { label: 'Reasoning' },
  writing:        { label: 'Writing' },
  analysis:       { label: 'Analysis' },
  math:           { label: 'Math' },
  medical:        { label: 'Medical' },
  multilingual:   { label: 'Multilingual' },
  'long-context': { label: 'Long context' },
  agentic:        { label: 'Agentic' },
};

// ─── Trait metadata (the few badges that actually carry signal) ────────────
export const traitInfo: Record<Trait, { label: string; color: string; description: string }> = {
  new:        { label: 'New',        color: 'bg-blue-500',   description: 'Recently added to the catalog' },
  thinking:   { label: 'Thinking',   color: 'bg-purple-500', description: 'Reasons step-by-step before answering' },
  uncensored: { label: 'Uncensored', color: 'bg-red-500',    description: 'No content filtering or refusals' },
  moe:        { label: 'MoE',        color: 'bg-amber-500',  description: 'Mixture-of-experts — fast for its size' },
  distilled:  { label: 'Distilled',  color: 'bg-cyan-500',   description: 'Distilled from a larger teacher model' },
};

// ─── Modality metadata (text today; voice / image / video land here later) ─
export const modalityInfo: Record<Modality, { label: string }> = {
  text:   { label: 'Text' },
  vision: { label: 'Vision' },
  voice:  { label: 'Voice' },
  image:  { label: 'Image' },
  video:  { label: 'Video' },
};

const DEFAULT_MODALITY: { in: Modality[]; out: Modality[] } = { in: ['text'], out: ['text'] };

/** A family's modality, defaulting to text→text (most current models). */
export function getModality(family: ModelFamily): { in: Modality[]; out: Modality[] } {
  return family.modality ?? DEFAULT_MODALITY;
}

/**
 * Get the best variant for a model family based on system RAM/VRAM
 */
export function getBestVariantForSystem(
  family: ModelFamily,
  totalRAM: number,
  totalVRAM: number | null,
  freeRamGb?: number | null
): ModelVariant | null {
  const suitable = family.variants.filter(
    v => getRunMode(v, totalRAM, totalVRAM, freeRamGb) !== 'too-big',
  );
  if (suitable.length === 0) return null;

  // Prefer the largest variant that runs on the GPU (fast). On CPU, biggest
  // is NOT best - tokens/sec falls with size - so prefer the largest
  // fast-class (≤5GB) variant and only exceed it when nothing smaller exists.
  const onGpu = suitable.filter(v => getRunMode(v, totalRAM, totalVRAM, freeRamGb) === 'gpu');
  if (onGpu.length) return onGpu.sort((a, b) => b.size - a.size)[0];
  const fast = suitable.filter(
    v => v.size <= 5 || getRunMode(v, totalRAM, totalVRAM, freeRamGb) === 'moe-split',
  );
  if (fast.length) return fast.sort((a, b) => b.size - a.size)[0];
  return suitable.sort((a, b) => a.size - b.size)[0];
}

/** RAM to leave for the OS, this app, and everything else when sizing a
 *  CPU-only run. Scales down on small machines: a flat desktop-sized 7GB
 *  reserve marked EVERY model "too big" on an 8GB laptop, hiding even the
 *  2GB ones (found on an 8GB MacBook Air). */
function reservedRamGb(totalRAM: number): number {
  return Math.min(7, Math.max(3, totalRAM * 0.4));
}

/**
 * Where a variant would actually run on this machine — grounded in VRAM, not just
 * RAM. GPU if its estimated VRAM need (weights + KV cache + overhead at ~8K
 * context) fits the card; else CPU if it fits RAM (slower); else too big. Mirrors
 * the load-time decision (the conductor uses the exact GGUF-header estimate once a
 * model is downloaded), so the label matches what actually happens.
 */
export type RunMode = 'gpu' | 'cpu' | 'moe-split' | 'too-big';

/** A mixture-of-experts artifact: its experts (most of the file) can live in
 *  main memory while attention and the KV cache use the graphics card, so it
 *  runs on a card it does not fit in - fast for its size. Read off the
 *  variant's own label ("35B-A3B (MoE)", "20B (3.6B active)"). */
export function isMoeVariant(variant: ModelVariant): boolean {
  const label = variant.parameterCount;
  return /\bMoE\b/i.test(label) || /\bactive\b/i.test(label) || /\d+B-A\d+(\.\d+)?B/i.test(label);
}

/** Approx VRAM (GB) to load a model of `sizeGb` on the GPU. A pre-download estimate
 *  — the exact header-based number drives the real load decision. NB: sizing here
 *  and in getRunMode grades the GGUF artifact (`variant.size`); when a non-gguf
 *  engine lands, size must come from `artifactFor(v, activeEngineFormat())` and the
 *  fit math gets a per-engine strategy. */
export function estimateVramGb(sizeGb: number): number {
  return sizeGb * 1.05 + 0.85;
}

/** Human-readable context window, e.g. 262144 → "256K", 1048576 → "1M". */
export function formatContext(tokens: number): string {
  if (tokens >= 1_048_576) {
    const m = tokens / 1_048_576;
    return `${Number.isInteger(m) ? m : m.toFixed(1)}M`;
  }
  if (tokens >= 1024) return `${Math.round(tokens / 1024)}K`;
  return String(tokens);
}

/** CPU-run size budget. Sized from total RAM minus the OS reserve - and,
 *  when the caller knows how much RAM is actually FREE right now, clamped
 *  to that too (minus headroom for the KV cache and whatever the user has
 *  open staying open). Totals alone recommended a 7.1GB model to a machine
 *  with half its 16GB already in use; the load then OOM-killed the app. */
function cpuBudgetGb(totalRAM: number, freeRamGb?: number | null): number {
  const fromTotal = totalRAM - reservedRamGb(totalRAM);
  if (freeRamGb == null) return fromTotal;
  return Math.min(fromTotal, Math.max(1, freeRamGb - 1.5));
}

export function getRunMode(
  variant: ModelVariant,
  totalRAM: number,
  totalVRAM: number | null,
  freeRamGb?: number | null,
): RunMode {
  // With a GPU, a model runs on it or not at all — we don't fall back to a slow
  // CPU run (a too-large model is flagged so, not crawled). Without a discrete GPU,
  // the CPU is the only path, so RAM is what matters. Integrated GPUs share
  // system RAM - callers pass totalVRAM = null for them so they size as CPU.
  if (totalVRAM && totalVRAM > 0) {
    if (estimateVramGb(variant.size) <= totalVRAM) return 'gpu';
    // A mixture-of-experts model bigger than the card runs with its experts
    // in main memory (the loader passes --cpu-moe): the gate is RAM, not
    // VRAM. A 32 GB box carries the 21 GB 35B-A3B; a 16 GB box does not.
    if (isMoeVariant(variant) && variant.size * 1.1 <= cpuBudgetGb(totalRAM, freeRamGb)) {
      return 'moe-split';
    }
    return 'too-big';
  }
  return variant.size * 1.2 <= cpuBudgetGb(totalRAM, freeRamGb) ? 'cpu' : 'too-big';
}

/**
 * Get the best model family for user's system
 */
export function getBestFamilyForRAM(totalRAM: number, totalVRAM: number | null, freeRamGb?: number | null): ModelFamily | undefined {
  // Get families that have at least one suitable variant
  const suitableFamilies = modelFamilies.filter(family => {
    return getBestVariantForSystem(family, totalRAM, totalVRAM, freeRamGb) !== null;
  });

  // Filter to recommended families
  const recommended = suitableFamilies.filter(f => f.recommended);

  if (recommended.length === 0) return suitableFamilies[0];

  // Rank families by the SAME philosophy the variant picker uses, instead
  // of raw pick size. Sorting by size descending contradicted the variant
  // rule ("on CPU, biggest is NOT best") one level up: a family whose only
  // variant was a 15.7GB 27B outranked a sensible 4GB pick on a 32GB-RAM /
  // 2GB-VRAM machine BECAUSE it was huge - a 75-minute welcome download
  // into CPU-crawl territory (seen in the field). Preference order:
  //   1. a pick that runs on the GPU (fast) beats any CPU pick;
  //      among GPU picks, larger = more capable and still fast.
  //   2. among CPU picks, a fast-class (<=5GB) pick beats an oversized
  //      one; larger wins WITHIN fast-class, smaller wins outside it.
  const rank = (f: ModelFamily) => {
    const v = getBestVariantForSystem(f, totalRAM, totalVRAM, freeRamGb);
    if (!v) return { mode: -1, size: 0 };
    const mode = getRunMode(v, totalRAM, totalVRAM, freeRamGb);
    // moe-split runs fast for its size (experts in RAM, the rest on the
    // card) - a fast-class pick, below a fully-on-GPU one.
    return { mode: mode === 'gpu' ? 2 : mode === 'moe-split' || v.size <= 5 ? 1 : 0, size: v.size };
  };
  return recommended.sort((a, b) => {
    const ra = rank(a);
    const rb = rank(b);
    if (ra.mode !== rb.mode) return rb.mode - ra.mode;
    // GPU and fast-class CPU: bigger is better. Oversized CPU: smaller is.
    return ra.mode === 0 ? ra.size - rb.size : rb.size - ra.size;
  })[0];
}

/**
 * Get model family by ID
 */
export function getModelFamilyById(id: string): ModelFamily | undefined {
  return modelFamilies.find(f => f.id === id);
}

/**
 * Check if a variant is suitable for the system
 */
export function isVariantSuitable(variant: ModelVariant, totalRAM: number, totalVRAM: number | null, freeRamGb?: number | null): boolean {
  return getRunMode(variant, totalRAM, totalVRAM, freeRamGb) !== 'too-big';
}

/**
 * Can this system run ANY variant of the family? Used to sort runnable models
 * first and to power the "show only models I can run" filter.
 */
export function isFamilyRunnable(family: ModelFamily, totalRAM: number, totalVRAM: number | null, freeRamGb?: number | null): boolean {
  return family.variants.some(v => isVariantSuitable(v, totalRAM, totalVRAM, freeRamGb));
}

/**
 * Routing-ready lookup: families that provide a capability, optionally limited
 * to what the system can actually run. A future query router shortlists
 * candidate models for a task by calling this — same metadata the UI uses.
 */
export function getFamiliesByCapability(
  capability: Capability,
  system?: { totalRAM: number; totalVRAM: number | null }
): ModelFamily[] {
  return modelFamilies.filter(f =>
    f.capabilities.includes(capability) &&
    (!system || isFamilyRunnable(f, system.totalRAM, system.totalVRAM))
  );
}

/**
 * Get GPU acceleration status for a variant
 */
export function getGPUStatus(variant: ModelVariant, totalVRAM: number | null): {
  isAccelerated: boolean;
  isFull: boolean;
  isPartial: boolean;
} {
  const hasFullGPU = totalVRAM !== null && totalVRAM >= variant.size;
  const hasPartialGPU = totalVRAM !== null && totalVRAM > 0 && totalVRAM < variant.size;

  return {
    isAccelerated: hasFullGPU || hasPartialGPU,
    isFull: hasFullGPU,
    isPartial: hasPartialGPU
  };
}

/**
 * Get model families grouped by category, sorted by category order
 */
export function getModelsByCategory(): { category: ModelFamily['category']; info: typeof categoryInfo[ModelFamily['category']]; families: ModelFamily[] }[] {
  const categories = Object.keys(categoryInfo) as ModelFamily['category'][];
  return categories
    .sort((a, b) => categoryInfo[a].order - categoryInfo[b].order)
    .map(cat => ({
      category: cat,
      info: categoryInfo[cat],
      families: modelFamilies.filter(f => f.category === cat),
    }))
    .filter(group => group.families.length > 0);
}