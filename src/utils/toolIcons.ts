import { invoke } from "@tauri-apps/api/core";

/**
 * Site icons fetched for hand-added tools (Rust `mcp_fetch_icon`), by tool
 * name, as data URLs. Loaded once per app run; the rail and the tools page
 * read the map directly. Drawn monochrome like every other glyph.
 */
const icons = new Map<string, string>();
let loaded = false;

export async function loadToolIcons(): Promise<void> {
  try {
    const m = await invoke<Record<string, string>>("mcp_icons");
    icons.clear();
    for (const [k, v] of Object.entries(m)) icons.set(k.toLowerCase(), v);
    loaded = true;
  } catch {
    /* no icons */
  }
}

export function toolIconsLoaded(): boolean {
  return loaded;
}

/** The fetched icon for a tool name, if there is one. */
export function toolImage(name: string | undefined): string | undefined {
  if (!name) return undefined;
  return icons.get(name.toLowerCase()) ?? icons.get(name.replace(/[^A-Za-z0-9_-]/g, "_").toLowerCase());
}

/** Fetch a tool's site icon on the person's say-so and keep it. */
export async function fetchToolIcon(name: string): Promise<string> {
  const url = await invoke<string>("mcp_fetch_icon", { name });
  icons.set(name.toLowerCase(), url);
  icons.set(name.replace(/[^A-Za-z0-9_-]/g, "_").toLowerCase(), url);
  return url;
}
