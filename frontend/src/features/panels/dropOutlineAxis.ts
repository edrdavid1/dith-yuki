/**
 * Classify a FlexLayout drop outline by its painted rect vs the layout host.
 *
 * FlexLayout paints:
 * - center  → nearly the full tabset (we remap center→bottom in onAction)
 * - top/bottom → wide + short strip
 * - left/right → tall + ~half-width strip
 *
 * Mis-labeling center as "side" made the side highlight appear while the drop
 * still stacked vertically — outline showed, panel "didn't move" sideways.
 */
export type DropOutlineAxis = 'center' | 'stack' | 'side';

export function classifyDropOutlineAxis(
  outlineW: number,
  outlineH: number,
  layoutW: number,
  layoutH: number,
): DropOutlineAxis {
  if (layoutW <= 0 || layoutH <= 0) {
    return outlineW > outlineH ? 'stack' : 'side';
  }
  const wr = outlineW / layoutW;
  const hr = outlineH / layoutH;
  // Full-panel overlay = center (or edge-dock of the whole column).
  if (wr >= 0.85 && hr >= 0.85) return 'center';
  // Horizontal band = top/bottom stack.
  if (wr >= 0.8 && hr <= 0.65) return 'stack';
  // Vertical band = left/right column split.
  if (hr >= 0.8 && wr <= 0.65) return 'side';
  // Diagonal / ambiguous leftovers: prefer stack (safe; matches center→bottom).
  return outlineW >= outlineH ? 'stack' : 'side';
}
