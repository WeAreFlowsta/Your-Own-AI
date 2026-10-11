// Memory rows (build-docs apps/your-own-ai/planning/MEMORY_REDESIGN.md §3):
// typed, dated, sealed records in the person's own network, in a scope -
// the person, one AI (by founding key), a project (by folder identity), a
// conversation. A correction or a deletion is a new revision with the same
// id; the Rust side reduces to the newest per id the same way on every
// device. The front end never sees ciphertext.
import { invoke } from "@tauri-apps/api/core";
import { v4 as uuidv4 } from "uuid";

export type MemoryKind = "rule" | "command" | "location" | "decision" | "fact" | "episode" | "note";

export interface MemoryRow {
  id: string;
  /** `person` | `ai:<founding key>` | `project:<folder identity>` | `conversation:<hash>` */
  scope: string;
  kind: MemoryKind;
  /** The statement, Markdown; a rule is the person's words verbatim. */
  text: string;
  structured?: Record<string, unknown> | null;
  /** ms epoch the statement holds from. */
  as_of: number;
  valid_to?: number | null;
  /** `person` | `ai:<founding key>` */
  saved_by: string;
  /** `authored` | `extracted:<hash>` | `distilled:<hash>` | `dream:<ids>` | `migrated:<store>` */
  provenance: string;
  pinned: boolean;
  topics: string[];
  replaces: string[];
  deleted: boolean;
  updated_at: number;
  action_hash?: string | null;
}

export const personScope = "person";
export const aiScope = (foundingKey: string) => `ai:${foundingKey}`;
export const projectScope = (folderIdentity: string) => `project:${folderIdentity}`;

/** A fresh row, authored by the person unless said otherwise. */
export function newRow(
  scope: string,
  kind: MemoryKind,
  text: string,
  extra: Partial<Pick<MemoryRow, "structured" | "as_of" | "saved_by" | "provenance" | "pinned" | "topics">> = {},
): MemoryRow {
  const now = Date.now();
  return {
    id: uuidv4(),
    scope,
    kind,
    text,
    structured: extra.structured ?? null,
    as_of: extra.as_of ?? now,
    saved_by: extra.saved_by ?? "person",
    provenance: extra.provenance ?? "authored",
    pinned: extra.pinned ?? false,
    topics: extra.topics ?? [],
    replaces: [],
    deleted: false,
    updated_at: now,
  };
}

/** A revision of an existing row: same id, newer time. */
export function revise(row: MemoryRow, patch: Partial<Pick<MemoryRow, "text" | "structured" | "kind" | "pinned" | "topics" | "valid_to" | "as_of">>): MemoryRow {
  return { ...row, ...patch, replaces: [], deleted: false, updated_at: Date.now(), action_hash: undefined };
}

/** A deletion is a record: the newest revision says so. */
export function deletion(row: MemoryRow): MemoryRow {
  return { ...row, deleted: true, updated_at: Date.now(), action_hash: undefined };
}

/** Write a row. An `ai:` scope row goes through that AI's agent; a person-
 *  or project-scope row through any provisioned agent when none is given.
 *  Returns the record's action hash. */
export async function saveMemoryRow(row: MemoryRow, agentKey?: string): Promise<string> {
  return invoke<string>("save_memory_row", { agentKey: agentKey ?? null, row });
}

/** Every live row in a scope, pinned rules first, then newest (a live read;
 *  refreshes the local cache). */
export async function listMemoryRows(scope: string, agentKey?: string): Promise<MemoryRow[]> {
  return invoke<MemoryRow[]>("list_memory_rows", { agentKey: agentKey ?? null, scope });
}

/** Every revision ever written in a scope, newest first, live or
 *  superseded - the history view. A live read. */
export type MemoryRevision = MemoryRow & { superseded: boolean };
export async function listMemoryRevisions(scope: string, agentKey?: string): Promise<MemoryRevision[]> {
  return invoke<MemoryRevision[]>("list_memory_revisions", { agentKey: agentKey ?? null, scope });
}

/** A human line for a dream revision: what dreaming did to this row. */
export function describeDream(r: MemoryRevision, all: MemoryRevision[]): string {
  const prev = all.filter((x) => x.id === r.id && x.updated_at < r.updated_at).sort((a, b) => b.updated_at - a.updated_at)[0];
  if (r.replaces.length > 0) {
    const merged = r.replaces.map((id) => all.find((x) => x.id === id)?.text).filter(Boolean) as string[];
    return merged.length ? `Merged "${merged.map((t) => t.slice(0, 50)).join('", "')}" into "${r.text.slice(0, 60)}"` : `Merged ${r.replaces.length} into "${r.text.slice(0, 60)}"`;
  }
  if (prev && prev.kind !== r.kind) return `Filed "${r.text.slice(0, 60)}" as a ${r.kind} (was a ${prev.kind})`;
  if (r.valid_to && !(prev && prev.valid_to)) return `Moved "${r.text.slice(0, 60)}" to history (past the budget)`;
  return `Revised "${r.text.slice(0, 60)}"`;
}

/** Undo one dream revision: the row returns to its revision before the
 *  dream, and rows a merge folded in come back as they were. Every step is
 *  a new revision (nothing is deleted), marked undo:<the dream's record>. */
export async function undoDream(r: MemoryRevision, all: MemoryRevision[], agentKey?: string): Promise<number> {
  const now = Date.now();
  const mark = `undo:${r.action_hash ?? r.id}`;
  let n = 0;
  const latestBefore = (id: string) => all.filter((x) => x.id === id && x.updated_at < r.updated_at).sort((a, b) => b.updated_at - a.updated_at)[0];
  const prev = latestBefore(r.id);
  if (prev) {
    const { superseded: _s, ...row } = prev;
    await saveMemoryRow({ ...row, replaces: [], updated_at: now, provenance: `${mark};${row.provenance}`, action_hash: undefined }, agentKey);
    n++;
  }
  for (const id of r.replaces) {
    const back = all.filter((x) => x.id === id).sort((a, b) => b.updated_at - a.updated_at)[0];
    if (!back) continue;
    const { superseded: _s, ...row } = back;
    await saveMemoryRow({ ...row, replaces: [], deleted: false, updated_at: now + 1, provenance: `${mark};${row.provenance}`, action_hash: undefined }, agentKey);
    n++;
  }
  return n;
}

/** The scope's rows as of the last live read - instant. */
export async function listMemoryRowsCached(scope: string): Promise<MemoryRow[]> {
  return invoke<MemoryRow[]>("list_memory_rows_cached", { scope });
}
