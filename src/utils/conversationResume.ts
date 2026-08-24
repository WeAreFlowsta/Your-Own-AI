/**
 * Conversations are places, not documents: every conversation can be
 * re-entered. Resume = load the transcript by conversation hash, rebuild
 * the message list, keep appending to the same hash - the Holochain store
 * as living navigation.
 */
import { v4 as uuidv4 } from "uuid";
import { getConversations, getConversationsCached, getTranscript } from "./holochainTranscripts";
import type {
  HolochainConversation,
  HolochainTranscriptEntry,
  Message,
  SelectedAiModel,
} from "../types";

export interface ConversationListItem {
  conversation: HolochainConversation;
  /** Display title: rename override, else the stored title, tag-stripped. */
  title: string;
  /** The local AI this conversation belongs to (matched by agent key). */
  aiId: string;
  aiLabel: string;
  aiImageUrl?: string | null;
  /** Workspace folder this conversation worked in (client map for now). */
  folderPath?: string;
}

/** What the list orders by: the last recorded turn, else when it started
 *  (conversations not continued since last-activity tracking began). */
export function lastActiveAt(c: HolochainConversation): number {
  return c.last_active_at ?? c.started_at;
}

/** Titles come from raw first messages - agent-formatted ones carry tags
 *  like <user_query>. Strip markup, collapse whitespace. */
export function sanitizeTitle(raw?: string | null): string {
  const clean = (raw ?? "")
    .replace(/<[^>]*>/g, " ")
    .replace(/\s+/g, " ")
    .trim();
  return clean.length > 80 ? clean.slice(0, 80) + "..." : clean || "Conversation";
}

/** All conversations across every provisioned AI, most recently active first. */
export async function listAllConversations(
  ais: SelectedAiModel[],
): Promise<ConversationListItem[]> {
  const out: ConversationListItem[] = [];
  await Promise.all(
    ais
      .filter((ai) => ai.aiConfig?.agentPubKey)
      .map(async (ai) => {
        // One sick cell must never hold the whole list hostage - a zome
        // read that outlives 20s yields an empty slot for that AI this
        // pass (seen live: a cell timing out reads at 60s apiece kept
        // the drawer on "Loading" forever).
        const conversations = await Promise.race([
          getConversations(ai.aiConfig.agentPubKey!),
          new Promise<never>((_, reject) =>
            setTimeout(() => reject(new Error("conversation read timed out")), 20000),
          ),
        ]).catch((e) => {
          console.warn(`[Conversations] ${ai.label}'s records are slow to answer:`, e);
          return [] as Awaited<ReturnType<typeof getConversations>>;
        });
        for (const conversation of conversations) {
          // External-API conversations (source set - e.g. the Build agent's
          // own model calls through the local server) are records, not
          // places - they live on the Memory page, not here. Imported
          // history is the exception: an adopted conversation IS a place -
          // the user can pick up where they left off with their old app.
          if (conversation.source && !conversation.source.startsWith("import:")) continue;
          out.push({
            conversation,
            title:
              getConversationTitleOverride(conversation.hash) ??
              sanitizeTitle(conversation.title),
            aiId: ai.id,
            aiLabel: ai.label,
            aiImageUrl: ai.imageUrl,
            folderPath: getConversationFolder(conversation.hash),
          });
        }
      }),
  );
  out.sort((a, b) => lastActiveAt(b.conversation) - lastActiveAt(a.conversation));
  return out;
}

/** The cached view of the same list - instant (local encrypted files),
 *  served while live reads are slow or the conductor is starting. */
export async function listAllConversationsCached(
  ais: SelectedAiModel[],
): Promise<ConversationListItem[]> {
  const out: ConversationListItem[] = [];
  await Promise.all(
    ais
      .filter((ai) => ai.aiConfig?.agentPubKey)
      .map(async (ai) => {
        const conversations = await getConversationsCached(ai.aiConfig.agentPubKey!);
        for (const conversation of conversations) {
          if (conversation.source && !conversation.source.startsWith("import:")) continue;
          out.push({
            conversation,
            title:
              getConversationTitleOverride(conversation.hash) ??
              sanitizeTitle(conversation.title),
            aiId: ai.id,
            aiLabel: ai.label,
            aiImageUrl: ai.imageUrl,
            folderPath: getConversationFolder(conversation.hash),
          });
        }
      }),
  );
  out.sort((a, b) => lastActiveAt(b.conversation) - lastActiveAt(a.conversation));
  return out;
}

