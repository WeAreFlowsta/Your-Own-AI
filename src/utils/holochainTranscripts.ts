/**
 * Holochain transcript operations.
 *
 * All operations use the agent pub key hex as the identifier.
 * The agent key is returned by provisionAllAgents() at startup
 * and stored in UserDefinedAI.agentPubKey.
 *
 * If the conductor is down or an agent isn't provisioned,
 * these functions fail silently — chat always works.
 */
import { invoke } from "@tauri-apps/api/core";
import type {
  HolochainConversation,
  HolochainTranscriptEntry,
} from "../types";

/**
 * Start a new conversation for an AI agent.
 * Returns the conversation hash, or null on failure.
 */
export async function startConversation(
  agentKey: string,
  aiName: string,
  model: string,
  title?: string,
  source?: string,
): Promise<string | null> {
  try {
    return await invoke<string>("start_conversation", {
      agentKey,
      aiName,
      model,
      title: title ?? null,
      source: source ?? null,
    });
  } catch (e) {
    console.warn("[Holochain] Failed to start conversation:", e);
    return null;
  }
}

/**
 * Record a message in a conversation.
 * Fire-and-forget — returns silently on failure.
 */
export async function recordMessage(
  agentKey: string,
  conversationHash: string,
  role: "user" | "assistant",
  content: string,
  sequence: number,
  model: string,
  thinking?: string,
  tokens?: { prompt_tokens: number; completion_tokens: number; total_tokens: number; tokens_per_second?: number },
  provenance?: {
    sources?: { url: string; title: string }[];
    system_prompt?: string;
    mode?: string;
    attachments?: { bytes: number; sha256: string; content?: string };
    images?: {
      filename: string;
      mime: string;
      bytes: number;
      sha256: string;
      content?: string;
    }[];
    grounded?: {
      kind: "document" | "image";
      doc_sha256: string;
      doc_name?: string;
      claim?: string;
      quote?: string;
      span?: [number, number] | null;
    }[];
    runtime?: { app_version: string; online: boolean; max_tokens?: number };
    routing_reason?: string;
    routing_task?: string;
  },
  /** Agent turn extras: the working log (sanitized) + workspace folder.
   *  Client-side schema, encrypted before the zome sees it - no DNA change. */
  agentExtras?: { agentLog?: unknown; folderPath?: string },
): Promise<string | null> {
  try {
    // Returns the entry's Holochain action hash (hex) — used as the
    // provenance anchor when memory extraction learns a fact from this turn.
    const actionHash = await invoke<string>("record_transcript_entry", {
      agentKey,
      conversationHash,
      role,
      content,
      sequence,
      model,
      thinking: thinking ?? null,
      tokens: tokens ? {
        prompt_tokens: tokens.prompt_tokens,
        completion_tokens: tokens.completion_tokens,
        total_tokens: tokens.total_tokens,
        tokens_per_second: tokens.tokens_per_second ?? null,
      } : null,
      provenance: provenance ?? null,
      agentLog: agentExtras?.agentLog ?? null,
      folderPath: agentExtras?.folderPath ?? null,
    });
    return actionHash;
  } catch (e) {
    console.warn("[Holochain] Failed to record message:", e);
    return null;
  }
}

/**
 * Get all conversations for an AI agent.
 */
export async function getConversations(
  agentKey: string,
): Promise<HolochainConversation[]> {
  try {
    return await invoke<HolochainConversation[]>("get_conversations", {
      agentKey,
    });
  } catch (e) {
    console.warn("[Holochain] Failed to get conversations:", e);
    return [];
  }
}

/**
 * Persist an on-demand grounding result, linked to the message it annotates.
 * Recorded as its own encrypted conversation entry (no zome change), so the
 * grounding survives a reload — same as auto-grounding. Best-effort.
 */
export async function recordGroundingAnnotation(
  agentKey: string,
  conversationHash: string,
  targetHash: string,
  grounded: {
    kind: "document" | "image";
    doc_sha256: string;
    doc_name?: string;
    claim?: string;
    quote?: string;
    span?: [number, number] | null;
  }[],
): Promise<string | null> {
  try {
    return await invoke<string>("record_grounding_annotation", {
      agentKey,
      conversationHash,
      targetHash,
      grounded,
    });
  } catch (e) {
    console.warn("[Holochain] Failed to record grounding annotation:", e);
    return null;
  }
}

/**
 * Get all messages in a conversation.
 */
export async function getTranscript(
  agentKey: string,
  conversationHash: string,
): Promise<HolochainTranscriptEntry[]> {
  try {
    return await invoke<HolochainTranscriptEntry[]>(
      "get_conversation_transcript",
      { agentKey, conversationHash },
    );
  } catch (e) {
    console.warn("[Holochain] Failed to get transcript:", e);
    return [];
  }
}

/**
 * Provision a single AI agent. Returns the agent pub key hex.
 * `aiId` is the stable local AI ID used as the lair seed tag.
 */
export async function provisionAgent(aiId: string): Promise<string | null> {
  try {
    return await invoke<string>("provision_ai_agent", { aiId });
  } catch (e) {
    console.warn("[Holochain] Failed to provision agent:", e);
    return null;
  }
}

/**
 * Provision agents for all AIs at once.
 * `aiIds` are the stable local AI IDs used as lair seed tags.
 * Returns a map of AI ID → agent pub key hex.
 */
export async function provisionAllAgents(
  aiIds: string[],
): Promise<Record<string, string>> {
  try {
    return await invoke<Record<string, string>>("provision_all_agents", {
      aiIds,
    });
  } catch (e) {
    console.warn("[Holochain] Failed to provision agents:", e);
    return {};
  }
}

/**
 * Check if an AI has been provisioned on Holochain.
 */
export async function isAiProvisioned(aiId: string): Promise<boolean> {
  try {
    return await invoke<boolean>("get_ai_holochain_status", { aiId });
  } catch {
    return false;
  }
}
