/**
 * Share an add-on to the directory - the one-button path. The app signs
 * with the Flowsta Vault (a share carries your name), then hands the
 * signed file and its listing to the share service, which opens the
 * directory pull request for you. See build-docs SKILLS.md.
 */
import { invoke } from "@tauri-apps/api/core";
import type { AiPack } from "./aiPack";
import type { McpServer } from "./mcp";

export const LICENSES = [
  { id: "CC-BY-4.0", label: "CC BY 4.0 - anyone may use and adapt it, with credit" },
  { id: "CC-BY-SA-4.0", label: "CC BY-SA 4.0 - same, and adaptations stay shareable" },
  { id: "CC0-1.0", label: "CC0 - public domain, no credit needed" },
  { id: "MIT", label: "MIT" },
  { id: "Apache-2.0", label: "Apache 2.0" },
  { id: "GPL-3.0-or-later", label: "GPL 3.0 or later" },
  { id: "GPL-2.0-or-later", label: "GPL 2.0 or later" },
];

export interface Maker {
  name: string;
  handle: string;
  agent_pub_key: string;
}

export interface ShareResult {
  ok: boolean;
  id: string;
  pr_url: string;
  page: string;
}

/** The signed-in Flowsta identity as a maker, or null when not signed in. */
export async function currentMaker(): Promise<Maker | null> {
  try {
    const s = await invoke<{ signed_in: boolean; agent_pub_key?: string | null; display_name?: string | null; web_username?: string | null }>("flowsta_session");
    if (!s?.signed_in || !s.agent_pub_key || !s.web_username) return null;
    return { name: s.display_name || s.web_username, handle: s.web_username, agent_pub_key: s.agent_pub_key };
  } catch {
    return null;
  }
}

function bytesToBase64(bytes: Uint8Array): string {
  let bin = "";
  for (const b of bytes) bin += String.fromCharCode(b);
  return btoa(bin);
}

/** Share a signed AI pack as a character. */
export async function shareCharacter(pack: AiPack, opts: { description: string; license: string; title?: string; maker: Maker }): Promise<ShareResult> {
  if (!pack.signature) throw new Error("pack must be signed");
  const manifest = {
    schema: 1,
    kind: "character",
    name: pack.name,
    title: opts.title || undefined,
    description: opts.description,
    license: opts.license,
    terms: "free",
    maker: opts.maker,
    portrait: pack.thumbnail || undefined,
  };
  const file_b64 = btoa(unescape(encodeURIComponent(JSON.stringify(pack, null, 2))));
  return invoke<ShareResult>("share_submit", { submission: { kind: "character", manifest, file_b64, signature: pack.signature } });
}

/** Share an installed skill folder: zip it, sign the zip's digest, submit. */
export async function shareSkill(
  name: string,
  opts: { title: string; description: string; license: string; runsPrograms: boolean; maker: Maker },
): Promise<ShareResult> {
  const zipped = await invoke<{ zip_b64: string; sha256: string; files: number; bytes: number }>("skills_pack_zip", { name });
  const digest = new Uint8Array(zipped.sha256.match(/.{2}/g)!.map((h) => parseInt(h, 16)));
  const res = await invoke<{ signature: string; agent_pub_key: string }>("vault_sign", {
    bytesB64: bytesToBase64(digest),
    reason: `Share skill "${opts.title}"`,
  });
  if (!res.signature || !res.agent_pub_key) throw new Error("sign_failed");
  const signature = {
    algo: "ed25519",
    agent_pub_key: res.agent_pub_key,
    manifest_hash: `sha256:${zipped.sha256}`,
    signed_at: new Date().toISOString(),
    value: res.signature,
  };
  const manifest = {
    schema: 1,
    kind: "skill",
    name: opts.title,
    description: opts.description,
    license: opts.license,
    terms: "free",
    maker: opts.maker,
    sha256: zipped.sha256,
    runs_programs: opts.runsPrograms,
  };
  return invoke<ShareResult>("share_submit", { submission: { kind: "skill", manifest, file_b64: zipped.zip_b64, signature } });
}

/** A plain error line for the share dialogs. */
export function shareErrorText(e: unknown): string {
  const m = e instanceof Error ? e.message : String(e);
  if (m.includes("vault_locked")) return "Flowsta Vault is locked - unlock it to sign.";
  if (m.includes("vault_not_found")) return "Flowsta Vault isn't running.";
  if (m.includes("sign_failed") || m.includes("denied")) return "Signing was declined in Vault.";
  if (m.includes("share service")) return "The share service could not be reached - check your connection and try again.";
  return m.length < 160 ? m : "Couldn't share - please try again.";
}

