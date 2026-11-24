/**
 * Remember, pressed on a message: the exchange is kept as one memory item
 * that points at where it was said. The same call serves every place a
 * Remember button stands (a reply, the person's own message, selected
 * words, an entry on the Memory page).
 */
import { invoke } from "@tauri-apps/api/core";
import { getLocalCustomAis } from "./localAiStorage";
import { foundingOf } from "./lookalikes";
import { openConversation } from "./turns";

/** Who may use an item. */
export type Audience = { kind: "all" } | { kind: "ai"; ai: string } | { kind: "project"; project: string } | { kind: "local_ais" } | { kind: "none" };

export interface KeptItem {
  id: string;
  text: string;
  asOf: number;
  updatedAt: number;
  savedBy: string;
  item: { state: string; audience?: Audience; source?: { kind: string } | null; [k: string]: unknown };
}

/** The answer to a keep: the item, the change an undo names, and whether
 *  it was kept before (then nothing was written). */
export interface Kept {
  item: KeptItem;
  change: string | null;
  already: boolean;
}

export interface KeepRequest {
  /** The AI's id on this device (a message's `model`). */
  aiId: string;
  role: "user" | "assistant";
  /** The message as its bubble shows it. */
  text: string;
  seq?: number;
  /** The selected words, for Remember on a selection. */
  selection?: string;
  /** The conversation, when it is not the open one. */
  conversation?: string;
}

/** The AI's keys: where its conversations are recorded, and the key that
 *  names it on every device. */
export async function keysOf(aiId: string): Promise<{ agent: string; founding: string; name: string }> {
  const ai = (await getLocalCustomAis().catch(() => [])).find((a) => a.id === aiId || a.agentPubKey === aiId);
  return { agent: ai?.agentPubKey ?? "", founding: ai ? foundingOf(ai) : "", name: ai?.name ?? "" };
}

export async function keepMessage(req: KeepRequest): Promise<Kept> {
  const conversation = req.conversation ?? openConversation();
  if (!conversation) throw new Error("This conversation is still being saved. Try again in a moment.");
  const { agent } = await keysOf(req.aiId);
  return invoke<Kept>("memory_keep_message", {
    input: {
      conversation,
      agentKey: agent || null,
      seq: typeof req.seq === "number" ? req.seq : null,
      role: req.role,
      text: req.text,
      selection: req.selection ?? null,
    },
  });
}

/** Take a keep back: the item is forgotten. */
export async function undoKeep(change: string): Promise<void> {
  await invoke("memory_undo", { changeId: change });
}

export async function setAudience(id: string, audience: Audience): Promise<KeptItem> {
  return invoke<KeptItem>("memory_item_change", { id, edit: { do: "audience", audience } });
}

/** "March 3", or "March 3, 2025" for another year. */
export function dayOf(ms: number): string {
  const d = new Date(ms);
  const sameYear = d.getFullYear() === new Date().getFullYear();
  return d.toLocaleDateString("en-US", sameYear ? { month: "long", day: "numeric" } : { month: "long", day: "numeric", year: "numeric" });
}

/** Whom an item is for, in the Kept line's words. */
export function audienceWords(a: Audience | undefined, aiName: string): string {
  switch (a?.kind) {
    case "ai":
      return `only for ${aiName || "this AI"}`;
    case "project":
      return "for this project";
    case "local_ais":
      return "for the AIs on your devices";
    case "none":
      return "for no AI";
    default:
      return "for all your AIs";
  }
}

/** A message from the backend, as the person should read it. */
export function keepError(e: unknown): string {
  const s = String((e as { message?: string })?.message ?? e ?? "").replace(/^Error:\s*/, "");
  if (/still being saved|not saved yet|not in the list yet/.test(s)) return "This message is still being saved. Try again in a moment.";
  return "Could not keep it just now.";
}
