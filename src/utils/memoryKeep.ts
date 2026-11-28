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
  /** The item was open and is now marked done. */
  closed?: boolean;
  /** Said in the chat in place of a control: what the app did. */
  said?: "confirmed" | "forgotten" | "kept_out" | "left_out" | "off_here" | "on_here";
  /** The conversation, and the messages left out, for the last two. */
  conversation?: { startedAt: number; seqs: number[] };
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

/** Sent when something was kept for a message from outside its own
 *  buttons (selected words, or "remember that" said in the chat): the
 *  message shows the Kept line. */
export const KEPT_EVENT = "yoai-kept";
export interface KeptEventDetail {
  messageId: string;
  kept: Kept;
  selection?: boolean;
}
export function announceKept(detail: KeptEventDetail): void {
  if (typeof window !== "undefined") window.dispatchEvent(new CustomEvent<KeptEventDetail>(KEPT_EVENT, { detail }));
}

/**
 * "Remember that", said in the chat: keep the message the words point at,
 * once the turn is recorded, and show the Kept line under it. The records
 * can lag the reply by a moment, so a "not saved yet" answer is asked
 * again a few times.
 */
export async function keepSaid(req: KeepRequest & { messageId: string }): Promise<Kept | null> {
  for (let attempt = 0; attempt < 6; attempt++) {
    try {
      const kept = await invoke<Kept>("memory_keep_message", {
        input: {
          conversation: req.conversation ?? openConversation(),
          agentKey: (await keysOf(req.aiId)).agent || null,
          seq: typeof req.seq === "number" ? req.seq : null,
          role: req.role,
          text: req.text,
          why: "said",
        },
      });
      announceKept({ messageId: req.messageId, kept });
      return kept;
    } catch (e) {
      if (!/still being saved|not saved yet|not in the list yet/.test(String((e as { message?: string })?.message ?? e))) throw e;
      await new Promise((r) => setTimeout(r, 2500));
    }
  }
  return null;
}

/**
 * "That's done", said in the chat: when the conversation left one thing
 * open, it is marked done and the line under the message says so, with
 * Undo. The words are read on the device by a fixed rule; any other
 * message changes nothing. Asked once the turn is recorded.
 */
export async function doneSaid(req: { messageId: string; aiId: string; text: string; conversation?: string }): Promise<Kept | null> {
  for (let attempt = 0; attempt < 6; attempt++) {
    try {
      const closed = await invoke<Kept | null>("memory_done_said", {
        input: { conversation: req.conversation ?? openConversation(), agentKey: (await keysOf(req.aiId)).agent || null, seq: null, role: "user", text: req.text },
      });
      if (closed) {
        announceKept({ messageId: req.messageId, kept: closed });
        if (typeof window !== "undefined") window.dispatchEvent(new CustomEvent("yoai-memory-changed"));
      }
      return closed;
    } catch (e) {
      if (!/still being saved|not saved yet|not in the list yet/.test(String((e as { message?: string })?.message ?? e))) throw e;
      await new Promise((r) => setTimeout(r, 2500));
    }
  }
  return null;
}

/**
 * A control said in the chat ("that's right", "forget that", "keep this
 * conversation out of memory"): the words are read on the device by a
 * fixed rule, and when it is clear what they are about the app does it
 * and the line under the message says so, with Undo. Any other message,
 * or "that" with two things it could mean, changes nothing. `used`: the
 * items the reply before was given in its note. Asked once the turn is
 * recorded.
 */
export async function saidControl(req: { messageId: string; aiId: string; text: string; conversation?: string; used: string[] }): Promise<Kept | null> {
  for (let attempt = 0; attempt < 6; attempt++) {
    try {
      const did = await invoke<{ did: Kept["said"]; item: KeptItem | null; change: string | null; startedAt: number; seqs: number[] } | null>("memory_said", {
        input: { conversation: req.conversation ?? openConversation(), agentKey: (await keysOf(req.aiId)).agent || null, seq: null, role: "user", text: req.text },
        used: req.used,
      });
      if (!did) return null;
      const kept: Kept = {
        item: did.item ?? { id: "", text: "", asOf: Date.now(), updatedAt: Date.now(), savedBy: "person", item: { state: "current" } },
        change: did.change,
        already: false,
        said: did.did,
        conversation: { startedAt: did.startedAt, seqs: did.seqs },
      };
      announceKept({ messageId: req.messageId, kept });
      if (typeof window !== "undefined") window.dispatchEvent(new CustomEvent("yoai-memory-changed"));
      return kept;
    } catch (e) {
      if (!/still being saved|not saved yet|not in the list yet/.test(String((e as { message?: string })?.message ?? e))) throw e;
      await new Promise((r) => setTimeout(r, 2500));
    }
  }
  return null;
}

/** Do replies on this computer look in memory? (This computer only; it
 *  is never synced.) */
export function memoryInReplies(): Promise<boolean> {
  return invoke<boolean>("memory_in_replies");
}

/** Turn using memory in replies on or off, on this computer only. */
export async function setMemoryInReplies(on: boolean): Promise<void> {
  await invoke("memory_in_replies_set", { on });
  if (typeof window !== "undefined") window.dispatchEvent(new CustomEvent("yoai-memory-in-replies", { detail: { on } }));
}

/** Take back "keep this conversation out of memory" or a "forget that"
 *  that left an exchange out. */
export async function undoSaidAboutConversation(c: { startedAt: number; seqs: number[] }): Promise<void> {
  if (c.seqs.length === 0) {
    await invoke("memory_conversation_use_set", { change: { startedAt: c.startedAt, private: false } });
    return;
  }
  for (const seq of c.seqs) await invoke("memory_conversation_use_set", { change: { startedAt: c.startedAt, include: seq } });
}

const SEEN_KEY = "yoai-memory-changes-seen-at";

/** How many changes memory made by itself (an AI kept something, tidying
 *  replaced or closed something) since the person last looked at Recently. */
export async function unseenChanges(): Promise<number> {
  let seen = 0;
  try {
    seen = Number(localStorage.getItem(SEEN_KEY) ?? 0) || 0;
  } catch {
    /* counted from the beginning */
  }
  const log = await invoke<{ at: number; by: string; what: string }[]>("memory_changes");
  return log.filter((c) => c.at > seen && c.by !== "person" && c.what !== "undone").length;
}

/** The person is looking at Recently: everything up to now is seen. */
export function markChangesSeen(): void {
  try {
    localStorage.setItem(SEEN_KEY, String(Date.now()));
  } catch {
    /* the count then stays until the next look */
  }
}
