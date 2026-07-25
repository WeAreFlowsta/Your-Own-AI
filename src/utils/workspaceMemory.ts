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
import {
  getConversations,
  getTranscript,
  startConversation,
  recordMessage,
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

/** The current memory for one folder - newest revision across all chains. */
export async function getWorkspaceMemory(folderPath: string): Promise<WorkspaceMemory> {
  const best: WorkspaceMemory = {
    folderPath,
    content: "",
    updatedAt: 0,
    revisions: 0,
  };
  for (const agent of await provisionedAgents()) {
    const conversations = await getConversations(agent.agentKey);
    for (const c of conversations) {
      if (c.source !== WORKSPACE_MEMORY_SOURCE || c.title !== folderPath) continue;
      const entries = await getTranscript(c.agent_key || agent.agentKey, c.hash);
      best.revisions += entries.length;
      for (const e of entries) {
        if (e.timestamp > best.updatedAt) {
          best.updatedAt = e.timestamp;
          best.content = e.content;
          best.agentKey = c.agent_key || agent.agentKey;
          best.conversationHash = c.hash;
        }
      }
    }
  }
  return best;
}

/** All folders that have memory anywhere (Memory page's Workspaces list). */
export async function listWorkspaceMemories(): Promise<WorkspaceMemory[]> {
  const byFolder = new Map<string, WorkspaceMemory>();
  for (const agent of await provisionedAgents()) {
    const conversations = await getConversations(agent.agentKey);
    for (const c of conversations) {
      if (c.source !== WORKSPACE_MEMORY_SOURCE || !c.title) continue;
      const entries = await getTranscript(c.agent_key || agent.agentKey, c.hash);
      const cur =
        byFolder.get(c.title) ?? {
          folderPath: c.title,
          content: "",
          updatedAt: 0,
          revisions: 0,
        };
      cur.revisions += entries.length;
      for (const e of entries) {
        if (e.timestamp > cur.updatedAt) {
          cur.updatedAt = e.timestamp;
          cur.content = e.content;
          cur.agentKey = c.agent_key || agent.agentKey;
          cur.conversationHash = c.hash;
        }
      }
      byFolder.set(c.title, cur);
    }
  }
  return [...byFolder.values()].sort((a, b) => b.updatedAt - a.updatedAt);
}

/** Write a new revision. `writer` = the AI whose chain takes it (the one in
 *  the session, or any provisioned AI for manual edits). */
export async function saveWorkspaceMemory(
  writer: { agentPubKey: string; label: string },
  folderPath: string,
  content: string,
): Promise<boolean> {
  const trimmed = clampMemory(content);
  // Reuse this AI's existing memory conversation for the folder, else start one.
  const conversations = await getConversations(writer.agentPubKey);
  let conv = conversations.find(
    (c) => c.source === WORKSPACE_MEMORY_SOURCE && c.title === folderPath,
  );
  let hash = conv?.hash;
  let seq = 0;
  if (conv) {
    const entries = await getTranscript(conv.agent_key || writer.agentPubKey, conv.hash);
    seq = entries.length ? Math.max(...entries.map((e) => e.sequence)) + 1 : 0;
  } else {
    hash =
      (await startConversation(
        writer.agentPubKey,
        writer.label,
        "workspace-memory",
        folderPath,
        WORKSPACE_MEMORY_SOURCE,
      )) ?? undefined;
  }
  if (!hash) {
    console.error("[WorkspaceMemory] Could not start the memory conversation");
    return false;
  }
  const actionHash = await recordMessage(
    conv?.agent_key || writer.agentPubKey,
    hash,
    "assistant",
    trimmed,
    seq,
    "workspace-memory",
  );
  if (!actionHash) {
    console.error("[WorkspaceMemory] Revision write FAILED - see warning above");
    return false;
  }
  return true;
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

/** The block that rides ahead of a folder session's first prompt. */
export function memoryPromptBlock(memory: string): string {
  if (!memory.trim()) return "";
  return (
    "[Workspace memory - durable notes about this folder from earlier work. Trust them, but verify anything that looks stale:]\n" +
    memory.trim() +
    "\n[End of workspace memory.]\n\n"
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
    "You maintain a compact WORKSPACE MEMORY for a project folder: durable facts that help future sessions (build/deploy/test commands exactly as they work, key file locations, project conventions, decisions).\n\n" +
    `CURRENT MEMORY (may be empty):\n${oldMemory || "(empty)"}\n\n` +
    `WHAT HAPPENED THIS SESSION:\n${sessionDigest}\n\n` +
    "Return ONLY the updated memory as markdown bullet lines. Rules: update in place rather than append; drop anything stale or proven wrong; keep commands verbatim; no conversation summaries or one-off details; maximum " +
    `${MEMORY_MAX_LINES} lines. If nothing durable was learned, return the current memory unchanged.`;
  try {
    const r = await fetch("http://127.0.0.1:11435/v1/chat/completions", {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "X-Your-Own-AI-Memory": "off",
        "X-Your-Own-AI-Record": "off",
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
