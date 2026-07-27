import { useRef, useEffect, useCallback } from 'react';
import { listen } from '@tauri-apps/api/event';

// ─── Types ────────────────────────────────────────────────────────────────────

export interface ViewportState {
  zoom: number;
  panX: number;
  panY: number;
  canvasWidth: number;
  canvasHeight: number;
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
 * level = max(0, floor(log2(1.0 / zoom))), clamped to maxLevel.
 *
 * NOTE: Currently forced to 0 because pyramid tiles (level > 0) are not
 * generated during image load. The frontend always requests level 0 tiles
 * and scales them. Proper pyramid generation will be added later.
 */
export function computePyramidLevel(
  _zoom: number,
  _docWidth: number,
  _docHeight: number,
): number {
  // TODO: Re-enable pyramid levels once downsample pipeline is integrated
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

  // ─── Draw tiles to canvas ───────────────────────────────────────────────

  const drawTiles = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    ctx.clearRect(0, 0, canvas.width, canvas.height);

    for (const [key, bitmap] of tileMapRef.current) {
      const { x, y, level } = parseTileKey(key);
      const screenPos = tileToScreen(x, y, level, viewport);
      const scale = viewport.zoom * (1 << level);
      const drawSize = TILE_SIZE * scale;
      // Round positions and ceil size to prevent sub-pixel gaps between tiles
      const dx = Math.round(screenPos.x);
      const dy = Math.round(screenPos.y);
      const dw = Math.ceil(drawSize + (screenPos.x - dx));
      const dh = Math.ceil(drawSize + (screenPos.y - dy));
      ctx.drawImage(bitmap, dx, dy, dw, dh);
    }
  }, [viewport]);

  /**
   * Schedule a redraw via requestAnimationFrame, coalescing multiple
   * requests within the same frame.
   */
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
        // Store the decoded ImageBitmap and schedule a redraw
        tileMapRef.current.set(msg.key, msg.bitmap);
        scheduleRedraw();
      }
      // 'tile-pending' and 'tile-error' are handled by task 9.3 (fallback display)
    },
    [scheduleRedraw],
  );

  // ─── Initialize Web Worker on mount, terminate on unmount ───────────────

  useEffect(() => {
    const worker = new Worker(
      new URL('../workers/tileWorker.ts', import.meta.url),
      { type: 'module' },
    );
    worker.onmessage = handleWorkerMessage;
    workerRef.current = worker;

    return () => {
      worker.terminate();
      workerRef.current = null;
      // Clean up any pending animation frame
      if (rafRef.current !== null) {
        cancelAnimationFrame(rafRef.current);
        rafRef.current = null;
      }
      // Close all ImageBitmaps to free GPU memory
      for (const bitmap of tileMapRef.current.values()) {
        bitmap.close();
      }
      tileMapRef.current.clear();
    };
  }, [handleWorkerMessage]);

  // ─── When viewport changes, compute visible tiles and request them ──────

  useEffect(() => {
    if (!workerRef.current) return;
    if (viewport.canvasWidth === 0 || viewport.canvasHeight === 0) return;

    const visible = computeVisibleTiles(viewport, docWidth, docHeight);
    workerRef.current.postMessage({
      type: 'request-tiles',
      tiles: visible,
      docId,
    });

    // Trigger a redraw for any already-cached tiles at new positions
    scheduleRedraw();
  }, [viewport, docId, docWidth, docHeight, scheduleRedraw]);

  // ─── Listen for tile-ready events from Tauri backend ────────────────────

  useEffect(() => {
    const unlisten = listen<TileReadyPayload>('tile-ready', (event) => {
      const { level, x, y } = event.payload;
      // Re-fetch the updated tile via the worker
      workerRef.current?.postMessage({
        type: 'fetch-tile',
        level,
        x,
        y,
        docId,
      });
    });

    return () => {
      unlisten.then((fn) => fn());
    };
  }, [docId]);

  // ─── Sync canvas dimensions to viewport size ───────────────────────────

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;

    if (
      canvas.width !== viewport.canvasWidth ||
      canvas.height !== viewport.canvasHeight
    ) {
      canvas.width = viewport.canvasWidth;
      canvas.height = viewport.canvasHeight;
      scheduleRedraw();
    }
  }, [viewport.canvasWidth, viewport.canvasHeight, scheduleRedraw]);

  // ─── Render ─────────────────────────────────────────────────────────────

  return (
    <canvas
      ref={canvasRef}
      className="tile-canvas"
      width={viewport.canvasWidth}
      height={viewport.canvasHeight}
      style={{
        display: 'block',
        width: '100%',
        height: '100%',
      }}
    />
  );
}
