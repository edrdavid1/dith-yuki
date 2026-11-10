import { describe, it, expect } from 'vitest';
import {
  buildBrowsablePaletteList,
  buildPaletteList,
  browseIndexForSelection,
  currentPaletteKey,
  stepBrowseIndex,
} from '../paletteListOrder';
import type { BuiltinPaletteDto, PaletteDto } from '../../../shared/ipc';

const builtins: BuiltinPaletteDto[] = [
  { id: 'a', name: 'A', colors: [[1, 2, 3]], color_count: 1 },
  { id: 'b', name: 'B', colors: [[4, 5, 6]], color_count: 1 },
];

const saved: PaletteDto[] = [
  {
    id: 10,
    name: 'Saved',
    colors: [[7, 8, 9]],
    hex_colors: ['#070809'],
    color_count: 1,
  },
];

describe('paletteListOrder', () => {
  it('builds Raw tone → New → builtins → saved', () => {
    const list = buildPaletteList(builtins, saved);
    expect(list.map((e) => e.key)).toEqual([
      'tone-steps',
      'new',
      'builtin:a',
      'builtin:b',
      'saved:10',
    ]);
    expect(list[0]).toMatchObject({ kind: 'tone-steps', name: 'Raw tone' });
  });

  it('browse list keeps Raw tone and skips New', () => {
    const list = buildBrowsablePaletteList(builtins, saved);
    expect(list.map((e) => e.key)).toEqual([
      'tone-steps',
      'builtin:a',
      'builtin:b',
      'saved:10',
    ]);
  });

  it('wraps browse indices', () => {
    expect(stepBrowseIndex(3, 0, 1)).toBe(1);
    expect(stepBrowseIndex(3, 2, 1)).toBe(0);
    expect(stepBrowseIndex(3, 0, -1)).toBe(2);
  });

  it('keeps sticky preferred browse index', () => {
    const list = buildBrowsablePaletteList(builtins, saved);
    expect(browseIndexForSelection(list, 10, saved, 1)).toBe(1);
  });

  it('maps selection to the dropdown key', () => {
    expect(currentPaletteKey(null)).toBe('tone-steps');
    expect(currentPaletteKey(null, 'new')).toBe('new');
    expect(currentPaletteKey(10)).toBe('saved:10');
    expect(currentPaletteKey(null, 'tone-steps', 10)).toBe('saved:10');
  });

  it('browse index lands on Raw tone when unbound', () => {
    const list = buildBrowsablePaletteList(builtins, saved);
    expect(browseIndexForSelection(list, null, saved, null)).toBe(0);
    expect(list[0]?.kind).toBe('tone-steps');
  });
});
