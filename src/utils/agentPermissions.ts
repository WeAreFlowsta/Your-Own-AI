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
  title: "Auto permissions",
  body:
    "Routine work inside the project folder - reading, edits, builds, tests, everyday git - runs without asking. Anything risky, irreversible, or outside the folder still asks. Every decision goes in your records.",
  judgeTitle: "Let the AI judge unusual commands",
  judgeBody:
    "When a command isn't clearly routine, the AI's model decides whether it's ordinary work before falling back to asking you. An online AI sends the command to its provider to decide.",
};

/** Auto permissions need harness v0.2.0+ (folder-bounded edits, decision
 *  records, card reasons). On an older install the toggles stay off with a
 *  pointer at the update card - Auto on v0.1.0 would mean upstream's
 *  semantics: edits allowed anywhere, no records. Dev builds (a manually
 *  set binary path with no install record) are treated as current. */
export const AUTO_PERMISSIONS_MIN_BUILD = "0.2.0";

export function buildSupportsAutoPermissions(installedVersion: string | null | undefined): boolean {
  if (!installedVersion) return true; // dev override path: no install record
  const parse = (v: string) => v.split(".").map((n) => parseInt(n, 10) || 0);
  const [a, b, c] = parse(installedVersion);
  const [x, y, z] = parse(AUTO_PERMISSIONS_MIN_BUILD);
  return a !== x ? a > x : b !== y ? b > y : c >= z;
}
