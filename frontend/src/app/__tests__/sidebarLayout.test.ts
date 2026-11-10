import { describe, it, expect } from 'vitest';
import {
  clampSidebarsToViewport,
  sidebarColumnWidth,
} from '../../features/panels/panelDragMode';
import { CENTER_COLUMN_MIN_WIDTH } from '../../defaults/DefaultLayouts';

describe('sidebarColumnWidth', () => {
  it('docked expanded uses preferred width', () => {
    expect(sidebarColumnWidth(true, false, 332)).toBe(332);
    expect(sidebarColumnWidth(true, false, 280)).toBe(280);
  });

  it('collapsed strip is 40px when docked', () => {
    expect(sidebarColumnWidth(true, true, 332)).toBe(40);
    expect(sidebarColumnWidth(true, true, 500)).toBe(40);
  });

  it('empty side is 0 regardless of collapsed/width prefs', () => {
    expect(sidebarColumnWidth(false, false, 332)).toBe(0);
    expect(sidebarColumnWidth(false, true, 332)).toBe(0);
  });
});

describe('clampSidebarsToViewport', () => {
  it('leaves widths alone when preview still fits', () => {
    expect(clampSidebarsToViewport(332, 332, 1200)).toEqual([332, 332]);
  });

  it('scales both sides so center keeps its min width', () => {
    const [l, r] = clampSidebarsToViewport(480, 480, 1000);
    expect(l + r).toBeLessThanOrEqual(1000 - CENTER_COLUMN_MIN_WIDTH);
    expect(l).toBeGreaterThan(0);
    expect(r).toBeGreaterThan(0);
  });

  it('never shrinks a side below its horizontal-span floor', () => {
    const [l, r] = clampSidebarsToViewport(480, 480, 900, CENTER_COLUMN_MIN_WIDTH, 480, 240);
    expect(l).toBeGreaterThanOrEqual(480);
    expect(r).toBeGreaterThanOrEqual(240);
  });
});
