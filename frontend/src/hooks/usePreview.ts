import { useState, useCallback, useEffect } from 'react';
import { renderPreview } from '../ipc/commands';

/**
 * Compute display dimensions that fit the image into the viewport
 * while preserving aspect ratio (fit-to-view).
 *
 * Returns dimensions where:
 * - width <= vpW and height <= vpH
 * - aspect ratio is preserved (within rounding)
 * - at least one dimension fills its axis
 */
export function computeFitToView(
  imgW: number,
  imgH: number,
  vpW: number,
  vpH: number
): { width: number; height: number } {
  if (imgW <= 0 || imgH <= 0 || vpW <= 0 || vpH <= 0) {
    return { width: 0, height: 0 };
  }

  const scaleX = vpW / imgW;
  const scaleY = vpH / imgH;
  const scale = Math.min(scaleX, scaleY);

  return {
    width: Math.round(imgW * scale),
    height: Math.round(imgH * scale),
  };
}

export function usePreview(docId: number | null) {
  const [previewSrc, setPreviewSrc] = useState<string | null>(null);
  const [isRendering, setIsRendering] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const refresh = useCallback(async () => {
    if (docId === null) return;

    console.log('[usePreview] refresh triggered for docId:', docId);
    setIsRendering(true);
    setError(null);

    try {
      const response = await renderPreview(docId);
      console.log('[usePreview] render complete, image size:', response.width, 'x', response.height);
      setPreviewSrc(`data:image/png;base64,${response.base64_png}`);
    } catch (err) {
      console.error('[usePreview] render error:', err);
      setError(typeof err === 'string' ? err : String(err));
      // Keep last successful preview on error
    } finally {
      setIsRendering(false);
    }
  }, [docId]);

  // Auto-refresh when docId changes (image loaded)
  useEffect(() => {
    if (docId !== null) {
      refresh();
    } else {
      setPreviewSrc(null);
    }
  }, [docId, refresh]);

  return {
    previewSrc,
    isRendering,
    error,
    refresh,
  };
}
