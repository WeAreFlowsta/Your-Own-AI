/**
 * Auto permissions for the agent - the user's standing instruction that
 * ordinary work INSIDE the project folder may run without asking.
 *
 * Off by default, everywhere. Two layers:
 *   - a default for new projects (Settings > Agent),
 *   - a per-folder choice (the project chip in the header), which wins.
 * Plus one separate opt-in: "let the AI judge grey areas" - when a command
 * isn't on the harness's routine list, ask the AI's model whether it is
 * ordinary work before asking the user. For an online AI that sends the
 * command to the provider (a small call), so it is its own switch.
 *
 * The harness does the judging (its rule-based, fail-closed classifier;
 * unknown => ask). Every decision - auto or not - is written to the user's
 * records by useAgentSession.
 */

export type AgentPermissionMode = "ask" | "auto";

const KEY_DEFAULT = "agentPermissionsDefault";
const KEY_JUDGE = "agentPermissionsJudge";
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

/** Default mode for projects without their own choice. Off unless set. */
export function defaultPermissionMode(): AgentPermissionMode {
  return read(KEY_DEFAULT) === "auto" ? "auto" : "ask";
}

export function setDefaultPermissionMode(mode: AgentPermissionMode): void {
  write(KEY_DEFAULT, mode === "auto" ? "auto" : null);
}

/** Whether the AI's model may judge grey-area commands (off unless set). */
export function judgeEnabled(): boolean {
  return read(KEY_JUDGE) === "true";
}

export function setJudgeEnabled(on: boolean): void {
  write(KEY_JUDGE, on ? "true" : null);
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
  const own = byFolder()[path];
  return own === "auto" || own === "ask" ? own : defaultPermissionMode();
}

/** Remember a folder's own choice (null = forget it, fall back to the default). */
export function setPermissionModeForFolder(path: string, mode: AgentPermissionMode | null): void {
  const map = byFolder();
  if (mode === null) delete map[path];
  else map[path] = mode;
  write(KEY_BY_FOLDER, Object.keys(map).length ? JSON.stringify(map) : null);
}

/** Copy shared by Settings and the first-switch Callout. */
export const AUTO_PERMISSIONS_COPY = {
  title: "Run ordinary project work without asking",
  body:
    "Your AI reads, searches, edits files inside the project folder, builds, tests, and runs routine git without asking. Anything that reaches beyond the folder, cannot be undone, or that it isn't sure about still asks. Every decision is written to your records.",
  judgeTitle: "Let the AI judge grey areas",
  judgeBody:
    "When a command isn't on the routine list, ask the AI's model whether it is ordinary project work before asking you. Off: anything not on the list asks you. For an online AI this sends the command to the provider - a small call each time.",
};
