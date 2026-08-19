import { useRef, useEffect, useCallback, useState } from 'react';
import TileCanvas from '../features/preview/TileCanvas';
import type { ViewportState } from '../features/preview/TileCanvas';
import type { ZoomMode } from '../features/preview/zoomSnap';
import WindowTitlebar from '../shared/ui/WindowTitlebar';
import Icon from '../icons/iconRegistry';
import Tooltip from '../shared/ui/Tooltip';
import { useShell } from '../app/shell/ShellContext';
import { previewBackgroundStyle } from '../features/preview/previewBackground';
import styles from '../features/preview/PreviewWindow.module.css';
import previewStyles from '../features/preview/Preview.module.css';
import { bind } from '../shared/ui/cn';
const cn = bind({ ...styles, ...previewStyles });

// ─── Props ────────────────────────────────────────────────────────────────────

export interface PreviewWindowProps {
  docId: number;
  docWidth: number;
  docHeight: number;
  viewport: ViewportState;
  zoom: number;
  zoomMode: ZoomMode;
  onZoomModeChange: (mode: ZoomMode) => void;
  onZoomIn: () => void;
  onZoomOut: () => void;
  onFit: () => void;
  onSetZoom: (zoom: number) => void;
  onWheel: (e: WheelEvent) => void;
  onPanDrag: (dx: number, dy: number) => void;
  onTitleBarMouseDown?: (e: React.MouseEvent) => void;
  hideTitleBar?: boolean;
}

// ─── Component ────────────────────────────────────────────────────────────────

/**
 * PreviewWindow wraps TileCanvas with a retro Mac OS title bar ("Preview")
 * and a footer with canvas size, editable zoom %, and Fit.
 */
