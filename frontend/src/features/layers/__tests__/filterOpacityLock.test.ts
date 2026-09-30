import { describe, it, expect } from 'vitest';
import { ditherFilterLocksOpacity } from '../filterOpacityLock';
import type { FilterInfo } from '../../types';

function ditherFilter(
  overrides: Partial<FilterInfo> & { params?: Record<string, unknown> } = {}
): FilterInfo {
  const { params: paramOverrides, ...rest } = overrides;
  return {
    id: 'f1',
    kind: 'DitherV2',
    enabled: true,
    opacity: 0.5,
    blend_mode: 'Normal',
    params: {
      type: 'DitherV2',
      mode: 'bayer_4x4',
      levels: 4,
      threshold_scale: 1,
      pixel_size: 1,
      color_mode: 'rgb',
      palette_id: 1,
      palette_dither_mode: 'strict',
      ...paramOverrides,
    } as FilterInfo['params'],
    ...rest,
  };
}

describe('ditherFilterLocksOpacity', () => {
  it('locks Strict with a bound palette', () => {
    expect(ditherFilterLocksOpacity(ditherFilter())).toBe(true);
  });

  it('locks Mixed with a bound palette', () => {
    expect(
      ditherFilterLocksOpacity(
        ditherFilter({
          params: { palette_dither_mode: { mixed: { channel_levels: 4 } } },
        })
      )
    ).toBe(true);
  });

  it('allows Guided Fade', () => {
    expect(
      ditherFilterLocksOpacity(
        ditherFilter({
          params: { palette_dither_mode: { guided: { channel_levels: 4 } } },
        })
      )
    ).toBe(false);
  });

  it('allows Simple Fade', () => {
    expect(
      ditherFilterLocksOpacity(
        ditherFilter({ params: { palette_dither_mode: 'simple' } })
      )
    ).toBe(false);
  });

  it('allows paletteless dither regardless of mode field', () => {
    expect(
      ditherFilterLocksOpacity(
        ditherFilter({ params: { palette_id: null, palette_dither_mode: 'strict' } })
      )
    ).toBe(false);
  });

  it('ignores non-dither filters', () => {
    expect(
      ditherFilterLocksOpacity({
        id: 'g',
        kind: 'Glow',
        enabled: true,
        opacity: 0.5,
        blend_mode: 'Normal',
        params: { type: 'Glow', radius: 1, intensity: 1, threshold: 0 } as FilterInfo['params'],
      })
    ).toBe(false);
  });
});
