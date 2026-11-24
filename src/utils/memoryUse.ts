/**
 * How a conversation is used by memory: kept out altogether, or single
 * messages left out. A change takes effect with the next question; nothing
 * is read or imported again.
 */
import { invoke } from "@tauri-apps/api/core";
import type { Audience } from "./memoryKeep";

export interface ConvUse {
  started_at: number;
  audience?: Audience;
  private: boolean;
  excluded?: number[];
}

export const CONV_USE_EVENT = "yoai-conversation-use";

export async function conversationsNotUsed(): Promise<ConvUse[]> {
  return invoke<ConvUse[]>("memory_conversations_not_used").catch(() => []);
}

async function change(input: { startedAt: number; private?: boolean; audience?: Audience; exclude?: number; include?: number }): Promise<ConvUse> {
  const next = await invoke<ConvUse>("memory_conversation_use_set", { change: input });
  if (typeof window !== "undefined") window.dispatchEvent(new CustomEvent(CONV_USE_EVENT));
  return next;
}

/** Keep a conversation out of memory, or let it be used again. */
export function setPrivate(startedAt: number, on: boolean): Promise<ConvUse> {
  return change({ startedAt, private: on });
}

/** Leave one message out, or use it again. */
export function setMessageUsed(startedAt: number, seq: number, used: boolean): Promise<ConvUse> {
  return change(used ? { startedAt, include: seq } : { startedAt, exclude: seq });
}

/** Everything about a conversation back to the default. */
export async function useAgain(u: ConvUse): Promise<void> {
  let next = await change({ startedAt: u.started_at, private: false, audience: { kind: "all" } });
  for (const seq of next.excluded ?? []) next = await change({ startedAt: u.started_at, include: seq });
}

/** What is held back, in the list's words. */
export function notUsedWords(u: ConvUse): string {
  if (u.private) return "Kept out of memory. It is never searched and nothing is kept from it.";
  const n = u.excluded?.length ?? 0;
  const parts: string[] = [];
  if (n > 0) parts.push(`${n} ${n === 1 ? "message is" : "messages are"} not used`);
  switch (u.audience?.kind) {
    case "ai":
      parts.push("only one AI may use it");
      break;
    case "project":
      parts.push("used only in its project");
      break;
    case "local_ais":
      parts.push("used only by AIs on your devices");
      break;
    case "none":
      parts.push("no AI may use it");
      break;
  }
  const s = parts.join("; ");
  return s ? `${s[0].toUpperCase()}${s.slice(1)}.` : "";
}
