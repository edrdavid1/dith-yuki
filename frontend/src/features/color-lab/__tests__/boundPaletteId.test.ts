import { describe, expect, it } from 'vitest';
import { boundPaletteIdFromFilter } from '../boundPaletteId';

describe('boundPaletteIdFromFilter', () => {
  it('returns undefined when params have no palette_id', () => {
    expect(boundPaletteIdFromFilter({ params: { type: 'Curves', curve: [] } })).toBeUndefined();
    expect(boundPaletteIdFromFilter(null)).toBeUndefined();
    expect(boundPaletteIdFromFilter(undefined)).toBeUndefined();
  });

  it('returns the numeric palette_id', () => {
    expect(
      boundPaletteIdFromFilter({ params: { type: 'DitherV2', palette_id: 7 } })
    ).toBe(7);
    expect(
      boundPaletteIdFromFilter({ params: { type: 'PaletteQuantize', palette_id: 3 } })
    ).toBe(3);
  });

  it('returns null for Tone Steps (explicit null palette_id)', () => {
    expect(
      boundPaletteIdFromFilter({ params: { type: 'DitherV2', palette_id: null } })
    ).toBeNull();
  });
});
