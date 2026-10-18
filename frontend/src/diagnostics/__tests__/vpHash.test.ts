import { describe, expect, it } from 'vitest';
import { tileRangeFromCoords, vpHash } from '../vpHash';
import {
  checkViewportInvariant,
  checkWorkerSpin,
  emptySeriesTrace,
  feSnapshotFromTiles,
} from '../viewportTraceChecker';

describe('vpHash', () => {
  it('is stable and length-16 hex (lockstep with Rust)', () => {
    const h = vpHash(1, 0, 0, 3, 2);
    expect(h).toBe('513f4888a1847625');
    expect(h).not.toBe(vpHash(0, 0, 0, 3, 2));
  });

  it('matches range helper', () => {
    const tiles = [
      { level: 1, x: 0, y: 0 },
      { level: 1, x: 3, y: 2 },
      { level: 1, x: 1, y: 1 },
    ];
    const r = tileRangeFromCoords(1, tiles);
    expect(r.vp_hash).toBe(vpHash(1, 0, 0, 3, 2));
    expect(r.count).toBe(3);
  });
});

describe('viewportTraceChecker', () => {
  it('flags VP_MISMATCH_FE_BE and TILE_NOT_REQUESTED', () => {
    const tiles = [{ level: 0, x: 1, y: 2 }];
    const fe = feSnapshotFromTiles(0, tiles, 3);
    const trace = emptySeriesTrace();
    const v = checkViewportInvariant({
      fe,
      beHash: 'deadbeefdeadbeef',
      scheduleHash: 'deadbeefdeadbeef',
      trace,
    });
    expect(v.some((x) => x.class === 'VP_MISMATCH_FE_BE')).toBe(true);
    expect(v.some((x) => x.class === 'TILE_NOT_REQUESTED')).toBe(true);
  });

  it('does not flag TILE_NOT_REQUESTED when tile was drawn from cache', () => {
    const tiles = [{ level: 0, x: 1, y: 2 }];
    const fe = feSnapshotFromTiles(0, tiles, 3);
    const trace = emptySeriesTrace();
    trace.drawn.add('0/1/2');
    const v = checkViewportInvariant({
      fe,
      beHash: fe.vp_hash,
      scheduleHash: fe.vp_hash,
      trace,
    });
    expect(v.some((x) => x.class === 'TILE_NOT_REQUESTED')).toBe(false);
  });

  it('ignores DRAWN_NOT_VISIBLE for draws under a prior vp_hash', () => {
    const tiles = [{ level: 2, x: 0, y: 0 }];
    const fe = feSnapshotFromTiles(2, tiles, 1);
    const trace = emptySeriesTrace();
    trace.requested.add('2/0/0');
    trace.response200.add('2/0/0');
    trace.drawn.add('2/0/0');
    trace.drawn.add('2/9/9'); // earlier pan at same level
    trace.drawnAtCurrentHash.add('2/0/0');
    const v = checkViewportInvariant({
      fe,
      beHash: fe.vp_hash,
      scheduleHash: fe.vp_hash,
      trace,
    });
    expect(v.some((x) => x.class === 'DRAWN_NOT_VISIBLE')).toBe(false);
  });

  it('ignores DRAWN_NOT_VISIBLE for requested ancestor LOD draws', () => {
    const tiles = [{ level: 2, x: 0, y: 0 }];
    const fe = feSnapshotFromTiles(2, tiles, 1);
    const trace = emptySeriesTrace();
    trace.requested.add('2/0/0');
    trace.response200.add('2/0/0');
    trace.drawn.add('2/0/0');
    trace.drawnAtCurrentHash.add('2/0/0');
    // Covering ancestor fetched+drawn while current level had a hole.
    trace.requested.add('3/0/0');
    trace.drawnAtCurrentHash.add('3/0/0');
    const v = checkViewportInvariant({
      fe,
      beHash: fe.vp_hash,
      scheduleHash: fe.vp_hash,
      trace,
    });
    expect(v.some((x) => x.class === 'DRAWN_NOT_VISIBLE')).toBe(false);
  });

  it('skips tile coverage classes when quietOk is false', () => {
    const tiles = [{ level: 0, x: 0, y: 0 }];
    const fe = feSnapshotFromTiles(0, tiles, 1);
    const trace = emptySeriesTrace();
    trace.requested.add('0/0/0');
    trace.responseStuck.add('0/0/0');
    const v = checkViewportInvariant({
      fe,
      beHash: fe.vp_hash,
      scheduleHash: fe.vp_hash,
      trace,
      quietOk: false,
    });
    expect(v.some((x) => x.class === 'TILE_NOT_SERVED')).toBe(false);
  });

  it('flags TILE_SERVED_NOT_DRAWN', () => {
    const tiles = [{ level: 0, x: 0, y: 0 }];
    const fe = feSnapshotFromTiles(0, tiles, 1);
    const trace = emptySeriesTrace();
    trace.requested.add('0/0/0');
    trace.response200.add('0/0/0');
    const v = checkViewportInvariant({
      fe,
      beHash: fe.vp_hash,
      scheduleHash: fe.vp_hash,
      trace,
    });
    expect(v.map((x) => x.class)).toContain('TILE_SERVED_NOT_DRAWN');
  });

  it('flags VP_NOT_SENT and VP_LOST_IN_FLIGHT', () => {
    const fe = feSnapshotFromTiles(0, [{ level: 0, x: 0, y: 0 }], 1);
    const trace = emptySeriesTrace();
    trace.feChangedWithoutSend = true;
    trace.sentSeqs.push(9);
    trace.requested.add('0/0/0');
    trace.response200.add('0/0/0');
    trace.drawn.add('0/0/0');
    const v = checkViewportInvariant({
      fe,
      beHash: fe.vp_hash,
      scheduleHash: fe.vp_hash,
      trace,
    });
    expect(v.map((x) => x.class)).toEqual(
      expect.arrayContaining(['VP_NOT_SENT', 'VP_LOST_IN_FLIGHT']),
    );
  });

  it('flags WORKER_SPIN above threshold', () => {
    const v = checkWorkerSpin([
      {
        tile_key: '2/1/0',
        level: 2,
        x: 1,
        y: 0,
        doc_gen: 3,
        dequeues_without_compute: 200,
        requeue_attempt: 5,
      },
    ]);
    expect(v).toHaveLength(1);
    expect(v[0].class).toBe('WORKER_SPIN');
    expect(
      checkWorkerSpin([
        {
          tile_key: '2/1/0',
          level: 2,
          x: 1,
          y: 0,
          doc_gen: 3,
          dequeues_without_compute: 20,
          requeue_attempt: 0,
        },
      ]),
    ).toHaveLength(0);
  });
});
