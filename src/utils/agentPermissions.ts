/**
 * Agent permissions - one three-level choice, per project:
 *
 *   "ask"  - every action asks (the default).
 *   "auto" - ordinary work inside the project folder runs without asking;
 *            risky, irreversible, or outside-the-folder actions ask. Grey
 *            commands are judged by the AI itself (it proposed them - the
 *            judge call sends nothing it hasn't already seen).
 *   "all"  - nothing asks. Every action still lands in your records.
 *
 * Off by default, everywhere. Settings holds the default for new projects;
 * the project chip holds each project's own choice, which wins.
 */

export type AgentPermissionMode = "ask" | "auto" | "all";

const KEY_DEFAULT = "agentPermissionsDefault";
const KEY_BY_FOLDER = "agentPermissionsByFolder";

function read(key: string): string | null {
  try {
    return typeof localStorage === "undefined" ? null : localStorage.getItem(key);
  } catch {
    return null;
  }
}

function write(key: string, value: string | null): void {
  try {
    if (typeof localStorage === "undefined") return;
    if (value === null) localStorage.removeItem(key);
    else localStorage.setItem(key, value);
  } catch {
    /* settings are a convenience; never throw into a click */
  }
}

function asMode(v: string | null | undefined): AgentPermissionMode | null {
  return v === "ask" || v === "auto" || v === "all" ? v : null;
}

/** Default mode for projects without their own choice. Ask unless set. */
export function defaultPermissionMode(): AgentPermissionMode {
  return asMode(read(KEY_DEFAULT)) ?? "ask";
}

export function setDefaultPermissionMode(mode: AgentPermissionMode): void {
  write(KEY_DEFAULT, mode === "ask" ? null : mode);
}

function byFolder(): Record<string, AgentPermissionMode> {
  try {
    const raw = read(KEY_BY_FOLDER);
    const parsed = raw ? (JSON.parse(raw) as unknown) : null;
    return parsed && typeof parsed === "object" ? (parsed as Record<string, AgentPermissionMode>) : {};
  } catch {
    return {};
  }
}

/** The mode a folder opens with: its own choice if it made one, else the default. */
export function permissionModeForFolder(path: string): AgentPermissionMode {
  return asMode(byFolder()[path]) ?? defaultPermissionMode();
}

/** Remember a folder's own choice (null = forget it, fall back to the default). */
export function setPermissionModeForFolder(path: string, mode: AgentPermissionMode | null): void {
  const map = byFolder();
  if (mode === null) delete map[path];
  else map[path] = mode;
  write(KEY_BY_FOLDER, Object.keys(map).length ? JSON.stringify(map) : null);
}

const KEY_BY_TOOLS_AI = "agentPermissionsByToolsAi";

function byToolsAi(): Record<string, AgentPermissionMode> {
  try {
    const raw = read(KEY_BY_TOOLS_AI);
    const parsed = raw ? (JSON.parse(raw) as unknown) : null;
    return parsed && typeof parsed === "object" ? (parsed as Record<string, AgentPermissionMode>) : {};
  } catch {
    return {};
  }
}

/** The mode an AI's tools-in-chat session opens with: its own choice, else the default. */
export function permissionModeForTools(aiId: string | undefined | null): AgentPermissionMode {
  return (aiId ? asMode(byToolsAi()[aiId]) : null) ?? defaultPermissionMode();
}

/** Remember an AI's choice for its tools-in-chat sessions. */
export function setPermissionModeForTools(aiId: string, mode: AgentPermissionMode): void {
  const map = byToolsAi();
  map[aiId] = mode;
  write(KEY_BY_TOOLS_AI, JSON.stringify(map));
}

/** One-line description per mode - the same words everywhere. */
export const PERMISSION_MODE_COPY: Record<AgentPermissionMode, { label: string; hint: string }> = {
  ask: { label: "Ask every time", hint: "Your AI asks before every command and file change." },
  auto: {
    label: "Auto",
    hint: "Ordinary work in the project folder runs without asking; risky, irreversible, or outside-the-folder actions ask.",
  },
  all: {
    label: "Approve everything",
    hint: "Nothing asks. Every action is still written to your records.",
  },
};

/** Auto/all need harness v0.2.0+ (folder boundary, decision records). Dev
 *  builds (a manually set binary path with no install record) count as
 *  current. */
export const AUTO_PERMISSIONS_MIN_BUILD = "0.2.0";

export function buildSupportsAutoPermissions(installedVersion: string | null | undefined): boolean {
  if (!installedVersion) return true; // dev override path: no install record
  const parse = (v: string) => v.split(".").map((n) => parseInt(n, 10) || 0);
  const [a, b, c] = parse(installedVersion);
  const [x, y, z] = parse(AUTO_PERMISSIONS_MIN_BUILD);
  return a !== x ? a > x : b !== y ? b > y : c >= z;
}
