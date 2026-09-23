// The rail's labels, without the app: every tool the agent has, as the
// harness really sends it (wire name + kind + label in `_meta["x.ai/tool"]`,
// its title, its rawInput as captured from recorded sessions on 2026-09-23),
// through `describeAction`. Runs in `npm run build`. If a label here is
// wrong, the fix is the table in src/utils/actionLabels.ts - never a
// special case in the event handler.
import { readFileSync } from "node:fs";
import { transformSync } from "esbuild";

const src = readFileSync("src/utils/actionLabels.ts", "utf8");
const js = transformSync(src.replace(/^export /gm, ""), { loader: "ts" }).code;
const { describeAction, subjectOfLabel, errorOf } = new Function(
  `${js}; return { describeAction, subjectOfLabel, errorOf };`,
)();

/** An ACP tool call as the harness stamps it. */
const call = (name, kind, label, title, rawInput, extra = {}) => ({
  toolCallId: `call_${name}`,
  title,
  kind,
  rawInput,
  _meta: { "x.ai/tool": { version: 1, name, kind, namespace: "grok_build", label, read_only: false } },
  ...extra,
});

const ctx = {
  subjectOf: (id) => ({ call_gen: "generate-articles.sh", call_helper: "the helper: find every page without a title" })[id],
  skillNameFromPath: (p) => (p && p.includes("/skills/release-notes/") ? "Release notes" : null),
  serverLabel: (s) => ({ obsidian: "Obsidian", logseq: "Logseq", blender: "Blender" })[s],
};

let bad = 0, n = 0;
const eq = (what, got, want) => {
  n++;
  if (got !== want) {
    console.error(`action labels: ${what}\n   got  ${JSON.stringify(got)}\n   want ${JSON.stringify(want)}`);
    bad++;
  }
};
const check = (name, update, want) => {
  const d = describeAction(update, ctx);
  eq(`${name} label`, d.label, want.label);
  if (want.done) eq(`${name} done`, d.labelDone, want.done);
  eq(`${name} icon`, d.icon, want.icon);
  if (want.kind) eq(`${name} kind`, d.kind, want.kind);
  if (want.waitFor) eq(`${name} waitFor`, JSON.stringify(d.waitFor), JSON.stringify(want.waitFor));
  if (want.server) eq(`${name} server`, d.server, want.server);
  if (want.specificity != null) eq(`${name} specificity`, d.specificity, want.specificity);
  return d;
};

// --- the tools, as recorded ------------------------------------------------
check("read_file",
  call("read_file", "read", "Read", "Read `build-docs/ARTICLE_GENERATION_GUIDE.md`",
    { target_file: "build-docs/ARTICLE_GENERATION_GUIDE.md", offset: 1, limit: 220 }),
  { label: "Reading ARTICLE_GENERATION_GUIDE.md", done: "Read ARTICLE_GENERATION_GUIDE.md", icon: "read", kind: "read", specificity: 3 });

check("read_file inside a skill",
  call("read_file", "read", "Read", "Read `~/.your-own-ai-build/skills/release-notes/SKILL.md`",
    { target_file: "/home/e/.your-own-ai-build/skills/release-notes/SKILL.md" }),
  { label: "Using skill: Release notes", done: "Used skill: Release notes", icon: "skill", kind: "skill" });

check("list_dir",
  call("list_dir", "list", "List Files", "List `.`", { target_directory: "." }),
  { label: "Looking through the project", done: "Looked through the project", icon: "folder", kind: "list" });

check("list_dir subfolder",
  call("list_dir", "list", "List Files", "List `src/routes`", { target_directory: "src/routes" }),
  { label: "Looking through routes/", icon: "folder" });

check("grep",
  call("grep", "search", "Search", "balanced|candidate|publish",
    { pattern: "balanced|candidate|publish", path: ".", glob: "*.{md,json}", "-i": true, head_limit: 200 }),
  { label: 'Searching for "balanced|candidate|publish"', done: 'Searched for "balanced|candidate|publish"', icon: "search", kind: "search" });

check("search_replace",
  call("search_replace", "edit", "Edit", "Edit `tmp/editorial-corrections-2026-09-10.json`",
    { file_path: "tmp/editorial-corrections-2026-09-10.json", old_string: "{\n", new_string: "{\n  x" }),
  { label: "Editing editorial-corrections-2026-09-10.json", done: "Edited editorial-corrections-2026-09-10.json", icon: "edit", kind: "edit" });

