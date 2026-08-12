import { describe, expect, it } from 'vitest';
import { computeInsertIndex } from '../panelDockGeometry';

describe('computeInsertIndex', () => {
  it('returns 0 for empty slots', () => {
    expect(computeInsertIndex([], 100)).toBe(0);
  });

  it('returns 0 above a single slot midpoint', () => {
    expect(computeInsertIndex([50], 10)).toBe(0);
  });

  it('returns 1 at or below a single slot midpoint', () => {
    expect(computeInsertIndex([50], 50)).toBe(1);
    expect(computeInsertIndex([50], 80)).toBe(1);
  });

  it('inserts between two slots', () => {
    const mids = [40, 120];
    expect(computeInsertIndex(mids, 10)).toBe(0);
    expect(computeInsertIndex(mids, 40)).toBe(1);
    expect(computeInsertIndex(mids, 80)).toBe(1);
    expect(computeInsertIndex(mids, 120)).toBe(2);
    expect(computeInsertIndex(mids, 200)).toBe(2);
  });

  it('handles three slots at edges', () => {
    const mids = [20, 60, 100];
    expect(computeInsertIndex(mids, Number.NEGATIVE_INFINITY)).toBe(0);
    expect(computeInsertIndex(mids, Number.POSITIVE_INFINITY)).toBe(3);
  });
});
