/**
 * An AI's description is a small template, filled from the AI's live
 * settings wherever it is shown - so a change of personality or model never
 * leaves the words stale (Eric, 2026-09-05: the generated sentence went
 * stale the moment the personality changed).
 *
 * Placeholders, all optional, in any order and any sentence:
 *   {name}            Veebo
 *   {personality}     a playful explorer  (with its article; "an AI with a
 *                     personality of its own" when no archetype)
 *   {models}          a phrase for the model setting, see describeModels()
 *   {mode}            chat | report | code
 *   {mode.sentence}   "Every reply is a structured report." or nothing
 *   {tools}           "2 tools and 1 skill" | "no tools or skills"
 *   {tools.sentence}  "Carries 2 tools and 1 skill." or nothing
 * A description with no placeholders is shown as written.
 */
import { getArchetypeById } from '../data/bundled-archetypes';
import { richModelName } from './modelNameFormatter';

export const DEFAULT_DESCRIPTION_TEMPLATE =
  '{name} is {personality}. Answers come from {models}. {mode.sentence} {tools.sentence}';

export const DESCRIPTION_PLACEHOLDERS: { token: string; means: string }[] = [
  { token: '{name}', means: 'the name' },
  { token: '{personality}', means: 'the personality, as "a playful explorer"' },
  { token: '{models}', means: 'the model setting, in words' },
  { token: '{mode.sentence}', means: 'a sentence when replies are always a report or code' },
  { token: '{tools.sentence}', means: 'a sentence when tools or skills are carried' },
];

/** The fields the template reads; every AI record and the edit dialog's store have them. */
export interface DescribableAi {
  name: string;
  description?: string;
  baseArchetypeId?: string;
  model?: string;
  defaultMode?: string;
  skills?: string[];
  mcp?: string[];
}

export interface DescribeOptions {
  /** id -> display name, for "GPT-6 Astra, online" instead of the id. */
  onlineNames?: Record<string, string>;
}

function article(phrase: string): string {
  return /^[aeiou]/i.test(phrase) ? 'an' : 'a';
}

function humanizeId(id: string): string {
  return id
    .split(/[-_]/)
    .filter(Boolean)
    .map((w) => (/^[a-z]+$/.test(w) ? w[0].toUpperCase() + w.slice(1) : w.toUpperCase()))
    .join(' ');
}

/** The model setting as a phrase that reads inside a sentence. */
export function describeModels(model: string | undefined, opts?: DescribeOptions): string {
  const m = (model || '').trim();
  if (!m || m === 'auto:offline') return 'models on this computer';
  if (m === 'auto:online-offline') return 'the best model for each question, online or on this computer';
  if (m === 'auto:my-hardware') return 'the models this computer runs well';
  if (m.startsWith('online:')) {
    const id = m.slice(7);
    return `${opts?.onlineNames?.[m] || opts?.onlineNames?.[id] || humanizeId(id)}, online`;
  }
  if (m.startsWith('external:')) return 'a model on your own server';
  return `${richModelName(m)} on this computer`;
}

export function describePersonality(archetypeId: string | undefined): string {
  const a = archetypeId ? getArchetypeById(archetypeId) : undefined;
  if (!a?.name) return 'an AI with a personality of its own';
  const n = a.name.toLowerCase();
  return `${article(n)} ${n}`;
}

function modeSentence(mode: string | undefined): string {
  if (mode === 'report') return 'Every reply is a structured report.';
  if (mode === 'code') return 'Every reply leads with code.';
  return '';
}

function toolsPhrase(ai: DescribableAi): { bare: string; sentence: string } {
  const t = (ai.mcp || []).length;
  const s = (ai.skills || []).length;
  const parts: string[] = [];
  if (t) parts.push(`${t} ${t === 1 ? 'tool' : 'tools'}`);
  if (s) parts.push(`${s} ${s === 1 ? 'skill' : 'skills'}`);
  if (!parts.length) return { bare: 'no tools or skills', sentence: '' };
  const bare = parts.join(' and ');
  return { bare, sentence: `Carries ${bare}.` };
}

/** The old generated sentence ("X is my custom AI with the personality of a ...") - treated as "not customized". */
export function isLegacyDefaultDescription(text: string | undefined): boolean {
  return /^.+ is my custom AI with the personality of an? .+\.$/.test((text || '').trim());
}

/** The template to edit: the stored one, or the default when empty or legacy. */
export function descriptionTemplateFor(ai: DescribableAi): string {
  const d = (ai.description || '').trim();
  if (!d || isLegacyDefaultDescription(d)) return DEFAULT_DESCRIPTION_TEMPLATE;
  return d;
}

/** Fill the placeholders from the AI's live settings; tidy the spacing. */
export function renderAiDescription(ai: DescribableAi, opts?: DescribeOptions): string {
  const tools = toolsPhrase(ai);
  const values: Record<string, string> = {
    name: ai.name || 'This AI',
    personality: describePersonality(ai.baseArchetypeId),
    models: describeModels(ai.model, opts),
    mode: ai.defaultMode || 'chat',
    'mode.sentence': modeSentence(ai.defaultMode),
    tools: tools.bare,
    'tools.sentence': tools.sentence,
  };
  const out = descriptionTemplateFor(ai).replace(/\{([a-z]+(?:\.[a-z]+)?)\}/g, (whole, key: string) =>
    key in values ? values[key] : whole,
  );
  return out.replace(/[ \t]+/g, ' ').replace(/ +([.,;:!?])/g, '$1').replace(/\s+\n/g, '\n').trim();
}
