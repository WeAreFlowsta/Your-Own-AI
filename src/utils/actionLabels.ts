/**
 * What a tool call reads as, in the rail: a label, its finished form, an
 * icon kind and what to keep as detail. Pure - no imports - so the build
 * check (`scripts/check-action-labels.mjs`) can run it against captured
 * events.
 *
 * Order of decision, most specific first:
 *   1. the tool's wire name (`_meta["x.ai/tool"].name`, else ACP `name`)
 *      with the fields that tool carries in `rawInput` / `meta.input`;
 *   2. the ACP kind (read, edit, delete, move, search, execute, fetch,
 *      think) with a path or term when one is there;
 *   3. the agent's own `title`, cleaned of its backticks.
 * Never "Working...". A later update only replaces a label with one at
 * least as specific (`specificity`), so a wait named after its task is
 * never overwritten by a bare "Background Task".
 */

export type IconKind =
  | "read"
  | "folder"
  | "search"
  | "edit"
  | "delete"
  | "move"
  | "run"
  | "web"
  | "helper"
  | "wait"
  | "stop"
  | "mcp"
  | "skill"
  | "plan"
  | "think"
  | "ask"
  | "memory"
  | "tool";

export interface ActionDescription {
  /** While it runs: "Reading package.json". */
  label: string;
  /** Once it is done: "Read package.json". */
  labelDone: string;
  /** The rail's own kind: read | list | search | edit | delete | execute |
   *  web | helper | wait | stop | mcp | skill | plan | think | ask | memory | tool. */
  kind: string;
  icon: IconKind;
  /** The expandable detail: the path, the command, the query, the input. */
  detail?: string;
  /** For a wait: the tool-call ids of the tasks it blocks on. */
  waitFor?: string[];
  /** The tool's wire name, kept in the record so the icon table works on
   *  reopen without guessing from the label. */
  name?: string;
  /** The MCP server for a `use_tool` call ("obsidian", "project-memory"). */
  server?: string;
  /** 3 = named tool with its subject; 2 = named tool, no subject;
   *  1 = ACP kind; 0 = the agent's title. */
  specificity: number;
}

export interface DescribeContext {
  /** The subject of an earlier step by its tool-call id ("generate.sh",
   *  "the helper: find every page without a title") - names a wait. */
  subjectOf?: (toolCallId: string) => string | undefined;
  /** The installed skill a path belongs to, if any. */
  skillNameFromPath?: (path: string | undefined) => string | null;
  /** A person-facing name for an MCP server id ("obsidian" -> "Obsidian"). */
  serverLabel?: (server: string) => string | undefined;
}

const basename = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() || p;

const cut = (s: string, n: number) => (s.length > n ? s.slice(0, n - 3).trimEnd() + "..." : s);

/** A command's own description when the agent gave one ("Check the
 *  current branch"), which reads as the row's whole label; else the
 *  command itself, which reads after "Running". */
function commandSubject(input: any): { text: string; described: boolean } | undefined {
  const d = typeof input.description === "string" ? input.description.trim() : "";
  if (d) return { text: cut(d.replace(/\.$/, ""), 72), described: true };
  const c = typeof input.command === "string" ? input.command.trim() : "";
  return c ? { text: cut(c, 48), described: false } : undefined;
}

function commandRow(input: any, name: string | undefined, specificity: number): ActionDescription {
  const subject = commandSubject(input);
  if (!subject) return make("Running a command", "execute", "run", Math.min(specificity, 2), { name });
  if (subject.described) {
    return { label: subject.text, labelDone: subject.text, kind: "execute", icon: "run", detail: input.command, name, specificity };
  }
  return make(`Running ${subject.text}`, "execute", "run", specificity, { detail: input.command, name });
}

function hostOf(url: string): string {
  try {
    const u = new URL(url);
    const path = u.pathname.replace(/\/$/, "");
    return cut(u.host + (path && path !== "/" ? path : ""), 60);
  } catch {
    return cut(url, 60);
  }
}

/** "<server>__<tool>" -> ["obsidian", "read note"]. */
function splitMcp(toolName: string): [string | undefined, string] {
  const i = toolName.indexOf("__");
  if (i < 0) return [undefined, toolName.replace(/[_-]+/g, " ").trim()];
  return [toolName.slice(0, i), toolName.slice(i + 2).replace(/[_-]+/g, " ").trim()];
}

