import { describe, it, expect } from 'vitest';
import { ALGORITHM_FAMILIES, selectionFromMode } from './algorithmFamilies';

describe('algorithmFamilies', () => {
  it('families_have_unique_ids', () => {
    const ids = ALGORITHM_FAMILIES.map((family) => family.id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it('variants_unique_within_family', () => {
    for (const family of ALGORITHM_FAMILIES) {
      const ids = family.variants.map((variant) => variant.id);
      expect(new Set(ids).size).toBe(ids.length);
    }
  });

  it('default_variant_in_variants', () => {
    for (const family of ALGORITHM_FAMILIES) {
      expect(family.variants.some((variant) => variant.id === family.defaultVariant)).toBe(
        true,
      );
    }
  });

  it('all_families_have_tradeoff_line', () => {
    for (const family of ALGORITHM_FAMILIES) {
      expect(family.tradeoff.trim().length).toBeGreaterThan(0);
    }
  });

  it('selectionFromMode maps known modes to their family', () => {
    expect(selectionFromMode('floyd_steinberg')).toEqual({
      family: 'cpu_ed',
      variant: 'floyd_steinberg',
    });
    expect(selectionFromMode('bayer_4x4')).toEqual({
      family: 'ordered',
      variant: 'bayer_4x4',
    });
    expect(selectionFromMode('jarvis_judice_ninke')).toEqual({
      family: 'cpu_ed',
      variant: 'jarvis_judice_ninke',
    });
  });
});
