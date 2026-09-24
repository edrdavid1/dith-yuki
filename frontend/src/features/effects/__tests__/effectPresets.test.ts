import { describe, it, expect } from 'vitest';
import {
  CROSS_STITCH_PRESET,
  EFFECT_PRESETS,
  findEffectPreset,
} from '../effectPresets';

describe('effectPresets (Batch F)', () => {
  it('lists cross_stitch_pattern and does not invent an AlgorithmId', () => {
    expect(EFFECT_PRESETS.map((p) => p.id)).toContain('cross_stitch_pattern');
    expect(findEffectPreset('cross_stitch_pattern')).toBe(CROSS_STITCH_PRESET);
  });

  it('builds a DitherV2 bayer_8x8 stack with coarse pixel_size', () => {
    const spec = CROSS_STITCH_PRESET.buildSpec(42);
    expect(spec.kind).toBe('DitherV2');
    expect(spec.params).toMatchObject({
      mode: 'bayer_8x8',
      pixel_size: 8,
      levels: 2,
      palette_id: 42,
      palette_dither_mode: 'strict',
    });
  });

  it('allows null palette (RGB without Color Lab binding)', () => {
    const spec = CROSS_STITCH_PRESET.buildSpec(null);
    expect(spec.params.palette_id).toBeNull();
  });
});
