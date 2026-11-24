/**
 * When a rule is offered. A rule is never made by itself: an instruction
 * about how to work is noted the first time, and the offer to keep it as
 * a rule comes the second time it is given. An answered offer (kept, or
 * No) is not made again. The counting is in the backend; this is the
 * page's side of it.
 */
import { invoke } from "@tauri-apps/api/core";
import { looksLikeInstruction, offerRule } from "./memory";
import { aiScope, listMemoryRowsCached, personScope, projectScope } from "./memoryRows";

interface HeardAnswer {
  offer: boolean;
  earlier: string | null;
  why: string;
}

/** The project that is open in this window, by its id; empty when none. */
let openProject = "";
export function setOpenProject(id: string): void {
  openProject = id;
}
export function openProjectId(): string {
  return openProject;
}

const plain = (t: string) => t.trim().toLowerCase().replace(/\s+/g, " ");
/** The instruction last passed on, so a turn that is sent again (after a
 *  question to the person, or on another model) is not its own second time. */
let last = { text: "", at: 0 };

/** The person sent `message`. When it reads as a standing instruction it
 *  is noted, and the offer is raised under it if this is the second time.
 *  Never awaited by the turn. */
export async function hearInstruction(message: string, conversation: string | null, foundingKey: string | null): Promise<void> {
  if (!looksLikeInstruction(message)) return;
  const now = Date.now();
  if (last.text === plain(message) && now - last.at < 5 * 60_000) return;
  last = { text: plain(message), at: now };
  try {
    const scopes = [personScope, ...(foundingKey ? [aiScope(foundingKey)] : []), ...(openProject ? [projectScope(openProject)] : [])];
    const rules = (await Promise.all(scopes.map((s) => listMemoryRowsCached(s).catch(() => [])))).flat().filter((r) => r.kind === "rule" && !r.deleted).map((r) => r.text);
    const a = await invoke<HeardAnswer>("memory_rule_heard", { input: { text: message.trim(), conversation: conversation ?? "", at: now, rules } });
    if (a.offer) offerRule(message, message.trim());
  } catch (e) {
    console.warn("[rules] could not note the instruction:", e);
  }
}

/** The person answered an offer: it is not made again. */
export async function answerOffer(text: string, answer: "kept" | "declined"): Promise<void> {
  await invoke("memory_rule_offer_answered", { text: text.trim(), answer }).catch((e) => console.warn("[rules] could not record the answer:", e));
}

/** Whom a rule kept from an offer is for. */
export type RuleFor = "ai" | "all" | "project";

export function ruleKeptWords(whom: RuleFor, aiName: string): string {
  switch (whom) {
    case "ai":
      return `Kept as a rule for ${aiName || "this AI"}.`;
    case "project":
      return "Kept as a rule for this project.";
    default:
      return "Kept as a rule. Every AI is given it with every message.";
  }
}
