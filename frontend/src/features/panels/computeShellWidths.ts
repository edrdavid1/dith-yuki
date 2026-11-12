/**
 * W8: pure shell geometry solver — preview + side columns always fit the
 * viewport (no horizontal scroll). Emergency collapse is temporary UI only.
 */

import {
  CENTER_COLUMN_MIN_WIDTH,
  DOCK_TABSET_MIN_WIDTH,
} from '../../defaults/DefaultLayouts';

/** Soft floor per side-by-side column (CSS px). */
export const COLUMN_SOFT_MIN = DOCK_TABSET_MIN_WIDTH; // 240
/** Hard floor when space is short (CSS px). */
export const COLUMN_HARD_MIN = 200;
/** Temporary strip when a side cannot keep hard floors (CSS px). */
export const EMERGENCY_COLLAPSE_STRIP = 40;
/** User-collapsed / float-host strip. */
export const COLLAPSED_STRIP = 40;
/**
 * Resize handles between sidebars and canvas (CSS px). Two handles when both
 * sides are expanded.
 */
export const SHELL_HANDLE_WIDTH = 0;

export type ShellCollapsedPrefs = {
  left: boolean;
  right: boolean;
};

export type ComputeShellWidthsInput = {
  viewportWidth: number;
  prefs: { leftWidth: number; rightWidth: number };
  /** Docked horizontal columns on the left (0 = empty / float-host). */
  columnsLeft: number;
  columnsRight: number;
  collapsed: ShellCollapsedPrefs;
};

export type ComputeShellWidthsResult = {
  leftWidth: number;
  rightWidth: number;
  centerWidth: number;
  /** True when left is forced to a 40px strip without writing prefs. */
  emergencyCollapseLeft: boolean;
  emergencyCollapseRight: boolean;
  /** True when the requested column layout fits without emergency collapse. */
  fitsWithoutEmergency: boolean;
};

function desiredSideWidth(
  columns: number,
  collapsed: boolean,
  preferred: number,
): number {
  if (columns <= 0) return 0;
  if (collapsed) return COLLAPSED_STRIP;
  const soft = Math.max(1, columns) * COLUMN_SOFT_MIN;
  return Math.max(preferred, soft);
}

function hardFloor(columns: number, collapsed: boolean): number {
  if (columns <= 0) return 0;
  if (collapsed) return COLLAPSED_STRIP;
  return Math.max(1, columns) * COLUMN_HARD_MIN;
}

/**
 * Allocate left | center | right so:
 * (1) center ≥ 320
 * (2) columns prefer ≥ 240 each
 * (3) shrink toward ≥ 200 each when short
 * (4) emergency-collapse a side to 40px if still impossible
 */
