import { useState, useCallback, useRef, useEffect } from 'react';
import type { ViewportState } from '../features/preview/TileCanvas';
import {
  nextIntegerZoom,
  prevIntegerZoom,
  snapIntegerZoom,
  snapIntegerZoomFloor,
  type ZoomMode,
  ZOOM_MAX,
  ZOOM_MIN,
} from '../features/preview/zoomSnap';
import { logIpcError, setViewport as setViewportIPC } from '../shared/ipc';
import { nextZoomPreset, prevZoomPreset } from '../types/effects';

// ─── Utility ──────────────────────────────────────────────────────────────────

function clamp(value: number, min: number, max: number): number {
  return Math.max(min, Math.min(max, value));
}

/** Convert wheel deltas to CSS pixels (WebView2 may send DOM_DELTA_LINE). */
export function normalizeWheelDelta(e: WheelEvent): { dx: number; dy: number } {
  const LINE_HEIGHT_PX = 16;
  const PAGE_HEIGHT_PX = 800;

  let scale = 1;
  if (e.deltaMode === WheelEvent.DOM_DELTA_LINE) {
    scale = LINE_HEIGHT_PX;
  } else if (e.deltaMode === WheelEvent.DOM_DELTA_PAGE) {
    scale = PAGE_HEIGHT_PX;
  }
  return { dx: e.deltaX * scale, dy: e.deltaY * scale };
}

/**
 * Cap one wheel event so WebView2 line/page spikes (or driver glitches)
 * cannot throw the viewport in a single jump. Tunable starting point.
 */
export const MAX_DELTA_PER_EVENT = 120;

export function clampWheelDelta(value: number): number {
  return Math.max(-MAX_DELTA_PER_EVENT, Math.min(MAX_DELTA_PER_EVENT, value));
}

const IPC_DEBOUNCE_MS = 16;
/** Tile refetch after trackpad gesture settles (spec §2.2). */
const WHEEL_IPC_DEBOUNCE_MS = 120;

// ─── Pan constraint ───────────────────────────────────────────────────────────

/**
 * Constrain pan so viewport center stays within 50% of viewport dimensions
 * beyond document bounds in each direction.
 */
function constrainPan(vp: ViewportState, docW: number, docH: number): ViewportState {
  const vpDocW = vp.canvasWidth / vp.zoom;
  const vpDocH = vp.canvasHeight / vp.zoom;
  const centerX = vp.panX + vpDocW / 2;
  const centerY = vp.panY + vpDocH / 2;

  const minCenterX = -vpDocW * 0.5;
  const maxCenterX = docW + vpDocW * 0.5;
  const minCenterY = -vpDocH * 0.5;
  const maxCenterY = docH + vpDocH * 0.5;

  const clampedCX = clamp(centerX, minCenterX, maxCenterX);
  const clampedCY = clamp(centerY, minCenterY, maxCenterY);

  return { ...vp, panX: clampedCX - vpDocW / 2, panY: clampedCY - vpDocH / 2 };
}

function zoomAboutCenter(prev: ViewportState, newZoom: number): ViewportState {
  const centerDocX = prev.panX + prev.canvasWidth / prev.zoom / 2;
  const centerDocY = prev.panY + prev.canvasHeight / prev.zoom / 2;
  const newPanX = centerDocX - prev.canvasWidth / newZoom / 2;
  const newPanY = centerDocY - prev.canvasHeight / newZoom / 2;
  return { ...prev, zoom: newZoom, panX: newPanX, panY: newPanY };
}

// ─── Hook ─────────────────────────────────────────────────────────────────────

const INTEGER_SNAP_IDLE_MS = 120;

export interface UseViewportReturn {
  viewport: ViewportState;
  zoomMode: ZoomMode;
  setZoomMode: (mode: ZoomMode) => void;
  handleWheel: (e: WheelEvent) => void;
  handlePanDrag: (deltaScreenX: number, deltaScreenY: number) => void;
  fitToView: () => void;
  setZoom: (zoom: number) => void;
  setCanvasSize: (width: number, height: number) => void;
  zoomToNextPreset: () => void;
  zoomToPrevPreset: () => void;
}

