import { describe, it, expect } from 'vitest';
import {
  addPoint,
  clamp01,
  evaluateCurve,
  fromByte,
  IDENTITY_CURVE,
  MIN_POINT_GAP,
  movePoint,
  nearestPointIndex,
  pixelToCurve,
  removePoint,
  toByte,
  xBounds,
  type CurvePoint,
} from '../curveMath';

describe('evaluateCurve', () => {
  it('is identity for the default two-point curve', () => {
    const curve = IDENTITY_CURVE;
    expect(evaluateCurve(curve, 0)).toBeCloseTo(0, 2);
    expect(evaluateCurve(curve, 0.5)).toBeCloseTo(0.5, 2);
    expect(evaluateCurve(curve, 1)).toBeCloseTo(1, 2);
  });

  it('inverts with [[0,1],[1,0]]', () => {
    const curve: CurvePoint[] = [[0, 1], [1, 0]];
    expect(evaluateCurve(curve, 0)).toBeCloseTo(1, 2);
    expect(evaluateCurve(curve, 0.5)).toBeCloseTo(0.5, 2);
    expect(evaluateCurve(curve, 1)).toBeCloseTo(0, 2);
  });

  it('raises midtones with a lift at 0.5', () => {
    const curve: CurvePoint[] = [[0, 0], [0.5, 0.7], [1, 1]];
    expect(evaluateCurve(curve, 0.5)).toBeGreaterThan(0.5);
  });

  it('darkens shadows and lifts highlights on an S-curve', () => {
    const curve: CurvePoint[] = [[0, 0], [0.25, 0.1], [0.5, 0.5], [0.75, 0.9], [1, 1]];
    expect(evaluateCurve(curve, 0.25)).toBeLessThan(0.25);
    expect(evaluateCurve(curve, 0.75)).toBeGreaterThan(0.75);
  });

  it('clamps out-of-range x', () => {
    expect(evaluateCurve(IDENTITY_CURVE, -0.5)).toBeGreaterThanOrEqual(0);
    expect(evaluateCurve(IDENTITY_CURVE, 1.5)).toBeLessThanOrEqual(1);
  });
});

describe('toByte / fromByte', () => {
  it('round-trips 0 and 255', () => {
    expect(toByte(0)).toBe(0);
    expect(toByte(1)).toBe(255);
    expect(fromByte(0)).toBe(0);
    expect(fromByte(255)).toBe(1);
  });

  it('round-trips 128', () => {
    expect(toByte(fromByte(128))).toBe(128);
  });
});

describe('movePoint', () => {
  it('does not let a point cross its neighbors', () => {
    const curve: CurvePoint[] = [[0, 0], [0.5, 0.5], [1, 1]];
    const moved = movePoint(curve, 1, 0.99, 0.2);
    expect(moved[1][0]).toBeLessThan(1 - MIN_POINT_GAP + 1e-9);
    expect(moved[1][0]).toBeGreaterThan(MIN_POINT_GAP - 1e-9);
    expect(moved[1][1]).toBeCloseTo(0.2, 5);
  });

  it('clamps y to [0, 1]', () => {
    const curve: CurvePoint[] = [[0, 0], [1, 1]];
    expect(movePoint(curve, 0, 0, 2)[0][1]).toBe(1);
    expect(movePoint(curve, 0, 0, -1)[0][1]).toBe(0);
  });
});

describe('addPoint / removePoint', () => {
  it('inserts sorted by x and reports the new index', () => {
    const added = addPoint(IDENTITY_CURVE, 0.5, 0.75);
    expect(added).not.toBeNull();
    expect(added!.curve).toHaveLength(3);
    expect(added!.index).toBe(1);
    expect(added!.curve[1][1]).toBeCloseTo(0.75, 5);
  });

  it('nudges x when it collides with an existing point', () => {
    const added = addPoint(IDENTITY_CURVE, 0, 0.4);
    expect(added).not.toBeNull();
    expect(Math.abs(added!.curve[added!.index][0] - 0)).toBeGreaterThanOrEqual(MIN_POINT_GAP);
  });

  it('refuses to drop below two points', () => {
    expect(removePoint(IDENTITY_CURVE, 0)).toBeNull();
  });

  it('removes a middle point', () => {
    const three: CurvePoint[] = [[0, 0], [0.5, 0.5], [1, 1]];
    const next = removePoint(three, 1);
    expect(next).toEqual([[0, 0], [1, 1]]);
  });
});

describe('nearestPointIndex / xBounds / pixelToCurve', () => {
  it('hits the closer control point', () => {
    const curve: CurvePoint[] = [[0, 0], [1, 1]];
    expect(nearestPointIndex(curve, 0.02, 0.02, 0.1)).toBe(0);
    expect(nearestPointIndex(curve, 0.98, 0.98, 0.1)).toBe(1);
    expect(nearestPointIndex(curve, 0.5, 0.5, 0.1)).toBeNull();
  });

  it('gives exclusive x bounds between neighbors', () => {
    const curve: CurvePoint[] = [[0, 0], [0.5, 0.5], [1, 1]];
    const bounds = xBounds(curve, 1);
    expect(bounds.min).toBeCloseTo(MIN_POINT_GAP, 8);
    expect(bounds.max).toBeCloseTo(1 - MIN_POINT_GAP, 8);
  });

  it('maps the graph center to (0.5, 0.5) with pad', () => {
    const [x, y] = pixelToCurve(128, 128, 256, 256, 6 / 256);
    expect(x).toBeCloseTo(0.5, 3);
    expect(y).toBeCloseTo(0.5, 3);
  });

  it('clamps via clamp01', () => {
    expect(clamp01(-1)).toBe(0);
    expect(clamp01(2)).toBe(1);
  });
});
