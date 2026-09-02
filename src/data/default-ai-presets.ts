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
    { name: 'Veebo', description: 'Playful, curious and a little unpredictable. Good company for ideas and everyday questions.', archetypeId: 'veebo', thumbnail: '/bundled/veebo.jpg' },
    { name: 'Teresa', description: 'Warm and supportive. Talk through feelings, plans and the things on your mind.', archetypeId: 'teresa', thumbnail: '/bundled/teresa.jpg' },
    { name: 'Reeves', description: 'Straight answers with no personality in the way. Facts, explanations, quick help.', archetypeId: 'reeves', thumbnail: '/bundled/reeves.jpg' },
  ],
  work: [
    { name: 'Assistant', description: 'Writing, planning, email drafts and everyday questions at work.', archetypeId: 'reeves', thumbnail: '/bundled/gradient-ocean.jpg' },
    { name: 'Coder', description: 'Code, debugging and technical explanations in any language.', archetypeId: 'reeves', thumbnail: '/bundled/gradient-midnight.jpg' },
    { name: 'Analyst', description: 'Data, summaries and clear-eyed analysis of documents and numbers.', archetypeId: 'reeves', thumbnail: '/bundled/gradient-meadow.jpg' },
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
