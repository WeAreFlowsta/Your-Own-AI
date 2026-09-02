/**
 * Turn-mode classifier: detects report/code requests automatically.
 *
 * Detects whether a single turn should switch to report/code mode, so a casual
 * conversational AI auto-produces a report when asked — without the user clicking
 * the sparkle. Upgrade-only and high-precision (a leaky guess here turns chat into
 * a report, which is worse than missing one — the sparkle is always the fallback).
 *
 * How: a few-shot generative classification on the model that's already loaded
 * (offline) or the AI's online model — one tiny call, grammar-constrained to
 * REPORT|CODE|NEITHER offline (capable online models follow the one-word format).
 * Measured 20/21 on the shipped small models; never throws (degrades to null).
 *
 * A free keyword gate skips the model call entirely for plainly-casual messages,
 * so normal chat pays nothing — the classification only runs when something looks
 * like a report/code request, or we're continuing a report/code session.
 */

import { llamaServerApi } from './llamaServerApi';
import { CODE_KW } from './routingKeywords';
import type { ChatMessage, TurnMode } from '../types';

const GRAMMAR = 'root ::= "REPORT" | "CODE" | "NEITHER"';

const REPORT_KW = [
  'report', 'write up', 'write-up', 'writeup', 'analysis', 'analyze', 'analyse',
  'deep dive', 'deep-dive', 'breakdown', 'rundown', 'overview', 'draft', 'write me',
  'put together', 'in-depth', 'in depth', 'dossier', 'white paper', 'comprehensive',
];

/** Coarse, free pre-gate: does the message look report/code-ish at all? */
function looksRelevant(message: string): boolean {
  const m = ` ${message.toLowerCase()} `;
  return REPORT_KW.some((k) => m.includes(k)) || CODE_KW.some((k) => m.includes(k));
}

/** Free code-ish hint for the ROUTER's base task - lets routing run in
 *  parallel with the model classification instead of waiting for it. The
 *  routing-task classifier can still upgrade/correct it. */
export function codeKeywordHint(message: string): boolean {
  const m = ` ${message.toLowerCase()} `;
  return CODE_KW.some((k) => m.includes(k));
}

// High-precision keyword verdicts for when the local server CANNOT classify
// (mid-load) without stalling the whole turn behind server health. Upgrade-
// only philosophy applies double here: these fire only on unambiguous
// ask-the-assistant phrasings; anything subtler just stays chat.
const REPORT_STRONG = [
  'write me a report', 'write a report', 'write up a', 'write a detailed',
  'put together a report', 'do a deep dive', 'deep dive on', 'in-depth report',
  'write a comprehensive', 'draft a report',
];
const CODE_STRONG = [
  'write a function', 'write code', 'write a script', 'write me a script',
  'write a program', 'implement a', 'implement the', 'fix this code',
  'debug this', 'refactor this',
];
function keywordVerdict(message: string): 'report' | 'code' | null {
  const m = message.toLowerCase();
  if (REPORT_STRONG.some((k) => m.includes(k))) return 'report';
  if (CODE_STRONG.some((k) => m.includes(k))) return 'code';
  return null;
}

const BASE = `Classify what the user is asking the assistant to DO right now. Output exactly one word: REPORT, CODE, or NEITHER.
Only answer REPORT or CODE if the user is asking YOU, the assistant, to PRODUCE it for them right now. If they are merely talking about, mentioning, complaining about, or planning a report/code in their own work or life, answer NEITHER.
REPORT = asking you to WRITE/DRAFT/PUT TOGETHER a report, write-up, analysis, deep-dive, or structured document.
CODE = asking you to write, implement, or fix code / a function / a script.
NEITHER = everything else, INCLUDING ordinary questions ('explain X', 'how does X work'), capability questions ('can you code?'), and any mention of a report/code that isn't a request for you to make one.

Examples:
write me a report on solar energy => REPORT
put together a detailed write-up of our options => REPORT
do a deep dive on how transformers work => REPORT
explain how vaccines work => NEITHER
how does the TLS handshake work? => NEITHER
my boss wants a report by friday => NEITHER
i need to write a report for work => NEITHER
the quarterly report is due tomorrow => NEITHER
i hate writing reports => NEITHER
the news report said it might rain => NEITHER
write a python function to parse a csv => CODE
fix this code: def f(): retrun 1 => CODE
our codebase is a mess => NEITHER
do you know how to code? => NEITHER
hey how's it going => NEITHER`;

function followBlock(prev: 'report' | 'code'): string {
  const P = prev.toUpperCase();
  return `\n\nThe assistant's PREVIOUS reply was a ${P}. If the user is now asking to continue, expand, go deeper, or elaborate on it, answer ${P}. If they've clearly moved to a new or casual topic, classify normally.
go deeper on the second section => ${P}
expand that => ${P}
thanks, that's helpful => NEITHER
anyway, any dinner ideas? => NEITHER`;
}

/**
 * @returns 'report' | 'code' to upgrade this turn, or null (no change — the caller
 *   falls back to the AI's defaultMode/chat). Never throws.
 */
export async function classifyTurnMode(
  message: string,
  model: string | undefined,
  previousMode: TurnMode | undefined,
): Promise<'report' | 'code' | null> {
  const isFollowUp = previousMode === 'report' || previousMode === 'code';

  // Pre-gate: plainly-casual message that isn't continuing a report/code session
  // → skip the model call entirely (instant, the common case).
  if (!isFollowUp && !looksRelevant(message)) return null;

  // A LOCAL model that is still loading can't classify without stalling the
  // whole turn behind server health (the 6s abort burned in full right after
  // launch - measured 2026-07-17). Use the keyword verdict instead of
  // waiting; the sparkle menu and the AI's default mode always still win.
  const isLocal =
    !model || (!model.startsWith('online:') && !model.startsWith('external:'));
  if (isLocal) {
    try {
      const { invoke } = await import('@tauri-apps/api/core');
      const ready = await invoke<boolean>('is_llama_server_ready');
      if (!ready) return keywordVerdict(message);
    } catch {
      /* can't tell - proceed with the normal model call */
    }
  }

  const sys = BASE + (isFollowUp ? followBlock(previousMode as 'report' | 'code') : '');
  const msgs: ChatMessage[] = [{ role: 'user', content: message } as ChatMessage];

  // Runs on the ACTIVE chat/online model, NOT the utility model: measured
  // 2026-06-28 that the small utility model (Ministral) is a weaker classifier
  // than a typical chat model (it under-fires REPORT on "deep dive"/"write-up"),
  // and the classifier's privacy cost is trivial (one word, on a message that
  // already went to that model). Extraction still prefers the utility model.
  try {
    const controller = new AbortController();
    const timer = setTimeout(() => controller.abort(), 6000);
    let out = '';
    for await (const chunk of llamaServerApi.chatCompletion(
      msgs, sys, controller.signal, 3, false, model, GRAMMAR,
    )) {
      if (chunk.type === 'text') out += chunk.content;
    }
    clearTimeout(timer);
    const w = out.trim().toUpperCase();
    if (w.startsWith('REPORT')) return 'report';
    if (w.startsWith('CODE')) return 'code';
    return null;
  } catch {
    return null; // classification must never break a turn
  }
}
