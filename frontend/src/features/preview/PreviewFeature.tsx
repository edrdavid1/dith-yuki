import { useEffect, useRef } from 'react';
import { flushSync } from 'react-dom';
import PreviewWindow from '../../components/PreviewWindow';
import EmptyState from '../../components/EmptyState';
import { useAppSelector } from '../../app/hooks';
import { useViewport } from '../../hooks/useViewport';
import { logIpcError, setViewport } from '../../shared/ipc';
import type { PanelChromeProps } from '../panels/PanelChrome';

export type PreviewFeatureProps = PanelChromeProps & {
  /** When true, hide the Preview titlebar (floating window has its own chrome). */
  hideTitleBar?: boolean;
  /** Stretch to fill floating window content area. */
  fill?: boolean;
};

/**
 * Preview with local viewport camera; IPC via shared/ipc/viewport only.
 * Document changes arrive via RTK (engine bridge) — resync viewport from doc meta.
 */
export default function PreviewFeature({
  onTitleBarMouseDown,
  hideTitleBar = false,
  fill = false,
}: PreviewFeatureProps) {
  const docId = useAppSelector((s) => s.document.docId);
  const docWidth = useAppSelector((s) => s.document.width);
  const docHeight = useAppSelector((s) => s.document.height);
  const hasDocument = useAppSelector((s) => s.document.hasDocument);

  const {
    viewport,
    zoomMode,
    setZoomMode,
    handleWheel,
    handlePanDrag,
    setCanvasSize,
    fitToView,
    setZoom,
    zoomToNextPreset,
    zoomToPrevPreset,
  } = useViewport(docWidth, docHeight);

  const containerRef = useRef<HTMLDivElement>(null);
  const viewportRef = useRef(viewport);
  viewportRef.current = viewport;
  const fittedDocKeyRef = useRef<string | null>(null);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        const { width, height } = entry.contentRect;
        if (width > 0 && height > 0) {
          // Commit size before paint so TileCanvas can rebuild its backing
          // store in useLayoutEffect (avoids a stretched/flickering frame).
          flushSync(() => {
            setCanvasSize(width, height);
          });
        }
      }
    });
    observer.observe(el);
    const rect = el.getBoundingClientRect();
    if (rect.width > 0 && rect.height > 0) {
      setCanvasSize(rect.width, rect.height);
    }
    return () => observer.disconnect();
  }, [setCanvasSize]);

  // Fit + center when a document opens (or its size changes) and the canvas
  // has a real size — avoids the default pan (0,0) top-left placement.
  useEffect(() => {
    if (!hasDocument || docId === null) {
      fittedDocKeyRef.current = null;
      return;
    }
    if (viewport.canvasWidth <= 0 || viewport.canvasHeight <= 0) return;
    if (docWidth <= 0 || docHeight <= 0) return;
    const key = `${docId}:${docWidth}x${docHeight}`;
    if (fittedDocKeyRef.current === key) return;
    fittedDocKeyRef.current = key;
    fitToView();
  }, [
    hasDocument,
    docId,
    docWidth,
    docHeight,
    viewport.canvasWidth,
    viewport.canvasHeight,
    fitToView,
  ]);

  // Floating / multi-window: re-send viewport when document meta refreshes
  // so this window's tiles get scheduled (engine bridge owns document events).
  useEffect(() => {
    if (!hasDocument || docId === null) return;
    const vp = viewportRef.current;
    if (vp.canvasWidth <= 0 || vp.canvasHeight <= 0) return;
    setViewport({
      zoom: vp.zoom,
      x: vp.panX,
      y: vp.panY,
      width: vp.canvasWidth,
      height: vp.canvasHeight,
    }).catch((err) => {
      logIpcError('PreviewFeature.setViewport', err);
    });
  }, [docId, docWidth, docHeight, hasDocument]);

  if (!hasDocument || !docId) {
    if (fill) {
      return (
        <div
          style={{
            flex: 1,
            display: 'flex',
            alignItems: 'center',
            justifyContent: 'center',
            color: '#888',
            fontFamily: 'var(--font-family)',
          }}
        >
          No document open
        </div>
      );
    }
    return <EmptyState />;
  }

  const showCanvas = viewport.canvasWidth > 0 && viewport.canvasHeight > 0;

  return (
    <div
      ref={containerRef}
      style={fill ? { flex: 1, display: 'flex', overflow: 'hidden' } : { width: '100%', height: '100%' }}
    >
      {showCanvas && (
        <PreviewWindow
          docId={docId}
          docWidth={docWidth}
          docHeight={docHeight}
          viewport={{ ...viewport, zoomMode }}
          zoom={viewport.zoom}
          zoomMode={zoomMode}
          onZoomModeChange={setZoomMode}
          onZoomIn={zoomToNextPreset}
          onZoomOut={zoomToPrevPreset}
          onFit={fitToView}
          onSetZoom={setZoom}
          onWheel={handleWheel}
          onPanDrag={handlePanDrag}
          onTitleBarMouseDown={onTitleBarMouseDown}
          hideTitleBar={hideTitleBar}
        />
      )}
    </div>
  );
}
