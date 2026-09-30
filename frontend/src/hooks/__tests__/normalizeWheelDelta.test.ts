import { describe, expect, it } from 'vitest';
import {
  clampWheelDelta,
  MAX_DELTA_PER_EVENT,
  normalizeWheelDelta,
} from '../useViewport';

function wheel(partial: Partial<WheelEvent>): WheelEvent {
  return {
    deltaX: 0,
    deltaY: 0,
    deltaMode: 0,
    ...partial,
  } as WheelEvent;
}

describe('normalizeWheelDelta', () => {
  it('passes through pixel deltas', () => {
    expect(normalizeWheelDelta(wheel({ deltaX: 3, deltaY: -4, deltaMode: 0 }))).toEqual({
      dx: 3,
      dy: -4,
    });
  });

  it('scales line mode (WebView2 trackpad often)', () => {
    expect(
      normalizeWheelDelta(
        wheel({ deltaX: 1, deltaY: 2, deltaMode: WheelEvent.DOM_DELTA_LINE }),
      ),
    ).toEqual({ dx: 16, dy: 32 });
  });

  it('scales page mode', () => {
    expect(
      normalizeWheelDelta(
        wheel({ deltaX: 0, deltaY: 1, deltaMode: WheelEvent.DOM_DELTA_PAGE }),
      ),
    ).toEqual({ dx: 0, dy: 800 });
  });
});

describe('clampWheelDelta', () => {
  it('passes through values within the per-event cap', () => {
    expect(clampWheelDelta(40)).toBe(40);
    expect(clampWheelDelta(-40)).toBe(-40);
  });

  it('caps oversized deltas from a single wheel event', () => {
    expect(clampWheelDelta(500)).toBe(MAX_DELTA_PER_EVENT);
    expect(clampWheelDelta(-500)).toBe(-MAX_DELTA_PER_EVENT);
  });
});
