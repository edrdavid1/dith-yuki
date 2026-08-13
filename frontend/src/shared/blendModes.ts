/** Blend modes exposed in layer and per-filter UI (reserved slots omitted). */
export const BLEND_MODES = [
  'Normal',
  'Multiply',
  'Screen',
  'Overlay',
  'Darken',
  'Lighten',
  'ColorDodge',
  'ColorBurn',
  'HardLight',
  'SoftLight',
  'Difference',
  'Exclusion',
] as const;

export type BlendModeName = (typeof BLEND_MODES)[number];
