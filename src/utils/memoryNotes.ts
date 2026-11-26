/**
 * Notes in memory. A line of a note that answered a question sits on a
 * Subject page and points at the file: the note stays the person's own,
 * changed where it lives. "Do not use this" has a reach (this line, this
 * note, this folder) and can be taken back from the Not used list.
 */
import { invoke } from "@tauri-apps/api/core";

/** How a note an item points at stands now. */
export interface NoteView {
  id: string;
  name: string;
  /** When the file last changed (seconds). */
  changed: number;
  state: "here" | "line-gone" | "deleted" | "unreachable" | "not-used";
  asked?: string | null;
  path?: string | null;
}

export type Reach = "line" | "note" | "folder";

/** A line, a note or a folder the person chose not to have used. */
export interface NotUsedNote {
  id: string;
  reach: Reach;
  name: string;
  line?: string | null;
  folder?: string | null;
  at: number;
}

export const NOTES_EVENT = "yoai-notes-use";

let following: Promise<NoteView[]> | null = null;

/** Every note item, looked at again as the library holds its file now.
 *  Calls made together share one look. */
export function followNotes(): Promise<NoteView[]> {
  if (!following) {
    following = invoke<NoteView[]>("memory_notes_follow")
      .catch(() => [] as NoteView[])
      .finally(() => setTimeout(() => (following = null), 2000));
  }
  return following;
}

function changed(): void {
  following = null;
  if (typeof window === "undefined") return;
  window.dispatchEvent(new CustomEvent(NOTES_EVENT));
  window.dispatchEvent(new CustomEvent("yoai-memory-changed"));
}

export async function doNotUse(id: string, reach: Reach): Promise<void> {
  await invoke("memory_note_do_not_use", { id, reach });
  changed();
}

export function notesNotUsed(): Promise<NotUsedNote[]> {
  return invoke<NotUsedNote[]>("memory_notes_not_used").catch(() => []);
}

export async function useNoteAgain(id: string): Promise<void> {
  await invoke("memory_note_use_again", { id });
  changed();
}

/** A reply was given passages of notes: the lines that answered are kept
 *  (a rule on the device decides; no model is asked). The message may
 *  still be on its way into the record, so it is asked again for a while. */
export async function noteAnswered(req: { conversation: string; agentKey: string | null; question: string; reply: string; given: { doc_id: string; idx?: number[] }[] }): Promise<void> {
  const given = req.given.filter((g) => g.idx?.length).map((g) => ({ doc_id: g.doc_id, idx: g.idx }));
  if (given.length === 0 || !req.conversation) return;
  for (let attempt = 0; attempt < 6; attempt++) {
    try {
      const kept = await invoke<unknown[]>("memory_note_answered", {
        input: { conversation: req.conversation, agentKey: req.agentKey, seq: null, role: "user", text: req.question },
        reply: req.reply,
        given,
      });
      if (kept.length > 0) changed();
      return;
    } catch (e) {
      if (!/still being saved|not saved yet|not in the list yet/.test(String((e as { message?: string })?.message ?? e))) throw e;
      await new Promise((r) => setTimeout(r, 2500));
    }
  }
}

const day = (secs: number) => new Date(secs * 1000).toLocaleDateString("en-US", { month: "long", day: "numeric", year: "numeric" });

/** "From your notes: Site checklist.md, edited April 28, 2026". */
export function noteHeading(n: { name: string; changed: number }): string {
  return `From your notes: ${n.name}, edited ${day(n.changed)}`;
}

/** Why the line is on the page. */
export function noteReason(asked: string | null | undefined, askedAtMs: number): string {
  const when = new Date(askedAtMs).toLocaleDateString("en-US", { month: "long", day: "numeric" });
  const q = (asked ?? "").trim();
  return q ? `You asked "${q.length > 90 ? `${q.slice(0, 90)}...` : q}" on ${when} and this line answered it.` : `This line answered a question on ${when}.`;
}

/** What a state says under the line, when the note is not simply there. */
export function noteStateWords(state: NoteView["state"]): string {
  switch (state) {
    case "deleted":
      return "The note was deleted. This line is no longer used.";
    case "line-gone":
      return "You changed this line in the note. It is no longer used.";
    case "unreachable":
      return "The folder is not reachable right now. Nothing is lost.";
    default:
      return "";
  }
}

/** A Not used entry, in the list's words. */
export function notUsedNoteWords(u: NotUsedNote): { title: string; detail: string } {
  if (u.reach === "folder") return { title: `Folder: ${u.name}`, detail: "No note in this folder is used." };
  if (u.reach === "note") return { title: `Note: ${u.name}`, detail: "Nothing in this note is used." };
  return { title: u.line ?? "", detail: `This line of ${u.name} is not used.` };
}
