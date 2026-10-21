import { describe, expect, it } from 'vitest';
import { checkStaleGenAtQuiet, feSnapshotFromTiles } from '../viewportTraceChecker';

describe('STALE_GEN_AT_QUIET', () => {
  it('flags visible tile below session maxKnownGen', () => {
    const fe = feSnapshotFromTiles(
      0,
      [
        { level: 0, x: 0, y: 0 },
        { level: 0, x: 1, y: 0 },
      ],
      12,
      {
        tileGens: { '0/0/0': 41, '0/1/0': 42 },
        maxKnownGen: 42,
      },
    );
    const v = checkStaleGenAtQuiet(fe);
    expect(v).toHaveLength(1);
    expect(v[0]?.class).toBe('STALE_GEN_AT_QUIET');
    expect(v[0]?.tileKey).toBe('0/0/0');
  });

  it('passes when all visible gens match maxKnown', () => {
    const fe = feSnapshotFromTiles(0, [{ level: 0, x: 0, y: 0 }], 12, {
      tileGens: { '0/0/0': 42 },
      maxKnownGen: 42,
    });
    expect(checkStaleGenAtQuiet(fe)).toEqual([]);
  });
});
