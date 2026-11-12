import { describe, it, expect } from 'vitest';
import {
  cssRectToPhysicalScreen,
  physicalEdgeStrip,
} from '../dockZoneCoords';

describe('cssRectToPhysicalScreen (W6)', () => {
  it('scales by dpr 1.0 at origin', () => {
    expect(
      cssRectToPhysicalScreen(
        { left: 10, top: 20, width: 100, height: 200 },
        { physicalX: 0, physicalY: 0, dpr: 1 },
      ),
    ).toEqual({ x: 10, y: 20, width: 100, height: 200 });
  });

  it('scales by dpr 2.0', () => {
    expect(
      cssRectToPhysicalScreen(
        { left: 10, top: 20, width: 100, height: 50 },
        { physicalX: 100, physicalY: 200, dpr: 2 },
      ),
    ).toEqual({ x: 120, y: 240, width: 200, height: 100 });
  });

  it('scales by dpr 1.5', () => {
    const r = cssRectToPhysicalScreen(
      { left: 0, top: 0, width: 240, height: 40 },
      { physicalX: 0, physicalY: 0, dpr: 1.5 },
    );
    expect(r.width).toBeCloseTo(360);
    expect(r.height).toBeCloseTo(60);
  });

  it('supports negative monitor origins (display to the left of primary)', () => {
    expect(
      cssRectToPhysicalScreen(
        { left: 40, top: 10, width: 200, height: 100 },
        { physicalX: -1920, physicalY: 100, dpr: 1 },
      ),
    ).toEqual({ x: -1880, y: 110, width: 200, height: 100 });
  });
});

describe('physicalEdgeStrip (W6)', () => {
  it('places left strip at physical origin', () => {
    const r = physicalEdgeStrip(
      'left',
      { physicalX: -100, physicalY: 50, dpr: 2 },
      2000,
      1600,
      64,
    );
    expect(r).toEqual({ x: -100, y: 50, width: 128, height: 1600 });
  });

  it('places right strip at the trailing edge', () => {
    const r = physicalEdgeStrip(
      'right',
      { physicalX: 0, physicalY: 0, dpr: 1 },
      1280,
      800,
      96,
    );
    expect(r.x).toBe(1280 - 96);
    expect(r.width).toBe(96);
  });
});
