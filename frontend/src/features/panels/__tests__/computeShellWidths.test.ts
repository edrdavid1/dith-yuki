import { describe, expect, it } from 'vitest';
import {
  CENTER_COLUMN_MIN_WIDTH,
  DOCK_TABSET_MIN_WIDTH,
} from '../../../defaults/DefaultLayouts';
import {
  canAcceptExtraColumn,
  COLUMN_HARD_MIN,
  COLUMN_SOFT_MIN,
  computeShellWidths,
  EMERGENCY_COLLAPSE_STRIP,
  shellMinWindowWidth,
} from '../computeShellWidths';

type Case = {
  name: string;
  input: Parameters<typeof computeShellWidths>[0];
  expect: {
    centerMin?: number;
    left?: number;
    right?: number;
    emergencyLeft?: boolean;
    emergencyRight?: boolean;
    fits?: boolean;
  };
};

describe('computeShellWidths', () => {
  const cases: Case[] = [
    {
      name: 'comfortable 1+1 keeps prefs above soft floor',
      input: {
        viewportWidth: 1400,
        prefs: { leftWidth: 320, rightWidth: 280 },
        columnsLeft: 1,
        columnsRight: 1,
        collapsed: { left: false, right: false },
      },
      expect: {
        left: 320,
        right: 280,
        centerMin: CENTER_COLUMN_MIN_WIDTH,
        fits: true,
        emergencyLeft: false,
        emergencyRight: false,
      },
    },
    {
      name: 'soft floor lifts narrow prefs when 2 columns on a side',
      input: {
        viewportWidth: 1600,
        prefs: { leftWidth: 300, rightWidth: 240 },
        columnsLeft: 2,
        columnsRight: 1,
        collapsed: { left: false, right: false },
      },
      expect: {
        left: 2 * COLUMN_SOFT_MIN,
        right: COLUMN_SOFT_MIN,
        fits: true,
        emergencyLeft: false,
      },
    },
    {
      name: 'shrinks toward hard floors before emergency',
      input: {
        viewportWidth: 720,
        prefs: { leftWidth: 300, rightWidth: 300 },
        columnsLeft: 1,
        columnsRight: 1,
        collapsed: { left: false, right: false },
      },
      expect: {
        left: COLUMN_HARD_MIN,
        right: COLUMN_HARD_MIN,
        centerMin: CENTER_COLUMN_MIN_WIDTH,
        fits: true,
        emergencyLeft: false,
        emergencyRight: false,
      },
    },
    {
      name: 'emergency-collapses a side when hard floors do not fit',
      input: {
        viewportWidth: 600,
        prefs: { leftWidth: 320, rightWidth: 200 },
        columnsLeft: 1,
        columnsRight: 1,
        collapsed: { left: false, right: false },
      },
      expect: {
        emergencyLeft: true,
        emergencyRight: false,
        left: EMERGENCY_COLLAPSE_STRIP,
        fits: false,
        centerMin: CENTER_COLUMN_MIN_WIDTH,
      },
    },
    {
      name: 'user-collapsed side stays strip without emergency flag',
      input: {
        viewportWidth: 900,
        prefs: { leftWidth: 320, rightWidth: 280 },
        columnsLeft: 1,
        columnsRight: 1,
        collapsed: { left: true, right: false },
      },
      expect: {
        left: EMERGENCY_COLLAPSE_STRIP,
        right: 280,
        emergencyLeft: false,
        emergencyRight: false,
        fits: true,
      },
    },
    {
      name: 'empty side contributes 0 width',
      input: {
        viewportWidth: 1000,
        prefs: { leftWidth: 320, rightWidth: 280 },
        columnsLeft: 1,
        columnsRight: 0,
        collapsed: { left: false, right: false },
      },
      expect: {
        left: 320,
        right: 0,
        fits: true,
      },
    },
  ];

  it.each(cases)('$name', ({ input, expect: exp }) => {
    const r = computeShellWidths(input);
    if (exp.left !== undefined) expect(r.leftWidth).toBe(exp.left);
    if (exp.right !== undefined) expect(r.rightWidth).toBe(exp.right);
    if (exp.centerMin !== undefined) {
      expect(r.centerWidth).toBeGreaterThanOrEqual(exp.centerMin);
    }
    if (exp.emergencyLeft !== undefined) {
      expect(r.emergencyCollapseLeft).toBe(exp.emergencyLeft);
    }
    if (exp.emergencyRight !== undefined) {
      expect(r.emergencyCollapseRight).toBe(exp.emergencyRight);
    }
    if (exp.fits !== undefined) expect(r.fitsWithoutEmergency).toBe(exp.fits);
    expect(r.leftWidth + r.rightWidth + r.centerWidth).toBe(input.viewportWidth);
  });

  it('shellMinWindowWidth covers preview + two soft columns', () => {
    expect(shellMinWindowWidth()).toBe(
      CENTER_COLUMN_MIN_WIDTH + 2 * DOCK_TABSET_MIN_WIDTH,
    );
  });
});

