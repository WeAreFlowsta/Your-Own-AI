/**
 * AI packs - a whole character (or business assistant) as one shareable
 * file: identity, personality, appearance, and its authored knowledge.
 * Deliberately EXCLUDED: conversations, learned memories about the user,
 * and the model choice (the importer's hardware decides; imports start on
 * "Auto - Offline Only").
 *
 * Signing mirrors knowledge packs: a canonical SHA-256 over the CONTENT
 * (never the signature) signed with the user's Flowsta identity via the
 * Vault. Import verifies: valid = "verified", none = "unsigned", mismatch
 * = "tampered" (blocked).
 */
import { invoke } from "@tauri-apps/api/core";
import type { UserDefinedAI } from "../types";
import type { PackSignature, VerifyState } from "./packSigning";

export const AI_PACK_FORMAT = "your-own-ai/ai-pack";

export interface AiPack {
  format: string;
  version: number;
  name: string;
  description: string;
  baseArchetypeId: string;
  systemPrompt: string;
  askBlurb?: string;
  emoji?: string;
  useEmojis?: boolean;
  lengthDisposition?: string;
  defaultMode?: string;
  /** Data URL (jpeg/png) of the AI's portrait. */
  thumbnail?: string;
  knowledge: { text: string }[];
  signature?: PackSignature;
}

/** Canonical SHA-256 over every content field, in fixed order. */
async function manifestHash(pack: AiPack): Promise<{ b64: string; hex: string }> {
  const canonical = JSON.stringify({
    format: pack.format,
    version: pack.version,
    name: pack.name,
    description: pack.description,
    baseArchetypeId: pack.baseArchetypeId,
    systemPrompt: pack.systemPrompt,
    askBlurb: pack.askBlurb ?? "",
    emoji: pack.emoji ?? "",
    useEmojis: pack.useEmojis ?? null,
    lengthDisposition: pack.lengthDisposition ?? "",
    defaultMode: pack.defaultMode ?? "",
    thumbnail: pack.thumbnail ?? "",
    knowledge: pack.knowledge.map((e) => e.text),
  });
  const digest = await crypto.subtle.digest(
    "SHA-256",
    new TextEncoder().encode(canonical),
  );
  const bytes = new Uint8Array(digest);
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return {
    b64: btoa(bin),
    hex: [...bytes].map((b) => b.toString(16).padStart(2, "0")).join(""),
  };
}

/** Assemble a pack from an AI + its portrait bytes + authored knowledge. */
export function buildAiPack(
  ai: UserDefinedAI,
  thumbnailDataUrl: string | null,
  knowledge: { text: string }[],
): AiPack {
  return {
    format: AI_PACK_FORMAT,
    version: 1,
    name: ai.name,
    description: ai.description,
    baseArchetypeId: ai.baseArchetypeId,
    systemPrompt: ai.systemPrompt,
    askBlurb: ai.askBlurb,
    emoji: ai.emoji,
    useEmojis: ai.useEmojis,
    lengthDisposition: ai.lengthDisposition,
    defaultMode: ai.defaultMode,
    thumbnail: thumbnailDataUrl ?? undefined,
    knowledge,
  };
}

export async function signAiPack(pack: AiPack): Promise<PackSignature> {
  const { b64, hex } = await manifestHash(pack);
  const res = await invoke<{ signature: string; agent_pub_key: string }>(
    "vault_sign",
    { bytesB64: b64, reason: `Sign AI pack "${pack.name}"` },
  );
  if (!res.signature || !res.agent_pub_key) throw new Error("sign_failed");
  return {
    algo: "ed25519",
    agent_pub_key: res.agent_pub_key,
    manifest_hash: `sha256:${hex}`,
    signed_at: new Date().toISOString(),
    value: res.signature,
  };
}

export async function verifyAiPack(pack: AiPack): Promise<VerifyState> {
  const sig = pack.signature;
  if (!sig || !sig.value || !sig.agent_pub_key) return "unsigned";
  try {
    const { b64 } = await manifestHash(pack);
    const ok = await invoke<boolean>("verify_pack_signature", {
      agentPubKey: sig.agent_pub_key,
      hashB64: b64,
      signatureB64: sig.value,
    });
    return ok ? "verified" : "tampered";
  } catch {
    return "tampered";
  }
}

/** Parse + shape-check a candidate pack file. Returns null when the file
 *  isn't an AI pack (so callers can fall back or show a clear message). */
export function parseAiPack(text: string): AiPack | null {
  try {
    const data = JSON.parse(text);
    if (data?.format !== AI_PACK_FORMAT) return null;
    if (typeof data.name !== "string" || !data.name.trim()) return null;
    if (typeof data.baseArchetypeId !== "string") return null;
    return {
      format: data.format,
      version: typeof data.version === "number" ? data.version : 1,
      name: data.name,
      description: typeof data.description === "string" ? data.description : "",
      baseArchetypeId: data.baseArchetypeId,
      systemPrompt: typeof data.systemPrompt === "string" ? data.systemPrompt : "",
      askBlurb: typeof data.askBlurb === "string" ? data.askBlurb : undefined,
      emoji: typeof data.emoji === "string" ? data.emoji : undefined,
      useEmojis: typeof data.useEmojis === "boolean" ? data.useEmojis : undefined,
      lengthDisposition:
        typeof data.lengthDisposition === "string" ? data.lengthDisposition : undefined,
      defaultMode: typeof data.defaultMode === "string" ? data.defaultMode : undefined,
      thumbnail:
        typeof data.thumbnail === "string" && data.thumbnail.startsWith("data:image/")
          ? data.thumbnail
          : undefined,
      knowledge: Array.isArray(data.knowledge)
        ? data.knowledge
            .map((e: unknown) => ({
              text: typeof e === "string" ? e : ((e as { text?: string })?.text ?? ""),
            }))
            .filter((e: { text: string }) => e.text.trim().length > 0)
        : [],
      signature: data.signature,
    };
  } catch {
    return null;
  }
}

/** Portrait bytes → data URL for embedding in a pack. */
export function thumbnailBytesToDataUrl(bytes: number[]): string {
  let bin = "";
  const arr = new Uint8Array(bytes);
  for (const b of arr) bin += String.fromCharCode(b);
  // Thumbnails are stored as JPEG; PNG magic tolerated for older saves.
  const mime = arr[0] === 0x89 ? "image/png" : "image/jpeg";
  return `data:${mime};base64,${btoa(bin)}`;
}

/** Data URL → bytes for save_ai_thumbnail. */
export function thumbnailDataUrlToBytes(dataUrl: string): number[] | null {
  const m = /^data:image\/[a-z+]+;base64,(.*)$/s.exec(dataUrl);
  if (!m) return null;
  try {
    const bin = atob(m[1]);
    const out = new Array<number>(bin.length);
    for (let i = 0; i < bin.length; i++) out[i] = bin.charCodeAt(i);
    return out;
  } catch {
    return null;
  }
}

/** Suggested filename for an exported pack. */
export function aiPackFilename(pack: AiPack): string {
  const slug = pack.name.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "");
  return `${slug || "ai"}-pack.json`;
}
