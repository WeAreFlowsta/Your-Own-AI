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
      return { label: null, text: `${ai} answered without looking at your earlier conversations: ${whyNotWords(line.why)}.`, show: false, readRest: false };
    case "withheld":
      return { label: searched, text: `${ai} cannot use ${line.conversations} ${line.conversations === 1 ? "conversation" : "conversations"} you had with another AI that ${line.conversations === 1 ? "mentions" : "mention"} this.`, show: true, readRest: false };
  }
}

/** "March 3" from "2026-03-03" (the year too when it is not this one). */
export function dayWords(day: string, thisYear: number = new Date().getFullYear()): string {
  const m = /^(\d{4})-(\d{2})-(\d{2})$/.exec(day);
  if (!m) return day;
  const months = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
  const text = `${months[Number(m[2]) - 1] ?? m[2]} ${Number(m[3])}`;
  return Number(m[1]) === thisYear ? text : `${text}, ${m[1]}`;
}