check("write",
  call("write", "edit", "Write", "Write `/home/e/site/tmp/verify.mjs`",
    { file_path: "/home/e/site/tmp/verify.mjs", content: "import x" }),
  { label: "Writing verify.mjs", done: "Wrote verify.mjs", icon: "edit", kind: "edit" });

check("command with its description",
  call("run_terminal_command", "execute", "Run Command", "Execute `git status --short --branch && git log -5 --oneline`",
    { command: "git status --short --branch && git log -5 --oneline", timeout: 120000,
      description: "Check the current branch, working tree, and recent article-publishing commits before making changes.", background: false }),
  { label: "Check the current branch, working tree, and recent article-publishing...", done: "Check the current branch, working tree, and recent article-publishing...", icon: "run", kind: "execute" });

check("command without a description",
  call("run_terminal_command", "execute", "Run Command", "Execute `npm run build`",
    { command: "npm run build", timeout: 120000 }),
  { label: "Running npm run build", done: "Ran npm run build", icon: "run" });

check("backgrounded command",
  call("run_terminal_command", "execute", "Run Command", "Execute `./generate-articles.sh`",
    { command: "./generate-articles.sh", background: true, description: "Regenerate the articles with the new template" }),
  { label: "Regenerate the articles with the new template", icon: "run" });

// --- the family that used to read "Waiting for a background task" --------
check("wait on one task, named after it",
  call("get_command_or_subagent_output", "other", "Background Task", "Get task output: call_gen",
    { task_ids: ["call_gen"], timeout_ms: 600000 }),
  { label: "Waiting for generate-articles.sh", done: "Waited for generate-articles.sh", icon: "wait", kind: "wait", waitFor: ["call_gen"], specificity: 3 });

check("wait on a helper",
  call("get_command_or_subagent_output", "other", "Background Task", "Get task output: call_helper",
    { task_ids: ["call_helper"], timeout_ms: 600000 }),
  { label: "Waiting for the helper: find every page without a title", icon: "wait" });

check("wait on an unknown task still says what it is",
  call("get_command_or_subagent_output", "other", "Background Task", "Get task output: call_x",
    { task_id: "call_x" }),
  { label: "Waiting for a background task", icon: "wait", waitFor: ["call_x"], specificity: 2 });

check("wait on three tasks",
  call("get_command_or_subagent_output", "other", "Background Task", "Get task output: 3 tasks",
    { task_ids: ["call_a", "call_b", "call_c"] }),
  { label: "Waiting for 3 steps", icon: "wait", waitFor: ["call_a", "call_b", "call_c"] });

check("kill is a stop, not a wait",
  call("kill_command_or_subagent", "other", "Kill Task", "Kill task: call_gen", { task_id: "call_gen" }),
  { label: "Stopping generate-articles.sh", done: "Stopped generate-articles.sh", icon: "stop", kind: "stop" });

check("spawn a helper",
  call("spawn_subagent", "other", "Subagent", "Subagent",
    { prompt: "Find every page without a title\nList them with paths.", subagent_type: "explore" }),
  { label: "Starting a helper: Find every page without a title", done: "Started a helper: Find every page without a title", icon: "helper", kind: "helper" });

check("message a helper",
  call("send_subagent_message", "other", "Send Subagent Message", "Message subagent", { task_id: "call_helper", message: "Skip the drafts folder" }),
  { label: "Messaging the helper: Skip the drafts folder", icon: "helper" });

// --- the web ---------------------------------------------------------------
check("web_search",
  call("web_search", "other", "Web Search", 'Web search: "qwik city sitemap"', { query: "qwik city sitemap" }),
  { label: 'Searching the web for "qwik city sitemap"', done: 'Searched the web for "qwik city sitemap"', icon: "web", kind: "web" });

check("web_fetch",
  call("web_fetch", "fetch", "Web Fetch", "Fetch: https://yourownai.net/docs/getting-started/", { url: "https://yourownai.net/docs/getting-started/" }),
  { label: "Reading yourownai.net/docs/getting-started", done: "Read yourownai.net/docs/getting-started", icon: "web" });

// --- MCP, skills, memory, plan ---------------------------------------------
check("use_tool project memory",
  call("use_tool", "other", "Use Tool", "Use Tool", { tool_name: "project-memory__read_project_memory", tool_input: {} }),
  { label: "Reading the project's memory", done: "Read the project's memory", icon: "mcp", kind: "mcp", server: "project-memory" });

