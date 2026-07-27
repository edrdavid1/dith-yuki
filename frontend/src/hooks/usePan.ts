import { useRef, useEffect, useCallback } from 'react';

// ─── Types ────────────────────────────────────────────────────────────────────

export interface UsePanOptions {
  /** Called during pan drag with the screen-space delta (pixels moved since last event). */
  onPanDrag: (deltaX: number, deltaY: number) => void;
}

export interface UsePanReturn {
  /** Attach this ref to the container element that should support panning. */
  containerRef: React.RefObject<HTMLElement | null>;
}

// ─── Hook ─────────────────────────────────────────────────────────────────────

/**
 * usePan implements pan mode activation and cursor handling for the tile canvas.
 *
 * Pan mode is activated by:
 * - Middle mouse button hold (button === 1)
 * - Space + left mouse button (Space held, then left click)
 *
 * During pan mode:
 * - Cursor changes to 'grabbing'
 * - Mouse move deltas are reported via onPanDrag callback
 * - Tiles are repositioned within one animation frame (handled by the viewport hook)
 *
 * When Space is held (but not yet dragging):
 * - Cursor changes to 'grab' to indicate readiness
 *
 * On release, the previous cursor is restored.
 *
 * This hook attaches native DOM event listeners to the container element
 * (since React synthetic events don't reliably handle middle mouse button).
 */
export function usePan({ onPanDrag }: UsePanOptions): UsePanReturn {
  const containerRef = useRef<HTMLElement | null>(null);

  // Mutable state refs to avoid re-registering listeners on every render
  const isPanningRef = useRef(false);
  const spaceHeldRef = useRef(false);
  const lastMouseRef = useRef<{ x: number; y: number }>({ x: 0, y: 0 });
  const previousCursorRef = useRef<string>('');
  const onPanDragRef = useRef(onPanDrag);

  // Keep the callback ref up to date
  useEffect(() => {
    onPanDragRef.current = onPanDrag;
  }, [onPanDrag]);

  // ─── Cursor helpers ───────────────────────────────────────────────────

  const setCursor = useCallback((cursor: string) => {
    const el = containerRef.current;
    if (el) {
      el.style.cursor = cursor;
    }
  }, []);

  const saveCursor = useCallback(() => {
    const el = containerRef.current;
    if (el) {
      previousCursorRef.current = el.style.cursor || '';
    }
  }, []);

  const restoreCursor = useCallback(() => {
    setCursor(previousCursorRef.current);
  }, [setCursor]);

  // ─── Event handlers ─────────────────────────────────────────────────────

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    function handleMouseDown(e: MouseEvent) {
      // Middle mouse button (button === 1)
      const isMiddle = e.button === 1;
      // Space + left mouse button (button === 0 while space is held)
      const isSpaceLeft = e.button === 0 && spaceHeldRef.current;

      if (isMiddle || isSpaceLeft) {
        e.preventDefault();
        isPanningRef.current = true;
        lastMouseRef.current = { x: e.clientX, y: e.clientY };
        saveCursor();
        setCursor('grabbing');
      }
    }

    function handleMouseMove(e: MouseEvent) {
      if (!isPanningRef.current) return;

      const deltaX = e.clientX - lastMouseRef.current.x;
      const deltaY = e.clientY - lastMouseRef.current.y;
      lastMouseRef.current = { x: e.clientX, y: e.clientY };

      if (deltaX !== 0 || deltaY !== 0) {
        onPanDragRef.current(deltaX, deltaY);
      }
    }

    function handleMouseUp(e: MouseEvent) {
      // Release on middle button or left button (if panning via Space+left)
      if (!isPanningRef.current) return;

      const isMiddle = e.button === 1;
      const isLeft = e.button === 0;

      if (isMiddle || isLeft) {
        isPanningRef.current = false;
        // If space is still held, show grab cursor; otherwise restore
        if (spaceHeldRef.current) {
          setCursor('grab');
        } else {
          restoreCursor();
        }
      }
    }

    function handleKeyDown(e: KeyboardEvent) {
      if (e.code === 'Space' && !e.repeat) {
        e.preventDefault();
        spaceHeldRef.current = true;
        // Show grab cursor when space is held (ready to pan)
        if (!isPanningRef.current) {
          saveCursor();
          setCursor('grab');
        }
      }
    }

    function handleKeyUp(e: KeyboardEvent) {
      if (e.code === 'Space') {
        e.preventDefault();
        spaceHeldRef.current = false;
        // If we were panning via Space+left, stop panning
        if (isPanningRef.current) {
          isPanningRef.current = false;
          restoreCursor();
        } else {
          // Just releasing space without drag — restore cursor
          restoreCursor();
        }
      }
    }

    // Prevent context menu on middle click
    function handleContextMenu(e: MouseEvent) {
      if (e.button === 1) {
        e.preventDefault();
      }
    }

    // Prevent default middle-click paste/autoscroll behavior
    function handleAuxClick(e: MouseEvent) {
      if (e.button === 1) {
        e.preventDefault();
      }
    }

    el.addEventListener('mousedown', handleMouseDown);
    el.addEventListener('mousemove', handleMouseMove);
    el.addEventListener('mouseup', handleMouseUp);
    el.addEventListener('keydown', handleKeyDown);
    el.addEventListener('keyup', handleKeyUp);
    el.addEventListener('contextmenu', handleContextMenu);
    el.addEventListener('auxclick', handleAuxClick);

    // Also listen on window for mouseup/keyup in case mouse leaves the element
    window.addEventListener('mouseup', handleMouseUp);
    window.addEventListener('keyup', handleKeyUp);

    return () => {
      el.removeEventListener('mousedown', handleMouseDown);
      el.removeEventListener('mousemove', handleMouseMove);
      el.removeEventListener('mouseup', handleMouseUp);
      el.removeEventListener('keydown', handleKeyDown);
      el.removeEventListener('keyup', handleKeyUp);
      el.removeEventListener('contextmenu', handleContextMenu);
      el.removeEventListener('auxclick', handleAuxClick);
      window.removeEventListener('mouseup', handleMouseUp);
      window.removeEventListener('keyup', handleKeyUp);

      // Reset state on cleanup
      isPanningRef.current = false;
      spaceHeldRef.current = false;
    };
  }, [saveCursor, setCursor, restoreCursor]);

  return { containerRef };
}
