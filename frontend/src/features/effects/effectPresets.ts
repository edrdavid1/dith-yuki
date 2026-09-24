/**
 * Named effect presets — parameter combinations of existing algorithms.
 *
 * Batch F of ALGORITHM_BATCH_ADDITION_spec.md: not AlgorithmIds / FilterAlgorithm
 * impls. Keep this list data-driven; do not register these in ALGORITHM_ID_REGISTRY.
 */

import { EFFECT_DEFAULTS, type AlgorithmAddSpec } from '../../types/effects';

export interface EffectPreset {
  id: string;
  label: string;
  /** Short hint for the chooser row. */
  hint: string;
  buildSpec: (paletteId: number | null) => AlgorithmAddSpec;
}

/**
 * Cross-stitch look: coarse Bayer cells + optional bound palette (floss colours).
 * Uses existing `bayer_8x8` + `pixel_size` — no new dither kernel.
 */
export const CROSS_STITCH_PRESET: EffectPreset = {
  id: 'cross_stitch_pattern',
  label: 'Cross Stitch',
  hint: 'Coarse Bayer blocks · optional palette',
  buildSpec: (paletteId) => ({
    kind: 'DitherV2',
    params: {
      ...EFFECT_DEFAULTS.Dithering,
      mode: 'bayer_8x8',
      levels: 2,
      pixel_size: 8,
      color_mode: 'rgb',
      palette_id: paletteId,
      palette_dither_mode: 'strict',
      threshold_scale: 1.0,
      serpentine: false,
      dither_alpha: true,
    },
  }),
};

/** All UI-facing presets (Batch F and later). */
export const EFFECT_PRESETS: readonly EffectPreset[] = [CROSS_STITCH_PRESET];

export function findEffectPreset(id: string): EffectPreset | undefined {
  return EFFECT_PRESETS.find((p) => p.id === id);
}
