/**
 * Add-ons > Tools - MCP servers an AI may use in a project session.
 * The list is the agent's (`~/.your-own-ai-build/mcp-servers.json`); which
 * servers an AI carries is chosen on the AI, like skills.
 */
import { invoke } from "@tauri-apps/api/core";
import type { UserDefinedAI } from "../types";
import { directoryItems, type DirectoryItem } from "./directory";

export interface McpServer {
  name: string;
  description: string;
  transport: "stdio" | "http";
  command?: string;
  args: string[];
  env: [string, string][];
  url?: string;
  source: string;
  added_at: number;
}

export async function listMcpServers(): Promise<McpServer[]> {
  try {
    return await invoke<McpServer[]>("mcp_list");
  } catch {
    return [];
  }
}
export function addMcpServer(server: McpServer): Promise<McpServer[]> {
  return invoke<McpServer[]>("mcp_add", { server });
}
export function removeMcpServer(name: string): Promise<McpServer[]> {
  return invoke<McpServer[]>("mcp_remove", { name });
}
/** Path of a program on this machine, or null when it is not installed. */
export function whichProgram(program: string): Promise<string | null> {
  return invoke<string | null>("mcp_which", { program });
}
export interface RequirementPlan { mode: "run" | "terminal" | "link"; command: string; note: string }
/** How this machine can get a program a tool needs. */
export function requirementPlan(program: string): Promise<RequirementPlan> {
  return invoke<RequirementPlan>("mcp_requirement_plan", { program });
}
/** Run a "run"-mode plan in the app; resolves with the installer's last lines. */
export function requirementInstall(program: string): Promise<string> {
  return invoke<string>("mcp_requirement_install", { program });
}
/** Clone (or update) a preset's source under the home folder; returns the path. */
export function fetchGit(url: string, dest: string): Promise<string> {
  return invoke<string>("mcp_fetch_git", { url, dest });
}
export function mcpUsedBy(ais: UserDefinedAI[], name: string): string[] {
  return ais.filter((a) => a.status === "active" && Array.isArray(a.mcp) && a.mcp.includes(name)).map((a) => a.name);
}

/** One line of what a server is, for lists. */
export function mcpSummary(s: McpServer): string {
  if (s.transport === "http") return s.url ?? "";
  return [s.command, ...s.args].filter(Boolean).join(" ");
}

/**
 * Presets: servers we know how to set up. Each says what it needs; the page
 * checks the program is there before adding (a consented install pointer
 * otherwise, never a silent download).
 */
export interface McpPreset {
  id: string;
  name: string;
  title: string;
  blurb: string;
  needs: { program: string; label: string; install: string }[];
  notes: string;
  /** The one download a preset needs, shown before anything is fetched. */
  fetch?: { url: string; dest: string; size: string };
  build: () => McpServer;
}

export const MCP_PRESETS: McpPreset[] = [
  {
    id: "blender",
    name: "blender",
    title: "Blender",
    blurb:
      "Analyze a scene, find what is slowing it down, search the Python API docs, run code in Blender, take a viewport screenshot, render a thumbnail. The official Blender Lab server.",
    needs: [
      { program: "uv", label: "uv (runs the Python server)", install: "https://docs.astral.sh/uv/getting-started/installation/" },
      { program: "git", label: "git (fetches the server once)", install: "https://git-scm.com/downloads" },
    ],
    notes:
      "Also needs Blender 5.1 or newer with the Blender Lab MCP add-on installed and running (blender.org/lab/mcp-server). The AI runs code in Blender only through your approve step.",
    fetch: { url: "https://projects.blender.org/lab/blender_mcp.git", dest: "blender_mcp", size: "a few MB" },
    build: () => ({
      name: "blender",
      description: "Blender - scenes, the Python API, code, screenshots, renders (Blender Lab)",
      transport: "stdio",
      command: "uv",
      args: ["--directory", "~/blender_mcp/mcp", "run", "blender-mcp"],
      env: [],
      source: "preset:blender",
      added_at: 0,
    }),
  },
];

/** A directory tool listing as a preset the page can add. */
export function presetFromDirectory(d: DirectoryItem): McpPreset | null {
  const r = d.mcp;
  if (d.kind !== "mcp" || !r) return null;
  return {
    id: d.id,
    name: d.id,
    title: d.name,
    blurb: d.description,
    needs: r.needs ?? [],
    notes: r.also ?? "",
    fetch: r.fetch,
    build: () => ({
      name: d.id,
      description: d.title ? `${d.name} - ${d.title}` : d.name,
      transport: r.transport,
      command: r.transport === "stdio" ? r.command : undefined,
      args: r.transport === "stdio" ? (r.args ?? []) : [],
      env: [],
      url: r.transport === "http" ? r.url : undefined,
      source: `directory:${d.id}`,
      added_at: 0,
    }),
  };
}

/**
 * "Ready to add": the directory's tool listings (reviewed, updated without
 * an app release), with the built-in presets as the offline fallback. Same
 * id = the directory wins.
 */
export async function readyPresets(): Promise<McpPreset[]> {
  const items = await directoryItems();
  const fromDir = (items ?? []).map(presetFromDirectory).filter((p): p is McpPreset => !!p);
  const seen = new Set(fromDir.map((p) => p.id));
  return [...fromDir, ...MCP_PRESETS.filter((p) => !seen.has(p.id))];
}
