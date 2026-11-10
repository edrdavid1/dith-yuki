import { describe, it, expect } from 'vitest';
import {
  RETRO_SLIDER_END_INSET_PX,
  RETRO_SLIDER_THUMB_WIDTH_PX,
  sliderEndPadRatio,
  sliderRatioFromClientX,
  sliderThumbLeftCss,
  sliderValueRatioFromVisual,
  sliderVisualRatio,
} from '../retroSliderGeometry';

describe('retroSliderGeometry', () => {
  it('insets min/max by half the carriage (Pixel Size @ 2 look)', () => {
    expect(RETRO_SLIDER_END_INSET_PX).toBe(RETRO_SLIDER_THUMB_WIDTH_PX / 2);
    const track = 248; // ≈ 1/31 of track ≈ one Pixel Size step on 1…32
    const pad = sliderEndPadRatio(track);
    expect(pad).toBeCloseTo(RETRO_SLIDER_END_INSET_PX / track);
    // Same visual as unpadded ratio for Pixel Size value 2: (2-1)/(32-1) = 1/31
    expect(sliderVisualRatio(0, pad)).toBeCloseTo(1 / 31, 1);
    expect(sliderVisualRatio(1, pad)).toBeCloseTo(1 - 1 / 31, 1);
  });

  it('keeps pad identical for any value domain (only track width matters)', () => {
    expect(sliderEndPadRatio(160)).toBeGreaterThan(sliderEndPadRatio(320));
  });

  it('inverts visual mapping for pointer hits', () => {
    const pad = 0.1;
    expect(sliderValueRatioFromVisual(pad, pad)).toBeCloseTo(0);
    expect(sliderValueRatioFromVisual(1 - pad, pad)).toBeCloseTo(1);
    expect(sliderValueRatioFromVisual(0.5, pad)).toBeCloseTo(0.5);
  });

  it('emits CSS percents with padding', () => {
    expect(sliderThumbLeftCss(0, 0.1)).toBe('10%');
    expect(sliderThumbLeftCss(1, 0.1)).toBe('90%');
  });

  it('maps track ends through the pad to 0 and 1', () => {
    const pad = 0.1;
    expect(sliderRatioFromClientX(20, 0, 200, pad)).toBeCloseTo(0);
    expect(sliderRatioFromClientX(180, 0, 200, pad)).toBeCloseTo(1);
  });
});
