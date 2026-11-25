/**
 * The words of the line under a reply, from what the backend decided the
 * reply gets (`ReplyLine`, chosen in `memory_quotes`). Words only: no
 * imports, so the wording can be tested by itself.
 */
export type WhyNot =
  | "component_missing"
  | "component_downloading"
  | "not_enough_memory"
  | "did_not_start_in_time"
  | "timed_out"
  | "vault_locked"
  | "index_rebuilding"
  | "switched_off_for_this_ai";

export type ReplyLine =
  | { kind: "given"; passages: number }
  | { kind: "quoted-none"; passages: number }
  | { kind: "did-not-fit"; given: number; found: number }
  | { kind: "read-so-far"; found: number; percent: number }
  | { kind: "no-mention"; names: string[] }
  | { kind: "did-not-look"; why: WhyNot }
  | { kind: "withheld"; conversations: number };

const passages = (n: number) => `${n} ${n === 1 ? "passage" : "passages"}`;

/** Why the AI did not look, as the person is told. */
export function whyNotWords(why: WhyNot): string {
  switch (why) {
    case "component_missing": return "the part of the app that searches is not installed";
    case "component_downloading": return "the part of the app that searches is still downloading";
    case "not_enough_memory": return "the computer did not have enough free memory to search";
    case "did_not_start_in_time": return "the search did not start in time";
    case "timed_out": return "the search took too long";
    case "vault_locked": return "your Vault is locked";
    case "index_rebuilding": return "the search index is being rebuilt";
    case "switched_off_for_this_ai": return "memory is switched off for this AI";
  }
}

export interface LineWords {
  /** The small label over the line, when a search ran. */
  label: string | null;
  text: string;
  /** Whether "Show" belongs on this line (there are passages to show). */
  show: boolean;
  /** "Read the rest now" belongs on this line. */
  readRest: boolean;
}

export function givenLineWords(line: ReplyLine, ai: string): LineWords {
  const searched = "Searched your conversations";
  switch (line.kind) {
    case "given":
      return { label: searched, text: `${ai} was given ${passages(line.passages)} from your conversations.`, show: true, readRest: false };
    case "quoted-none":
      return { label: searched, text: `${ai} was given ${passages(line.passages)} and quoted none.`, show: true, readRest: false };
    case "did-not-fit":
      return { label: searched, text: `${ai} was given ${line.given} of ${passages(line.found)} found; the rest did not fit.`, show: line.given > 0, readRest: false };
    case "read-so-far":
      return { label: "Searched what is read so far", text: `${line.found} found in the ${line.percent}% of your conversations read so far. The answer may be in the rest.`, show: line.found > 0, readRest: true };
    case "no-mention": {
      const names = line.names.map((n) => `"${n}"`);
      const list = names.length > 1 ? `${names.slice(0, -1).join(", ")} or ${names[names.length - 1]}` : names[0];
      return { label: searched, text: `No conversation ${ai} can use mentions ${list}.`, show: true, readRest: false };
    }
    case "did-not-look":
      if (line.why === "switched_off_for_this_ai") {
        return { label: null, text: `Memory is not shared with ${ai}. ${ai} answered from this conversation only, without looking at your earlier conversations. Your rules were still sent.`, show: false, readRest: false };
      }
      return { label: null, text: `${ai} answered without looking at your earlier conversations: ${whyNotWords(line.why)}.`, show: false, readRest: false };
    case "withheld":
      return { label: searched, text: `${ai} cannot use ${line.conversations} ${line.conversations === 1 ? "conversation" : "conversations"} you had with another AI that ${line.conversations === 1 ? "mentions" : "mention"} this.`, show: true, readRest: false };
  }
}

/** What went with a message that left this computer, as its reply's record holds it. */
export interface Sent {
  passages: number;
  lines: number;
  note: boolean;
  /** Lines the person keeps about themselves, sent with every message. */
  about?: number;
}

/** Did anything from memory go with the message? */
export const sentAnything = (sent: Sent) => sent.passages + sent.lines + (sent.about ?? 0) > 0 || sent.note;

/** Where a reply was made, from its recorded model. */
export function whereWords(servedBy: string): string {
  return servedBy.startsWith("external:") ? `your server (${servedBy.slice(9)})` : servedBy.startsWith("online:") ? `${servedBy.slice(7)}, online` : "an online model";
}

/** The line shown once, under the first reply for which memory went with
 *  a message to an AI that answers elsewhere. `changed`: the AI was here
 *  before memory was rebuilt, so what it is sent is different now. */
export function firstSentWords(ai: string, servedBy: string, changed: boolean): string {
  if (changed) {
    return `What ${ai} is sent from memory has changed. It is now what memory finds for each question you ask. The list of facts and the short description of you are no longer sent.`;
  }
  return `${ai} runs online, so what memory found for your question was sent to ${whereWords(servedBy)} with your message. The line under each reply shows exactly what was sent.`;
}

/** The sentence that says what memory sent with a message to a model that
 *  runs elsewhere. `servedBy` is the reply's model as recorded
 *  ("online:<name>" or "external:<name>"). */
export function sentWords(sent: Sent, servedBy: string): string {
  const where = whereWords(servedBy);
  const n = sent.passages + sent.lines;
  const about = sent.about ?? 0;
  if (!sentAnything(sent)) {
    return `Nothing from memory was sent. Memory found nothing for this question. Only this conversation and your rules went to ${where}.`;
  }
  const parts = [
    n > 0 ? `${passages(n)} from your conversations` : "",
    sent.note ? "what memory holds now" : "",
    about > 0 ? `${about} ${about === 1 ? "thing" : "things"} you keep about yourself` : "",
  ].filter(Boolean);
  const what = parts.length > 1 ? `${parts.slice(0, -1).join(", ")} and ${parts[parts.length - 1]}` : parts[0];
  return `Sent to ${where}. With your question: ${what}.`;
}

/** "March 3" from "2026-03-03" (the year too when it is not this one). */
export function dayWords(day: string, thisYear: number = new Date().getFullYear()): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day);
  if (!m) return day;
  const months = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
  const text = `${months[Number(m[2]) - 1] ?? m[2]} ${Number(m[3])}`;
  return Number(m[1]) === thisYear ? text : `${text}, ${m[1]}`;
}
