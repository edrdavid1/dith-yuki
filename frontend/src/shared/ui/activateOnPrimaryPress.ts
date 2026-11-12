/**
 * Fire an action on the first primary press in a Tauri / WKWebView popout.
 *
 * Some controls historically used pointerdown-only (with click prevented) so a
 * first click on an inactive window still worked. In floated `flex-popout-*`
 * windows that path can miss while a normal `click` still arrives (Color Lab
 * palette rows use onClick and work on the first click).
 *
 * Bind both events and dedupe so we never run the action twice for one gesture.
 */

import type { MouseEvent as ReactMouseEvent, PointerEvent as ReactPointerEvent } from 'react';

const DEDUPE_MS = 450;

export type PrimaryPressHandlers = {
  onPointerDown: (e: ReactPointerEvent) => void;
  onClick: (e: ReactMouseEvent) => void;
};

export function activateOnPrimaryPress(action: () => void): PrimaryPressHandlers {
  let lastFireAt = 0;
  const fire = () => {
    const now = performance.now();
    if (now - lastFireAt < DEDUPE_MS) return;
    lastFireAt = now;
    action();
  };

  return {
    onPointerDown: (e) => {
      if (e.button !== 0) return;
      e.preventDefault();
      e.stopPropagation();
      fire();
    },
    onClick: (e) => {
      e.preventDefault();
      e.stopPropagation();
      fire();
    },
  };
}
