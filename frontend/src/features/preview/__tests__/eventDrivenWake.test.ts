import { describe, expect, it } from 'vitest';
import {
  bumpMaxKnownGen,
  collapsedRetryStillStale,
  decideEventDrivenWake,
  parseTileGenHeader,
  shouldDiscardStaleGen,
  shouldRetryForMaxGen,
} from '../eventDrivenWake';

describe('decideEventDrivenWake (Step B)', () => {
  it('fetches when no pending entry', () => {
    expect(decideEventDrivenWake(undefined, 'tile_ready')).toEqual({ action: 'fetch' });
  });

  it('fetches when pending but not inflight', () => {
    expect(decideEventDrivenWake({ inflight: false }, 'ed_progress')).toEqual({
      action: 'fetch',
    });
  });

  it('marks needs_refetch instead of skipping when inflight (H4 fix)', () => {
    expect(decideEventDrivenWake({ inflight: true }, 'tile_ready')).toEqual({
      action: 'needs_refetch',
      reason: 'inflight',
      cause: 'tile_ready',
    });
  });
});

describe('gen gates', () => {
  it('bumps max known gen', () => {
    expect(bumpMaxKnownGen(3, 5)).toBe(5);
    expect(bumpMaxKnownGen(5, 2)).toBe(5);
    expect(bumpMaxKnownGen(5, null)).toBe(5);
  });

  it('discards response older than displayed', () => {
    expect(shouldDiscardStaleGen(41, 42)).toBe(true);
    expect(shouldDiscardStaleGen(42, 41)).toBe(false);
    expect(shouldDiscardStaleGen(null, 41)).toBe(false);
  });

  it('retries once when response behind maxKnown or needsRefetch', () => {
    expect(
      shouldRetryForMaxGen({
        responseGen: 41,
        maxKnownGen: 42,
        needsRefetch: false,
        refetchDone: false,
      }),
    ).toBe(true);
    expect(
      shouldRetryForMaxGen({
        responseGen: 42,
        maxKnownGen: 42,
        needsRefetch: true,
        refetchDone: false,
      }),
    ).toBe(true);
    expect(
      shouldRetryForMaxGen({
        responseGen: 41,
        maxKnownGen: 42,
        needsRefetch: true,
        refetchDone: true,
      }),
    ).toBe(false);
    expect(
      shouldRetryForMaxGen({
        responseGen: 42,
        maxKnownGen: 42,
        needsRefetch: false,
        refetchDone: false,
      }),
    ).toBe(false);
    // Null response must not spuriously retry (ready-only maxKnown).
    expect(
      shouldRetryForMaxGen({
        responseGen: null,
        maxKnownGen: 42,
        needsRefetch: false,
        refetchDone: false,
      }),
    ).toBe(false);
  });

  it('flags collapsed retry that still returned stale for safety recovery', () => {
    expect(
      collapsedRetryStillStale({
        responseGen: 41,
        maxKnownGen: 42,
        refetchDone: true,
        quality: 'fresh',
      }),
    ).toBe(true);
    expect(
      collapsedRetryStillStale({
        responseGen: 42,
        maxKnownGen: 42,
        refetchDone: true,
        quality: 'stale',
      }),
    ).toBe(true);
    expect(
      collapsedRetryStillStale({
        responseGen: 42,
        maxKnownGen: 42,
        refetchDone: true,
        quality: 'fresh',
      }),
    ).toBe(false);
  });

  it('parses X-Tile-Gen header', () => {
    expect(parseTileGenHeader('8.3')).toBe(8);
    expect(parseTileGenHeader('8.3.42')).toBe(42);
    expect(parseTileGenHeader('12')).toBe(12);
    expect(parseTileGenHeader(null)).toBe(null);
  });
});
