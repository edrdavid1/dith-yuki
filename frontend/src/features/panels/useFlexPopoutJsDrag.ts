/**
 * W1 / B4c: flex-popout drag via pointer capture + JS setPosition.
 *
 * State machine: idle → dragging → (completing | cancelled) → idle.
 * All terminators go through a single idempotent finish(reason).
 *
 * Listeners attach to the *popout* ownerDocument / capture target — not main
 * `window`. Position is always startWindow + (cursor − startCursor) so lag does
 * not accumulate.
 */

import { useCallback, useEffect, useRef } from 'react';
import type { WebviewWindow } from '@tauri-apps/api/webviewWindow';
import { LogicalPosition } from '@tauri-apps/api/dpi';
import {
  beginFloatDrag,
  bindFloatDragWindow,
  cancelFloatDrag,
  completeFloatDrag,
} from '../../shared/ipc';
import { findFlexPopoutWebviewForDomWindow } from './closeFlexPopoutShell';
import { rememberFlexPopoutLabel } from './flexPopoutRegistry';
import { jsDragWindowPosition, type JsDragOrigin } from './jsPopoutDrag';
import {
  applyFinish,
  beginDrag,
  createIdleMachine,
  HARD_CAP_MS,
  settleToIdle,
  WATCHDOG_MS,
  type FinishReason,
  type MachineState,
} from './flexPopoutDragMachine';

type DragSession = {
  machine: MachineState;
  origin: JsDragOrigin | null;
  view: Window;
  doc: Document;
  captureEl: HTMLElement | null;
  pointerId: number;
  tauriWin: WebviewWindow | null;
  windowLabel: string | null;
  pendingCursor: { x: number; y: number } | null;
  moving: boolean;
  raf: number | null;
  lastMoveAt: number;
  startedAt: number;
  watchdogTimer: ReturnType<typeof setTimeout> | null;
  hardCapTimer: ReturnType<typeof setTimeout> | null;
};

type AttachedListeners = {
  doc: Document;
  view: Window;
  captureEl: HTMLElement | null;
  pointerId: number;
  onPointerMove: (e: PointerEvent) => void;
  onPointerUp: (e: PointerEvent) => void;
  onPointerCancel: (e: PointerEvent) => void;
  onLostCapture: (e: PointerEvent) => void;
  onBlur: () => void;
  onVisibility: () => void;
  onKeyDown: (e: KeyboardEvent) => void;
};

/** Test seam: counts add/remove for listener leak checks. */
export const __flexPopoutDragListenerStats = {
  added: 0,
  removed: 0,
  reset() {
    this.added = 0;
    this.removed = 0;
  },
};

function logDock(level: 'debug' | 'warn' | 'error', msg: string, extra?: unknown) {
  const line = `[flex-dock] ${msg}`;
  if (level === 'error') console.error(line, extra ?? '');
  else if (level === 'warn') console.warn(line, extra ?? '');
  else console.debug(line, extra ?? '');
}

