/**
 * Workspace memory - what a FOLDER's work has taught the AIs, kept on the
 * chain and shared by every AI (deploy commands and project facts are
 * workspace truths, not persona truths).
 *
 * Storage: one special conversation per folder per AI chain
 * (source = "workspace-memory", title = the folder path) whose entries are
 * memory REVISIONS - the newest entry across ALL AIs' chains is the current
 * memory, and the chain keeps an auditable history of how it evolved.
 * Zero DNA changes: this is all client-side schema.
 */
import { invoke } from "@tauri-apps/api/core";
import {
  getConversations,
  getTranscript,
} from "./holochainTranscripts";
import { getLocalCustomAis } from "./localAiStorage";

export const WORKSPACE_MEMORY_SOURCE = "workspace-memory";
/** The cap is the staleness defense - curation forced, accumulation refused. */
export const MEMORY_MAX_LINES = 60;

export interface WorkspaceMemory {
  folderPath: string;
  /** Current memory markdown ("" = no memory yet). */
  content: string;
  /** Microseconds timestamp of the newest revision. */
  updatedAt: number;
  /** Chain owner (agent key hex) + conversation holding the newest revision. */
  agentKey?: string;
  conversationHash?: string;
  revisions: number;
}

/** Every provisioned AI's agent key (the chains to scan). */
async function provisionedAgents(): Promise<{ agentKey: string; name: string }[]> {
  const ais = await getLocalCustomAis();
  return ais
    .filter((a) => a.agentPubKey)
    .map((a) => ({ agentKey: a.agentPubKey!, name: a.name }));
}

/** One chain's project-notes conversations, transcripts included. Chains
 *  are independent, so the scans below run these CONCURRENTLY - the serial
 *  walk made the Projects tab minutes-slow on installs with many AIs and
 *  long agent lineages. Read errors surface as an empty slice (the utils
 *  already swallow them), never as a rejected scan. */
type ScannedNotes = {
  agentKey: string;
  conv: Awaited<ReturnType<typeof getConversations>>[number];
  entries: Awaited<ReturnType<typeof getTranscript>>;
};
async function scanAgentProjectNotes(
  agentKey: string,
  folderPath?: string,
): Promise<ScannedNotes[]> {
  const conversations = await getConversations(agentKey, 600, "projects list");
  const matches = conversations.filter(
    (c) =>
      c.source === WORKSPACE_MEMORY_SOURCE &&
      (folderPath ? c.title === folderPath : !!c.title),
  );
  return Promise.all(
    matches.map(async (conv) => ({
      agentKey,
      conv,
      entries: await getTranscript(conv.agent_key || agentKey, conv.hash),
    })),
  );
}

/** Newest-revision-wins merge of scanned conversations into folders. */
function foldScans(scans: ScannedNotes[], byFolder: Map<string, WorkspaceMemory>) {
  for (const { agentKey, conv: c, entries } of scans) {
    const title = c.title!;
    const cur =
      byFolder.get(title) ?? {
        folderPath: title,
        content: "",
        updatedAt: 0,
        revisions: 0,
      };
    cur.revisions += entries.length;
    for (const e of entries) {
      if (e.timestamp > cur.updatedAt) {
        cur.updatedAt = e.timestamp;
        cur.content = e.content;
        cur.agentKey = c.agent_key || agentKey;
        cur.conversationHash = c.hash;
      }
    }
    byFolder.set(title, cur);
  }
}

/** Session cache of the last NON-EMPTY list - the full scan is the app's
 *  heaviest read, so the Memory page shows the last known list instantly
 *  and lets the fresh scan replace it. Empty results are never cached: an
 *  empty answer during records warmup must not masquerade as "no
 *  projects" on the next visit. */
let lastScan: WorkspaceMemory[] | null = null;
export function cachedWorkspaceMemories(): WorkspaceMemory[] | null {
  return lastScan;
}

/** The current memory for one folder - newest revision across all chains.
 *  Read in the backend from its copy (instant) or, with `fresh`, from the
 *  records through the project-memory index (the dialog's count and date).
 *  It used to read EVERY AI's whole conversation list to find the folder's
 *  memory conversations - minutes on a busy install (10-16). */
export async function getWorkspaceMemory(folderPath: string, fresh = false): Promise<WorkspaceMemory> {
  try {
    const m = await invoke<{ folderPath: string; content: string; updatedAt: number; revisions: number }>(
      "project_memory_get",
      { folder: folderPath, fresh },
    );
    return { folderPath, content: m.content, updatedAt: m.updatedAt, revisions: m.revisions };
  } catch (e) {
    console.warn("[WorkspaceMemory] read failed:", e);
    return { folderPath, content: "", updatedAt: 0, revisions: 0 };
  }
}

/** All folders that have memory anywhere (Memory page's Projects list). */
export async function listWorkspaceMemories(): Promise<WorkspaceMemory[]> {
  const agents = await provisionedAgents();
  const scans = await Promise.all(
    agents.map((a) => scanAgentProjectNotes(a.agentKey)),
  );
  const byFolder = new Map<string, WorkspaceMemory>();
  foldScans(scans.flat(), byFolder);
  const out = [...byFolder.values()].sort((a, b) => b.updatedAt - a.updatedAt);
  if (out.length > 0) lastScan = out;
  return out;
}

/** Write a new revision. `writer` = the AI whose chain takes it (the one in
 *  the session, or any provisioned AI for manual edits). The backend finds
 *  the writer's memory conversation through the project-memory index and
 *  never starts a second one because a read was slow or failed. */
