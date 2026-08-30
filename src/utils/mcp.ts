/**
 * Add-ons > Tools - MCP servers an AI may use in a project session.
 * The list is the agent's (`~/.your-own-ai-build/mcp-servers.json`); which
 * servers an AI carries is chosen on the AI, like skills.
 */
import { invoke } from "@tauri-apps/api/core";
import type { UserDefinedAI } from "../types";
import { directoryItems, type DirectoryItem } from "./directory";

export interface ConfigField {
  key: string;
  label: string;
  kind: "url" | "secret" | "text" | "path";
  required?: boolean;
  hint?: string;
  where?: string;
  prefix?: string;
}
export interface McpServer {
  name: string;
  description: string;
  transport: "stdio" | "http";
  command?: string;
  args: string[];
  env: [string, string][];
  url?: string;
  /** Settings the tool asks for (from its listing); values are kept on this device. */
  config?: ConfigField[];
  values?: Record<string, string>;
  /** How to use it well - handed to the agent with the tool. */
  guidance?: string;
  source: string;
  /** The clone the app fetched (`~/<dest>`), if any - for "Check for updates". */
  fetch_dir?: string;
  added_at: number;
}
export interface SourceStatus { behind: boolean; local: string; remote: string }
/** One explicit network call: is the fetched source behind? */
export function checkToolSource(name: string): Promise<SourceStatus> {
  return invoke<SourceStatus>("mcp_source_check", { name });
}
/** Fast-forward the fetched source; resolves with the new short commit. */
export function updateToolSource(name: string): Promise<string> {
  return invoke<string>("mcp_source_update", { name });
}
export interface BlenderAddonStatus { blender: string | null; installed: boolean; source_present: boolean; listening: boolean }
export function blenderAddonStatus(): Promise<BlenderAddonStatus> {
  return invoke<BlenderAddonStatus>("mcp_blender_addon_status");
}
export function blenderAddonInstall(): Promise<string> {
  return invoke<string>("mcp_blender_addon_install");
}
/** Save a tool's settings; secret-kind fields go to the encrypted store. */
export function setToolConfig(name: string, values: Record<string, string>): Promise<McpServer[]> {
  return invoke<McpServer[]>("mcp_set_config", { name, values });
}
/** Which settings are filled in (secrets reported as present, never returned). */
export function toolConfigStatus(name: string): Promise<Record<string, boolean>> {
  return invoke<Record<string, boolean>>("mcp_config_status", { name });
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

/** The block the first project prompt carries for the tools an AI has: how to use each well. */
export async function toolsGuidanceBlock(names: string[] | undefined): Promise<string> {
  if (!names?.length) return "";
  const all = await listMcpServers();
  const lines = names
    .map((n) => all.find((s) => s.name === n))
    .filter((s): s is McpServer => !!s && !!s.guidance)
    .map((s) => `- ${s.name}: ${s.guidance}`);
  if (!lines.length) return "";
  return `<tools_you_carry>\nThese tool servers are attached to this session. Do the work through their tools, not terminal workarounds (no python, pip or app binaries from the shell to reach what a tool already reaches) - the tools act where the person is looking.\n${lines.join("\n")}\n</tools_you_carry>\n\n`;
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
      // mcp pinned below 2.0: the Blender Lab code still uses the older Python API (2026-08).
      args: ["--directory", "~/blender_mcp/mcp", "run", "--with", "mcp[cli]<2", "blender-mcp"],
      env: [],
      guidance: "Blender is open and connected to you through its add-on. Make every change with execute_blender_code in that live session - the person watches it happen in their viewport. Work in small steps: several short calls of a few seconds each rather than one long script, so Blender stays responsive and the person sees progress; keep geometry simple unless asked for detail. Never run blender --background, --python or --python-expr from the terminal on the open file: that edits a second copy on disk that the open Blender does not show; never run python from the terminal either - Blender's own Python is inside the tool. If a tool returns a picture you cannot see, verify with get_objects_summary instead. Look before you act (get_objects_summary), do not save the file unless asked, and use the _for_cli variants only when no Blender is open.",
      source: "preset:blender",
      fetch_dir: "~/blender_mcp",
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
      config: r.config ?? [],
      guidance: r.guidance,
      source: `directory:${d.id}`,
      fetch_dir: r.fetch ? `~/${r.fetch.dest}` : undefined,
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
