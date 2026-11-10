// src/components/AppTitlebar.tsx
import React, { useCallback, useEffect, useRef } from 'react';
import { getCurrentWindow } from '@tauri-apps/api/window';
import { WindowControls } from './WindowControls';
import { isMacOS } from '../lib/platform';
import { zoomWindow } from '../shared/ipc/app';

interface AppTitlebarProps {
  children?: React.ReactNode;
  title?: string;
}

/** Delay before startDragging so a double-click can cancel and zoom instead. */
const MAC_DRAG_DELAY_MS = 180;

/**
 * Main window titlebar.
 *
 * macOS Overlay CSD: do **not** use `-webkit-app-region: drag`. That path also
 * handles titlebar double-click (often as fullscreen/maximize) and races our
 * AppKit `zoom:` — expand then immediate collapse. Own both gestures in JS:
 * delayed `startDragging` + a single `zoom_window` on double-click (same as ●).
 *
 * Windows/Linux: `data-tauri-drag-region` + double-click maximize via zoom_window.
 */
export const AppTitlebar = React.memo(function AppTitlebar({ children, title }: AppTitlebarProps) {
  const mac = isMacOS();
  const dragTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const clearDragTimer = useCallback(() => {
    if (dragTimerRef.current != null) {
      clearTimeout(dragTimerRef.current);
      dragTimerRef.current = null;
    }
  }, []);

  useEffect(() => () => clearDragTimer(), [clearDragTimer]);

  const isChromeTarget = (target: EventTarget | null) => {
    const el = target as HTMLElement | null;
    return Boolean(
      el?.closest('button, input, select, a, [data-tauri-drag-region="false"]'),
    );
  };

  const handleMouseDown = useCallback(
    (e: React.MouseEvent) => {
      if (!mac || e.button !== 0) return;
      if (isChromeTarget(e.target)) return;

      // Second click of a double-click — never start a drag.
      if (e.detail > 1) {
        clearDragTimer();
        return;
      }

      clearDragTimer();
      dragTimerRef.current = setTimeout(() => {
        dragTimerRef.current = null;
        void getCurrentWindow()
          .startDragging()
          .catch((err) => {
            console.error('[AppTitlebar] startDragging failed:', err);
          });
      }, MAC_DRAG_DELAY_MS);
    },
    [mac, clearDragTimer],
  );

  const handleDoubleClick = useCallback(
    async (e: React.MouseEvent) => {
      if (isChromeTarget(e.target)) return;

      clearDragTimer();
      e.preventDefault();
      e.stopPropagation();
      try {
        await zoomWindow();
      } catch (err) {
        console.error('[AppTitlebar] double-click zoom/maximize failed:', err);
      }
    },
    [clearDragTimer],
  );

  return (
    <div
      className="app-titlebar"
      // Win/Linux: native drag region. macOS: omit — see file comment.
      {...(mac ? {} : { 'data-tauri-drag-region': true })}
      onMouseDown={handleMouseDown}
      onDoubleClick={handleDoubleClick}
    >
      {title && (
        <span
          className="app-titlebar-title"
          {...(mac ? {} : { 'data-tauri-drag-region': true })}
        >
          {title}
        </span>
      )}
      {children}
      <WindowControls />
    </div>
  );
});
