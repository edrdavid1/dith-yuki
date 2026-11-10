/**
 * Side-by-side dock policy inside a FlexLayout sidebar column.
 *
 * Model: each side is still one FlexLayout Model; horizontal splits are rows of
 * tabsets (columns), vertical stacks are columns of tabsets. Shell owns the
 * outer left | preview | right chrome widths.
 */

import { Model, Orientation, TabNode, TabSetNode } from 'flexlayout-react';
import {
  CENTER_COLUMN_MIN_WIDTH,
  DOCK_TABSET_MIN_WIDTH,
  SIDEBAR_EXPANDED_MAX_WIDTH,
  SIDEBAR_EXPANDED_MIN_WIDTH,
} from '../../defaults/DefaultLayouts';

/** Hard cap: 600 / 240 → at most two visible columns per side. */
export const MAX_SIDE_COLUMNS = Math.floor(
  SIDEBAR_EXPANDED_MAX_WIDTH / DOCK_TABSET_MIN_WIDTH,
);

function rowOrientation(node: { getOrientation?: () => unknown }): unknown {
  return typeof node.getOrientation === 'function' ? node.getOrientation() : null;
}

/**
 * Widest *horizontal* row of docked columns (ignore vertical stacks).
 * Used to grow the sidebar so each visible panel keeps DOCK_TABSET_MIN_WIDTH.
 */
export function maxDockedHorizontalSpan(model: Model | null): number {
  if (!model) return 1;
  let maxSpan = 1;
  model.visitNodes((node) => {
    if (node.getType() !== 'row') return;
    // Vertical row = stacked tabsets; not a side-by-side span.
    if (rowOrientation(node) === Orientation.VERT) return;
    let span = 0;
    for (const child of node.getChildren()) {
      const t = child.getType();
      if (t === TabSetNode.TYPE) {
        const hasDocked = (child as TabSetNode).getChildren().some(
          (c) => c.getType() === TabNode.TYPE && !(c as TabNode).isFloating(),
        );
        if (hasDocked) span += 1;
      } else if (t === 'column' || t === 'row') {
        span += 1;
      }
    }
    if (span > maxSpan) maxSpan = span;
  });
  return maxSpan;
}

/** Preferred sidebar width so each side-by-side slot keeps DOCK_TABSET_MIN_WIDTH. */
export function sidebarWidthForHorizontalSpan(span: number, preferred: number): number {
  const slots = Math.max(1, span);
  const fromSpan = slots * DOCK_TABSET_MIN_WIDTH;
  return Math.max(preferred, fromSpan, SIDEBAR_EXPANDED_MIN_WIDTH);
}

export type HorizontalDropBudget = {
  currentSpan: number;
  sidebarWidth: number;
  otherSidebarWidth: number;
  viewportWidth: number;
  maxSidebarWidth?: number;
  centerMinWidth?: number;
};

/**
 * Room we can give this sidebar after preview keeps its min and the opposite
 * side can shrink to a single-column floor (not its preferred width).
 */
export function horizontalSidebarRoom(budget: HorizontalDropBudget): number {
  const maxSide = budget.maxSidebarWidth ?? SIDEBAR_EXPANDED_MAX_WIDTH;
  const centerMin = budget.centerMinWidth ?? CENTER_COLUMN_MIN_WIDTH;
  // Opposite side may shrink to one column (or 0 if empty) so we can grow.
  const otherFloor =
    budget.otherSidebarWidth <= 0
      ? 0
      : Math.min(budget.otherSidebarWidth, DOCK_TABSET_MIN_WIDTH);
  const viewportBudget = Math.max(
    0,
    budget.viewportWidth - centerMin - otherFloor,
  );
  return Math.min(maxSide, Math.max(budget.sidebarWidth, viewportBudget));
}

/**
 * Whether a left/right drop that adds a column can keep every column ≥ 240px
 * after growing this sidebar (and shrinking the opposite side toward its floor).
 */
export function canFitAdditionalColumn(budget: HorizontalDropBudget): boolean {
  const next = budget.currentSpan + 1;
  if (next > MAX_SIDE_COLUMNS) return false;
  return horizontalSidebarRoom(budget) >= next * DOCK_TABSET_MIN_WIDTH;
}

/** True when from/to already sit as siblings under the same horizontal row. */
export function isHorizontalRearrange(
  model: Model,
  fromNodeId: string,
  toNodeId: string,
): boolean {
  const fromTs = tabsetOf(model, fromNodeId);
  const toTs = tabsetOf(model, toNodeId);
  if (!fromTs || !toTs || fromTs.getId() === toTs.getId()) return false;
  const fromParent = fromTs.getParent();
  const toParent = toTs.getParent();
  if (!fromParent || !toParent) return false;
  if (fromParent.getId() !== toParent.getId()) return false;
  if (fromParent.getType() !== 'row') return false;
  // Vertical parent = stacked panels; left/right there creates a *new* column.
  return rowOrientation(fromParent) === Orientation.HORZ;
}

/**
 * Accept left/right dock when under the column cap. Prefer completing the dock
 * and growing the sidebar (`ensureSidebarFitsSpan`) over rejecting — a hard
 * reject made left-edge docks feel broken (gesture fell through to float).
 *
 * Still deny a 3rd column (MAX_SIDE_COLUMNS). Rearranges of an existing row
 * always allowed. Outline preview (no from/to) stays allowed so rearrange
 * highlights aren't dimmed at max span.
 */
export function canAcceptHorizontalDrop(
  model: Model,
  fromNodeId: string | undefined,
  toNodeId: string | undefined,
  _budget: Omit<HorizontalDropBudget, 'currentSpan'>,
): boolean {
  const currentSpan = maxDockedHorizontalSpan(model);

  if (!fromNodeId || !toNodeId) {
    return true;
  }

  if (isHorizontalRearrange(model, fromNodeId, toNodeId)) {
    return true;
  }

  // New column: allow whenever we stay within the hard column cap.
  return currentSpan < MAX_SIDE_COLUMNS;
}

/** Clamp a persisted expanded sidebar width into the legal range. */
export function clampExpandedSidebarWidth(
  width: number,
  minWidth: number = SIDEBAR_EXPANDED_MIN_WIDTH,
  maxWidth: number = SIDEBAR_EXPANDED_MAX_WIDTH,
): number {
  if (!Number.isFinite(width)) return minWidth;
  return Math.min(maxWidth, Math.max(minWidth, Math.round(width)));
}

function tabsetOf(model: Model, nodeId: string): TabSetNode | null {
  const node = model.getNodeById(nodeId);
  if (!node) return null;
  if (node.getType() === TabSetNode.TYPE) return node as TabSetNode;
  if (node.getType() === TabNode.TYPE) {
    const parent = node.getParent();
    if (parent && parent.getType() === TabSetNode.TYPE) {
      return parent as TabSetNode;
    }
  }
  return null;
}
