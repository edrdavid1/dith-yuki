/**
 * Mark the FlexLayout chrome that draws the bottom window border so CSS can
 * round that border (not inner panel widgets).
 *
 * `.flexlayout__tab` is often an absolute overlay — not always a child of its
 * tabset — so we match tabs by geometry against the chosen tabset.
 *
 * With side-by-side columns, only the tabset that actually meets the app
 * window corner is rounded (rightmost for a right edge, leftmost for left).
 * A neighbor that only touches another panel stays square.
 */
export function markDockBottomCorner(
  root: HTMLElement,
  corner: 'left' | 'right' | null,
): void {
  root.querySelectorAll('[data-dock-bottom-corner]').forEach((el) => {
    el.removeAttribute('data-dock-bottom-corner');
  });
  if (!corner) return;

  const tabsets = Array.from(root.querySelectorAll<HTMLElement>('.flexlayout__tabset'));
  if (tabsets.length === 0) return;

  let maxBottom = -Infinity;
  for (const ts of tabsets) {
    maxBottom = Math.max(maxBottom, ts.getBoundingClientRect().bottom);
  }

  // Candidates flush with the bottom of the column (side-by-side share this).
  const bottomFlush = tabsets.filter((ts) => {
    const r = ts.getBoundingClientRect();
    return r.height >= 2 && Math.abs(r.bottom - maxBottom) <= 3;
  });
  if (bottomFlush.length === 0) return;

  // Outer app corner: rightmost for right edge, leftmost for left.
  let chosen = bottomFlush[0]!;
  for (let i = 1; i < bottomFlush.length; i++) {
    const ts = bottomFlush[i]!;
    const a = chosen.getBoundingClientRect();
    const b = ts.getBoundingClientRect();
    if (corner === 'right' ? b.right > a.right : b.left < a.left) {
      chosen = ts;
    }
  }

  const setRect = chosen.getBoundingClientRect();
  chosen.setAttribute('data-dock-bottom-corner', corner);

  // Frame pieces: tabbar (top/sides) + tab (sides/bottom). Round only the tab
  // for the bottom-outer corner — that element owns the bottom border stroke.
  root.querySelectorAll<HTMLElement>('.flexlayout__tab').forEach((tab) => {
    const r = tab.getBoundingClientRect();
    if (r.width < 2 || r.height < 2) return;
    const overlapsHoriz = r.left < setRect.right - 1 && r.right > setRect.left + 1;
    const sitsInTabset = r.top < setRect.bottom - 1 && r.bottom > setRect.top + 1;
    const flushBottom = Math.abs(r.bottom - setRect.bottom) <= 3;
    if (overlapsHoriz && sitsInTabset && flushBottom) {
      tab.setAttribute('data-dock-bottom-corner', corner);
    }
  });
}

export function dockBottomCornerFromEdges(
  edges: string | undefined,
): 'left' | 'right' | null {
  if (!edges) return null;
  const tokens = new Set(edges.split(/\s+/));
  if (!tokens.has('bottom')) return null;
  if (tokens.has('right')) return 'right';
  if (tokens.has('left')) return 'left';
  return null;
}
