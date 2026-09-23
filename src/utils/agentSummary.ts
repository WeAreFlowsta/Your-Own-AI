import type { AgentLogItem } from "../types";
import { iconForKind } from "../components/ActionIcon";

const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
const times = (n: number) => (n === 1 ? "once" : n === 2 ? "twice" : `${n} times`);

/** The finished turn's one line: "Read 4 files, ran 3 commands, edited 2 files". */
export function summaryOf(log: AgentLogItem[]): string {
  const counts: Record<string, number> = {};
  for (const item of log) {
    if (item.type !== "action") continue;
    const icon = item.action.icon ?? iconForKind(item.action.kind);
    counts[icon] = (counts[icon] ?? 0) + 1;
  }
  const parts: string[] = [];
  const reads = (counts.read ?? 0) + (counts.folder ?? 0);
  if (reads) parts.push(`read ${plural(reads, "file", "files")}`);
  if (counts.search) parts.push(`searched ${times(counts.search)}`);
  if (counts.run) parts.push(`ran ${plural(counts.run, "command", "commands")}`);
  if (counts.edit) parts.push(`edited ${plural(counts.edit, "file", "files")}`);
  if (counts.delete) parts.push(`deleted ${plural(counts.delete, "file", "files")}`);
  if (counts.web) parts.push(`used the web ${times(counts.web)}`);
  if (counts.helper) parts.push(`started ${plural(counts.helper, "helper", "helpers")}`);
  if (counts.mcp) parts.push(`used ${plural(counts.mcp, "tool", "tools")}`);
  if (counts.skill) parts.push(`used ${plural(counts.skill, "skill", "skills")}`);
  const s = parts.join(", ");
  return s ? s.charAt(0).toUpperCase() + s.slice(1) : "";
}

