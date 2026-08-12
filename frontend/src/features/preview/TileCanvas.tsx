import { useRef, useEffect, useLayoutEffect, useCallback } from 'react';
import { listen } from '@tauri-apps/api/event';
import styles from './TileCanvas.module.css';
import { bind } from '../../shared/ui/cn';
import { snapTileDrawRect } from './zoomSnap';

const cn = bind(styles);

// ─── Types ────────────────────────────────────────────────────────────────────

export interface ViewportState {
  zoom: number;
  panX: number;
  panY: number;
  canvasWidth: number;
  canvasHeight: number;
  /** When `'integer'`, draw path uses DPR-aware origin/size snap. */
  zoomMode?: 'integer' | 'free';
}

export interface TileCanvasProps {
  docId: number;
  docWidth: number;
  docHeight: number;
  viewport: ViewportState;
  onViewportChange: (vp: ViewportState) => void;
}

interface TileReadyPayload {
  doc_id: number;
  layer_id: number;
  stage: string;
  level: number;
  x: number;
  y: number;
}

interface TileCoord {
  level: number;
  x: number;
  y: number;
}

// ─── Constants ────────────────────────────────────────────────────────────────

const TILE_SIZE = 256;

// ─── Utility functions ────────────────────────────────────────────────────────

/**
 * Compute which tiles are visible given the current viewport state.
 * Mirrors the backend logic in `compute_visible_tiles`.
 */
export function computeVisibleTiles(
  viewport: ViewportState,
  docWidth: number,
  docHeight: number,
): TileCoord[] {
  const level = computePyramidLevel(viewport.zoom, docWidth, docHeight);
  const scale = 1 << level;
  const tileSizeAtLevel = TILE_SIZE * scale;

  // Viewport bounds in document pixels
  const vpLeft = viewport.panX;
  const vpTop = viewport.panY;
  const vpRight = viewport.panX + viewport.canvasWidth / viewport.zoom;
  const vpBottom = viewport.panY + viewport.canvasHeight / viewport.zoom;

  // Convert to tile indices at this level
  const minTx = Math.max(0, Math.floor(vpLeft / tileSizeAtLevel));
  const minTy = Math.max(0, Math.floor(vpTop / tileSizeAtLevel));
  const maxTx = Math.ceil(vpRight / tileSizeAtLevel);
  const maxTy = Math.ceil(vpBottom / tileSizeAtLevel);

  // Clamp to grid bounds at this level
  const gridCols = Math.ceil(docWidth / tileSizeAtLevel);
  const gridRows = Math.ceil(docHeight / tileSizeAtLevel);

  const tiles: TileCoord[] = [];
  for (let ty = minTy; ty < Math.min(maxTy, gridRows); ty++) {
    for (let tx = minTx; tx < Math.min(maxTx, gridCols); tx++) {
      tiles.push({ level, x: tx, y: ty });
    }
  }
  return tiles;
}

/**
 * Compute the pyramid level for the given zoom factor.
 *
 * NOTE: Pyramid level rendering is disabled until the full pyramid generation
 * pipeline is integrated (level 0 composite tiles must be pre-computed before
 * higher levels can be generated). Currently always returns 0 to ensure all
 * tiles load correctly at any image size.
 */
export function computePyramidLevel(
  _zoom: number,
  _docWidth: number,
  _docHeight: number,
): number {
  // TODO: Re-enable once pyramid tile generation pipeline reliably pre-computes
  // level 0 composites before attempting higher-level generation.
  return 0;
}

/**
 * Convert a tile's grid coordinates to screen pixel position
 * based on the current viewport state.
 */
export function tileToScreen(
  x: number,
  y: number,
  level: number,
  viewport: ViewportState,
): { x: number; y: number } {
  const scale = 1 << level;
  const tileSizeAtLevel = TILE_SIZE * scale;

  // Tile position in document pixels
  const docX = x * tileSizeAtLevel;
  const docY = y * tileSizeAtLevel;

  // Convert document position to screen position
  const screenX = (docX - viewport.panX) * viewport.zoom;
  const screenY = (docY - viewport.panY) * viewport.zoom;

  return { x: screenX, y: screenY };
}