export async function saveWorkspaceMemory(
  writer: { agentPubKey: string; label: string },
  folderPath: string,
  content: string,
): Promise<boolean> {
  try {
    await invoke("project_memory_save", {
      writerKey: writer.agentPubKey,
      writerLabel: writer.label,
      folder: folderPath,
      content: clampMemory(content),
    });
    return true;
  } catch (e) {
    console.error("[WorkspaceMemory] Revision write FAILED:", e);
    return false;
  }
}

/** A user-saved note from the Remember button: appended as its own row,
 *  marked as the user's, trimmed oldest-first when the cap would drop it
 *  (the next automatic revision curates it into shape). */
export async function appendWorkspaceNote(folderPath: string, note: string): Promise<boolean> {
  if (!note.trim()) return false;
  const ais = await getLocalCustomAis();
  const writer = ais.find((a) => a.agentPubKey);
  if (!writer) {
    console.error("[WorkspaceMemory] No provisioned AI to write the note");
    return false;
  }
  try {
    await invoke("project_memory_note", {
      writerKey: writer.agentPubKey,
      writerLabel: writer.name,
      folder: folderPath,
      note,
    });
    return true;
  } catch (e) {
    console.error("[WorkspaceMemory] Note write FAILED:", e);
    return false;
  }
}

/** Enforce the line cap + tidy blank runs. */
export function clampMemory(content: string): string {
  const lines = content
    .split("\n")
    .map((l) => l.trimEnd())
    .filter((l, i, arr) => l.trim() !== "" || (i > 0 && arr[i - 1].trim() !== ""));
  return lines.slice(0, MEMORY_MAX_LINES).join("\n").trim();
}

/** Rows view for the editor: each non-empty line is an editable row. */
export function memoryRows(content: string): string[] {
  return content.split("\n").filter((l) => l.trim() !== "");
}

/** The block that rides ahead of a project session's first prompt. The
 *  tool hint teaches the agent its deliberate-memory habit - saved notes
 *  land on the chain the moment they are learned, not just at session end. */
export function memoryPromptBlock(memory: string): string {
  const hint =
    "[This project has a shared memory. When you learn something durable - a command that works, a key file location, a convention, a decision, or the person's preference or correction about how to work - save it immediately with the remember_for_project tool. A preference or correction is saved the moment it is said, in the person's words.]\n";
  if (!memory.trim()) return hint + "\n";
  return (
    "[Project memory - durable notes from earlier work. Trust them, but verify anything that looks stale:]\n" +
    memory.trim() +
    "\n[End of project memory.]\n" +
    hint +
    "\n"
  );
}

/** Ask a model (via the local server, unrecorded) for the updated memory
 *  after a session. Returns the new memory, or null when nothing usable
 *  came back. */
export async function reviseWorkspaceMemory(
  aiId: string,
  oldMemory: string,
  sessionDigest: string,
): Promise<string | null> {
  const prompt =
    "You maintain a compact WORKSPACE MEMORY for a project folder: durable facts that help future sessions (build/deploy/test commands exactly as they work, key file locations, project conventions, decisions, and the person's preferences and corrections about how to work - these are the most durable of all and are kept in the person's words).\n\n" +
    `CURRENT MEMORY (may be empty):\n${oldMemory || "(empty)"}\n\n` +
    `WHAT HAPPENED THIS SESSION:\n${sessionDigest}\n\n` +
    "Return ONLY the COMPLETE updated memory as markdown bullet lines - every line that still holds, not only what changed. Rules: update in place rather than append; drop anything stale or proven wrong - but NEVER drop or reword a preference or correction the person gave; keep commands verbatim; no conversation summaries or one-off details; maximum " +
    `${MEMORY_MAX_LINES} lines. If nothing durable was learned, return the current memory unchanged.`;
  try {
    // The port follows the app's port switch (two instances on one box), never a literal.
    const inferencePort = await invoke<number>("inference_port").catch(() => 11435);
    const r = await fetch(`http://127.0.0.1:${inferencePort}/v1/chat/completions`, {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Your-Own-AI-Memory": "off",
        "X-Your-Own-AI-Record": "off",
        // A memory writer never runs online (MEMORY_REDESIGN.md §7): the
        // session digest stays on this machine whatever the AI's own model.
        "X-Your-Own-AI-Online-Share": "local",
        "X-Your-Own-AI-Task": "memory-prose",
      },
      body: JSON.stringify({
        model: aiId,
        stream: true,
        max_tokens: 1500,
        messages: [{ role: "user", content: prompt }],
      }),
    });
    if (!r.ok) return null;
    const reader = r.body?.getReader();
    if (!reader) return null;
    const dec = new TextDecoder();
    let buf = "";
    let content = "";
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;
      buf += dec.decode(value, { stream: true });
      let i;
      while ((i = buf.indexOf("\n")) >= 0) {
        const line = buf.slice(0, i).trim();
        buf = buf.slice(i + 1);
        if (!line.startsWith("data:")) continue;
        const p = line.slice(5).trim();
        if (!p || p === "[DONE]") continue;
        try {
          const ev = JSON.parse(p);
          content += ev.choices?.[0]?.delta?.content ?? "";
        } catch {
          /* keep scanning */
        }
      }
    }
    const cleaned = clampMemory(content);
    return cleaned || null;
  } catch (e) {
    console.warn("[WorkspaceMemory] Revision call failed:", e);
    return null;
  }
}