/** Rebuild the chat's message list from a stored transcript. */
export async function loadConversationMessages(
  agentKey: string,
  conversationHash: string,
  ai: SelectedAiModel,
): Promise<{ messages: Message[]; nextSequence: number; folderPath?: string }> {
  const entries = await getTranscript(agentKey, conversationHash);
  entries.sort((a, b) => a.sequence - b.sequence);
  let folderPath: string | undefined;
  const messages = entries.map((e: HolochainTranscriptEntry): Message => {
    if (e.folder_path) folderPath = e.folder_path;
    if (e.role === "user") {
      return {
        id: uuidv4(),
        role: "user",
        content: e.content,
        model: "user",
      };
    }
    // A recorded agent turn whose model spoke its conclusion mid-way and
    // finished on silent tool steps has EMPTY content with the words inside
    // the log (turns recorded before the 0.5.0 promotion fix, and any log
    // shape that slips through) - promote the last substantial spoken
    // passage for display so history never shows a wordless fold.
    let content = e.content;
    let logItems = e.agent_log?.items?.length ? e.agent_log.items : undefined;
    if (content === "" && logItems) {
      for (let i = logItems.length - 1; i >= 0; i--) {
        const item = logItems[i] as { type?: string; id?: string; text?: string };
        if (item.type !== "narration") continue;
        if (item.id?.startsWith("hint-") || item.id === "log-trimmed") continue;
        if (!item.text || item.text.trim().length < 40) continue;
        content = item.text;
        logItems = [...logItems.slice(0, i), ...logItems.slice(i + 1)];
        break;
      }
    }
    return {
      id: uuidv4(),
      role: "assistant",
      content,
      model: ai.id,
      aiLabel: ai.label,
      aiImageUrl: ai.imageUrl || undefined,
      thinking: e.thinking || undefined,
      tokens: e.tokens
        ? {
            prompt_tokens: e.tokens.prompt_tokens,
            completion_tokens: e.tokens.completion_tokens,
            total_tokens: e.tokens.total_tokens,
            tokens_per_second: e.tokens.tokens_per_second ?? undefined,
          }
        : undefined,
      transcriptHash: e.hash,
      // The routing receipt comes back too: the entry's model field IS the
      // served model, and the reason rides provenance - so the Model button
      // works on resumed conversations exactly as it did live.
      servedBy: e.model || undefined,
      routingReason: e.routing_reason || undefined,
      stopped: e.stopped || undefined,
      routingTask: e.routing_task || undefined,
      // Agent turns come back whole: the rail's stub renders from the
      // stored working log, expandable to the full story (step outputs
      // are not persisted - those rows simply are not expandable).
      agentTurn: e.agent_log?.items?.length ? true : undefined,
      agentLog: logItems,
      agentStats: e.agent_log?.stats ?? undefined,
    };
  });
  const nextSequence =
    entries.length > 0 ? Math.max(...entries.map((e) => e.sequence)) + 1 : 0;
  // A conversation can honestly end on an unanswered question (the turn was
  // stopped, or died before producing anything - only what really happened
  // is recorded). Say so, or the resume looks broken.
  const last = messages[messages.length - 1];
  if (last?.role === "user") {
    messages.push({
      id: uuidv4(),
      role: "assistant",
      content: "",
      model: ai.id,
      aiLabel: ai.label,
      aiImageUrl: ai.imageUrl || undefined,
      error:
        "This question never got an answer - the turn was stopped or failed. Ask again to continue.",
    });
  }
  return { messages, nextSequence, folderPath };
}

/* ---- client-side conversation metadata (until the DNA work lands) ---- */

const FOLDER_MAP_KEY = "conversation-folders";
const LAST_CONVERSATION_KEY = "last-conversation";
const TITLE_MAP_KEY = "conversation-titles";

/** Rename is a client-side override for now: on-chain rename needs a small
 *  zome addition (conversation titles are zome-side metadata, unlike the
 *  encrypted entries) - queued for the next DNA window. */
export function setConversationTitleOverride(hash: string, title: string) {
  try {
    const map = JSON.parse(localStorage.getItem(TITLE_MAP_KEY) || "{}");
    map[hash] = title;
    localStorage.setItem(TITLE_MAP_KEY, JSON.stringify(map));
  } catch {
    /* convenience metadata */
  }
}

export function getConversationTitleOverride(hash: string): string | undefined {
  try {
    const map = JSON.parse(localStorage.getItem(TITLE_MAP_KEY) || "{}");
    return typeof map[hash] === "string" && map[hash].trim()
      ? map[hash]
      : undefined;
  } catch {
    return undefined;
  }
}

export function rememberConversationFolder(hash: string, folderPath: string) {
  try {
    const map = JSON.parse(localStorage.getItem(FOLDER_MAP_KEY) || "{}");
    map[hash] = folderPath;
    localStorage.setItem(FOLDER_MAP_KEY, JSON.stringify(map));
  } catch {
    /* convenience metadata */
  }
}

export function getConversationFolder(hash: string): string | undefined {
  try {
    const map = JSON.parse(localStorage.getItem(FOLDER_MAP_KEY) || "{}");
    return typeof map[hash] === "string" ? map[hash] : undefined;
  } catch {
    return undefined;
  }
}

export interface LastConversationPointer {
  hash: string;
  agentKey: string;
  aiId: string;
  title: string;
}

export function rememberLastConversation(p: LastConversationPointer) {
  try {
    localStorage.setItem(LAST_CONVERSATION_KEY, JSON.stringify(p));
  } catch {
    /* convenience metadata */
  }
}

export function readLastConversation(): LastConversationPointer | null {
  try {
    const raw = JSON.parse(localStorage.getItem(LAST_CONVERSATION_KEY) || "null");
    return raw && typeof raw.hash === "string" ? raw : null;
  } catch {
    return null;
  }
}