/** The canonical recipe bytes a maker signs (mirrors the directory's recipeDigest). */
export function recipeCanonical(m: { name: string; description: string; license: string; source_url: string; mcp: Record<string, unknown> }): string {
  const r = m.mcp as { transport?: string; command?: string; args?: string[]; url?: string; needs?: { program: string; label: string; install: string }[]; config?: { key: string; label: string; kind: string; required?: boolean; hint?: string; where?: string; prefix?: string }[]; fetch?: { url: string; dest: string } | null };
  return JSON.stringify({
    kind: "mcp",
    name: m.name,
    description: m.description,
    license: m.license,
    source_url: m.source_url,
    mcp: {
      transport: r.transport,
      command: r.command ?? "",
      args: r.args ?? [],
      url: r.url ?? "",
      needs: (r.needs ?? []).map((n) => ({ program: n.program, label: n.label, install: n.install })),
      config: (r.config ?? []).map((f) => ({ key: f.key, label: f.label, kind: f.kind, required: !!f.required, hint: f.hint ?? "", where: f.where ?? "", prefix: f.prefix ?? "" })),
      fetch: r.fetch ? { url: r.fetch.url, dest: r.fetch.dest } : null,
    },
  });
}

/** Programs the app can install, with the labels and pages a listing carries. */
const KNOWN_NEEDS: Record<string, { label: string; install: string }> = {
  uv: { label: "uv (runs the Python server)", install: "https://docs.astral.sh/uv/getting-started/installation/" },
  uvx: { label: "uv (uvx runs the Python server)", install: "https://docs.astral.sh/uv/getting-started/installation/" },
  npx: { label: "Node.js (npx runs the server)", install: "https://nodejs.org/en/download" },
  node: { label: "Node.js", install: "https://nodejs.org/en/download" },
  python: { label: "Python", install: "https://www.python.org/downloads/" },
  python3: { label: "Python", install: "https://www.python.org/downloads/" },
  docker: { label: "Docker", install: "https://docs.docker.com/get-docker/" },
  pipx: { label: "pipx (runs the Python server)", install: "https://pipx.pypa.io/stable/installation/" },
};

/** Share one of your own tools as a recipe: signed with your Flowsta identity, no file, nothing of yours in it. */
export async function shareTool(
  server: McpServer,
  opts: { title: string; description: string; license: string; sourceUrl: string; also?: string; maker: Maker },
): Promise<ShareResult> {
  const launcher = (server.command ?? "").split(/[\\/]/).pop() ?? "";
  const needs = launcher && KNOWN_NEEDS[launcher] ? [{ program: launcher, ...KNOWN_NEEDS[launcher] }] : [];
  const mcp = {
    transport: server.transport,
    command: server.transport === "stdio" ? server.command : undefined,
    args: server.transport === "stdio" ? server.args : undefined,
    url: server.transport === "http" ? server.url : undefined,
    needs,
    config: (server.config ?? []).map((f) => ({ key: f.key, label: f.label, kind: f.kind, required: !!f.required, hint: f.hint ?? "", where: f.where ?? "", prefix: f.prefix ?? "" })),
    also: opts.also || undefined,
  };
  const canonical = recipeCanonical({ name: opts.title, description: opts.description, license: opts.license, source_url: opts.sourceUrl, mcp });
  const digest = new Uint8Array(await crypto.subtle.digest("SHA-256", new TextEncoder().encode(canonical)));
  const res = await invoke<{ signature: string; agent_pub_key: string }>("vault_sign", {
    bytesB64: bytesToBase64(digest),
    reason: `Share tool "${opts.title}"`,
  });
  if (!res.signature || !res.agent_pub_key) throw new Error("sign_failed");
  const signature = { algo: "ed25519", agent_pub_key: res.agent_pub_key, value: res.signature };
  const manifest = {
    schema: 1,
    kind: "mcp",
    name: opts.title,
    description: opts.description,
    license: opts.license,
    terms: "free",
    maker: opts.maker,
    source: { kind: "url", url: opts.sourceUrl },
    runs_programs: true,
    mcp,
  };
  return invoke<ShareResult>("share_submit", { submission: { kind: "mcp", manifest, signature } });
}