export default function PreviewWindow({
  docId,
  docWidth,
  docHeight,
  viewport,
  zoom,
  zoomMode,
  onZoomModeChange,
  onZoomIn,
  onZoomOut,
  onFit,
  onSetZoom,
  onWheel,
  onPanDrag,
  onTitleBarMouseDown,
  hideTitleBar,
}: PreviewWindowProps) {
  const { previewBackground } = useShell();
  const containerRef = useRef<HTMLDivElement>(null);
  const draggingRef = useRef(false);
  const lastPosRef = useRef<{ x: number; y: number }>({ x: 0, y: 0 });
  const spaceHeldRef = useRef(false);

  const zoomPercent = Math.round(zoom * 100);
  const isMinZoom = zoom <= 0.01;
  const isMaxZoom = zoom >= 64.0;

  const [editingZoom, setEditingZoom] = useState(false);
  const [zoomDraft, setZoomDraft] = useState(String(zoomPercent));

  useEffect(() => {
    if (!editingZoom) {
      setZoomDraft(String(zoomPercent));
    }
  }, [zoomPercent, editingZoom]);

  const commitZoomDraft = useCallback(() => {
    setEditingZoom(false);
    const raw = zoomDraft.trim().replace(/%/g, '');
    const parsed = Number.parseFloat(raw);
    if (!Number.isFinite(parsed)) {
      setZoomDraft(String(zoomPercent));
      return;
    }
    onSetZoom(parsed / 100);
  }, [zoomDraft, zoomPercent, onSetZoom]);

  // ─── Space key tracking (Photoshop-style hand tool) ────────────────────

  useEffect(() => {
    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.code !== 'Space' || e.repeat) return;
      const tag = (e.target as HTMLElement | null)?.tagName;
      if (tag === 'INPUT' || tag === 'TEXTAREA') return;
      spaceHeldRef.current = true;
      if (containerRef.current) {
        containerRef.current.style.cursor = 'grab';
      }
    };
    const handleKeyUp = (e: KeyboardEvent) => {
      if (e.code !== 'Space') return;
      spaceHeldRef.current = false;
      if (!draggingRef.current && containerRef.current) {
        containerRef.current.style.cursor = 'default';
      }
    };
    window.addEventListener('keydown', handleKeyDown);
    window.addEventListener('keyup', handleKeyUp);
    return () => {
      window.removeEventListener('keydown', handleKeyDown);
      window.removeEventListener('keyup', handleKeyUp);
    };
  }, []);

  // ─── Pan drag handling (Space+left or middle mouse — like Photoshop) ───

  const handleMouseDown = useCallback((e: React.MouseEvent) => {
    // Middle mouse (button 1) always pans
    // Left mouse (button 0) only pans when Space is held
    if (e.button === 1 || (e.button === 0 && spaceHeldRef.current)) {
      e.preventDefault();
      draggingRef.current = true;
      lastPosRef.current = { x: e.clientX, y: e.clientY };
      if (containerRef.current) {
        containerRef.current.style.cursor = 'grabbing';
      }
    }
  }, []);

  const handleMouseMove = useCallback((e: React.MouseEvent) => {
    if (!draggingRef.current) return;
    const dx = e.clientX - lastPosRef.current.x;
    const dy = e.clientY - lastPosRef.current.y;
    lastPosRef.current = { x: e.clientX, y: e.clientY };
    onPanDrag(dx, dy);
  }, [onPanDrag]);

  const handleMouseUp = useCallback(() => {
    draggingRef.current = false;
    if (containerRef.current) {
      containerRef.current.style.cursor = spaceHeldRef.current ? 'grab' : 'default';
    }
  }, []);

  // Clean up drag state if mouse leaves window
  useEffect(() => {
    const handleGlobalMouseUp = () => {
      draggingRef.current = false;
      if (containerRef.current) {
        containerRef.current.style.cursor = spaceHeldRef.current ? 'grab' : 'default';
      }
    };
    window.addEventListener('mouseup', handleGlobalMouseUp);
    return () => window.removeEventListener('mouseup', handleGlobalMouseUp);
  }, []);

  // ─── Wheel forwarding ─────────────────────────────────────────────────

  const handleWheel = useCallback((e: React.WheelEvent) => {
    onWheel(e.nativeEvent);
  }, [onWheel]);

  // ─── No-op viewport change handler (useViewport hook manages state) ───

  const handleViewportChange = useCallback(() => {}, []);

  return (
    <div className={cn("preview-window")} style={inlineStyles.wrapper}>
      {/* Title Bar */}
      {!hideTitleBar && (
        <WindowTitlebar
          title="Preview"
          style={inlineStyles.titlebar}
          onMouseDown={onTitleBarMouseDown}
        />
      )}

      {/* Canvas Area */}
      <div
        ref={containerRef}
        className={cn("preview-container")}
        style={{ ...inlineStyles.canvasArea, ...previewBackgroundStyle(previewBackground) }}
        tabIndex={0}
        onWheel={handleWheel}
        onMouseDown={handleMouseDown}
        onMouseMove={handleMouseMove}
        onMouseUp={handleMouseUp}
      >
        <TileCanvas
          docId={docId}
          docWidth={docWidth}
          docHeight={docHeight}
          viewport={viewport}
          onViewportChange={handleViewportChange}
        />
      </div>

      {/* Footer: resolution | zoom | fit */}
      <div className={cn('preview-footer')}>
        <span className={cn('pv-footer-resolution')} aria-label="Canvas size">
          {docWidth} × {docHeight}
        </span>
        <div className={cn('pv-footer-zoom')}>
          <Tooltip label="Zoom out">
            <button
              className={cn('pv-zoom-btn')}
              onClick={onZoomOut}
              disabled={isMinZoom}
              aria-label="Zoom out"
            >
              <Icon name="zoom-out" width={12} height={12} />
            </button>
          </Tooltip>
          <label className={cn('pv-zoom-field')}>
            <input
              className={cn('pv-zoom-input')}
              type="text"
              inputMode="decimal"
              aria-label="Zoom percent"
              value={editingZoom ? zoomDraft : `${zoomPercent}%`}
              onFocus={(e) => {
                setEditingZoom(true);
                setZoomDraft(String(zoomPercent));
                requestAnimationFrame(() => e.target.select());
              }}
              onChange={(e) => setZoomDraft(e.target.value)}
              onBlur={commitZoomDraft}
              onKeyDown={(e) => {
                if (e.key === 'Enter') {
                  e.currentTarget.blur();
                } else if (e.key === 'Escape') {
                  setZoomDraft(String(zoomPercent));
                  setEditingZoom(false);
                  e.currentTarget.blur();
                }
              }}
            />
          </label>
          <Tooltip label="Zoom in">
            <button
              className={cn('pv-zoom-btn')}
              onClick={onZoomIn}
              disabled={isMaxZoom}
              aria-label="Zoom in"
            >
              <Icon name="zoom-in" width={12} height={12} />
            </button>
          </Tooltip>
        </div>
        <div className={cn('pv-footer-actions')}>
          <Tooltip label="Actual size — 1 document pixel = 1 screen pixel (100%)">
            <button
              type="button"
              className={cn('pv-zoom-mode-btn', Math.abs(zoom - 1) < 1e-6 && 'pv-zoom-mode-btn-active')}
              aria-label="Actual size"
              aria-pressed={Math.abs(zoom - 1) < 1e-6}
              onClick={() => onSetZoom(1)}
            >
              <Icon name="zoom-1x" width={14} height={14} />
            </button>
          </Tooltip>
          <Tooltip
            label={
              zoomMode === 'integer'
                ? 'Integer zoom on — snap to 1×, 2×, 50%, 25% so pixels stay sharp'
                : 'Integer zoom — snap to 1×, 2×, 50%, 25% so pixels stay sharp'
            }
          >
            <button
              type="button"
              className={cn('pv-zoom-mode-btn', zoomMode === 'integer' && 'pv-zoom-mode-btn-active')}
              aria-label="Integer zoom"
              aria-pressed={zoomMode === 'integer'}
              onClick={() => onZoomModeChange(zoomMode === 'integer' ? 'free' : 'integer')}
            >
              <Icon name="zoom-integer" width={14} height={14} />
            </button>
          </Tooltip>
          <Tooltip label="Fit to view">
            <button
              className={cn('pv-fit-btn')}
              onClick={onFit}
              aria-label="Fit to view"
            >
              <Icon name="zoom-fit" width={14} height={14} />
            </button>
          </Tooltip>
        </div>
      </div>
    </div>
  );
}

// ─── Inline Styles (retro Mac OS aesthetic) ───────────────────────────────────

const inlineStyles: Record<string, React.CSSProperties> = {
  wrapper: {
    width: '100%',
    height: '100%',
    display: 'flex',
    flexDirection: 'column',
    background: '#cdcdcd',
  },
  titlebar: {},
  canvasArea: {
    flex: 1,
    position: 'relative',
    overflow: 'hidden',
    outline: 'none',
    cursor: 'default',
  },
};
