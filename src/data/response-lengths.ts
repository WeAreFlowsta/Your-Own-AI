/**
 * Length dispositions
 *
 * The AI's resting *length lean* — a soft default, not a hard cap. The model
 * calibrates around it per question (and honors explicit asks in the message);
 * the token budget is generous for all so nothing truncates. The actual length
 * is shaped by the prompt lean, not by maxTokens.
 */

import { ResponseLengthOption, LengthDisposition } from '../types';

// Generous ceiling for every disposition — the prompt does the shaping, this just
// prevents truncation (incl. long code / thorough answers).
const GENEROUS_MAX_TOKENS = 8192;

export const responseLengthOptions: ResponseLengthOption[] = [
  {
    id: 'conversational',
    name: 'Conversational',
    description: 'Rests short and chatty, but opens up fully when a question needs it',
    maxTokens: GENEROUS_MAX_TOKENS,
  },
  {
    id: 'balanced',
    name: 'Balanced',
    description: 'Natural length, no strong lean either way',
    maxTokens: GENEROUS_MAX_TOKENS,
  },
  {
    id: 'thorough',
    name: 'Thorough',
    description: 'Leans into depth and structure, reasoning things through',
    maxTokens: GENEROUS_MAX_TOKENS,
  },
];

export function getResponseLengthById(id: string): ResponseLengthOption | undefined {
  return responseLengthOptions.find(opt => opt.id === id);
}

/** Default disposition: Conversational (rests chatty, flexes up when warranted). */
export function getDefaultResponseLength(): ResponseLengthOption {
  return responseLengthOptions[0];
}

export function getResponseLengthName(id: string): string {
  const option = getResponseLengthById(id);
  return option?.name || 'Conversational';
}

/** Default for new/seeded AIs. */
export const DEFAULT_DISPOSITION: LengthDisposition = 'conversational';
