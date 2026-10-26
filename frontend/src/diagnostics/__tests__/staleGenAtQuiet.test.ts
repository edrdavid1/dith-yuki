import { describe, expect, it } from 'vitest';
import {
  checkStaleGenAtQuiet,
  feSnapshotFromTiles,
  summarizeStaleByColumnRow,
} from '../viewportTraceChecker';

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

  it('summarizes stale by column (x=0 focus)', () => {
    const s = summarizeStaleByColumnRow({
      tiles: [
        { level: 0, x: 0, y: 0 },
        { level: 0, x: 0, y: 1 },
        { level: 0, x: 1, y: 0 },
      ],
      tileGens: { '0/0/0': 40, '0/0/1': 40, '0/1/0': 42 },
      expectedGen: 42,
    });
    expect(s.byCol['0']?.stale).toBe(2);
    expect(s.byCol['1']?.stale).toBe(0);
    expect(s.staleKeys).toEqual(['0/0/0', '0/0/1']);
  });
});
