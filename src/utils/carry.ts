/**
 * What an AI carries - tools (Add-ons > Tools) and skills - and which of
 * them are ON. Adding and removing happens in the AI's form; the chip by
 * the message field switches each one on or off without removing it, so
 * "used by" stays true and a tool can rest for a plain question.
 */
import type { UserDefinedAI } from "../types";

type Carrier = Pick<UserDefinedAI, "mcp" | "mcpOff" | "skills" | "skillsOff"> | undefined | null;

/** The tools that act this turn: carried and not switched off. */
export function activeTools(ai: Carrier): string[] {
  const off = new Set(ai?.mcpOff ?? []);
  return (ai?.mcp ?? []).filter((n) => !off.has(n));
}

/** The skills that ride this turn: carried and not switched off. */
export function activeSkills(ai: Carrier): string[] {
  const off = new Set(ai?.skillsOff ?? []);
  return (ai?.skills ?? []).filter((n) => !off.has(n));
}