/**
 * Parse a tile key string "level/x/y" into its components.
 */
export function parseTileKey(key: string): TileCoord {
  const parts = key.split('/');
  return {
    level: parseInt(parts[0], 10),
    x: parseInt(parts[1], 10),
    y: parseInt(parts[2], 10),
  };
}

// ─── Component ────────────────────────────────────────────────────────────────

/**
 * TileCanvas renders tile-based image data onto an HTML5 <canvas>.
 * It manages a Web Worker for off-thread tile fetching/decoding and
 * listens for `tile-ready` events from the Tauri backend to refresh tiles.
 */
export default function TileCanvas({
  docId,
  docWidth,
  docHeight,
  viewport,
  onViewportChange: _onViewportChange,
}: TileCanvasProps) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const workerRef = useRef<Worker | null>(null);
  const tileMapRef = useRef<Map<string, ImageBitmap>>(new Map());
  const rafRef = useRef<number | null>(null);
  const viewportRef = useRef(viewport);
  const lastDrawnViewportRef = useRef(viewport);

  viewportRef.current = viewport;

  // ─── Draw tiles to canvas ──────────────────────────────────────────────

  const drawTiles = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const vp = viewportRef.current;
    lastDrawnViewportRef.current = vp;

    ctx.fillStyle = '#666666';
    ctx.fillRect(0, 0, canvas.width, canvas.height);
    ctx.imageSmoothingEnabled = false;

    for (const [key, bitmap] of tileMapRef.current) {
      const { x, y, level } = parseTileKey(key);
      const screenPos = tileToScreen(x, y, level, vp);
      const scale = vp.zoom * (1 << level);
      const drawSize = TILE_SIZE * scale;

      let dx: number;
      let dy: number;
      let dw: number;
      let dh: number;
      if (vp.zoomMode === 'integer') {
        // Canvas2D path (no WebGL). DPR-aware snap avoids subpixel gaps at 2×/3×.
        const dpr = typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;
        ({ dx, dy, dw, dh } = snapTileDrawRect(screenPos.x, screenPos.y, drawSize, dpr));
      } else {
        dx = Math.floor(screenPos.x);
        dy = Math.floor(screenPos.y);
        dw = Math.ceil(screenPos.x + drawSize) - dx;
        dh = Math.ceil(screenPos.y + drawSize) - dy;
      }

      if (dx + dw < 0 || dy + dh < 0 || dx > canvas.width || dy > canvas.height) continue;
      ctx.drawImage(bitmap, dx, dy, dw, dh);
    }

    // Reset CSS transform since canvas is now drawn at correct position
    if (canvas) {
      canvas.style.transform = '';
    }
  }, []);

  const scheduleRedraw = useCallback(() => {
    if (rafRef.current !== null) return;
    rafRef.current = requestAnimationFrame(() => {
      rafRef.current = null;
      drawTiles();
    });
  }, [drawTiles]);

  // ─── Handle worker messages ─────────────────────────────────────────────

  const handleWorkerMessage = useCallback(
    (e: MessageEvent) => {
      const msg = e.data;
      if (msg.type === 'tile-decoded') {
        tileMapRef.current.set(msg.key, msg.bitmap);
        scheduleRedraw();
      }
    },
    [scheduleRedraw],
  );

  // ─── Initialize Web Worker ──────────────────────────────────────────────

  useEffect(() => {
    const worker = new Worker(
      new URL('../../workers/tileWorker.ts', import.meta.url),
      { type: 'module' },
    );
    worker.onmessage = handleWorkerMessage;
    workerRef.current = worker;

    return () => {
      worker.terminate();
      workerRef.current = null;
      if (rafRef.current !== null) {
        cancelAnimationFrame(rafRef.current);
        rafRef.current = null;
      }
      for (const bitmap of tileMapRef.current.values()) {
        bitmap.close();
      }
      tileMapRef.current.clear();
    };
  }, [handleWorkerMessage]);

  // ─── Viewport change: CSS transform for instant pan, then async redraw ──

  useEffect(() => {
    if (viewport.canvasWidth === 0 || viewport.canvasHeight === 0) return;

    const canvas = canvasRef.current;
    const lastVp = lastDrawnViewportRef.current;

    // If only pan changed (same zoom), apply CSS transform for instant visual shift
    if (canvas && viewport.zoom === lastVp.zoom &&
        viewport.canvasWidth === lastVp.canvasWidth &&
        viewport.canvasHeight === lastVp.canvasHeight) {
      const dx = (lastVp.panX - viewport.panX) * viewport.zoom;
      const dy = (lastVp.panY - viewport.panY) * viewport.zoom;
      canvas.style.transform = `translate(${dx}px, ${dy}px)`;
    }

    // Schedule actual redraw (async) — when it fires it resets the transform
    scheduleRedraw();

    // Request missing tiles
    if (workerRef.current) {
      const visible = computeVisibleTiles(viewport, docWidth, docHeight);
      const needed = visible.filter(t => !tileMapRef.current.has(`${t.level}/${t.x}/${t.y}`));
      if (needed.length > 0) {
        workerRef.current.postMessage({
          type: 'request-tiles',
          tiles: needed,
          docId,
        });
      }
    }
  }, [viewport, docId, docWidth, docHeight, scheduleRedraw]);

  // ─── Listen for tile-ready events from Tauri backend ────────────────────

  useEffect(() => {
    const unlisten = listen<TileReadyPayload>('tile-ready', (event) => {
      const { level, x, y } = event.payload;
      workerRef.current?.postMessage({
        type: 'fetch-tile',
        level,
        x,
        y,
        docId,
      });
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [docId]);

  // ─── Force refetch all visible tiles after docId changes (initial load) ─

  useEffect(() => {
    if (!docId || viewport.canvasWidth === 0 || viewport.canvasHeight === 0) return;
    
    // After a short delay, force-refetch all visible tiles to catch any 
    // that were computed by the backend after our initial request
    const timer = setTimeout(() => {
      if (!workerRef.current) return;
      const visible = computeVisibleTiles(viewportRef.current, docWidth, docHeight);
      workerRef.current.postMessage({
        type: 'request-tiles',
        tiles: visible,
        docId,
      });
    }, 300);
    
    return () => clearTimeout(timer);
  }, [docId, docWidth, docHeight, viewport.canvasWidth, viewport.canvasHeight]);

  // ─── Sync canvas dimensions before paint ────────────────────────────────
  // Setting canvas.width/height clears the bitmap. Do it in useLayoutEffect
  // and redraw synchronously so the browser never paints an empty frame
  // (sidebar resize / collapse would otherwise flash gray).
  // Width/height attributes are managed here only — not via React props —
  // so React commits cannot clear the buffer after we redraw.

  useLayoutEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const w = Math.max(0, Math.round(viewport.canvasWidth));
    const h = Math.max(0, Math.round(viewport.canvasHeight));
    if (w === 0 || h === 0) return;

    if (canvas.width !== w || canvas.height !== h) {
      // Drop any pan translate before the backing store changes — a stale
      // transform + new CSS box reads as the image jumping sideways.
      canvas.style.transform = '';
      canvas.width = w;
      canvas.height = h;
      drawTiles();
    }
  }, [viewport.canvasWidth, viewport.canvasHeight, drawTiles]);

  // ─── Render ─────────────────────────────────────────────────────────────

  // Canvas CSS uses intrinsic backing-store size (not width/height: 100%).
  // Preview container is top-left anchored + overflow hidden, so a one-frame
  // size lag shows gutter instead of stretching the image.
  return (
    <canvas
      ref={canvasRef}
      className={cn('tile-canvas')}
    />
  );
}
