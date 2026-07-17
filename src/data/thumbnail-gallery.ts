/**
 * Bundled thumbnail gallery.
 *
 * Every entry points at an image under public/bundled/. Several archetypes
 * share one image (e.g. Caregiver art is also Teresa's personality default),
 * so the people group lists unique images, not archetypes.
 *
 * Groups are audience-neutral: Colors and Gradients lead (safe for anyone,
 * generated flats/radials), then People (photoreal portraits, including the
 * default AIs' faces), then Characters (creatures and objects, including
 * Veebo). There is deliberately NO "Default AIs" group - those three images
 * live in the group their art belongs to.
 *
 * Picking a gallery thumbnail saves it to disk via save_ai_thumbnail — the
 * same mechanism as a custom upload — so it sticks when the personality
 * changes. Only "match personality" mode follows archetype switches.
 */

export type GalleryGroup = 'colors' | 'gradients' | 'people' | 'characters';

export interface GalleryThumb {
  id: string;
  name: string;
  path: string;
  group: GalleryGroup;
}

export const THUMBNAIL_GALLERY: GalleryThumb[] = [
  // Colors — generated 512px flats
  { id: 'color-graphite', name: 'Graphite', path: '/bundled/color-graphite.jpg', group: 'colors' },
  { id: 'color-slate', name: 'Slate', path: '/bundled/color-slate.jpg', group: 'colors' },
  { id: 'color-navy', name: 'Navy', path: '/bundled/color-navy.jpg', group: 'colors' },
  { id: 'color-blue', name: 'Blue', path: '/bundled/color-blue.jpg', group: 'colors' },
  { id: 'color-teal', name: 'Teal', path: '/bundled/color-teal.jpg', group: 'colors' },
  { id: 'color-forest', name: 'Forest', path: '/bundled/color-forest.jpg', group: 'colors' },
  { id: 'color-amber', name: 'Amber', path: '/bundled/color-amber.jpg', group: 'colors' },
  { id: 'color-terracotta', name: 'Terracotta', path: '/bundled/color-terracotta.jpg', group: 'colors' },
  { id: 'color-burgundy', name: 'Burgundy', path: '/bundled/color-burgundy.jpg', group: 'colors' },
  { id: 'color-plum', name: 'Plum', path: '/bundled/color-plum.jpg', group: 'colors' },

  // Gradients — radial, one color in the middle fading to another outside
  { id: 'gradient-ocean', name: 'Ocean', path: '/bundled/gradient-ocean.jpg', group: 'gradients' },
  { id: 'gradient-lagoon', name: 'Lagoon', path: '/bundled/gradient-lagoon.jpg', group: 'gradients' },
  { id: 'gradient-meadow', name: 'Meadow', path: '/bundled/gradient-meadow.jpg', group: 'gradients' },
  { id: 'gradient-sunrise', name: 'Sunrise', path: '/bundled/gradient-sunrise.jpg', group: 'gradients' },
  { id: 'gradient-sunset', name: 'Sunset', path: '/bundled/gradient-sunset.jpg', group: 'gradients' },
  { id: 'gradient-orchid', name: 'Orchid', path: '/bundled/gradient-orchid.jpg', group: 'gradients' },
  { id: 'gradient-midnight', name: 'Midnight', path: '/bundled/gradient-midnight.jpg', group: 'gradients' },
  { id: 'gradient-silver', name: 'Silver', path: '/bundled/gradient-silver.jpg', group: 'gradients' },
  { id: 'gradient-gold', name: 'Gold', path: '/bundled/gradient-gold.jpg', group: 'gradients' },
  { id: 'gradient-rose', name: 'Rose', path: '/bundled/gradient-rose.jpg', group: 'gradients' },

  // People (unique archetype art + the default AIs' portraits)
  { id: 'everyday', name: 'Everyday Person', path: '/bundled/FlT8w1l8DeGjit9g3vcD.jpg', group: 'people' },
  { id: 'caregiver', name: 'Caregiver', path: '/bundled/mhJ4WU8bn34In1lJUuHH.jpg', group: 'people' },
  { id: 'quirky', name: 'Quirky', path: '/bundled/a7ov7JfOR6rOW3SPKJHt.jpg', group: 'people' },
  { id: 'joker', name: 'Joker', path: '/bundled/2ZFcK040ISYBVbO2DBux.jpg', group: 'people' },
  { id: 'creator', name: 'Creator', path: '/bundled/40antA9yedUCfnQ3TRvc.jpg', group: 'people' },
  { id: 'wizard', name: 'Wizard', path: '/bundled/4vDr8qmETSRqBIyjdbRE.jpg', group: 'people' },
  { id: 'explorer', name: 'Explorer', path: '/bundled/LlBsimMQAlkbLdS2qurw.jpg', group: 'people' },
  { id: 'sage', name: 'Sage', path: '/bundled/mQuddS8PXJRTCr7XE77U.jpg', group: 'people' },
  { id: 'rebel', name: 'Rebel', path: '/bundled/oxS4yyX7EfFzm0TyqFSC.jpg', group: 'people' },
  { id: 'lover', name: 'Lover', path: '/bundled/ps7KvwieRk9Mar8AljWN.jpg', group: 'people' },
  { id: 'artist', name: 'Artist', path: '/bundled/syIqu1aP1UuQACtLCZUK.jpg', group: 'people' },
  { id: 'neutral', name: 'Neutral', path: '/bundled/teJ8K9PHyejIYmdZj8wk.jpg', group: 'people' },
  { id: 'hero', name: 'Hero', path: '/bundled/UAf529RyG1TtqSfjQpZw.jpg', group: 'people' },
  { id: 'innocent', name: 'Innocent', path: '/bundled/VUu6lu2tR6BS1Kvr68OQ.jpg', group: 'people' },
  { id: 'leader', name: 'Leader', path: '/bundled/xgVlq8deiKzBHYn3bw5P.jpg', group: 'people' },
  { id: 'reeves', name: 'Reeves', path: '/bundled/reeves.jpg', group: 'people' },
  { id: 'teresa', name: 'Teresa', path: '/bundled/teresa.jpg', group: 'people' },

  // Characters (creatures and objects + Veebo, the app's robot)
  { id: 'veebo', name: 'Veebo', path: '/bundled/veebo.jpg', group: 'characters' },
  { id: 'light-wisp', name: 'Light Wisp', path: '/bundled/light-wisp.jpg', group: 'characters' },
  { id: 'owl', name: 'Owl', path: '/bundled/owl.jpg', group: 'characters' },
  { id: 'crystal', name: 'Crystal', path: '/bundled/crystal.jpg', group: 'characters' },
  { id: 'chess-knight', name: 'Chess Knight', path: '/bundled/chess-knight.jpg', group: 'characters' },
  { id: 'cosmic-being', name: 'Cosmic Being', path: '/bundled/cosmic-being.jpg', group: 'characters' },
  { id: 'liquid-chrome', name: 'Liquid Chrome', path: '/bundled/liquid-chrome.jpg', group: 'characters' },
  { id: 'water-being', name: 'Water Being', path: '/bundled/water-being.jpg', group: 'characters' },
  { id: 'golden-retriever', name: 'Golden Retriever', path: '/bundled/golden-retriever.jpg', group: 'characters' },
  { id: 'tortoise', name: 'Tortoise', path: '/bundled/tortoise.jpg', group: 'characters' },
  { id: 'tux-cat', name: 'Tuxedo Cat', path: '/bundled/tux-cat.jpg', group: 'characters' },
];

export const GALLERY_GROUP_LABELS: Record<GalleryGroup, string> = {
  colors: 'Colors',
  gradients: 'Gradients',
  people: 'People',
  characters: 'Characters',
};
