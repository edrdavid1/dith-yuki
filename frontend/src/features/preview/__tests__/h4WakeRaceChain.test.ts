/**
 * Step B — H4 wake race: needs_refetch + collapsed retry (green after fix).
 */
import { describe, expect, it } from 'vitest';
import {
  bumpMaxKnownGen,
  decideEventDrivenWake,
  shouldDiscardStaleGen,
  shouldRetryForMaxGen,
} from '../eventDrivenWake';

type TraceEvt = {
  event: string;
  tile?: string;
  gen?: number;
  details?: Record<string, unknown>;
};

function simulateFixed00Chain(): TraceEvt[] {
  const out: TraceEvt[] = [];
  let tileRev = 10;
  let inflight = false;
  let needsRefetch = false;
  let refetchDone = false;
  let maxKnownGen = 0;
  let drawnGen: number | null = null;
  const tile = '0/0/0';

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

  // First tile-ready → fetch
  let wake = decideEventDrivenWake({ inflight }, 'tile_ready');
  expect(wake.action).toBe('fetch');
  inflight = true;
  maxKnownGen = bumpMaxKnownGen(maxKnownGen, 41);
  out.push({ event: 'tile_ready_emitted', tile, gen: 41 });
  out.push({ event: 'fetch_tile_posted', tile, gen: 41, details: { wake: 'tile_ready' } });

  // Newer ready while inflight → needs_refetch (not skip)
  maxKnownGen = bumpMaxKnownGen(maxKnownGen, 42);
  out.push({ event: 'tile_ready_emitted', tile, gen: 42 });
  wake = decideEventDrivenWake({ inflight }, 'tile_ready');
  expect(wake).toEqual({
    action: 'needs_refetch',
    reason: 'inflight',
    cause: 'tile_ready',
  });
  needsRefetch = true;
  out.push({
    event: 'wake_needs_refetch',
    tile,
    gen: 42,
    details: { cause: 'tile_ready', maxKnownGen },
  });

  // Inflight returns gen 41 — behind maxKnown → collapsed retry, interim may draw
  expect(
    shouldRetryForMaxGen({
      responseGen: 41,
      maxKnownGen,
      needsRefetch,
      refetchDone,
    }),
  ).toBe(true);
  refetchDone = true;
  needsRefetch = false;
  inflight = true;
  drawnGen = 41;
  out.push({
    event: 'wake_collapsed_retry',
    tile,
    gen: 41,
    details: { responseGen: 41, maxKnownGen: 42 },
  });
  out.push({ event: 'tile_drawn', tile, gen: 41, details: { interim: true } });

  // Collapsed retry returns 42 — accept as final, discard would apply to older
  expect(shouldDiscardStaleGen(41, 42)).toBe(true);
  expect(
    shouldRetryForMaxGen({
      responseGen: 42,
      maxKnownGen,
      needsRefetch: false,
      refetchDone: true,
    }),
  ).toBe(false);
  drawnGen = 42;
  out.push({ event: 'tile_drawn', tile, gen: 42, details: { source: 'fresh' } });

  expect(drawnGen).toBe(42);
  expect(out.some((e) => e.event === 'wake_skipped_inflight')).toBe(false);
  expect(out.some((e) => e.event === 'wake_needs_refetch')).toBe(true);
  expect(out.filter((e) => e.event === 'wake_collapsed_retry')).toHaveLength(1);
  return out;
}

describe('H4 wake race chain (Step B fix)', () => {
  it('coalesces inflight wakes into one retry and ends on newer gen', () => {
    const chain = simulateFixed00Chain();
    const stick = chain.filter(
      (e) =>
        e.tile === '0/0/0' &&
        (e.event === 'wake_needs_refetch' ||
          e.event === 'wake_collapsed_retry' ||
          e.event === 'tile_drawn' ||
          e.event === 'tile_ready_emitted'),
    );
    expect(stick.map((e) => `${e.event}@${e.gen ?? '-'}`)).toEqual([
      'tile_ready_emitted@41',
      'tile_ready_emitted@42',
      'wake_needs_refetch@42',
      'wake_collapsed_retry@41',
      'tile_drawn@41',
      'tile_drawn@42',
    ]);
  });
});