const KNOWN_MCP: Record<string, [string, string]> = {
  "project-memory__remember_for_project": ["Remembering something for this project", "Remembered something for this project"],
  "project-memory__read_project_memory": ["Reading the project's memory", "Read the project's memory"],
};

function titleClean(title: string): string {
  return title.replace(/`/g, "").trim();
}

/** The agent's title as a present-tense label: "Execute ls" -> "Running ls". */
function fromTitle(title: string): [string, string] {
  const t = titleClean(title);
  const m = /^(\w+)(?::)?\s*(.*)$/.exec(t);
  if (!m) return [t, t];
  const verb = m[1].toLowerCase();
  const rest = m[2];
  const pairs: Record<string, [string, string]> = {
    execute: ["Running", "Ran"],
    read: ["Reading", "Read"],
    edit: ["Editing", "Edited"],
    write: ["Writing", "Wrote"],
    list: ["Looking through", "Looked through"],
    fetch: ["Reading", "Read"],
    search: ["Searching", "Searched"],
    skill: ["Using skill:", "Used skill:"],
    ask: ["Asking", "Asked"],
  };
  const p = pairs[verb];
  if (p && rest) return [`${p[0]} ${rest}`, `${p[1]} ${rest}`];
  return [t, t];
}

function done(label: string): string {
  // "Reading x" -> "Read x", "Editing x" -> "Edited x", and the rest.
  const swaps: [RegExp, string][] = [
    [/^Reading /, "Read "],
    [/^Editing /, "Edited "],
    [/^Writing /, "Wrote "],
    [/^Deleting /, "Deleted "],
    [/^Moving /, "Moved "],
    [/^Running /, "Ran "],
    [/^Looking through /, "Looked through "],
    [/^Looking for /, "Looked for "],
    [/^Searching /, "Searched "],
    [/^Waiting for /, "Waited for "],
    [/^Stopping /, "Stopped "],
    [/^Starting /, "Started "],
    [/^Messaging /, "Messaged "],
    [/^Using skill: /, "Used skill: "],
    [/^Remembering /, "Remembered "],
    [/^Asking /, "Asked "],
    [/^Planning$/, "Planned"],
  ];
  for (const [re, to] of swaps) if (re.test(label)) return label.replace(re, to);
  return label;
}

function make(
  label: string,
  kind: string,
  icon: IconKind,
  specificity: number,
  extra: Partial<ActionDescription> = {},
): ActionDescription {
  return { label, labelDone: done(label), kind, icon, specificity, ...extra };
}

export function describeAction(update: any, ctx: DescribeContext = {}): ActionDescription {
  const meta = update?._meta?.["x.ai/tool"] ?? {};
  const input: any = { ...(update?.rawInput ?? {}), ...(meta.input ?? {}) };
  const name: string | undefined =
    (typeof meta.name === "string" && meta.name) || (typeof update?.name === "string" && update.name) || undefined;
  const acpKind: string | undefined = typeof update?.kind === "string" ? update.kind : undefined;
  const metaKind: string | undefined = typeof meta.kind === "string" ? meta.kind : undefined;
  const title: string = typeof update?.title === "string" ? update.title : "";

  const path: string | undefined = input.path || input.target_file || input.file_path || input.file;
  const dir: string | undefined = input.directory || input.target_directory;
  const term: string | undefined = input.query || input.pattern || input.regex;

  // 1. By the tool's wire name.
  switch (name) {
    case "read_file": {
      const skill = ctx.skillNameFromPath?.(path);
      if (skill) return make(`Using skill: ${skill}`, "skill", "skill", 3, { detail: path, name });
      return path
        ? make(`Reading ${basename(path)}`, "read", "read", 3, { detail: path, name })
        : make("Reading a file", "read", "read", 2, { name });
    }
    case "list_dir":
      return dir && dir !== "."
        ? make(`Looking through ${basename(dir)}/`, "list", "folder", 3, { detail: dir, name })
        : make("Looking through the project", "list", "folder", 2, { name });
    case "grep":
      return term
        ? make(`Searching for "${cut(String(term), 48)}"`, "search", "search", 3, {
            detail: input.path && input.path !== "." ? `${term} in ${input.path}` : String(term),
            name,
          })
        : make("Searching the project", "search", "search", 2, { name });
    case "search_replace":
    case "edit":
    case "hashline_edit":
      return path
        ? make(`Editing ${basename(path)}`, "edit", "edit", 3, { detail: path, name })
        : make("Editing a file", "edit", "edit", 2, { name });
    case "write":
      return path
        ? make(`Writing ${basename(path)}`, "edit", "edit", 3, { detail: path, name })
        : make("Writing a file", "edit", "edit", 2, { name });
    case "delete":
    case "delete_file":
      return path
        ? make(`Deleting ${basename(path)}`, "delete", "delete", 3, { detail: path, name })
        : make("Deleting a file", "delete", "delete", 2, { name });
    case "run_terminal_command":
    case "bash":
      return commandRow(input, name, 3);
    case "get_command_or_subagent_output":
    case "wait_tasks": {
      const ids: string[] = Array.isArray(input.task_ids)
        ? input.task_ids.filter((t: unknown) => typeof t === "string")
        : typeof input.task_id === "string"
          ? [input.task_id]
          : [];
      const subjects = ids.map((id) => ctx.subjectOf?.(id)).filter((s): s is string => !!s);
      const what =
        ids.length === 1
          ? subjects[0] ?? "a background task"
          : ids.length > 1
            ? subjects.length === ids.length && ids.length <= 2
              ? subjects.join(" and ")
              : `${ids.length} steps`
            : "a background task";
      return make(`Waiting for ${what}`, "wait", "wait", subjects.length ? 3 : 2, {
        waitFor: ids.length ? ids : undefined,
        detail: ids.length > 1 ? `${ids.length} tasks` : undefined,
        name,
      });
    }
    case "kill_command_or_subagent": {
      const id = typeof input.task_id === "string" ? input.task_id : undefined;
      const subject = id ? ctx.subjectOf?.(id) : undefined;
      return make(`Stopping ${subject ?? "a background task"}`, "stop", "stop", subject ? 3 : 2, {
        waitFor: id ? [id] : undefined,
        name,
      });
    }
    case "spawn_subagent":
    case "task": {
      const brief: string =
        (typeof input.description === "string" && input.description) ||
        (typeof input.prompt === "string" && input.prompt.split("\n")[0]) ||
        "";
      return brief
        ? make(`Starting a helper: ${cut(brief.trim(), 72)}`, "helper", "helper", 3, { detail: input.prompt, name })
        : make("Starting a helper", "helper", "helper", 2, { name });
    }
    case "send_subagent_message": {
      const text = typeof input.message === "string" ? input.message : typeof input.text === "string" ? input.text : "";
      return text
        ? make(`Messaging the helper: ${cut(text.trim(), 60)}`, "helper", "helper", 3, { detail: text, name })
        : make("Messaging the helper", "helper", "helper", 2, { name });
    }
    case "web_search":
      return term
        ? make(`Searching the web for "${cut(String(term), 60)}"`, "web", "web", 3, { detail: String(term), name })
        : make("Searching the web", "web", "web", 2, { name });
    case "web_fetch":
      return typeof input.url === "string"
        ? make(`Reading ${hostOf(input.url)}`, "web", "web", 3, { detail: input.url, name })
        : make("Reading a web page", "web", "web", 2, { name });
    case "use_tool": {
      const toolName = typeof input.tool_name === "string" ? input.tool_name : "";
      let detail: string | undefined;
      try {
        detail =
          input.tool_input && Object.keys(input.tool_input).length
            ? JSON.stringify(input.tool_input, null, 1).slice(0, 400)
            : undefined;
      } catch {
        /* unserializable input */
      }
      if (!toolName) return make("Using a tool", "mcp", "mcp", 2, { name });
      const known = KNOWN_MCP[toolName];
      const [server, tool] = splitMcp(toolName);
      if (known) {
        return { label: known[0], labelDone: known[1], kind: "mcp", icon: "mcp", detail, name, server, specificity: 3 };
      }
      const serverLabel = server ? ctx.serverLabel?.(server) ?? server.replace(/[_-]+/g, " ") : undefined;
      const toolLabel = tool.charAt(0).toUpperCase() + tool.slice(1);
      const label = serverLabel ? `${serverLabel}: ${tool}` : toolLabel;
      return { label, labelDone: label, kind: "mcp", icon: "mcp", detail, name, server, specificity: 3 };
    }
    case "search_tool":
      return term
        ? make(`Looking for a tool: "${cut(String(term), 48)}"`, "search", "search", 3, { detail: String(term), name })
        : make("Looking for a tool", "search", "search", 2, { name });
    case "skill": {
      const skill = typeof input.skill === "string" ? input.skill : typeof input.name === "string" ? input.name : "";
      return skill
        ? make(`Using skill: ${skill}`, "skill", "skill", 3, { detail: input.args, name })
        : make("Using a skill", "skill", "skill", 2, { name });
    }
    case "memory_search":
      return term
        ? make(`Searching memory for "${cut(String(term), 48)}"`, "memory", "memory", 3, { detail: String(term), name })
        : make("Searching memory", "memory", "memory", 2, { name });
    case "memory_get":
      return path
        ? make(`Reading memory: ${basename(path)}`, "memory", "memory", 3, { detail: path, name })
        : make("Reading memory", "memory", "memory", 2, { name });
    case "todo_write":
    case "update_goal":
      return make("Planning", "plan", "plan", 2, { name });
    case "enter_plan_mode":
      return make("Planning before changing anything", "plan", "plan", 2, { name });
    case "exit_plan_mode":
      return { label: "Plan ready", labelDone: "Plan ready", kind: "plan", icon: "plan", name, specificity: 2 };
    case "ask_user_question":
      return make("Asking you a question", "ask", "ask", 2, { name });
  }

  // 2. By the ACP kind.
  const kind = acpKind || metaKind;
  switch (kind) {
    case "read":
      return path
        ? make(`Reading ${basename(path)}`, "read", "read", 1, { detail: path, name })
        : make("Reading a file", "read", "read", 1, { name });
    case "list":
    case "list_dir":
      return dir ? make(`Looking through ${basename(dir)}/`, "list", "folder", 1, { detail: dir, name }) : make("Looking through the project", "list", "folder", 1, { name });
    case "edit":
    case "write":
      return path
        ? make(`Editing ${basename(path)}`, "edit", "edit", 1, { detail: path, name })
        : make("Editing a file", "edit", "edit", 1, { name });
    case "delete":
      return path
        ? make(`Deleting ${basename(path)}`, "delete", "delete", 1, { detail: path, name })
        : make("Deleting a file", "delete", "delete", 1, { name });
    case "move":
      return make("Moving a file", "move", "move", 1, { name });
    case "search":
      return term ? make(`Searching for "${cut(String(term), 48)}"`, "search", "search", 1, { detail: String(term), name }) : make("Searching the project", "search", "search", 1, { name });
    case "execute":
      return commandRow(input, name, 1);
    case "fetch":
    case "web_fetch":
      return typeof input.url === "string"
        ? make(`Reading ${hostOf(input.url)}`, "web", "web", 1, { detail: input.url, name })
        : make("Reading a web page", "web", "web", 1, { name });
    case "web_search":
      return term ? make(`Searching the web for "${cut(String(term), 60)}"`, "web", "web", 1, { detail: String(term), name }) : make("Searching the web", "web", "web", 1, { name });
    case "think":
    case "plan":
      return make("Planning", "plan", "plan", 1, { name });
  }

  // 3. The agent's title.
  if (title) {
    const [label, labelDone] = fromTitle(title);
    return { label, labelDone, kind: kind ?? "tool", icon: "tool", name, specificity: 0 };
  }
  const fallback = typeof meta.label === "string" && meta.label ? meta.label : "Using a tool";
  return { label: fallback, labelDone: fallback, kind: kind ?? "tool", icon: "tool", name, specificity: 0 };
}

/** The subject of a step, for a wait or a stop to name: "Running x" -> "x". */
export function subjectOfLabel(label: string): string {
  return label.replace(/^(Running|Ran|Starting|Started|Reading|Read|Editing|Edited|Writing|Wrote) /, "").replace(/^a helper: /, "the helper: ");
}

/** The error text of a failed call, from what the agent returned. */
export function errorOf(update: any): string | undefined {
  if (update?.status !== "failed") return undefined;
  const texts: string[] = [];
  if (Array.isArray(update.content)) {
    for (const c of update.content) {
      const t = c?.content?.text ?? c?.text;
      if (typeof t === "string" && t.trim()) texts.push(t.trim());
    }
  }
  const ro = update.rawOutput;
  if (typeof ro === "string" && ro.trim()) texts.push(ro.trim());
  else if (ro && typeof ro === "object") {
    const t = ro.error ?? ro.message ?? ro.content;
    if (typeof t === "string" && t.trim()) texts.push(t.trim());
  }
  const text = texts.join("\n").trim();
  return text ? text.slice(-500) : "The step failed without saying why.";
}
