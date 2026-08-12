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

/** Clamp vertical split ratio used by DockedSidebar. */
export function clampSplitRatio(ratio: number): number {
  if (!Number.isFinite(ratio)) return 0.5;
  return Math.min(0.8, Math.max(0.2, ratio));
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
