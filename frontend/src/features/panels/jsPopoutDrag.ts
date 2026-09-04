/**
 * Pure helpers for B4c JS-driven flex-popout drag (no OS startDragging).
 * Logical screen coords — same space as dock_affinity zones (supports negative origins).
 */

export type JsDragOrigin = {
  /** Window outer top-left at mousedown (logical px). */
  winX: number;
  winY: number;
  /** Cursor screen position at mousedown (CSS/screen px ≈ logical on desktop). */
  cursorX: number;
  cursorY: number;
};

/** New window logical position for the current cursor during a drag. */
export function jsDragWindowPosition(
  origin: JsDragOrigin,
  cursorX: number,
  cursorY: number,
): { x: number; y: number } {
  return {
    x: origin.winX + (cursorX - origin.cursorX),
    y: origin.winY + (cursorY - origin.cursorY),
  };
}
