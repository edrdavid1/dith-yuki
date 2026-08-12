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
 * Default zoomMode is `'free'` (preserves continuous trackpad zoom).
 * Integer mode snaps on wheel-idle and on explicit setZoom / presets / fit.
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

  const sendViewportToBackend = useCallback((vp: ViewportState) => {
    if (debounceTimerRef.current !== null) {
      clearTimeout(debounceTimerRef.current);
    }
    debounceTimerRef.current = setTimeout(() => {
      debounceTimerRef.current = null;
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
    }, 16);
  }, []);

  // Clean up debounce timers on unmount
  useEffect(() => {
    return () => {
      if (debounceTimerRef.current !== null) {
        clearTimeout(debounceTimerRef.current);
      }
      if (integerSnapTimerRef.current !== null) {
        clearTimeout(integerSnapTimerRef.current);
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

  // ─── Zoom centered on cursor position ─────────────────────────────────

  const handleWheel = useCallback(
    (e: WheelEvent) => {
      // Continuous exponential zoom (trackpad-friendly). Discrete ×2/÷2 per
      // wheel event jumped ~200%→6000% in a single gesture.
      let dy = e.deltaY;
      if (e.deltaMode === 1) dy *= 16; // lines → px
      if (e.deltaMode === 2) dy *= 400; // pages → px
      const factor = Math.exp(-dy * 0.0012);

      setViewport((prev) => {
        const newZoom = clamp(prev.zoom * factor, ZOOM_MIN, ZOOM_MAX);
        if (newZoom === prev.zoom) return prev;

        // Keep document point under cursor stationary
        const cursorDocX = prev.panX + e.offsetX / prev.zoom;
        const cursorDocY = prev.panY + e.offsetY / prev.zoom;
        const newPanX = cursorDocX - e.offsetX / newZoom;
        const newPanY = cursorDocY - e.offsetY / newZoom;

        return constrainPan(
          { ...prev, zoom: newZoom, panX: newPanX, panY: newPanY },
          docWidth,
          docHeight,
        );
      });

      if (zoomModeRef.current === 'integer') {
        scheduleIntegerSnap(e.offsetX, e.offsetY);
      }
    },
    [docWidth, docHeight, scheduleIntegerSnap],
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
