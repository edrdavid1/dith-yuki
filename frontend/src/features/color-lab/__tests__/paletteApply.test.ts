import { describe, it, expect } from 'vitest';
import { shouldReplaceSelectedPalette } from '../paletteApply';

describe('shouldReplaceSelectedPalette', () => {
  const palettes = [{ id: 7 }, { id: 9 }];

  it('replaces when extract selected an existing palette', () => {
    expect(shouldReplaceSelectedPalette(7, palettes)).toBe(true);
  });

  it('adds when nothing is selected', () => {
    expect(shouldReplaceSelectedPalette(null, palettes)).toBe(false);
  });

  it('adds when the selection is not in the document list', () => {
    expect(shouldReplaceSelectedPalette(3, palettes)).toBe(false);
  });
});