export function useFlexPopoutJsDrag(panelId: string) {
  const sessionRef = useRef<DragSession | null>(null);
  const listenersRef = useRef<AttachedListeners | null>(null);
  const finishRef = useRef<(reason: FinishReason) => void>(() => {});

  const clearTimers = useCallback((session: DragSession) => {
    if (session.watchdogTimer != null) {
      clearTimeout(session.watchdogTimer);
      session.watchdogTimer = null;
    }
    if (session.hardCapTimer != null) {
      clearTimeout(session.hardCapTimer);
      session.hardCapTimer = null;
    }
  }, []);

  const removeListeners = useCallback(() => {
    const L = listenersRef.current;
    if (!L) return;
    L.doc.removeEventListener('pointermove', L.onPointerMove);
    L.doc.removeEventListener('pointerup', L.onPointerUp);
    L.doc.removeEventListener('pointercancel', L.onPointerCancel);
    L.doc.removeEventListener('visibilitychange', L.onVisibility);
    L.view.removeEventListener('blur', L.onBlur);
    L.doc.removeEventListener('keydown', L.onKeyDown, true);
    __flexPopoutDragListenerStats.removed += 6;
    if (L.captureEl) {
      try {
        L.captureEl.removeEventListener('lostpointercapture', L.onLostCapture);
        __flexPopoutDragListenerStats.removed += 1;
        if (L.captureEl.hasPointerCapture?.(L.pointerId)) {
          L.captureEl.releasePointerCapture(L.pointerId);
        }
      } catch (err) {
        logDock('warn', 'releasePointerCapture failed', err);
      }
    }
    listenersRef.current = null;
  }, []);

  const flushMove = useCallback(async (session: DragSession) => {
    session.raf = null;
    if (!session.origin || !session.tauriWin || session.moving) return;
    if (session.machine.phase !== 'dragging') return;
    session.moving = true;
    try {
      while (
        sessionRef.current === session &&
        session.machine.phase === 'dragging' &&
        session.pendingCursor
      ) {
        const cursor = session.pendingCursor;
        session.pendingCursor = null;
        const { x, y } = jsDragWindowPosition(session.origin, cursor.x, cursor.y);
        try {
          await session.tauriWin.setPosition(new LogicalPosition(x, y));
        } catch (err) {
          logDock('error', 'setPosition failed', err);
          break;
        }
      }
    } finally {
      session.moving = false;
      if (
        sessionRef.current === session &&
        session.machine.phase === 'dragging' &&
        session.pendingCursor &&
        session.raf == null
      ) {
        session.raf = requestAnimationFrame(() => {
          void flushMove(session);
        });
      }
    }
  }, []);

  /** Apply the last pending cursor once (drain) before finish IPC. */
  const drainLastPosition = useCallback(async (session: DragSession) => {
    if (session.raf != null) {
      cancelAnimationFrame(session.raf);
      session.raf = null;
    }
    if (!session.origin || !session.tauriWin || !session.pendingCursor) return;
    const cursor = session.pendingCursor;
    session.pendingCursor = null;
    const { x, y } = jsDragWindowPosition(session.origin, cursor.x, cursor.y);
    try {
      await session.tauriWin.setPosition(new LogicalPosition(x, y));
    } catch (err) {
      logDock('error', 'drain setPosition failed', err);
    }
  }, []);

  const armWatchdog = useCallback(
    (session: DragSession) => {
      if (session.watchdogTimer != null) clearTimeout(session.watchdogTimer);
      session.watchdogTimer = setTimeout(() => {
        if (sessionRef.current !== session) return;
        if (session.machine.phase !== 'dragging') return;
        logDock('debug', 'watchdog fired — cancelling');
        finishRef.current('watchdog');
      }, WATCHDOG_MS);
    },
    [],
  );

  const finish = useCallback(
    (reason: FinishReason) => {
      const session = sessionRef.current;
      if (!session) return;

      const { next, didFinish, shouldComplete } = applyFinish(session.machine, reason);
      session.machine = next;
      if (!didFinish) {
        logDock('debug', `finish ignored (phase=${next.phase})`, { reason });
        return;
      }

      logDock('debug', `finish(${reason})`, {
        panelId,
        label: session.windowLabel,
        shouldComplete,
      });

      clearTimers(session);
      removeListeners();

      const label = session.windowLabel;
      void (async () => {
        await drainLastPosition(session);
        try {
          if (shouldComplete) {
            await completeFloatDrag(label);
          } else {
            await cancelFloatDrag(label);
          }
        } catch (err) {
          logDock('error', `finish IPC failed (${reason})`, err);
          try {
            await cancelFloatDrag(label);
          } catch (cancelErr) {
            logDock('error', 'cancel after finish failure also failed', cancelErr);
          }
        } finally {
          if (sessionRef.current === session) {
            session.machine = settleToIdle(session.machine);
            sessionRef.current = null;
          }
        }
      })();
    },
    [panelId, clearTimers, removeListeners, drainLastPosition],
  );

  finishRef.current = finish;

  const onTitlebarPointerDown = useCallback(
    (e: React.PointerEvent) => {
      if (e.button !== 0) return;
      const target = e.target as HTMLElement;
      if (target.closest('button, input, select, textarea')) return;
      e.preventDefault();
      e.stopPropagation();

      const captureEl = e.currentTarget as HTMLElement;
      const doc = captureEl.ownerDocument;
      const view = doc.defaultView ?? window;
      const screenX = e.screenX;
      const screenY = e.screenY;
      const pointerId = e.pointerId;

      // Tear down any prior session synchronously so the new drag owns sessionRef.
      const prior = sessionRef.current;
      if (prior && prior.machine.phase === 'dragging') {
        const applied = applyFinish(prior.machine, 'superseded');
        prior.machine = applied.next;
        clearTimers(prior);
        if (prior.raf != null) {
          cancelAnimationFrame(prior.raf);
          prior.raf = null;
        }
        removeListeners();
        logDock('debug', 'finish(superseded)', { panelId, label: prior.windowLabel });
        void cancelFloatDrag(prior.windowLabel).catch((err) => {
          logDock('error', 'cancel after supersede failed', err);
        });
        sessionRef.current = null;
      } else {
        removeListeners();
      }

      const session: DragSession = {
        machine: beginDrag(createIdleMachine()),
        origin: null,
        view,
        doc,
        captureEl,
        pointerId,
        tauriWin: null,
        windowLabel: null,
        pendingCursor: { x: screenX, y: screenY },
        moving: false,
        raf: null,
        lastMoveAt: Date.now(),
        startedAt: Date.now(),
        watchdogTimer: null,
        hardCapTimer: null,
      };
      sessionRef.current = session;

      try {
        captureEl.setPointerCapture(pointerId);
      } catch (err) {
        logDock('error', 'setPointerCapture failed', err);
        finish('start-failed');
        return;
      }

      const onPointerMove = (ev: PointerEvent) => {
        const s = sessionRef.current;
        if (!s || s.machine.phase !== 'dragging') return;
        if (ev.pointerId !== s.pointerId) return;

        // Button released outside the webview — treat as pointerup.
        if (ev.buttons === 0) {
          finish('buttons-zero');
          return;
        }

        s.pendingCursor = { x: ev.screenX, y: ev.screenY };
        s.lastMoveAt = Date.now();
        armWatchdog(s);

        if (s.origin && s.tauriWin && s.raf == null && !s.moving) {
          s.raf = requestAnimationFrame(() => {
            void flushMove(s);
          });
        }
      };

      const onPointerUp = (ev: PointerEvent) => {
        if (sessionRef.current?.pointerId !== ev.pointerId) return;
        finish('pointerup');
      };
      const onPointerCancel = (ev: PointerEvent) => {
        if (sessionRef.current?.pointerId !== ev.pointerId) return;
        finish('pointercancel');
      };
      const onLostCapture = (ev: PointerEvent) => {
        if (sessionRef.current?.pointerId !== ev.pointerId) return;
        finish('lostpointercapture');
      };
      const onBlur = () => finish('blur');
      const onVisibility = () => {
        if (doc.visibilityState === 'hidden') finish('visibilityhidden');
      };
      const onKeyDown = (ev: KeyboardEvent) => {
        if (ev.key === 'Escape') finish('escape');
      };

      listenersRef.current = {
        doc,
        view,
        captureEl,
        pointerId,
        onPointerMove,
        onPointerUp,
        onPointerCancel,
        onLostCapture,
        onBlur,
        onVisibility,
        onKeyDown,
      };

      doc.addEventListener('pointermove', onPointerMove);
      doc.addEventListener('pointerup', onPointerUp);
      doc.addEventListener('pointercancel', onPointerCancel);
      doc.addEventListener('visibilitychange', onVisibility);
      view.addEventListener('blur', onBlur);
      doc.addEventListener('keydown', onKeyDown, true);
      captureEl.addEventListener('lostpointercapture', onLostCapture);
      __flexPopoutDragListenerStats.added += 7;

      armWatchdog(session);
      session.hardCapTimer = setTimeout(() => {
        if (sessionRef.current !== session) return;
        finish('hard-cap');
      }, HARD_CAP_MS);

      void (async () => {
        const tauriWin = await findFlexPopoutWebviewForDomWindow(view, {
          panelId,
          // First float has no registry yet — one live shell is unambiguous.
          allowSoleCandidate: true,
        });
        if (sessionRef.current !== session || session.machine.phase !== 'dragging') {
          return;
        }
        if (!tauriWin) {
          logDock('error', 'no flex-popout WebviewWindow', {
            panelId,
            screenX: view.screenX,
            screenY: view.screenY,
          });
          finish('start-failed');
          return;
        }

        const windowLabel = tauriWin.label;
        rememberFlexPopoutLabel(panelId, windowLabel, view);

        try {
          await beginFloatDrag(panelId, windowLabel);
        } catch (err) {
          logDock('error', 'beginFloatDrag failed', err);
          finish('start-failed');
          return;
        }

        if (sessionRef.current !== session || session.machine.phase !== 'dragging') {
          return;
        }

        try {
          await bindFloatDragWindow(windowLabel);
        } catch (err) {
          logDock('error', 'bindFloatDragWindow failed', err);
          finish('start-failed');
          return;
        }

        if (sessionRef.current !== session || session.machine.phase !== 'dragging') {
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
          logDock('error', 'outerPosition failed', err);
          finish('start-failed');
          return;
        }

        if (sessionRef.current !== session || session.machine.phase !== 'dragging') {
          return;
        }

        session.tauriWin = tauriWin;
        session.windowLabel = windowLabel;
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
    [panelId, finish, removeListeners, flushMove, armWatchdog, clearTimers],
  );

  useEffect(() => {
    return () => {
      if (sessionRef.current?.machine.phase === 'dragging') {
        finish('unmount');
      } else {
        removeListeners();
        if (sessionRef.current?.raf != null) {
          cancelAnimationFrame(sessionRef.current.raf);
        }
        if (sessionRef.current) clearTimers(sessionRef.current);
        sessionRef.current = null;
      }
    };
  }, [finish, removeListeners, clearTimers]);

  const cancelActiveDrag = useCallback(() => {
    finish('escape');
  }, [finish]);

  return {
    onTitlebarPointerDown,
    /** @deprecated use onTitlebarPointerDown — kept for call-site migration */
    onTitlebarMouseDown: onTitlebarPointerDown as unknown as (
      e: React.MouseEvent,
    ) => void,
    cancelActiveDrag,
  };
}
