import { describe, expect, it } from 'vitest';
import { decideEventDrivenWake } from '../eventDrivenWake';

describe('decideEventDrivenWake', () => {
  it('fetches when no pending entry', () => {
    expect(decideEventDrivenWake(undefined, 'tile_ready')).toEqual({ action: 'fetch' });
  });

  it('fetches when pending but not inflight', () => {
    expect(decideEventDrivenWake({ inflight: false }, 'ed_progress')).toEqual({
      action: 'fetch',
    });
  });

  it('skips with inflight cause when a wake fetch is already running (H4 race)', () => {
    expect(decideEventDrivenWake({ inflight: true }, 'tile_ready')).toEqual({
      action: 'skip',
      reason: 'inflight',
      cause: 'tile_ready',
    });
  });
});
