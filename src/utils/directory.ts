/**
 * The add-ons directory (WeAreFlowsta/your-own-ai-add-ons): `index.json` lists
 * every reviewed character and skill with raw file URLs. Shelves read it
 * first and fall back to what ships in the app when it cannot be fetched
 * (offline, or before the repo exists). Cached per session.
 */
export const DIRECTORY_INDEX_URL = "https://raw.githubusercontent.com/WeAreFlowsta/your-own-ai-add-ons/main/index.json";

export interface DirectoryItem {
  schema: number;
  kind: "character" | "skill" | "mcp";
  id: string;
  name: string;
  title?: string;
  group?: string;
  description: string;
  maker: { name: string; handle: string | null; agent_pub_key: string | null };
  license: string;
  terms: string;
  file?: string;
  sha256?: string;
  portrait?: string;
  source: { kind: string; repo?: string; commit?: string; path?: string; url?: string; ref?: string };
  /** Tools (kind "mcp"): how to start the server and what it needs. */
  mcp?: {
    transport: "stdio" | "http";
    command?: string;
    args?: string[];
    url?: string;
    needs?: { program: string; label: string; install: string }[];
    fetch?: { url: string; dest: string; size: string };
    also?: string;
  };
  size_chars?: number;
  runs_programs?: boolean;
  signed: boolean;
  listed_by: string;
  listed_at: string;
  claimed?: boolean;
  dir: string;
  file_url: string | null;
  portrait_url: string | null;
  page: string;
}

let cached: DirectoryItem[] | null = null;

export async function directoryItems(): Promise<DirectoryItem[] | null> {
  if (cached) return cached;
  try {
    const res = await fetch(DIRECTORY_INDEX_URL, { cache: "no-cache" });
    if (!res.ok) return null;
    const data = (await res.json()) as { schema: number; items: DirectoryItem[] };
    if (!Array.isArray(data.items)) return null;
    cached = data.items;
    return cached;
  } catch {
    return null;
  }
}
