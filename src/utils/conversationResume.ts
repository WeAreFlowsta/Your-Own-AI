/**
 * Conversations are places, not documents: every conversation can be
 * re-entered. Resume = load the transcript by conversation hash, rebuild
 * the message list, keep appending to the same hash - the Holochain store
 * as living navigation.
 */
import { v4 as uuidv4 } from "uuid";
import { getConversations, getTranscript } from "./holochainTranscripts";
import type {
  HolochainConversation,
  HolochainTranscriptEntry,
  Message,
  SelectedAiModel,
} from "../types";

export interface ConversationListItem {
  conversation: HolochainConversation;
  /** The local AI this conversation belongs to (matched by agent key). */
  aiId: string;
  aiLabel: string;
  aiImageUrl?: string | null;
  /** Workspace folder this conversation worked in (client map for now). */
  folderPath?: string;
}

/** All conversations across every provisioned AI, newest first. */
export async function listAllConversations(
  ais: SelectedAiModel[],
): Promise<ConversationListItem[]> {
  const out: ConversationListItem[] = [];
  await Promise.all(
    ais
      .filter((ai) => ai.aiConfig?.agentPubKey)
      .map(async (ai) => {
        const conversations = await getConversations(ai.aiConfig.agentPubKey!);
        for (const conversation of conversations) {
          out.push({
            conversation,
            aiId: ai.id,
            aiLabel: ai.label,
            aiImageUrl: ai.imageUrl,
            folderPath: getConversationFolder(conversation.hash),
          });
        }
      }),
  );
  out.sort((a, b) => b.conversation.started_at - a.conversation.started_at);
  return out;
}

/** Rebuild the chat's message list from a stored transcript. */
export async function loadConversationMessages(
  agentKey: string,
  conversationHash: string,
  ai: SelectedAiModel,
): Promise<{ messages: Message[]; nextSequence: number }> {
  const entries = await getTranscript(agentKey, conversationHash);
  entries.sort((a, b) => a.sequence - b.sequence);
  const messages = entries.map((e: HolochainTranscriptEntry): Message => {
    if (e.role === "user") {
      return {
        id: uuidv4(),
        role: "user",
        content: e.content,
        model: "user",
      };
    }
    return {
      id: uuidv4(),
      role: "assistant",
      content: e.content,
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
    };
  });
  const nextSequence =
    entries.length > 0 ? Math.max(...entries.map((e) => e.sequence)) + 1 : 0;
  return { messages, nextSequence };
}

/* ---- client-side conversation metadata (until the DNA work lands) ---- */

const FOLDER_MAP_KEY = "conversation-folders";
const LAST_CONVERSATION_KEY = "last-conversation";

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
