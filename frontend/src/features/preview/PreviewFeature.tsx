import { useEffect, useRef } from 'react';
import { registerPreviewCommands } from '../shortcuts/commandRegistry';
import { flushSync } from 'react-dom';
import PreviewWindow from '../../components/PreviewWindow';
import EmptyState from '../../components/EmptyState';
import { useAppSelector } from '../../app/hooks';
import { useShell } from '../../app/shell/ShellContext';
import { previewBackgroundStyle } from './previewBackground';
import { useViewport } from '../../hooks/useViewport';
import { logIpcError, setViewport } from '../../shared/ipc';
import type { PanelChromeProps } from '../panels/PanelChrome';
import type { WelcomeActions } from '../../hooks/useWelcomeScreen';

export type PreviewFeatureProps = PanelChromeProps & {
  /** When true, hide the Preview titlebar (floating window has its own chrome). */
  hideTitleBar?: boolean;
  /** Stretch to fill floating window content area. */
  fill?: boolean;
  /** Welcome actions for the no-document slot (main + floating preview). */
  welcome?: WelcomeActions;
};

/**
 * Preview with local viewport camera; IPC via shared/ipc/viewport only.
 * Document changes arrive via RTK (engine bridge) — resync viewport from doc meta.
 */
export default function PreviewFeature({
  onTitleBarMouseDown,
  hideTitleBar = false,
  fill = false,
  welcome,
}: PreviewFeatureProps) {
  const docId = useAppSelector((s) => s.document.docId);
  const tabsActiveId = useAppSelector((s) => s.tabs.activeId);
  const docWidth = useAppSelector((s) => s.document.width);
  const docHeight = useAppSelector((s) => s.document.height);
  const hasDocument = useAppSelector((s) => s.document.hasDocument);
  const hydrated = useAppSelector((s) => s.document.hydrated);
  const { previewBackground } = useShell();
  // Tabs stay correct even if a raced refreshDocument fails to parse snap.id.
  const effectiveDocId = docId ?? tabsActiveId;

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

  useEffect(() => {
    return registerPreviewCommands({
      zoomIn: zoomToNextPreset,
      zoomOut: zoomToPrevPreset,
      fitToView,
      actualPixels: () => setZoom(1),
    });
  }, [fitToView, setZoom, zoomToNextPreset, zoomToPrevPreset]);

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
  }, [setCanvasSize, hasDocument, effectiveDocId]);

  // Fit + center when a document opens (or its size changes) and the canvas
  // has a real size — avoids the default pan (0,0) top-left placement.
  useEffect(() => {
    if (!hasDocument || effectiveDocId === null) {
      fittedDocKeyRef.current = null;
      return;
    }
    if (viewport.canvasWidth <= 0 || viewport.canvasHeight <= 0) return;
    if (docWidth <= 0 || docHeight <= 0) return;
    const key = `${effectiveDocId}:${docWidth}x${docHeight}`;
    if (fittedDocKeyRef.current === key) return;
    fittedDocKeyRef.current = key;
    fitToView();
  }, [
    hasDocument,
    effectiveDocId,
    docWidth,
    docHeight,
    viewport.canvasWidth,
    viewport.canvasHeight,
    fitToView,
  ]);

  // Floating / multi-window: re-send viewport when document meta refreshes
  // so this window's tiles get scheduled (engine bridge owns document events).
  useEffect(() => {
    if (!hasDocument || effectiveDocId === null) return;
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
  }, [effectiveDocId, docWidth, docHeight, hasDocument]);

  const waiting = !hydrated;
  const empty = hydrated && (!hasDocument || effectiveDocId === null);
  // Mount PreviewWindow whenever a document exists — do not wait for canvas
  // size. Gating on canvasWidth caused a blank preview (bg only, no chrome)
  // when size lagged after open / tab switch; TileCanvas no-ops at 0×0.
  const showPreview = !empty && !waiting && effectiveDocId !== null;

  return (
    <div
      ref={containerRef}
      style={
        fill
          ? { flex: 1, display: 'flex', overflow: 'hidden', ...previewBackgroundStyle(previewBackground) }
          : { width: '100%', height: '100%', ...previewBackgroundStyle(previewBackground) }
      }
    >
      {waiting ? (
        <div aria-hidden style={{ flex: 1 }} />
      ) : empty ? (
        <EmptyState
          fill={fill}
          recentEntries={welcome?.recentEntries}
          onNewProject={welcome?.onNewProject}
          onOpenImage={welcome?.onOpenImage}
          onOpenProject={welcome?.onOpenProject}
          onOpenRecent={welcome?.onOpenRecent}
        />
      ) : (
        showPreview && effectiveDocId !== null && (
        <PreviewWindow
          docId={effectiveDocId}
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
        )
      )}
    </div>
  );
}
