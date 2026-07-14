/**
 * Bundled thumbnail gallery.
 *
 * Every entry points at an image under public/bundled/. Several archetypes
 * share one image (e.g. Caregiver art is also Teresa's personality default),
 * so the personalities group lists unique images, not archetypes.
 *
 * Picking a gallery thumbnail saves it to disk via save_ai_thumbnail — the
 * same mechanism as a custom upload — so it sticks when the personality
 * changes. Only "match personality" mode follows archetype switches.
 */

export type GalleryGroup = 'defaults' | 'personalities' | 'beings';

export interface GalleryThumb {
  id: string;
  name: string;
  path: string;
  group: GalleryGroup;
}

export const THUMBNAIL_GALLERY: GalleryThumb[] = [
  // Default AIs
  { id: 'veebo', name: 'Veebo', path: '/bundled/veebo.jpg', group: 'defaults' },
  { id: 'reeves', name: 'Reeves', path: '/bundled/reeves.jpg', group: 'defaults' },
  { id: 'teresa', name: 'Teresa', path: '/bundled/teresa.jpg', group: 'defaults' },

  // Personalities (unique archetype art)
  { id: 'everyday', name: 'Everyday Person', path: '/bundled/FlT8w1l8DeGjit9g3vcD.jpg', group: 'personalities' },
  { id: 'caregiver', name: 'Caregiver', path: '/bundled/mhJ4WU8bn34In1lJUuHH.jpg', group: 'personalities' },
  { id: 'quirky', name: 'Quirky', path: '/bundled/a7ov7JfOR6rOW3SPKJHt.jpg', group: 'personalities' },
  { id: 'joker', name: 'Joker', path: '/bundled/2ZFcK040ISYBVbO2DBux.jpg', group: 'personalities' },
  { id: 'creator', name: 'Creator', path: '/bundled/40antA9yedUCfnQ3TRvc.jpg', group: 'personalities' },
  { id: 'wizard', name: 'Wizard', path: '/bundled/4vDr8qmETSRqBIyjdbRE.jpg', group: 'personalities' },
  { id: 'explorer', name: 'Explorer', path: '/bundled/LlBsimMQAlkbLdS2qurw.jpg', group: 'personalities' },
  { id: 'sage', name: 'Sage', path: '/bundled/mQuddS8PXJRTCr7XE77U.jpg', group: 'personalities' },
  { id: 'rebel', name: 'Rebel', path: '/bundled/oxS4yyX7EfFzm0TyqFSC.jpg', group: 'personalities' },
  { id: 'lover', name: 'Lover', path: '/bundled/ps7KvwieRk9Mar8AljWN.jpg', group: 'personalities' },
  { id: 'artist', name: 'Artist', path: '/bundled/syIqu1aP1UuQACtLCZUK.jpg', group: 'personalities' },
  { id: 'neutral', name: 'Neutral', path: '/bundled/teJ8K9PHyejIYmdZj8wk.jpg', group: 'personalities' },
  { id: 'hero', name: 'Hero', path: '/bundled/UAf529RyG1TtqSfjQpZw.jpg', group: 'personalities' },
  { id: 'innocent', name: 'Innocent', path: '/bundled/VUu6lu2tR6BS1Kvr68OQ.jpg', group: 'personalities' },
  { id: 'leader', name: 'Leader', path: '/bundled/xgVlq8deiKzBHYn3bw5P.jpg', group: 'personalities' },

  // Beings
  { id: 'light-wisp', name: 'Light Wisp', path: '/bundled/light-wisp.jpg', group: 'beings' },
  { id: 'owl', name: 'Owl', path: '/bundled/owl.jpg', group: 'beings' },
  { id: 'crystal', name: 'Crystal', path: '/bundled/crystal.jpg', group: 'beings' },
  { id: 'chess-knight', name: 'Chess Knight', path: '/bundled/chess-knight.jpg', group: 'beings' },
  { id: 'cosmic-being', name: 'Cosmic Being', path: '/bundled/cosmic-being.jpg', group: 'beings' },
  { id: 'liquid-chrome', name: 'Liquid Chrome', path: '/bundled/liquid-chrome.jpg', group: 'beings' },
  { id: 'water-being', name: 'Water Being', path: '/bundled/water-being.jpg', group: 'beings' },
  { id: 'golden-retriever', name: 'Golden Retriever', path: '/bundled/golden-retriever.jpg', group: 'beings' },
  { id: 'tortoise', name: 'Tortoise', path: '/bundled/tortoise.jpg', group: 'beings' },
  { id: 'tux-cat', name: 'Tuxedo Cat', path: '/bundled/tux-cat.jpg', group: 'beings' },
];

export const GALLERY_GROUP_LABELS: Record<GalleryGroup, string> = {
  defaults: 'Default AIs',
  personalities: 'Personalities',
  beings: 'Beings',
};
