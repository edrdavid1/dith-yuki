/**
 * Shared insert-index geometry for docked reorder and float→dock affinity.
 *
 * Given vertical midpoints of candidate panel slots (excluding the dragged
 * panel) and a pointer/anchor Y, returns the insert index in [0, mids.length].
 */
export function computeInsertIndex(panelMids: number[], pointerY: number): number {
  if (panelMids.length === 0) return 0;
  let idx = 0;
  for (const mp of panelMids) {
    if (pointerY >= mp) idx++;
  }
  return idx;
}
