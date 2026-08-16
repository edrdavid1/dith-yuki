import { describe, expect, it } from 'vitest';
import {
  nextIntegerZoom,
  prevIntegerZoom,
  snapCssPx,
  snapIntegerZoom,
  snapIntegerZoomFloor,
  snapTileDrawRect,
} from '../zoomSnap';

describe('snapIntegerZoom', () => {
  it('rounds ≥1 to nearest integer', () => {
    expect(snapIntegerZoom(1.4)).toBe(1);
    expect(snapIntegerZoom(1.5)).toBe(2);
    expect(snapIntegerZoom(2.49)).toBe(2);
    expect(snapIntegerZoom(3)).toBe(3);
  });

  it('snaps <1 to power-of-two so pyramid tiles blit 1:1', () => {
    expect(snapIntegerZoom(0.5)).toBe(0.5);
    expect(snapIntegerZoom(0.4)).toBe(0.5);
    expect(snapIntegerZoom(0.3)).toBe(0.25);
    expect(snapIntegerZoom(0.26)).toBe(0.25);
    expect(snapIntegerZoom(1 / 3)).toBe(0.25);
  });

  it('clamps to max', () => {
    expect(snapIntegerZoom(100, 64)).toBe(64);
  });
});

describe('snapIntegerZoomFloor', () => {
  it('floors fit zoom so document still fits', () => {
    expect(snapIntegerZoomFloor(2.9)).toBe(2);
    expect(snapIntegerZoomFloor(1.01)).toBe(1);
    expect(snapIntegerZoomFloor(0.4)).toBe(0.25);
  });
});

describe('snapCssPx / snapTileDrawRect', () => {
  it('snaps to device pixels', () => {
    expect(snapCssPx(10.2, 2)).toBe(10);
    expect(snapCssPx(10.3, 2)).toBe(10.5);
  });

  it('derives size from snapped end to avoid gaps', () => {
    const a = snapTileDrawRect(0, 0, 256.4, 2);
    const b = snapTileDrawRect(256.4, 0, 256.4, 2);
    expect(a.dx + a.dw).toBeCloseTo(b.dx, 5);
  });
});

describe('integer zoom ladder', () => {
  it('steps up and down across 1×', () => {
    expect(nextIntegerZoom(1)).toBe(2);
    expect(prevIntegerZoom(1)).toBe(0.5);
    expect(nextIntegerZoom(0.5)).toBe(1);
    expect(prevIntegerZoom(2)).toBe(1);
    expect(nextIntegerZoom(0.25)).toBe(0.5);
    expect(prevIntegerZoom(0.5)).toBe(0.25);
  });
});
