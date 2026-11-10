import { CENTER_COLUMN_MIN_WIDTH } from '../../defaults/DefaultLayouts';
import type { DockSide } from '../../types/panels';

export type PanelDragMode = 'reorder' | 'undock' | 'cross';

export type ResolvePanelDragModeArgs = {
  clientX: number;
  side: DockSide;
  panelId: string | null | undefined;
  ownRect: { left: number; right: number } | null;
  oppositeRect: { left: number; right: number } | null;
  oppositeSide: DockSide;
  /** Viewport width for empty opposite-side edge strip hit-test. */
  viewportWidth: number;
  emptyEdgePx?: number;
};

/**
 * Decide docked-panel drag mode from pointer X and sidebar hit rects.
 * Cross-sidebar move wins over undock when the pointer is over the opposite column
 * (or its empty-edge strip).
 */
export function resolvePanelDragMode({
  clientX,
  side,
  panelId,
  ownRect,
  oppositeRect,
  oppositeSide,
  viewportWidth,
  emptyEdgePx = 48,
}: ResolvePanelDragModeArgs): PanelDragMode {
  if (panelId === 'preview') return 'undock';

  if (isOverOpposite(clientX, oppositeRect, oppositeSide, viewportWidth, emptyEdgePx)) {
    return 'cross';
  }

  if (!ownRect) return 'reorder';
  if (clientX < ownRect.left || clientX > ownRect.right) return 'undock';
  return 'reorder';
}

function isOverOpposite(
  clientX: number,
  oppositeRect: { left: number; right: number } | null,
  oppositeSide: DockSide,
  viewportWidth: number,
  emptyEdgePx: number
): boolean {
  if (oppositeRect && oppositeRect.right - oppositeRect.left > 0) {
    return clientX >= oppositeRect.left && clientX <= oppositeRect.right;
  }
  if (oppositeSide === 'left') return clientX <= emptyEdgePx;
  return clientX >= viewportWidth - emptyEdgePx;
}

/** Clamp vertical split ratio (shell presets / legacy stack math). */
export function clampSplitRatio(ratio: number): number {
  if (!Number.isFinite(ratio)) return 0.5;
  return Math.min(0.8, Math.max(0.2, ratio));
}

/** Column grid width for a Flex sidebar: 0 if empty, 40 if collapsed, else preferred. */
export function sidebarColumnWidth(
  hasDocked: boolean,
  collapsed: boolean,
  width: number
): number {
  if (!hasDocked) return 0;
  return collapsed ? 40 : width;
}

/**
 * Scale left/right pixel columns so the preview keeps CENTER_COLUMN_MIN_WIDTH
 * when possible. Per-side floors (e.g. 240 × horizontal span) are hard — the
 * preview may shrink below centerMin before a side-by-side column goes under
 * its floor.
 */
export function clampSidebarsToViewport(
  leftPx: number,
  rightPx: number,
  viewportW: number,
  centerMin: number = CENTER_COLUMN_MIN_WIDTH,
  leftMin: number = 0,
  rightMin: number = 0,
): [number, number] {
  const floorL = Math.max(0, leftMin);
  const floorR = Math.max(0, rightMin);
  let left = Math.max(leftPx, floorL);
  let right = Math.max(rightPx, floorR);
  const floors = floorL + floorR;
  // Prefer keeping the preview min; if floors need more, preview yields.
  const budget = Math.max(floors, Math.max(0, viewportW - centerMin));
  const sum = left + right;
  if (sum <= budget) return [left, right];
  if (sum <= 0) return [0, 0];
  const scale = budget / sum;
  left = Math.floor(left * scale);
  right = Math.floor(right * scale);
  left = Math.max(left, floorL);
  right = Math.max(right, floorR);
  // If floors alone exceed budget, still honor them (hard 240×N).
  if (left + right > viewportW && floors > 0) {
    return [floorL, floorR];
  }
  return [left, right];
}

/**
 * Flex-grow weights for stacked docked panels on one side.
 * Two panels use `ratio` / `1-ratio`; three+ share equally (MVP).
 */
export function panelStackFlex(index: number, count: number, ratio: number): number {
  if (count <= 1) return 1;
  if (count === 2) {
    const r = clampSplitRatio(ratio);
    return index === 0 ? r : 1 - r;
  }
  return 1;
}
