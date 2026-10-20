/**
 * Step A — H4 event-chain model (no production behavior change).
 *
 * Shows how wake_skipped_inflight + accepting a matching-tileRev decode of an
 * older X-Tile-Generation leaves (0,0) stuck until pan/zoom forces a new rev.
 */
import { describe, expect, it } from 'vitest';
import { decideEventDrivenWake } from '../eventDrivenWake';

type TraceEvt = {
  event: string;
  tile?: string;
  gen?: number;
  details?: Record<string, unknown>;
};

function simulateStuck00Chain(): TraceEvt[] {
  const out: TraceEvt[] = [];
  let tileRev = 10;
  let inflight = false;
  let drawnGen: number | null = null;
  const tile = '0/0/0';

  // Param change bumps rev and requests tiles.
  tileRev += 1;
  out.push({
    event: 'doc_changed_received',
    details: { tileRev, kind: 'filter_updated' },
  });
  out.push({
    event: 'tile_requested',
    tile,
    details: { tileRev, via: 'request-tiles' },
  });
  out.push({
    event: 'tile_response',
    tile,
    details: { status: 202, pending_reason: 'ed_prefix' },
  });

  // First tile-ready starts wake fetch (gen 41 still computing / about to land).
  let wake = decideEventDrivenWake({ inflight }, 'tile_ready');
  expect(wake.action).toBe('fetch');
  inflight = true;
  out.push({
    event: 'tile_ready_emitted',
    tile,
    gen: 41,
    details: { cause: 'be' },
  });
  out.push({
    event: 'tile_ready_received',
    tile,
    details: { accepted: true },
  });
  out.push({
    event: 'fetch_tile_posted',
    tile,
    details: { wake: 'tile_ready', tileRev },
  });

  // Newer Composite for same tile (gen 42) while fetch still inflight → skip.
  out.push({
    event: 'tile_ready_emitted',
    tile,
    gen: 42,
    details: { cause: 'be' },
  });
  wake = decideEventDrivenWake({ inflight }, 'tile_ready');
  expect(wake).toEqual({ action: 'skip', reason: 'inflight', cause: 'tile_ready' });
  out.push({
    event: 'wake_skipped_inflight',
    tile,
    gen: 42,
    details: { cause: 'tile_ready', tileRev },
  });

  // Inflight response returns gen 41 (stale vs live 42) but tileRev still matches.
  // FE accepts (no gen compare today) → drawn as final.
  inflight = false;
  drawnGen = 41;
  out.push({
    event: 'tile_decoded',
    tile,
    gen: 41,
    details: { requestRev: tileRev, currentRev: tileRev, source: 'fresh', status: 200 },
  });
  out.push({
    event: 'tile_drawn',
    tile,
    gen: 41,
    details: { source: 'fresh', tileRev },
  });

  expect(drawnGen).toBe(41);
  expect(out.some((e) => e.event === 'wake_skipped_inflight' && e.gen === 42)).toBe(true);
  expect(out.some((e) => e.event === 'tile_drawn' && e.gen === 42)).toBe(false);
  return out;
}

describe('H4 wake race chain (model)', () => {
  it('skips newer tile-ready while inflight and draws older gen as final', () => {
    const chain = simulateStuck00Chain();
    const stick = chain.filter(
      (e) =>
        e.tile === '0/0/0' &&
        (e.event === 'wake_skipped_inflight' ||
          e.event === 'tile_drawn' ||
          e.event === 'tile_ready_emitted'),
    );
    expect(stick.map((e) => `${e.event}@${e.gen ?? '-'}`)).toEqual([
      'tile_ready_emitted@41',
      'tile_ready_emitted@42',
      'wake_skipped_inflight@42',
      'tile_drawn@41',
    ]);
  });
});