export function computeShellWidths(
  input: ComputeShellWidthsInput,
): ComputeShellWidthsResult {
  const vp = Math.max(0, Math.floor(input.viewportWidth));
  const centerMin = CENTER_COLUMN_MIN_WIDTH;

  let left = desiredSideWidth(
    input.columnsLeft,
    input.collapsed.left,
    input.prefs.leftWidth,
  );
  let right = desiredSideWidth(
    input.columnsRight,
    input.collapsed.right,
    input.prefs.rightWidth,
  );

  const handles =
    (left > 0 ? SHELL_HANDLE_WIDTH : 0) + (right > 0 ? SHELL_HANDLE_WIDTH : 0);
  const contentBudget = Math.max(0, vp - handles);

  let emergencyCollapseLeft = false;
  let emergencyCollapseRight = false;

  const fitsIdeal = left + right + centerMin <= contentBudget;
  if (fitsIdeal) {
    return {
      leftWidth: left,
      rightWidth: right,
      centerWidth: contentBudget - left - right,
      emergencyCollapseLeft: false,
      emergencyCollapseRight: false,
      fitsWithoutEmergency: true,
    };
  }

  // Shrink proportionally toward hard floors.
  const floorL = hardFloor(input.columnsLeft, input.collapsed.left);
  const floorR = hardFloor(input.columnsRight, input.collapsed.right);
  const sideBudget = Math.max(0, contentBudget - centerMin);

  if (floorL + floorR <= sideBudget) {
    const sum = left + right;
    if (sum > 0) {
      const scale = sideBudget / sum;
      left = Math.max(floorL, Math.floor(left * scale));
      right = Math.max(floorR, Math.floor(right * scale));
      // Fix rounding so we never exceed budget.
      let over = left + right - sideBudget;
      if (over > 0) {
        const shrinkR = Math.min(over, Math.max(0, right - floorR));
        right -= shrinkR;
        over -= shrinkR;
        left -= Math.min(over, Math.max(0, left - floorL));
      }
    } else {
      left = 0;
      right = 0;
    }
    return {
      leftWidth: left,
      rightWidth: right,
      centerWidth: contentBudget - left - right,
      emergencyCollapseLeft: false,
      emergencyCollapseRight: false,
      fitsWithoutEmergency: true,
    };
  }

  // Emergency: collapse sides that need hard floors until center fits.
  // Prefer collapsing the wider preferred side first, then the other.
  const tryCollapse = (collapseLeft: boolean, collapseRight: boolean) => {
    const l = collapseLeft
      ? input.columnsLeft > 0
        ? EMERGENCY_COLLAPSE_STRIP
        : 0
      : hardFloor(input.columnsLeft, input.collapsed.left);
    const r = collapseRight
      ? input.columnsRight > 0
        ? EMERGENCY_COLLAPSE_STRIP
        : 0
      : hardFloor(input.columnsRight, input.collapsed.right);
    return { l, r, ok: l + r + centerMin <= contentBudget };
  };

  // Already user-collapsed sides stay strips; don't mark emergency for them.
  const needL =
    input.columnsLeft > 0 &&
    !input.collapsed.left &&
    hardFloor(input.columnsLeft, false) > 0;
  const needR =
    input.columnsRight > 0 &&
    !input.collapsed.right &&
    hardFloor(input.columnsRight, false) > 0;

  const order: Array<[boolean, boolean]> = [];
  if (needL && needR) {
    if (input.prefs.leftWidth >= input.prefs.rightWidth) {
      order.push([true, false], [false, true], [true, true]);
    } else {
      order.push([false, true], [true, false], [true, true]);
    }
  } else if (needL) {
    order.push([true, false]);
  } else if (needR) {
    order.push([false, true]);
  }
  order.push([needL, needR]); // both if still needed

  for (const [cL, cR] of order) {
    const t = tryCollapse(cL, cR);
    if (t.ok) {
      emergencyCollapseLeft = cL && needL;
      emergencyCollapseRight = cR && needR;
      left = t.l;
      right = t.r;
      break;
    }
  }

  // Last resort: both strips / empty — center takes the rest (may still be
  // below min only if viewport itself is under the window minWidth).
  if (left + right + centerMin > contentBudget) {
    emergencyCollapseLeft = needL;
    emergencyCollapseRight = needR;
    left = needL ? EMERGENCY_COLLAPSE_STRIP : left > 0 && input.collapsed.left ? COLLAPSED_STRIP : 0;
    right = needR
      ? EMERGENCY_COLLAPSE_STRIP
      : right > 0 && input.collapsed.right
        ? COLLAPSED_STRIP
        : 0;
    if (left + right > contentBudget) {
      left = 0;
      right = 0;
    }
  }

  return {
    leftWidth: left,
    rightWidth: right,
    centerWidth: Math.max(0, contentBudget - left - right),
    emergencyCollapseLeft,
    emergencyCollapseRight,
    fitsWithoutEmergency: !emergencyCollapseLeft && !emergencyCollapseRight,
  };
}

/**
 * Whether adding one more horizontal column on `side` fits without emergency
 * collapse (W8 drop-zone policy).
 */
export function canAcceptExtraColumn(args: {
  viewportWidth: number;
  prefs: { leftWidth: number; rightWidth: number };
  columnsLeft: number;
  columnsRight: number;
  collapsed: ShellCollapsedPrefs;
  side: 'left' | 'right';
  maxColumns?: number;
}): boolean {
  const max = args.maxColumns ?? 2;
  const nextLeft =
    args.side === 'left' ? args.columnsLeft + 1 : args.columnsLeft;
  const nextRight =
    args.side === 'right' ? args.columnsRight + 1 : args.columnsRight;
  if (args.side === 'left' && nextLeft > max) return false;
  if (args.side === 'right' && nextRight > max) return false;

  const result = computeShellWidths({
    viewportWidth: args.viewportWidth,
    prefs: args.prefs,
    columnsLeft: nextLeft,
    columnsRight: nextRight,
    collapsed: args.collapsed,
  });
  if (!result.fitsWithoutEmergency) return false;
  if (args.side === 'left' && result.emergencyCollapseLeft) return false;
  if (args.side === 'right' && result.emergencyCollapseRight) return false;
  return true;
}

/** Min main-window width for 1+1 columns + preview (W8). */
export function shellMinWindowWidth(): number {
  return (
    CENTER_COLUMN_MIN_WIDTH +
    2 * COLUMN_SOFT_MIN +
    2 * SHELL_HANDLE_WIDTH
  );
}
