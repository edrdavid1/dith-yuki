/**
 * Color Lab–style drag-to-undock from a docked FlexLayout titlebar.
 *
 * - `sidebar`: undock when the pointer leaves the column left/right (side panels).
 * - `anyDirection`: undock after a short drag in any direction (Preview canvas).
 */

import { useCallback, useRef } from 'react';

const THRESHOLD_PX = 6;
const EXIT_SLACK_PX = 28;
/** Preview canvas undock — short drag, any direction (old PreviewSlot behavior). */
const ANY_DIRECTION_PX = 12;

export function useFlexTitlebarUndock(options: {
  /** Sidebar / FlexLayout host element. */
  columnRef: React.RefObject<HTMLElement | null>;
  onUndock: () => void;
  /**
   * `sidebar` (default): leave the column horizontally.
   * `anyDirection`: drag past a distance threshold (Preview center host).
   */
  mode?: 'sidebar' | 'anyDirection';
}) {
  const columnElRef = useRef(options.columnRef);
  columnElRef.current = options.columnRef;
  const onUndockRef = useRef(options.onUndock);
  onUndockRef.current = options.onUndock;
  const modeRef = useRef(options.mode ?? 'sidebar');
  modeRef.current = options.mode ?? 'sidebar';

  return useCallback((e: React.MouseEvent) => {
    if (e.button !== 0) return;
    const target = e.target as HTMLElement;
    // Squares / menu buttons own their clicks.
    if (target.closest('button')) return;

    const startX = e.clientX;
    const startY = e.clientY;
    let armed = false;
    let done = false;

    const cleanup = () => {
      document.removeEventListener('mousemove', onMove);
      document.removeEventListener('mouseup', onUp);
      document.removeEventListener('keydown', onKey);
    };

    const fireUndock = () => {
      if (done) return;
      done = true;
      cleanup();
      onUndockRef.current();
    };

    const onMove = (ev: MouseEvent) => {
      if (done) return;
      const dx = ev.clientX - startX;
      const dy = ev.clientY - startY;
      const dist = Math.hypot(dx, dy);

      if (modeRef.current === 'anyDirection') {
        if (dist >= ANY_DIRECTION_PX) fireUndock();
        return;
      }

      if (!armed) {
        if (dist < THRESHOLD_PX) return;
        armed = true;
      }
      const col = columnElRef.current.current;
      if (!col) return;
      const rect = col.getBoundingClientRect();
      const outside =
        ev.clientX < rect.left - EXIT_SLACK_PX ||
        ev.clientX > rect.right + EXIT_SLACK_PX;
      if (outside) fireUndock();
    };

    const onUp = () => {
      cleanup();
    };

    const onKey = (ev: KeyboardEvent) => {
      if (ev.key === 'Escape') cleanup();
    };

    document.addEventListener('mousemove', onMove);
    document.addEventListener('mouseup', onUp);
    document.addEventListener('keydown', onKey);
  }, []);
}
