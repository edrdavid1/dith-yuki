import { useRef, useEffect, useLayoutEffect, useCallback, useState } from 'react';
import TileCanvas from '../features/preview/TileCanvas';
import type { ViewportState } from '../features/preview/TileCanvas';
import type { ZoomMode } from '../features/preview/zoomSnap';
import WindowTitlebar from '../shared/ui/WindowTitlebar';
import Icon from '../icons/iconRegistry';
import Tooltip from '../shared/ui/Tooltip';
import SoftProofControls from '../features/soft-proof/SoftProofControls';
import EdProgressIndicator from '../features/preview/EdProgressIndicator';
import { useShell } from '../app/shell/ShellContext';
import { useWorkspaceOptional } from '../features/workspace/WorkspaceContext';
import { useAppSelector } from '../app/hooks';
import { selectFiltersList } from '../app/slices/filtersSlice';
import { getAsciiPreview, setAsciiPreview } from '../shared/ipc/ascii';
import {
  getGpuComputeCapability,
  onFullDocumentBusy,
  onGpuDitherStatus,
  onGpuSelftestStatus,
} from '../shared/ipc/events';
import type {
  GpuDitherStatusPayload,
  GpuSelftestStatusPayload,
} from '../shared/ipc/events';
import { previewBackgroundStyle } from '../features/preview/previewBackground';
import {
  shouldShowRecomputingChip,
  shouldShowRenderingOverlay,
} from '../features/preview/previewBusyUi';
import styles from '../features/preview/PreviewWindow.module.css';
import previewStyles from '../features/preview/Preview.module.css';
import { bind } from '../shared/ui/cn';
import { labelWithShortcut } from '../features/shortcuts/bindings';
import { useShortcutBindings } from '../features/shortcuts/ShortcutsContext';
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
  const shortcuts = useShortcutBindings();
  const filters = useAppSelector(selectFiltersList);
  const hasAscii = filters.some((f) => f.kind === 'Ascii' && f.enabled);
  const [asciiPreview, setAsciiPreviewState] = useState(true);
  const [tileRepeat3x3, setTileRepeat3x3] = useState(false);
  const workspace = useWorkspaceOptional();
  const showSeamlessPreview = workspace?.visibility.showSeamlessPreview ?? false;
  const showGpuDitherIndicator = workspace?.gpuDitherFamilies ?? false;
  useEffect(() => {
    if (!showSeamlessPreview) setTileRepeat3x3(false);
  }, [showSeamlessPreview]);
  /** Filter / full-document recompute in flight (GPU or CPU path). */
  const [fullDocumentBusy, setFullDocumentBusy] = useState(false);
  /** After the first successful paint, keep the previous frame during recompute. */
  const [hadPreviewFrame, setHadPreviewFrame] = useState(false);
  const [gpuDitherStatus, setGpuDitherStatus] = useState<GpuDitherStatusPayload | null>(
    null,
  );
  const [gpuSelftest, setGpuSelftest] = useState<GpuSelftestStatusPayload | null>(null);
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
    if (!hasAscii) return;
    void getAsciiPreview().then(setAsciiPreviewState).catch(() => {});
  }, [hasAscii, docId]);

  useEffect(() => {
    setHadPreviewFrame(false);
    setGpuDitherStatus(null);
  }, [docId]);

  const handleFirstFrameDrawn = useCallback(() => {
    setHadPreviewFrame(true);
  }, []);

  useEffect(() => {
    let cancelled = false;
    let unlisten: (() => void) | undefined;
    void onFullDocumentBusy((event) => {
      if (cancelled) return;
      if (event.payload.doc_id !== docId) return;
      setFullDocumentBusy(event.payload.busy);
      // Keep busy:false → hadFrame as a belt-and-suspenders path; primary
      // latch is onFirstFrameDrawn (incl. retained tiles after content_changed).
      if (!event.payload.busy) {
        setHadPreviewFrame(true);
      }
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });
    return () => {
      cancelled = true;
      unlisten?.();
      setFullDocumentBusy(false);
    };
  }, [docId]);

  useEffect(() => {
    if (!showGpuDitherIndicator) {
      setGpuDitherStatus(null);
      setGpuSelftest(null);
      return;
    }
    let cancelled = false;
    let unlistenStatus: (() => void) | undefined;
    let unlistenSelftest: (() => void) | undefined;
    void onGpuDitherStatus((event) => {
      if (cancelled) return;
      if (event.payload.doc_id !== docId) return;
      setGpuDitherStatus(event.payload);
    }).then((fn) => {
      if (cancelled) fn();
      else unlistenStatus = fn;
    });
    void getGpuComputeCapability()
      .then((s) => {
        if (!cancelled) setGpuSelftest(s);
      })
      .catch(() => {});
    void onGpuSelftestStatus((event) => {
      if (cancelled) return;
      setGpuSelftest(event.payload);
    }).then((fn) => {
      if (cancelled) fn();
      else unlistenSelftest = fn;
    });
    return () => {
      cancelled = true;
      unlistenStatus?.();
      unlistenSelftest?.();
    };
  }, [docId, showGpuDitherIndicator]);

  const toggleAsciiPreview = useCallback(
    async (enabled: boolean) => {
      try {
        const next = await setAsciiPreview(enabled);
        setAsciiPreviewState(next);
      } catch {
        // IPC errors surface via engine events elsewhere
      }
    },
    []
  );

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

  useLayoutEffect(() => {
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
    // Prefer the browsing context that hosts the canvas (flex-popout when floated).
    const view = containerRef.current?.ownerDocument.defaultView ?? window;
    view.addEventListener('keydown', handleKeyDown);
    view.addEventListener('keyup', handleKeyUp);
    return () => {
      view.removeEventListener('keydown', handleKeyDown);
      view.removeEventListener('keyup', handleKeyUp);
    };
  }, [hideTitleBar]);

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

  // Clean up drag state if mouse leaves the preview surface (W5: popout owner view).
  useEffect(() => {
    const handleGlobalMouseUp = () => {
      draggingRef.current = false;
      if (containerRef.current) {
        containerRef.current.style.cursor = spaceHeldRef.current ? 'grab' : 'default';
      }
    };
    const view = containerRef.current?.ownerDocument.defaultView ?? window;
    view.addEventListener('mouseup', handleGlobalMouseUp);
    return () => view.removeEventListener('mouseup', handleGlobalMouseUp);
  }, []);

  // ─── Wheel: native + non-passive so trackpad pan can preventDefault ──

  const onWheelRef = useRef(onWheel);
  onWheelRef.current = onWheel;

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;
    const handleWheel = (e: WheelEvent) => {
      e.preventDefault();
      e.stopPropagation();
      onWheelRef.current(e);
    };
    el.addEventListener('wheel', handleWheel, { passive: false });
    return () => el.removeEventListener('wheel', handleWheel);
  }, []);

  // ─── No-op viewport change handler (useViewport hook manages state) ───

  const handleViewportChange = useCallback(() => {}, []);

  return (
    <div
      className={cn("preview-window")}
      style={inlineStyles.wrapper}
    >
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
          tileRepeat3x3={tileRepeat3x3}
          onFirstFrameDrawn={handleFirstFrameDrawn}
        />
        {shouldShowRenderingOverlay(fullDocumentBusy, hadPreviewFrame) && (
          <div className={cn('preview-loading')} role="status" aria-live="polite">
            <span className={cn('pv-rendering-label')}>Rendering…</span>
          </div>
        )}
        {shouldShowRecomputingChip(fullDocumentBusy, hadPreviewFrame) && (
          <div
            className={cn('preview-recomputing')}
            role="status"
            aria-live="polite"
            aria-label="Updating preview"
          />
        )}
      </div>

      {/* Footer: left meta | centered zoom | right progress + soft proof + icons */}
      <div className={cn('preview-footer')}>
        <div className={cn('pv-footer-left')}>
          <span className={cn('pv-footer-cell', 'pv-footer-size')} aria-label="Canvas size">
            {docWidth} x {docHeight}
          </span>
          {showGpuDitherIndicator &&
            (() => {
              const st = gpuSelftest?.state;
              let pathLabel = 'CPU';
              let msLabel: string | null = null;
              if (st === 'unknown') pathLabel = 'GPU…';
              else if (st === 'fail' || st === 'lost') pathLabel = 'CPU';
              else if (gpuDitherStatus) {
                pathLabel = gpuDitherStatus.path === 'CPU' ? 'CPU' : 'GPU';
                msLabel = `${gpuDitherStatus.elapsed_ms} ms`;
              } else if (st === 'pass') pathLabel = 'GPU';
              else if (st === 'unavailable') pathLabel = 'CPU';

              const gpuTitle =
                gpuSelftest?.adapter
                  ? `adapter=${gpuSelftest.adapter}${gpuSelftest.reason ? ` · ${gpuSelftest.reason}` : ''}`
                  : gpuDitherStatus?.path === 'CPU'
                    ? `CPU preview refresh (slider → tiles fresh)${
                        gpuDitherStatus.width && gpuDitherStatus.height
                          ? ` · ${gpuDitherStatus.width}×${gpuDitherStatus.height} · ps=${gpuDitherStatus.pixel_size ?? 1}`
                          : ''
                      }`
                    : gpuDitherStatus?.width && gpuDitherStatus?.height
                      ? `${gpuDitherStatus.width}×${gpuDitherStatus.height} · ps=${gpuDitherStatus.pixel_size ?? 1} · ${gpuDitherStatus.strips} strip(s)`
                      : gpuDitherStatus?.backend
                        ? `backend=${gpuDitherStatus.backend}`
                        : gpuSelftest?.backend
                          ? `backend=${gpuSelftest.backend}`
                          : undefined;

              return (
                <>
                  <span
                    className={cn('pv-footer-cell', 'pv-gpu-dither')}
                    title={gpuTitle}
                    aria-label="Dither path"
                  >
                    {pathLabel}
                  </span>
                  {msLabel != null && (
                    <span
                      className={cn('pv-footer-cell', 'pv-gpu-ms')}
                      title={gpuTitle}
                      aria-label="Dither timing"
                    >
                      {msLabel}
                    </span>
                  )}
                </>
              );
            })()}
          {hasAscii && (
            <span className={cn('pv-ascii-mode')} role="group" aria-label="Preview mode">
              <Tooltip label="Show image without ASCII">
                <button
                  type="button"
                  className={cn('pv-ascii-mode-btn', !asciiPreview && 'pv-ascii-mode-btn-active')}
                  aria-pressed={!asciiPreview}
                  onClick={() => void toggleAsciiPreview(false)}
                >
                  Image
                </button>
              </Tooltip>
              <Tooltip label="Show ASCII text-art preview">
                <button
                  type="button"
                  className={cn('pv-ascii-mode-btn', asciiPreview && 'pv-ascii-mode-btn-active')}
                  aria-pressed={asciiPreview}
                  onClick={() => void toggleAsciiPreview(true)}
                >
                  ASCII
                </button>
              </Tooltip>
            </span>
          )}
          {showSeamlessPreview && (
            <Tooltip label="Repeat preview in a 3×3 grid to inspect tile seams">
              <button
                type="button"
                className={cn('pv-ascii-mode-btn', tileRepeat3x3 && 'pv-ascii-mode-btn-active')}
                aria-pressed={tileRepeat3x3}
                onClick={() => setTileRepeat3x3((v) => !v)}
              >
                3×3
              </button>
            </Tooltip>
          )}
        </div>
        <div className={cn('pv-footer-zoom')}>
          <Tooltip label={labelWithShortcut('Zoom out', shortcuts.zoomOut)}>
            <button
              className={cn('pv-zoom-btn')}
              onClick={onZoomOut}
              disabled={isMinZoom}
              aria-label={labelWithShortcut('Zoom out', shortcuts.zoomOut)}
            >
              −
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
          <Tooltip label={labelWithShortcut('Zoom in', shortcuts.zoomIn)}>
            <button
              className={cn('pv-zoom-btn')}
              onClick={onZoomIn}
              disabled={isMaxZoom}
              aria-label={labelWithShortcut('Zoom in', shortcuts.zoomIn)}
            >
              +
            </button>
          </Tooltip>
        </div>
        <div className={cn('pv-footer-right')}>
          <div className={cn('pv-footer-progress')}>
            <EdProgressIndicator docId={docId} busy={fullDocumentBusy} />
          </div>
          <SoftProofControls docId={docId} layout="footer" />
          <div className={cn('pv-footer-icons')} role="group" aria-label="Zoom tools">
            <Tooltip
              label={labelWithShortcut(
                'Actual size — 1 document pixel = 1 screen pixel (100%)',
                shortcuts.zoomActual
              )}
            >
              <button
                type="button"
                className={cn('pv-zoom-mode-btn', Math.abs(zoom - 1) < 1e-6 && 'pv-zoom-mode-btn-active')}
                aria-label={labelWithShortcut('Actual size', shortcuts.zoomActual)}
                aria-pressed={Math.abs(zoom - 1) < 1e-6}
                onClick={() => onSetZoom(1)}
              >
                <Icon name="zoom-1x" width={12} height={12} />
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
                <Icon name="zoom-integer" width={12} height={12} />
              </button>
            </Tooltip>
            <Tooltip label={labelWithShortcut('Fit to view', shortcuts.zoomFit)}>
              <button
                className={cn('pv-fit-btn')}
                onClick={onFit}
                aria-label={labelWithShortcut('Fit to view', shortcuts.zoomFit)}
              >
                <Icon name="zoom-fit" width={12} height={12} />
              </button>
            </Tooltip>
          </div>
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
