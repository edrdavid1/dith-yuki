import { describe, expect, it } from 'vitest';
import {
  MAX_MARGIN_PX,
  MAX_PADDED_PIXELS,
  WRAP_PADDING_HINT,
  wrapPaddingFitsLimits,
} from '../wrapPadding';

describe('wrapPadding', () => {
  it('hint_text_matches_adr', () => {
    expect(WRAP_PADDING_HINT).toBe(
      'Approximate wrap-padding reduces seams for tiled export by padding the dithering buffer with wrapped edges. Error diffusion is sequential, so the result is approximate, not exact.',
    );
  });

  it('checkbox_disabled_when_oversize', () => {
    expect(wrapPaddingFitsLimits(64, 64, 1)).toBe(true);
    expect(wrapPaddingFitsLimits(20_000, 20_000, 50)).toBe(false);
    expect(MAX_PADDED_PIXELS).toBe(200_000_000);
    expect(MAX_MARGIN_PX).toBe(512);
  });

  it('rejects non-positive dims', () => {
    expect(wrapPaddingFitsLimits(0, 64, 1)).toBe(false);
    expect(wrapPaddingFitsLimits(64, -1, 1)).toBe(false);
  });
});
