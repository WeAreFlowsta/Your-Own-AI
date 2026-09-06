/**
 * Default AI presets for the welcome wizard.
 *
 * Every install gets three AIs. The wizard lets a new user pick the kind
 * of user they are - Personal keeps the three characters the app seeds;
 * Work renames the same three AIs into an Assistant, a Coder and an
 * Analyst, all on the Neutral personality with gradient thumbnails. Each
 * slot maps onto one seeded AI in seed order (veebo, teresa, reeves), so a
 * preset is an edit of the AIs that already exist, never a fourth AI.
 */

export type DefaultAiPreset = 'personal' | 'work';

export interface DefaultAiSlot {
  name: string;
  description: string;
  /** Bundled archetype id - carries the personality (system prompt). */
  archetypeId: string;
  /** Bundled image path saved as the AI's thumbnail. */
  thumbnail: string;
}

export const DEFAULT_AI_PRESETS: Record<DefaultAiPreset, DefaultAiSlot[]> = {
  personal: [
    { name: 'Veebo', description: '{name} is an AI with a {personality} personality - playful, curious and a little unpredictable. Good company for ideas and everyday questions. Answers come from {models}. {mode.sentence} {tools.sentence}', archetypeId: 'veebo', thumbnail: '/bundled/veebo.jpg' },
    { name: 'Teresa', description: '{name} is an AI with a {personality} personality - warm and supportive. Talk through feelings, plans and the things on your mind. Answers come from {models}. {mode.sentence} {tools.sentence}', archetypeId: 'teresa', thumbnail: '/bundled/teresa.jpg' },
    { name: 'Reeves', description: '{name} is an AI with a {personality} personality - straight answers with nothing in the way. Facts, explanations, quick help. Answers come from {models}. {mode.sentence} {tools.sentence}', archetypeId: 'reeves', thumbnail: '/bundled/reeves.jpg' },
  ],
  work: [
    { name: 'Assistant', description: '{name} is an AI with a {personality} personality - writing, planning, email drafts and everyday questions at work. Answers come from {models}. {mode.sentence} {tools.sentence}', archetypeId: 'reeves', thumbnail: '/bundled/gradient-ocean.jpg' },
    { name: 'Coder', description: '{name} is an AI with a {personality} personality - code, debugging and technical explanations in any language. Answers come from {models}. {mode.sentence} {tools.sentence}', archetypeId: 'reeves', thumbnail: '/bundled/gradient-midnight.jpg' },
    { name: 'Analyst', description: '{name} is an AI with a {personality} personality - data, summaries and clear-eyed analysis of documents and numbers. Answers come from {models}. {mode.sentence} {tools.sentence}', archetypeId: 'reeves', thumbnail: '/bundled/gradient-meadow.jpg' },
  ],
};

export const DEFAULT_AI_PRESET_LABELS: Record<DefaultAiPreset, { title: string; blurb: string }> = {
  personal: { title: 'Personal', blurb: 'Three characters with their own voices, for life outside work.' },
  work: { title: 'Work', blurb: 'An assistant, a coder and an analyst, all straight to the point.' },
};

/** The personalities the wizard offers on the Change tile - a short, clear
 *  set. The full set of eighteen lives on the Your AIs page. */
export const WIZARD_PERSONALITY_IDS = [
  'reeves',                 // Neutral
  'teresa',                 // Caregiver
  'veebo',                  // Quirky
  'mQuddS8PXJRTCr7XE77U',   // Sage
  'LlBsimMQAlkbLdS2qurw',   // Explorer
  '40antA9yedUCfnQ3TRvc',   // Creator
];