/** W8 acceptance: table across canonical viewports × column layouts. */
describe('computeShellWidths viewport table', () => {
  const prefs = { leftWidth: 320, rightWidth: 280 };
  const viewports = [800, 1000, 1100, 1280, 1920] as const;
  const layouts: Array<{
    name: string;
    columnsLeft: number;
    columnsRight: number;
    collapsed?: { left: boolean; right: boolean };
  }> = [
    { name: '1+1', columnsLeft: 1, columnsRight: 1 },
    { name: '2+1', columnsLeft: 2, columnsRight: 1 },
    { name: '2+2', columnsLeft: 2, columnsRight: 2 },
    {
      name: '1+1 left collapsed',
      columnsLeft: 1,
      columnsRight: 1,
      collapsed: { left: true, right: false },
    },
  ];

  it.each(
    viewports.flatMap((vp) =>
      layouts.map((layout) => ({ vp, ...layout })),
    ),
  )('$name @ $vp', ({ vp, columnsLeft, columnsRight, collapsed }) => {
    const r = computeShellWidths({
      viewportWidth: vp,
      prefs,
      columnsLeft,
      columnsRight,
      collapsed: collapsed ?? { left: false, right: false },
    });
    expect(r.leftWidth + r.rightWidth + r.centerWidth).toBe(vp);
    expect(r.centerWidth).toBeGreaterThanOrEqual(CENTER_COLUMN_MIN_WIDTH);
    if (r.fitsWithoutEmergency) {
      expect(r.emergencyCollapseLeft).toBe(false);
      expect(r.emergencyCollapseRight).toBe(false);
      if (!(collapsed?.left) && columnsLeft > 0) {
        expect(r.leftWidth).toBeGreaterThanOrEqual(columnsLeft * COLUMN_HARD_MIN);
      }
      if (!(collapsed?.right) && columnsRight > 0) {
        expect(r.rightWidth).toBeGreaterThanOrEqual(columnsRight * COLUMN_HARD_MIN);
      }
    } else {
      expect(r.emergencyCollapseLeft || r.emergencyCollapseRight).toBe(true);
    }
  });

  it('800px 1+1 fits at soft floors (window minWidth)', () => {
    const r = computeShellWidths({
      viewportWidth: 800,
      prefs: { leftWidth: 240, rightWidth: 240 },
      columnsLeft: 1,
      columnsRight: 1,
      collapsed: { left: false, right: false },
    });
    expect(r.fitsWithoutEmergency).toBe(true);
    expect(r.leftWidth).toBe(COLUMN_SOFT_MIN);
    expect(r.rightWidth).toBe(COLUMN_SOFT_MIN);
    expect(r.centerWidth).toBe(CENTER_COLUMN_MIN_WIDTH);
  });

  it('1100px 2+2 requires emergency (960+320 > 1100)', () => {
    const r = computeShellWidths({
      viewportWidth: 1100,
      prefs: { leftWidth: 480, rightWidth: 480 },
      columnsLeft: 2,
      columnsRight: 2,
      collapsed: { left: false, right: false },
    });
    expect(r.fitsWithoutEmergency).toBe(false);
    expect(r.centerWidth).toBeGreaterThanOrEqual(CENTER_COLUMN_MIN_WIDTH);
  });

  it('1280px 2+2 fits at soft floors', () => {
    const r = computeShellWidths({
      viewportWidth: 1280,
      prefs: { leftWidth: 480, rightWidth: 480 },
      columnsLeft: 2,
      columnsRight: 2,
      collapsed: { left: false, right: false },
    });
    expect(r.fitsWithoutEmergency).toBe(true);
    expect(r.leftWidth).toBe(2 * COLUMN_SOFT_MIN);
    expect(r.rightWidth).toBe(2 * COLUMN_SOFT_MIN);
    expect(r.centerWidth).toBe(CENTER_COLUMN_MIN_WIDTH);
  });
});

describe('canAcceptExtraColumn', () => {
  it('rejects a third column on a side', () => {
    expect(
      canAcceptExtraColumn({
        viewportWidth: 2000,
        prefs: { leftWidth: 480, rightWidth: 240 },
        columnsLeft: 2,
        columnsRight: 1,
        collapsed: { left: false, right: false },
        side: 'left',
      }),
    ).toBe(false);
  });

  it('rejects when the extra column would force emergency collapse', () => {
    expect(
      canAcceptExtraColumn({
        viewportWidth: 700,
        prefs: { leftWidth: 240, rightWidth: 240 },
        columnsLeft: 1,
        columnsRight: 1,
        collapsed: { left: false, right: false },
        side: 'left',
      }),
    ).toBe(false);
  });

  it('allows when soft floors still fit', () => {
    expect(
      canAcceptExtraColumn({
        viewportWidth: 1400,
        prefs: { leftWidth: 240, rightWidth: 240 },
        columnsLeft: 1,
        columnsRight: 1,
        collapsed: { left: false, right: false },
        side: 'left',
      }),
    ).toBe(true);
  });
});
