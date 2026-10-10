import { describe, expect, it } from 'vitest';
import {
  checkSeamless,
  patternAngleBreaksPeriodicity,
  seamlessWarningLines,
} from '../seamless';

describe('seamless', () => {
  it('flags non-multiple sizes for Bayer 8×8 with pixel_size 2', () => {
    const r = checkSeamless({
      mode: 'bayer_8x8',
      pixelSize: 2,
      patternAngle: 0,
      width: 1024,
      height: 1000,
    });
    expect(r.periodX).toBe(16);
    expect(r.widthOk).toBe(true);
    expect(r.heightOk).toBe(false);
    expect(r.nearestHeight).toEqual([992, 1008]);
    expect(seamlessWarningLines(r, 1024, 1000).length).toBeGreaterThan(0);
  });

  it('treats 0/90 angles as axis-periodic', () => {
    expect(patternAngleBreaksPeriodicity(0)).toBe(false);
    expect(patternAngleBreaksPeriodicity(90)).toBe(false);
    expect(patternAngleBreaksPeriodicity(45)).toBe(true);
  });

  it('marks error diffusion as approximate-only', () => {
    const r = checkSeamless({
      mode: 'floyd_steinberg',
      pixelSize: 1,
      patternAngle: 0,
      width: 64,
      height: 64,
    });
    expect(r.kind).toBe('approximate_only');
  });
});
