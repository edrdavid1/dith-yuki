import { useState, useCallback, useRef, useEffect } from 'react';
import { invoke } from '@tauri-apps/api/core';
import type { ViewportState } from '../components/TileCanvas';

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

// ─── Hook ─────────────────────────────────────────────────────────────────────

export interface UseViewportReturn {
  viewport: ViewportState;
  handleWheel: (e: WheelEvent) => void;
  handlePanDrag: (deltaScreenX: number, deltaScreenY: number) => void;
  fitToView: () => void;
  setZoom: (zoom: number) => void;
  setCanvasSize: (width: number, height: number) => void;
}

/**
 * Manages viewport state (zoom, pan, canvas dimensions) and communicates
 * changes to the Tauri backend via a debounced `set_viewport` IPC call.
 */
export function useViewport(docWidth: number, docHeight: number): UseViewportReturn {
  const [viewport, setViewport] = useState<ViewportState>({
    zoom: 1.0,
    panX: 0,
    panY: 0,
    canvasWidth: 0,
    canvasHeight: 0,
  });

  // ─── Debounced IPC call ───────────────────────────────────────────────

  const debounceTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);

  const sendViewportToBackend = useCallback((vp: ViewportState) => {
    if (debounceTimerRef.current !== null) {
      clearTimeout(debounceTimerRef.current);
    }
    debounceTimerRef.current = setTimeout(() => {
      debounceTimerRef.current = null;
      if (vp.canvasWidth > 0 && vp.canvasHeight > 0) {
        invoke('set_viewport', {
          zoom: vp.zoom,
          x: vp.panX,
          y: vp.panY,
          width: vp.canvasWidth,
          height: vp.canvasHeight,
        }).catch(() => {
          // Silently ignore IPC errors for viewport updates
        });
      }
    }, 16);
  }, []);

  // Clean up debounce timer on unmount
  useEffect(() => {
    return () => {
      if (debounceTimerRef.current !== null) {
        clearTimeout(debounceTimerRef.current);
      }
    };
  }, []);

  // Send viewport to backend whenever it changes
  useEffect(() => {
    sendViewportToBackend(viewport);
  }, [viewport, sendViewportToBackend]);

  // ─── Zoom centered on cursor position ─────────────────────────────────

  const handleWheel = useCallback(
    (e: WheelEvent) => {
      const factor = e.deltaY < 0 ? 2.0 : 0.5;

      setViewport(prev => {
        const newZoom = clamp(prev.zoom * factor, 0.01, 64.0);

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
    },
    [docWidth, docHeight],
  );

  // ─── Pan with middle mouse or Space+left mouse ────────────────────────

  const handlePanDrag = useCallback(
    (deltaScreenX: number, deltaScreenY: number) => {
      setViewport(prev =>
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
    setViewport(prev => {
      if (prev.canvasWidth === 0 || prev.canvasHeight === 0) return prev;
      if (docWidth === 0 || docHeight === 0) return prev;

      const fitZoom = Math.min(
        prev.canvasWidth / docWidth,
        prev.canvasHeight / docHeight,
      );
      const newZoom = clamp(fitZoom, 0.01, 64.0);

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
      setViewport(prev => {
        const clamped = clamp(newZoom, 0.01, 64.0);

        // Zoom centered on canvas center
        const centerDocX = prev.panX + (prev.canvasWidth / prev.zoom) / 2;
        const centerDocY = prev.panY + (prev.canvasHeight / prev.zoom) / 2;
        const newPanX = centerDocX - (prev.canvasWidth / clamped) / 2;
        const newPanY = centerDocY - (prev.canvasHeight / clamped) / 2;

        return constrainPan(
          { ...prev, zoom: clamped, panX: newPanX, panY: newPanY },
          docWidth,
          docHeight,
        );
      });
    },
    [docWidth, docHeight],
  );

  // ─── Set canvas size (called when container resizes) ──────────────────

  const setCanvasSize = useCallback(
    (width: number, height: number) => {
      setViewport(prev => {
        if (prev.canvasWidth === width && prev.canvasHeight === height) return prev;
        return constrainPan(
          { ...prev, canvasWidth: width, canvasHeight: height },
          docWidth,
          docHeight,
        );
      });
    },
    [docWidth, docHeight],
  );

  return { viewport, handleWheel, handlePanDrag, fitToView, setZoom, setCanvasSize };
}
