/**
 * Color Lab–style drag-to-undock from a docked FlexLayout titlebar.
 * Horizontal exit from the sidebar column → float to OS window.
 */

import { useCallback, useRef } from 'react';

const THRESHOLD_PX = 6;
const EXIT_SLACK_PX = 28;

export function useFlexTitlebarUndock(options: {
  /** Sidebar / FlexLayout host element. */
  columnRef: React.RefObject<HTMLElement | null>;
  onUndock: () => void;
}) {
  const columnElRef = useRef(options.columnRef);
  columnElRef.current = options.columnRef;
  const onUndockRef = useRef(options.onUndock);
  onUndockRef.current = options.onUndock;

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

    const onMove = (ev: MouseEvent) => {
      if (done) return;
      const dx = ev.clientX - startX;
      const dy = ev.clientY - startY;
      if (!armed) {
        if (Math.hypot(dx, dy) < THRESHOLD_PX) return;
        armed = true;
      }
      const col = columnElRef.current.current;
      if (!col) return;
      const rect = col.getBoundingClientRect();
      const outside =
        ev.clientX < rect.left - EXIT_SLACK_PX ||
        ev.clientX > rect.right + EXIT_SLACK_PX;
      if (outside) {
        done = true;
        cleanup();
        onUndockRef.current();
      }
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
