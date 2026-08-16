import { describe, it, expect } from 'vitest';
import { shouldReplaceSelectedPalette, shouldLiveReplacePalette } from '../paletteApply';

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

describe('shouldLiveReplacePalette', () => {
  const palettes = [
    {
      id: 7,
      name: 'PICO-8',
      colors: [
        [0, 0, 0],
        [255, 0, 77],
      ] as const,
    },
  ];
  const matching = [
    { valid: true, r: 0, g: 0, b: 0 },
    { valid: true, r: 255, g: 0, b: 77 },
  ];

  it('skips when draft already matches the bound palette', () => {
    expect(shouldLiveReplacePalette(7, palettes, 'PICO-8', matching)).toBe(false);
  });

  it('pushes when a draft color changed', () => {
    const edited = [
      { valid: true, r: 0, g: 0, b: 0 },
      { valid: true, r: 10, g: 20, b: 30 },
    ];
    expect(shouldLiveReplacePalette(7, palettes, 'PICO-8', edited)).toBe(true);
  });

  it('skips while a hex is incomplete', () => {
    const invalid = [
      { valid: true, r: 0, g: 0, b: 0 },
      { valid: false, r: 0, g: 0, b: 0 },
    ];
    expect(shouldLiveReplacePalette(7, palettes, 'PICO-8', invalid)).toBe(false);
  });

  it('skips when no document palette is selected', () => {
    expect(shouldLiveReplacePalette(null, palettes, 'PICO-8', matching)).toBe(false);
  });
});