check("use_tool obsidian",
  call("use_tool", "other", "Use Tool", "Use Tool", { tool_name: "obsidian__read_note", tool_input: { path: "Boat survey.md" } }),
  { label: "Obsidian: read note", icon: "mcp", server: "obsidian" });

check("use_tool blender",
  call("use_tool", "other", "Use Tool", "Use Tool", { tool_name: "blender__execute_blender_code", tool_input: { code: "bpy.ops" } }),
  { label: "Blender: execute blender code", icon: "mcp", server: "blender" });

check("search_tool",
  call("search_tool", "other", "Search Tools", 'Search tools: "project-memory recall"', { query: "project-memory recall", limit: 5 }),
  { label: 'Looking for a tool: "project-memory recall"', icon: "search" });

check("skill",
  call("skill", "other", "Skill", "Skill: release-notes", { skill: "release-notes", args: "" }),
  { label: "Using skill: release-notes", done: "Used skill: release-notes", icon: "skill", kind: "skill" });

check("memory_search",
  call("memory_search", "other", "Memory Search", 'Memory search: "sitemap"', { query: "sitemap" }),
  { label: 'Searching memory for "sitemap"', icon: "memory" });

check("todo_write is the plan (row suppressed by the handler)",
  call("todo_write", "think", "Plan", "Update TODOs", { merge: false, todos: [{ id: "a", content: "x", status: "pending" }] }),
  { label: "Planning", icon: "plan", kind: "plan" });

check("ask_user_question",
  call("ask_user_question", "other", "Ask User", "Ask: Which folder?", { questions: [{ question: "Which folder?" }] }),
  { label: "Asking you a question", icon: "ask" });

// --- fallbacks: a tool the table does not know ---------------------------
check("unknown tool with an ACP kind and a path",
  call("some_new_reader", "read", "Read", "Read `notes.md`", { path: "notes.md" }),
  { label: "Reading notes.md", icon: "read", specificity: 1 });

check("unknown tool with only a title",
  call("sports_search", "other", "Sports Search", "Sports search: NFL scores", { league: "nfl" }),
  { label: "Sports search: NFL scores", icon: "tool", specificity: 0 });

check("unknown tool with an Execute title",
  { toolCallId: "call_t", title: "Execute `ls -la`", kind: "other", rawInput: {} },
  { label: "Running ls -la", done: "Ran ls -la", icon: "tool", specificity: 0 });

// --- updates: the specific label wins ------------------------------------
{
  const first = describeAction(call("get_command_or_subagent_output", "other", "Background Task", "Get task output: call_gen", { task_ids: ["call_gen"] }), ctx);
  const later = describeAction({ toolCallId: "call_w", title: "Background Task", kind: "other", rawInput: {} }, ctx);
  eq("a bare later update is less specific than the named wait", later.specificity < first.specificity, true);
}

// --- the subject a wait is named after ------------------------------------
eq("subject of a command", subjectOfLabel("Ran npm run build"), "npm run build");
eq("subject of a helper", subjectOfLabel("Started a helper: find pages"), "the helper: find pages");

// --- the error text of a failed call --------------------------------------
eq("error from content", errorOf({ status: "failed", content: [{ type: "content", content: { type: "text", text: "exit code 2\nerror TS2304" } }] }), "exit code 2\nerror TS2304");
eq("error from rawOutput", errorOf({ status: "failed", rawOutput: { error: "timeout" } }), "timeout");
eq("no error on success", errorOf({ status: "completed", rawOutput: "fine" }), undefined);
eq("a silent failure still says so", errorOf({ status: "failed" }), "The step failed without saying why.");

// --- never the old generic label, never Working ---------------------------
for (const u of [
  call("get_command_or_subagent_output", "other", "Background Task", "Get task output: call_gen", { task_ids: ["call_gen"] }),
  call("kill_command_or_subagent", "other", "Kill Task", "Kill task: call_gen", { task_id: "call_gen" }),
  { toolCallId: "x", kind: "other", rawInput: {} },
]) {
  const d = describeAction(u, ctx);
  eq(`never generic: ${d.label}`, d.label === "Waiting for a background task" || d.label === "Working..." || d.label === "Background Task", false);
}

if (bad) {
  console.error(`action labels: ${bad} of ${n} checks failed`);
  process.exit(1);
}
console.log(`action labels: ${n} checks ok`);