/**
 * Manages viewport state (zoom, pan, canvas dimensions) and communicates
 * changes to the Tauri backend via a debounced `set_viewport` IPC call.
 *
 * Default zoomMode is `'free'` (pinch / Ctrl+wheel is continuous zoom).
 * Two-finger trackpad scroll pans. Integer mode snaps on pinch-idle and
 * on explicit setZoom / presets / fit.
 */
export function useViewport(docWidth: number, docHeight: number): UseViewportReturn {
  const [viewport, setViewport] = useState<ViewportState>({
    zoom: 1.0,
    panX: 0,
    panY: 0,
    canvasWidth: 0,
    canvasHeight: 0,
  });
  const [zoomMode, setZoomModeState] = useState<ZoomMode>('free');
  const zoomModeRef = useRef<ZoomMode>('free');
  zoomModeRef.current = zoomMode;

  // ─── Debounced IPC call ───────────────────────────────────────────────

  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const integerSnapTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const lastWheelCursorRef = useRef<{ x: number; y: number } | null>(null);
  const ipcDebounceMsRef = useRef(IPC_DEBOUNCE_MS);
  const wheelRafRef = useRef<number | null>(null);
  const pendingWheelRef = useRef<{
    panDx: number;
    panDy: number;
    zoomFactor: number;
    cursorX: number;
    cursorY: number;
    wantIntegerSnap: boolean;
  } | null>(null);

  const sendViewportToBackend = useCallback((vp: ViewportState) => {
    if (debounceTimerRef.current !== null) {
      clearTimeout(debounceTimerRef.current);
    }
    const delay = ipcDebounceMsRef.current;
    debounceTimerRef.current = setTimeout(() => {
      debounceTimerRef.current = null;
      ipcDebounceMsRef.current = IPC_DEBOUNCE_MS;
      if (vp.canvasWidth > 0 && vp.canvasHeight > 0) {
        setViewportIPC({
          zoom: vp.zoom,
          x: vp.panX,
          y: vp.panY,
          width: vp.canvasWidth,
          height: vp.canvasHeight,
        }).catch((err) => {
          // Viewport is local-first; log failures without rolling back camera
          logIpcError('useViewport.setViewport', err);
        });
      }
    }, delay);
  }, []);

  // Clean up debounce / rAF on unmount
  useEffect(() => {
    return () => {
      if (debounceTimerRef.current !== null) {
        clearTimeout(debounceTimerRef.current);
      }
      if (integerSnapTimerRef.current !== null) {
        clearTimeout(integerSnapTimerRef.current);
      }
      if (wheelRafRef.current !== null) {
        cancelAnimationFrame(wheelRafRef.current);
      }
    };
  }, []);

  // Send viewport to backend whenever it changes
  useEffect(() => {
    sendViewportToBackend(viewport);
  }, [viewport, sendViewportToBackend]);

  const scheduleIntegerSnap = useCallback(
    (cursorX: number, cursorY: number) => {
      lastWheelCursorRef.current = { x: cursorX, y: cursorY };
      if (integerSnapTimerRef.current !== null) {
        clearTimeout(integerSnapTimerRef.current);
      }
      integerSnapTimerRef.current = setTimeout(() => {
        integerSnapTimerRef.current = null;
        if (zoomModeRef.current !== 'integer') return;
        const cursor = lastWheelCursorRef.current;
        setViewport((prev) => {
          const snapped = snapIntegerZoom(prev.zoom, ZOOM_MAX);
          if (Math.abs(snapped - prev.zoom) < 1e-9) return prev;
          if (cursor) {
            const cursorDocX = prev.panX + cursor.x / prev.zoom;
            const cursorDocY = prev.panY + cursor.y / prev.zoom;
            const newPanX = cursorDocX - cursor.x / snapped;
            const newPanY = cursorDocY - cursor.y / snapped;
            return constrainPan(
              { ...prev, zoom: snapped, panX: newPanX, panY: newPanY },
              docWidth,
              docHeight,
            );
          }
          return constrainPan(zoomAboutCenter(prev, snapped), docWidth, docHeight);
        });
      }, INTEGER_SNAP_IDLE_MS);
    },
    [docWidth, docHeight],
  );

  const setZoomMode = useCallback(
    (mode: ZoomMode) => {
      setZoomModeState(mode);
      zoomModeRef.current = mode;
      if (mode === 'integer') {
        // Entering integer: snap immediately
        setViewport((prev) => {
          const snapped = snapIntegerZoom(prev.zoom, ZOOM_MAX);
          if (Math.abs(snapped - prev.zoom) < 1e-9) return prev;
          return constrainPan(zoomAboutCenter(prev, snapped), docWidth, docHeight);
        });
      }
    },
    [docWidth, docHeight],
  );

  // ─── Wheel: trackpad pan, pinch / Ctrl+wheel zoom ─────────────────────
  // Coalesce to one React update per frame; defer tile IPC (~120ms) so
  // WebView2 high-rate wheel streams do not thrash the tile pipeline.

  const flushPendingWheel = useCallback(() => {
    wheelRafRef.current = null;
    const pending = pendingWheelRef.current;
    if (!pending) return;
    pendingWheelRef.current = null;

    ipcDebounceMsRef.current = WHEEL_IPC_DEBOUNCE_MS;

    const { panDx, panDy, zoomFactor, cursorX, cursorY, wantIntegerSnap } = pending;

    if (zoomFactor !== 1) {
      setViewport((prev) => {
        const newZoom = clamp(prev.zoom * zoomFactor, ZOOM_MIN, ZOOM_MAX);
        if (newZoom === prev.zoom) return prev;
        const cursorDocX = prev.panX + cursorX / prev.zoom;
        const cursorDocY = prev.panY + cursorY / prev.zoom;
        const newPanX = cursorDocX - cursorX / newZoom;
        const newPanY = cursorDocY - cursorY / newZoom;
        return constrainPan(
          { ...prev, zoom: newZoom, panX: newPanX, panY: newPanY },
          docWidth,
          docHeight,
        );
      });
      if (wantIntegerSnap) {
        scheduleIntegerSnap(cursorX, cursorY);
      }
      return;
    }

    if (panDx === 0 && panDy === 0) return;
    setViewport((prev) =>
      constrainPan(
        {
          ...prev,
          panX: prev.panX + panDx / prev.zoom,
          panY: prev.panY + panDy / prev.zoom,
        },
        docWidth,
        docHeight,
      ),
    );
  }, [docWidth, docHeight, scheduleIntegerSnap]);

  const handleWheel = useCallback(
    (e: WheelEvent) => {
      const raw = normalizeWheelDelta(e);
      let dx = clampWheelDelta(raw.dx);
      let dy = clampWheelDelta(raw.dy);

      // macOS pinch-to-zoom is delivered as wheel + ctrlKey. Mouse Ctrl+wheel
      // zooms the same way. Two-finger trackpad scroll (no ctrl) pans, like
      // Photoshop / Preview — Space+drag remains available as a hand tool.
      const isPinchZoom = e.ctrlKey;

      let pending = pendingWheelRef.current;
      if (!pending) {
        pending = {
          panDx: 0,
          panDy: 0,
          zoomFactor: 1,
          cursorX: e.offsetX,
          cursorY: e.offsetY,
          wantIntegerSnap: false,
        };
        pendingWheelRef.current = pending;
      }

      if (!isPinchZoom) {
        if (e.shiftKey && dx === 0) {
          dx = dy;
          dy = 0;
        }
        pending.panDx += dx;
        pending.panDy += dy;
      } else {
        // Continuous exponential zoom. Discrete ×2/÷2 per wheel event jumped
        // ~200%→6000% in a single gesture.
        pending.zoomFactor *= Math.exp(-dy * 0.0012);
        pending.cursorX = e.offsetX;
        pending.cursorY = e.offsetY;
        if (zoomModeRef.current === 'integer') {
          pending.wantIntegerSnap = true;
        }
      }

      if (wheelRafRef.current === null) {
        wheelRafRef.current = requestAnimationFrame(flushPendingWheel);
      }
    },
    [flushPendingWheel],
  );

  // ─── Pan with middle mouse or Space+left mouse ────────────────────────

  const handlePanDrag = useCallback(
    (deltaScreenX: number, deltaScreenY: number) => {
      setViewport((prev) =>
        constrainPan(
          {
            ...prev,
            panX: prev.panX - deltaScreenX / prev.zoom,
            panY: prev.panY - deltaScreenY / prev.zoom,
          },
          docWidth,
          docHeight,
        ),
      );
    },
    [docWidth, docHeight],
  );

  // ─── Fit entire document in view ──────────────────────────────────────

  const fitToView = useCallback(() => {
    setViewport((prev) => {
      if (prev.canvasWidth === 0 || prev.canvasHeight === 0) return prev;
      if (docWidth === 0 || docHeight === 0) return prev;

      const fitZoom = Math.min(prev.canvasWidth / docWidth, prev.canvasHeight / docHeight);
      let newZoom = clamp(fitZoom, ZOOM_MIN, ZOOM_MAX);
      if (zoomModeRef.current === 'integer') {
        newZoom = snapIntegerZoomFloor(newZoom, ZOOM_MAX);
      }

      return {
        ...prev,
        zoom: newZoom,
        panX: (docWidth - prev.canvasWidth / newZoom) / 2,
        panY: (docHeight - prev.canvasHeight / newZoom) / 2,
      };
    });
  }, [docWidth, docHeight]);

  // ─── Set exact zoom value (for zoom indicator UI) ─────────────────────

  const setZoom = useCallback(
    (newZoom: number) => {
      setViewport((prev) => {
        let clamped = clamp(newZoom, ZOOM_MIN, ZOOM_MAX);
        if (zoomModeRef.current === 'integer') {
          clamped = snapIntegerZoom(clamped, ZOOM_MAX);
        }
        return constrainPan(zoomAboutCenter(prev, clamped), docWidth, docHeight);
      });
    },
    [docWidth, docHeight],
  );

  // ─── Set canvas size (called when container resizes) ──────────────────

  const setCanvasSize = useCallback(
    (width: number, height: number) => {
      // Integer CSS pixels — avoids subpixel ResizeObserver churn that
      // repeatedly clears the canvas backing store during layout changes.
      const w = Math.max(0, Math.round(width));
      const h = Math.max(0, Math.round(height));
      setViewport((prev) => {
        if (prev.canvasWidth === w && prev.canvasHeight === h) return prev;
        return constrainPan(
          { ...prev, canvasWidth: w, canvasHeight: h },
          docWidth,
          docHeight,
        );
      });
    },
    [docWidth, docHeight],
  );

  // ─── Zoom to next/previous preset ───────────────────────────────────

  const zoomToNextPreset = useCallback(() => {
    setViewport((prev) => {
      let newZoom: number;
      if (zoomModeRef.current === 'integer') {
        newZoom = nextIntegerZoom(prev.zoom, ZOOM_MAX);
      } else {
        const currentPercent = prev.zoom * 100;
        const nextPercent = nextZoomPreset(currentPercent);
        newZoom = clamp(nextPercent / 100, ZOOM_MIN, ZOOM_MAX);
      }
      return constrainPan(zoomAboutCenter(prev, newZoom), docWidth, docHeight);
    });
  }, [docWidth, docHeight]);

  const zoomToPrevPreset = useCallback(() => {
    setViewport((prev) => {
      let newZoom: number;
      if (zoomModeRef.current === 'integer') {
        newZoom = prevIntegerZoom(prev.zoom, ZOOM_MAX);
      } else {
        const currentPercent = prev.zoom * 100;
        const prevPercent = prevZoomPreset(currentPercent);
        newZoom = clamp(prevPercent / 100, ZOOM_MIN, ZOOM_MAX);
      }
      return constrainPan(zoomAboutCenter(prev, newZoom), docWidth, docHeight);
    });
  }, [docWidth, docHeight]);

  return {
    viewport,
    zoomMode,
    setZoomMode,
    handleWheel,
    handlePanDrag,
    fitToView,
    setZoom,
    setCanvasSize,
    zoomToNextPreset,
    zoomToPrevPreset,
  };
}
