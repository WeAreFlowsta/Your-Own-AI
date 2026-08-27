/**
 * Skills - folders of instructions an AI reads when the work calls for it
 * (SKILL.md + supporting files). Installed under the Build agent's own
 * skills folder, so projects discover them on their own; the chat path
 * hands the SKILL.md text to the model directly. All calls go to the
 * Rust side (src-tauri/src/skills.rs).
 */
import { invoke } from "@tauri-apps/api/core";

export interface SkillSource {
  kind: "folder" | "zip" | "link";
  url?: string;
  ref?: string;
  sha?: string;
  path?: string;
  installed_at: number;
}

export interface SkillInfo {
  name: string;
  description: string;
  dir: string;
  files: number;
  skill_md_chars: number;
  /** SKILL.md in tokens - what a chat turn pays to carry it. */
  tokens: number;
  /** Ships scripts, hooks or MCP servers (text-only skills never run anything). */
  runs_programs: boolean;
  source: SkillSource | null;
}

/** Above this a skill takes a real bite out of a small model's context. */
export const LARGE_SKILL_TOKENS = 4000;

export async function listSkills(): Promise<SkillInfo[]> {
  try {
    return await invoke<SkillInfo[]>("skills_list");
  } catch (e) {
    console.warn("[skills] list failed", e);
    return [];
  }
}

export function tokensLabel(n: number): string {
  if (n >= 1000) return `~${(n / 1000).toFixed(n >= 10000 ? 0 : 1)}k tokens`;
  return `~${n} tokens`;
}

/** "github.com/owner/repo @ 1a2b3c4" / "from a folder" / "from a zip file". */
export function sourceLabel(s: SkillSource | null): string {
  if (!s) return "added by hand";
  if (s.kind === "link" && s.url) {
    const short = s.url.replace(/^https?:\/\//, "");
    return s.sha ? `${short} @ ${s.sha.slice(0, 7)}` : short;
  }
  if (s.kind === "zip") return "from a zip file";
  return "from a folder";
}

/** Which active AIs have chosen a skill. */
export function usedBy(skill: string, ais: { name: string; status: string; skills?: string[] | null }[]): string[] {
  return ais
    .filter((a) => a.status === "active" && Array.isArray(a.skills) && a.skills.includes(skill))
    .map((a) => a.name);
}

export interface SkillsBlock {
  block: string;
  names: string[];
}

/** The chat path's skills block for one turn: the AI's chosen skills, the
 *  full text of the one(s) matching the question. Nothing chosen = "". */
export async function skillsPromptBlock(
  names: string[] | undefined,
  query: string,
  queryVec?: number[] | null,
): Promise<SkillsBlock> {
  if (!names || names.length === 0) return { block: "", names: [] };
  try {
    return await invoke<SkillsBlock>("skills_prompt_block", { names, query, queryVec: queryVec ?? null });
  } catch (e) {
    console.warn("[skills] prompt block unavailable", e);
    return { block: "", names: [] };
  }
}

/** The installed skills folder as it appears in a path, any OS. */
const SKILLS_DIR_RE = /[\/]\.your-own-ai-build[\/]skills[\/]([^\/]+)[\/]/;

/** If `path` is inside an installed skill, that skill's name. */
export function skillNameFromPath(path: string | undefined): string | null {
  if (!path) return null;
  const m = path.match(SKILLS_DIR_RE);
  return m ? m[1] : null;
}
