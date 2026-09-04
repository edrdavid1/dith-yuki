/**
 * B4c: move flex-popout via JS setPosition + complete redock on mouseup.
 *
 * FlexLayout portals chrome into the popout document from the main React tree.
 * Listeners and Tauri window handles must target the *popout* browsing context /
 * `flex-popout-*` WebviewWindow — not `window` / `getCurrentWindow()` (those are main).
 */

import { useCallback, useEffect, useRef } from 'react';
import { getAllWebviewWindows } from '@tauri-apps/api/webviewWindow';
import type { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { LogicalPosition } from '@tauri-apps/api/dpi';
import {
  beginFloatDrag,
  cancelFloatDrag,
  completeFloatDrag,
} from '../../shared/ipc';
import { jsDragWindowPosition, type JsDragOrigin } from './jsPopoutDrag';

type DragSession = {
  origin: JsDragOrigin | null;
  /** Popout DOM Window (mousemove/mouseup live here). */
  view: Window;
  /** Resolved Tauri handle for setPosition. */
  tauriWin: WebviewWindow | null;
  pendingCursor: { x: number; y: number } | null;
  moving: boolean;
  raf: number | null;
};

async function resolveFlexPopoutWebview(): Promise<WebviewWindow | null> {
  const wins = await getAllWebviewWindows();
  const flex = wins.filter((w) => w.label.startsWith('flex-popout-'));
  if (flex.length === 0) return null;
  for (const w of flex) {
    try {
      if (await w.isFocused()) return w;
    } catch {
      /* ignore */
    }
  }
  return flex[flex.length - 1] ?? null;
}

export function useFlexPopoutJsDrag(panelId: string) {
  const sessionRef = useRef<DragSession | null>(null);
  const listenersRef = useRef<{
    view: Window;
    move: (e: MouseEvent) => void;
    up: (e: MouseEvent) => void;
  } | null>(null);

  const removeListeners = useCallback(() => {
    const L = listenersRef.current;
    if (!L) return;
    L.view.removeEventListener('mousemove', L.move);
    L.view.removeEventListener('mouseup', L.up);
    listenersRef.current = null;
  }, []);

  const flushMove = useCallback(async (session: DragSession) => {
    session.raf = null;
    const cursor = session.pendingCursor;
    const origin = session.origin;
    const tauriWin = session.tauriWin;
    if (!cursor || !origin || !tauriWin || session.moving) return;
    session.pendingCursor = null;
    session.moving = true;
    const { x, y } = jsDragWindowPosition(origin, cursor.x, cursor.y);
    try {
      await tauriWin.setPosition(new LogicalPosition(x, y));
    } catch (err) {
      console.error('[useFlexPopoutJsDrag] setPosition failed:', err);
    } finally {
      session.moving = false;
      if (session.pendingCursor && sessionRef.current === session) {
        session.raf = requestAnimationFrame(() => {
          void flushMove(session);
        });
      }
    }
  }, []);

  const endDrag = useCallback(
    async (complete: boolean) => {
      const session = sessionRef.current;
      sessionRef.current = null;
      if (session?.raf != null) {
        cancelAnimationFrame(session.raf);
        session.raf = null;
      }
      removeListeners();

      if (complete) {
        try {
          await completeFloatDrag();
        } catch (err) {
          console.error('[useFlexPopoutJsDrag] completeFloatDrag failed:', err);
          void cancelFloatDrag();
        }
      } else {
        void cancelFloatDrag();
      }
    },
    [removeListeners],
  );

  const onTitlebarMouseDown = useCallback(
    (e: React.MouseEvent) => {
      if (e.button !== 0) return;
      const target = e.target as HTMLElement;
      if (target.closest('button, input, select, textarea')) return;
      e.preventDefault();
      e.stopPropagation();

      const view =
        (e.currentTarget as HTMLElement).ownerDocument.defaultView ?? window;
      const screenX = e.screenX;
      const screenY = e.screenY;

      // Tear down any prior session.
      removeListeners();
      if (sessionRef.current?.raf != null) {
        cancelAnimationFrame(sessionRef.current.raf);
      }

      const session: DragSession = {
        origin: null,
        view,
        tauriWin: null,
        pendingCursor: { x: screenX, y: screenY },
        moving: false,
        raf: null,
      };
      sessionRef.current = session;

      const onMove = (ev: MouseEvent) => {
        const s = sessionRef.current;
        if (!s) return;
        s.pendingCursor = { x: ev.screenX, y: ev.screenY };
        if (s.origin && s.tauriWin && s.raf == null && !s.moving) {
          s.raf = requestAnimationFrame(() => {
            void flushMove(s);
          });
        }
      };
      const onUp = () => {
        void endDrag(true);
      };

      // Attach to the *popout* window immediately (before await), or mouseup is lost.
      listenersRef.current = { view, move: onMove, up: onUp };
      view.addEventListener('mousemove', onMove);
      view.addEventListener('mouseup', onUp);

      void (async () => {
        try {
          await beginFloatDrag(panelId);
        } catch (err) {
          console.error('[useFlexPopoutJsDrag] beginFloatDrag failed:', err);
          void endDrag(false);
          return;
        }

        if (sessionRef.current !== session) return;

        const tauriWin = await resolveFlexPopoutWebview();
        if (sessionRef.current !== session) return;
        if (!tauriWin) {
          console.error('[useFlexPopoutJsDrag] no flex-popout WebviewWindow');
          void endDrag(false);
          return;
        }

        let winX = 0;
        let winY = 0;
        try {
          const scale = await tauriWin.scaleFactor();
          const pos = await tauriWin.outerPosition();
          winX = pos.x / scale;
          winY = pos.y / scale;
        } catch (err) {
          console.error('[useFlexPopoutJsDrag] outerPosition failed:', err);
          void endDrag(false);
          return;
        }

        if (sessionRef.current !== session) return;

        session.tauriWin = tauriWin;
        // Origin uses mousedown cursor so early moves stay consistent.
        session.origin = {
          winX,
          winY,
          cursorX: screenX,
          cursorY: screenY,
        };

        if (session.pendingCursor && session.raf == null && !session.moving) {
          session.raf = requestAnimationFrame(() => {
            void flushMove(session);
          });
        }
      })();
    },
    [panelId, endDrag, flushMove, removeListeners],
  );

  useEffect(() => {
    return () => {
      removeListeners();
      if (sessionRef.current?.raf != null) {
        cancelAnimationFrame(sessionRef.current.raf);
      }
      sessionRef.current = null;
      void cancelFloatDrag();
    };
  }, [removeListeners]);

  const cancelActiveDrag = useCallback(() => {
    void endDrag(false);
  }, [endDrag]);

  return { onTitlebarMouseDown, cancelActiveDrag };
}
