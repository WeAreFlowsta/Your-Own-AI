/**
 * Disposition + turn-mode prompt utilities.
 *
 * Two axes:
 *  - Disposition (per-AI): a soft *length lean* injected at {{aiResponseLength}}.
 *    NOT a hard cap — the model flexes per question and honors explicit asks.
 *  - Turn mode (per-turn): chat | report | code. Appends the report ceremony /
 *    code-mode block. Set by the sparkle action (explicit) → classifier → chat.
 */

import { TurnMode } from '../types';

const HONOR_EXPLICIT =
  " Match your length to what's actually being asked, and honor any explicit length or format the user requests in their message.";

/**
 * The resting length lean injected into the system prompt ({{aiResponseLength}}).
 */
export function getDispositionPrompt(disposition: string): string {
  switch (disposition) {
    case 'thorough':
      return "Response Length: This is a depth-first AI — give thorough, genuinely complete answers every time. Don't just answer the surface question: develop it fully — the background and context, every relevant angle, the reasoning and evidence behind each point, concrete examples, the trade-offs, the implications, and the caveats. Organize a longer answer into clear sections so it stays readable. Always prefer developing a point to summarizing it, and don't stop short — finish so the reader is left with nothing important missing. This depth applies to any kind of answer, code and explanations included. (Depth and completeness, never filler, padding, or repetition.)" + HONOR_EXPLICIT;

    case 'balanced':
      return "Response Length: Answer at a natural length for the question — neither clipped nor padded." + HONOR_EXPLICIT;

    case 'conversational':
    default:
      return "Response Length: Rest short and conversational — a sentence or a couple of short paragraphs for everyday questions — but open up fully and go deep when the question genuinely calls for it." + HONOR_EXPLICIT;
  }
}

/**
 * The per-turn mode's extra system-prompt block. Appended after the base prompt.
 * Report mode owns the <think> ceremony + structure; code mode the code-first block.
 */
export function getModePrompt(mode: TurnMode | null | undefined): string {
  switch (mode) {
    case 'report':
      return "\n\nReport Mode: Produce a long-form, in-depth report — comprehensive and genuinely substantial, the kind that runs several full sections and saves the reader hours. This overrides any brevity lean: go wide and deep. Be honest about uncertainty and assumptions, and never invent facts, figures, or sources to sound authoritative.\n\nFirst think it through inside <think></think> tags — the angles, the trade-offs, the structure you'll use — then write the report. The thinking is shown to the reader separately.\n\nFormat:\n<think>\n[Your reasoning: key angles, trade-offs, structure plan]\n</think>\n\n[Your report]\n\nIn the report:\n- Open with a one- or two-sentence bottom line.\n- Then develop the topic across several substantial sections with ## headings. Make each section genuinely developed — multiple full paragraphs that explain the how and the why, with concrete detail, examples, evidence, and trade-offs; use bullet points, bold for key terms, and tables where they help. Never write one-line or single-sentence sections.\n- Cover every important angle thoroughly, and surface caveats and nuances honestly.\n- Close with a short, actionable takeaway.\n- Keep a light touch of your own voice — it's a report, not a different personality.\n\nKeep developing the report in full depth until the topic is genuinely and completely covered — this is a detailed long-form report, not a summary. You MUST close the </think> tag before the report.";

    case 'code':
      return "\n\nCode Mode: The user has requested code. Prioritise providing complete, working code over explanation. Include helpful comments within the code. After the code, briefly explain key decisions or usage if needed. If the request involves multiple files or steps, present them in logical order.";

    case 'chat':
    default:
      return "";
  }
}

/**
 * Map a sparkle action to the turn mode it invokes. null → no explicit mode (chat).
 */
export function getTurnModeFromAction(action: string | null): TurnMode | null {
  if (action === 'Write a report...') return 'report';
  if (action === 'Write code...') return 'code';
  return null;
}
