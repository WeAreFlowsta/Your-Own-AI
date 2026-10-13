/**
 * Live turns at the app root (build-docs ALWAYS_ON_AND_THREADS.md, step 1).
 *
 * A reply used to live in the chat PAGE: leave the page and the streaming
 * text had no home; come back and the page rebuilt itself from the records,
 * where the reply was not yet (the Holochain team's 0.8.0 report). A turn is
 * now registered here the moment its request starts, updated on every
 * flush, and ended with its outcome - whatever page is open. The chat page
 * attaches to the live turn for its conversation; the activity tray and the
 * drawer read the same list (step 2).
 *
 * Plain module state (no framework): the AbortControllers stay here, never
 * in a reactive store. Components mirror through TurnsContext, which
 * listens for TURNS_EVENT.
 */
import type { Message } from "../types";

export type TurnKind = "chat" | "tools" | "project";
export type TurnState = "running" | "queued" | "waiting" | "done" | "stopped" | "error";

export interface Turn {
  /** = the assistant message's id. */
  id: string;
  kind: TurnKind;
  aiId: string;
  agentKey: string;
  aiLabel: string;
  /** Known once the records answered; null until then. */
  conversationHash: string | null;
  question: string;
  model: string | null;
  state: TurnState;
  startedAt: number;
  endedAt?: number;
  /** The two messages of the turn, as the page would render them. */
  user: Message;
  assistant: Message;
  /** The interjection: keep the reply up to this many visible characters, then stop. */
  stopAtBoundary: number | null;
}

export const TURNS_EVENT = "yoai-turns";
/** A finished turn stays listed this long for the tray and for a page that
 *  comes back late; the conversation holds it in the records by then. */
const KEEP_ENDED_MS = 10 * 60_000;

const turns = new Map<string, Turn>();
const controllers = new Map<string, AbortController>();

function changed(): void {
  try { window.dispatchEvent(new CustomEvent(TURNS_EVENT)); } catch { /* no window */ }
}

/** A line into the app log (the UI tests and support read it there). */
function logLine(line: string): void {
  try { void import("./uiLog").then(({ uiLog }) => uiLog(`[turns] ${line}`)); } catch { /* no window */ }
}

// Test aids: the live list, readable from the page (the UI tests print it), and the full records-side ids.
try { (window as any).__yoaiTurnsFull = () => allTurns().map((t) => ({ agentKey: t.agentKey, conversationHash: t.conversationHash, state: t.state })); } catch { /* no window */ }
try { (window as any).__yoaiTurns = () => allTurns().map((t) => ({ id: t.id.slice(0, 8), kind: t.kind, ai: t.aiId.slice(0, 12), conv: t.conversationHash?.slice(0, 12) ?? null, state: t.state, chars: t.assistant.content.length })); } catch { /* no window */ }

export function beginTurn(turn: Turn, controller: AbortController): void {
  turns.set(turn.id, turn);
  controllers.set(turn.id, controller);
  logLine(`begin ${turn.id.slice(0, 8)} ${turn.kind} ai ${turn.aiId.slice(0, 12)} conv ${turn.conversationHash?.slice(0, 12) ?? "-"}`);
  changed();
}

export function updateTurn(id: string, patch: Partial<Omit<Turn, "assistant" | "user">> & { assistant?: Partial<Message>; user?: Partial<Message> }): void {
  const t = turns.get(id);
  if (!t) return;
  const { assistant, user, ...rest } = patch;
  Object.assign(t, rest);
  if (assistant) t.assistant = { ...t.assistant, ...assistant };
  if (user) t.user = { ...t.user, ...user };
  changed();
}

export function endTurn(id: string, state: "done" | "stopped" | "error"): void {
  const t = turns.get(id);
  controllers.delete(id);
  if (!t || t.state === "done" || t.state === "stopped" || t.state === "error") return;
  t.state = state;
  t.endedAt = Date.now();
  logLine(`end ${id.slice(0, 8)} ${state} conv ${t.conversationHash?.slice(0, 12) ?? "-"} ${t.assistant.content.length} chars`);
  t.assistant = { ...t.assistant, isLoading: false, statusText: undefined, stopped: state === "stopped" ? true : t.assistant.stopped };
  changed();
  setTimeout(() => { turns.delete(id); changed(); }, KEEP_ENDED_MS);
}

export function getTurn(id: string): Turn | null {
  return turns.get(id) ?? null;
}

export function allTurns(): Turn[] {
  return [...turns.values()].sort((a, b) => a.startedAt - b.startedAt);
}

export function isLive(t: Turn): boolean {
  return t.state === "running" || t.state === "queued" || t.state === "waiting";
}

/** The live turn of a conversation, if any. */
export function liveTurnFor(conversationHash: string | null): Turn | null {
  if (!conversationHash) return null;
  for (const t of turns.values()) if (t.conversationHash === conversationHash && isLive(t)) return t;
  return null;
}

/** Stop a turn now, from any page. */
export function abortTurn(id: string): boolean {
  const c = controllers.get(id);
  if (!c) return false;
  c.abort();
  return true;
}

/** The interjection on a turn: finish the sentence in flight, then stop (a
 *  second request, or one before the first visible word, stops at once). */
export function requestBoundary(id: string): void {
  const t = turns.get(id);
  if (!t || !isLive(t)) return;
  const visible = t.assistant.content ?? "";
  if (!visible.trim() || t.stopAtBoundary !== null) {
    abortTurn(id);
    return;
  }
  t.stopAtBoundary = visible.length;
  changed();
}
