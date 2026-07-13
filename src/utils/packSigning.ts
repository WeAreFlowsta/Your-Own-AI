/**
 * Knowledge-pack signing & verification.
 *
 * A pack can be exported signed (with the user's Flowsta identity, via Vault's
 * /sign) or unsigned. On import we verify the signature against the pack's
 * current content: a valid signature = "verified", none = "unsigned", a
 * signature that doesn't match = "tampered" (blocked). The signature is over a
 * canonical SHA-256 of the content, so editing any entry breaks it.
 */
import { invoke } from "@tauri-apps/api/core";

export interface PackSignature {
  algo: string;
  agent_pub_key: string;
  manifest_hash: string; // "sha256:<hex>" of the signed content (informational)
  signed_at: string;
  value: string; // base64 ed25519 signature
}

export interface KnowledgePack {
  format: string;
  version: number;
  name?: string;
  entries: { text: string }[];
  signature?: PackSignature;
}

/** Canonical SHA-256 over the pack CONTENT (never the signature): format,
 *  version, name, and entry texts in order. Deterministic so the verifier
 *  recomputes the same hash. */
async function manifestHash(
  pack: KnowledgePack,
): Promise<{ b64: string; hex: string }> {
  const canonical = JSON.stringify({
    format: pack.format,
    version: pack.version,
    name: pack.name ?? "",
    entries: pack.entries.map((e) => e.text),
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

export interface VaultState {
  installed: boolean;
  unlocked: boolean;
}

export async function vaultState(): Promise<VaultState> {
  try {
    const s = await invoke<{ installed: boolean; unlocked: boolean }>(
      "flowsta_vault_status",
    );
    return { installed: !!s.installed, unlocked: !!s.unlocked };
  } catch {
    return { installed: false, unlocked: false };
  }
}

/** Sign the pack (Vault must be unlocked). Returns the signature block; throws
 *  an error string (vault_locked / vault_not_found / …) to surface in the UI. */
export async function signPack(pack: KnowledgePack): Promise<PackSignature> {
  const { b64, hex } = await manifestHash(pack);
  const res = await invoke<{ signature: string; agent_pub_key: string }>(
    "vault_sign",
    {
      bytesB64: b64,
      reason: `Sign knowledge pack${pack.name ? ` "${pack.name}"` : ""}`,
    },
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

export type VerifyState = "verified" | "unsigned" | "tampered";

/** Verify a pack's signature against its current content. */
export async function verifyPack(pack: KnowledgePack): Promise<VerifyState> {
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
    return "tampered"; // couldn't verify → don't trust it
  }
}

/** Trigger Vault sign-in (Vault shows its own approval dialog). Throws on
 *  failure (vault_locked / vault_not_found / user_denied / …). */
export async function signInToFlowsta(): Promise<void> {
  await invoke("flowsta_sign_in");
}
