import { useRef, useEffect, useLayoutEffect, useCallback } from 'react';
import { listen } from '@tauri-apps/api/event';
import { onDocumentChanged } from '../../shared/ipc';
import styles from './TileCanvas.module.css';
import { bind } from '../../shared/ui/cn';
import { useShell } from '../../app/shell/ShellContext';
import {
  fillPreviewCanvasBackground,
  loadHalftoneImage,
} from './previewBackground';
import { snapCssPx } from './zoomSnap';

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
 * Matches backend `compute_pyramid_level`: max(0, floor(log2(1/zoom))),
 * clamped to floor(log2(max_dim / 256)). Zoom ≥ 1 always uses level 0.
 */
export function computePyramidLevel(
  zoom: number,
  docWidth: number,
  docHeight: number,
): number {
  if (zoom >= 1) return 0;
  const maxDim = Math.max(docWidth, docHeight);
  if (maxDim <= TILE_SIZE) return 0;
  const maxLevel = Math.floor(Math.log2(maxDim / TILE_SIZE));
  const level = Math.floor(Math.log2(1 / zoom));
  return Math.max(0, Math.min(level, maxLevel));
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

export function tileKey(t: TileCoord): string {
  return `${t.level}/${t.x}/${t.y}`;
}

/**
 * True when every visible tile that is already on screen has a buffered
 * replacement. Used so a filter change paints as one frame, not a mix of
 * old and new effect.
 */
export function shouldCommitTileRefresh(
  displayedKeys: Iterable<string>,
  pendingKeys: Iterable<string>,
  visibleKeys: string[],
): boolean {
  const displayed = new Set(displayedKeys);
  const pending = new Set(pendingKeys);
  const displayedVisible = visibleKeys.filter((k) => displayed.has(k));
  if (displayedVisible.length === 0) return false;
  return displayedVisible.every((k) => pending.has(k));
}

/** Drop worker results from a previous filter/document generation. */
export function shouldAcceptDecodedRev(
  decodedRev: number | undefined,
  currentRev: number,
): boolean {
  return (decodedRev ?? 0) >= currentRev;
}

/** Safety valve after E+C: do not wait forever for one missing tile. */
export const COMMIT_WAIT_MS = 100;

export interface TileBlit {
  sx: number;
  sy: number;
  sw: number;
  sh: number;
  dx: number;
  dy: number;
  dw: number;
  dh: number;
}

function almostInt(v: number): number | null {
  const r = Math.round(v);
  return Math.abs(v - r) < 1e-6 ? r : null;
}

/**
 * Screen blit for one pyramid tile, clipped to document bounds.
 * Dest x/y/w/h are **device pixels** (HiDPI backing store), not CSS px.
 *
 * When `zoom * dpr` is an integer, every tile is placed on one shared
 * document-pixel grid so Bayer cells stay the same size and do not overlap.
 */
export function computeTileBlit(
  x: number,
  y: number,
  level: number,
  viewport: ViewportState,
  docWidth: number,
  docHeight: number,
  dpr: number = devicePixelRatio(),
): TileBlit | null {
  const scale = 1 << level;
  const tileDocSize = TILE_SIZE * scale;
  const docX = x * tileDocSize;
  const docY = y * tileDocSize;
  if (docX >= docWidth || docY >= docHeight) return null;

  const coverW = Math.min(tileDocSize, docWidth - docX);
  const coverH = Math.min(tileDocSize, docHeight - docY);
  if (coverW <= 0 || coverH <= 0) return null;

  const sx = 0;
  const sy = 0;
  const sw = (coverW / tileDocSize) * TILE_SIZE;
  const sh = (coverH / tileDocSize) * TILE_SIZE;

  const zoom = viewport.zoom;
  const docDevice = zoom * dpr;
  const dc = almostInt(docDevice);

  let dx: number;
  let dy: number;
  let dw: number;
  let dh: number;
  if (dc !== null) {
    const ox = Math.round(-viewport.panX * docDevice);
    const oy = Math.round(-viewport.panY * docDevice);
    dx = ox + docX * dc;
    dy = oy + docY * dc;
    dw = coverW * dc;
    dh = coverH * dc;
  } else {
    const startX = (docX - viewport.panX) * docDevice;
    const startY = (docY - viewport.panY) * docDevice;
    const endX = (docX + coverW - viewport.panX) * docDevice;
    const endY = (docY + coverH - viewport.panY) * docDevice;
    dx = Math.round(startX);
    dy = Math.round(startY);
    dw = Math.round(endX) - dx;
    dh = Math.round(endY) - dy;
  }

  if (dw <= 0 || dh <= 0 || sw <= 0 || sh <= 0) return null;
  return { sx, sy, sw, sh, dx, dy, dw, dh };
}

function devicePixelRatio(): number {
  return typeof window !== 'undefined' ? window.devicePixelRatio || 1 : 1;
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
  const { previewBackground } = useShell();
  const previewBackgroundRef = useRef(previewBackground);
  previewBackgroundRef.current = previewBackground;
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const workerRef = useRef<Worker | null>(null);
  const tileMapRef = useRef<Map<string, ImageBitmap>>(new Map());
  const refreshPendingRef = useRef<Map<string, ImageBitmap>>(new Map());
  const rafRef = useRef<number | null>(null);
  const viewportRef = useRef(viewport);
  const lastDrawnViewportRef = useRef(viewport);
  const docSizeRef = useRef({ docWidth, docHeight });
  const tileRevRef = useRef(0);
  const commitTimerRef = useRef<number | null>(null);

  viewportRef.current = viewport;
  docSizeRef.current = { docWidth, docHeight };

  // ─── Draw tiles to canvas ──────────────────────────────────────────────

  const drawTiles = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const vp = viewportRef.current;
    lastDrawnViewportRef.current = vp;

    const dpr = devicePixelRatio();
    ctx.setTransform(1, 0, 0, 1, 0, 0);
    fillPreviewCanvasBackground(
      ctx,
      previewBackgroundRef.current,
      dpr,
      canvas.width,
      canvas.height
    );
    ctx.imageSmoothingEnabled = false;

    const currentLevel = computePyramidLevel(vp.zoom, docWidth, docHeight);

    // Only the active pyramid level. Drawing coarser tiles underneath
    // (nearest-neighbour, 2^L × oversized) is what made zoom-in pixels look wrong.
    for (const [key, bitmap] of tileMapRef.current) {
      const { x, y, level } = parseTileKey(key);
      if (level !== currentLevel) continue;
      const blit = computeTileBlit(x, y, level, vp, docWidth, docHeight, dpr);
      if (!blit) continue;
      if (
        blit.dx + blit.dw < 0 ||
        blit.dy + blit.dh < 0 ||
        blit.dx > canvas.width ||
        blit.dy > canvas.height
      ) {
        continue;
      }
      ctx.drawImage(
        bitmap,
        blit.sx,
        blit.sy,
        blit.sw,
        blit.sh,
        blit.dx,
        blit.dy,
        blit.dw,
        blit.dh,
      );
    }

    // Reset CSS transform since canvas is now drawn at correct position
    if (canvas) {
      canvas.style.transform = '';
    }
  }, [docWidth, docHeight]);

  const scheduleRedraw = useCallback(() => {
    if (rafRef.current !== null) return;
    rafRef.current = requestAnimationFrame(() => {
      rafRef.current = null;
      drawTiles();
    });
  }, [drawTiles]);

  useEffect(() => {
    if (previewBackground === 'pattern') {
      void loadHalftoneImage().then(() => scheduleRedraw()).catch(() => {
        scheduleRedraw();
      });
      return;
    }
    scheduleRedraw();
  }, [previewBackground, scheduleRedraw]);

  const flushPendingCommit = useCallback(() => {
    const displayed = tileMapRef.current;
    const pending = refreshPendingRef.current;
    if (pending.size === 0) return;
    for (const [k, next] of [...pending.entries()]) {
      const old = displayed.get(k);
      if (old && old !== next) old.close();
      displayed.set(k, next);
      pending.delete(k);
    }
    scheduleRedraw();
  }, [scheduleRedraw]);

  const clearCommitTimer = useCallback(() => {
    if (commitTimerRef.current != null) {
      window.clearTimeout(commitTimerRef.current);
      commitTimerRef.current = null;
    }
  }, []);

  const armCommitTimeout = useCallback(() => {
    if (commitTimerRef.current != null) return;
    commitTimerRef.current = window.setTimeout(() => {
      commitTimerRef.current = null;
      flushPendingCommit();
    }, COMMIT_WAIT_MS);
  }, [flushPendingCommit]);

  // ─── Handle worker messages ─────────────────────────────────────────────

  const handleWorkerMessage = useCallback(
    (e: MessageEvent) => {
      const msg = e.data;
      if (msg.type !== 'tile-decoded') return;

      const key = msg.key as string;
      const bitmap = msg.bitmap as ImageBitmap;
      const decodedRev = msg.rev as number | undefined;
      if (!shouldAcceptDecodedRev(decodedRev, tileRevRef.current)) {
        bitmap.close();
        return;
      }

      const displayed = tileMapRef.current;
      const pending = refreshPendingRef.current;

      // First coverage of this tile (load / pan / LOD): show immediately.
      if (!displayed.has(key)) {
        displayed.set(key, bitmap);
        scheduleRedraw();
        return;
      }

      const prevPending = pending.get(key);
      if (prevPending && prevPending !== bitmap) prevPending.close();
      pending.set(key, bitmap);
      armCommitTimeout();

      const { docWidth: w, docHeight: h } = docSizeRef.current;
      const visibleKeys = computeVisibleTiles(viewportRef.current, w, h).map(tileKey);
      if (!shouldCommitTileRefresh(displayed.keys(), pending.keys(), visibleKeys)) {
        return;
      }

      clearCommitTimer();
      flushPendingCommit();
    },
    [armCommitTimeout, clearCommitTimer, flushPendingCommit, scheduleRedraw],
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
      if (commitTimerRef.current != null) {
        window.clearTimeout(commitTimerRef.current);
        commitTimerRef.current = null;
      }
      for (const bitmap of tileMapRef.current.values()) {
        bitmap.close();
      }
      tileMapRef.current.clear();
      for (const bitmap of refreshPendingRef.current.values()) {
        bitmap.close();
      }
      refreshPendingRef.current.clear();
    };
  }, [handleWorkerMessage]);

  // ─── Viewport change: CSS transform for instant pan, then async redraw ──

  useEffect(() => {
    if (viewport.canvasWidth === 0 || viewport.canvasHeight === 0) return;

    const canvas = canvasRef.current;
    const lastVp = lastDrawnViewportRef.current;

    // Drop bitmaps from other pyramid levels when zoom crosses a LOD boundary.
    const currentLevel = computePyramidLevel(viewport.zoom, docWidth, docHeight);
    for (const [key, bitmap] of [...tileMapRef.current.entries()]) {
      if (parseTileKey(key).level !== currentLevel) {
        bitmap.close();
        tileMapRef.current.delete(key);
      }
    }
    for (const [key, bitmap] of [...refreshPendingRef.current.entries()]) {
      if (parseTileKey(key).level !== currentLevel) {
        bitmap.close();
        refreshPendingRef.current.delete(key);
      }
    }

    // If only pan changed (same zoom), apply CSS transform for instant visual shift
    if (canvas && viewport.zoom === lastVp.zoom &&
        viewport.canvasWidth === lastVp.canvasWidth &&
        viewport.canvasHeight === lastVp.canvasHeight) {
      const dpr = devicePixelRatio();
      const dx = snapCssPx((lastVp.panX - viewport.panX) * viewport.zoom, dpr);
      const dy = snapCssPx((lastVp.panY - viewport.panY) * viewport.zoom, dpr);
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
          rev: tileRevRef.current,
        });
      }
    }
  }, [viewport, docId, docWidth, docHeight, scheduleRedraw]);

  // ─── Listen for tile-ready events from Tauri backend ────────────────────

  useEffect(() => {
    const unlisten = listen<TileReadyPayload>('tile-ready', (event) => {
      const { level, x, y, stage, doc_id } = event.payload;
      if (doc_id !== docId) return;
      if (stage && stage !== 'composite') return;
      const currentLevel = computePyramidLevel(
        viewportRef.current.zoom,
        docWidth,
        docHeight,
      );
      if (level !== currentLevel) return;
      workerRef.current?.postMessage({
        type: 'fetch-tile',
        level,
        x,
        y,
        docId,
        rev: tileRevRef.current,
      });
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [docId, docWidth, docHeight]);

  useEffect(() => {
    const unlisten = onDocumentChanged(() => {
      tileRevRef.current += 1;
      if (commitTimerRef.current != null) {
        window.clearTimeout(commitTimerRef.current);
        commitTimerRef.current = null;
      }
      for (const bitmap of refreshPendingRef.current.values()) {
        bitmap.close();
      }
      refreshPendingRef.current.clear();
      if (!workerRef.current) return;
      const visible = computeVisibleTiles(
        viewportRef.current,
        docWidth,
        docHeight,
      );
      if (visible.length === 0) return;
      workerRef.current.postMessage({
        type: 'request-tiles',
        tiles: visible,
        docId,
        rev: tileRevRef.current,
      });
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [docId, docWidth, docHeight]);

  useEffect(() => {
    for (const bitmap of tileMapRef.current.values()) {
      bitmap.close();
    }
    tileMapRef.current.clear();
    for (const bitmap of refreshPendingRef.current.values()) {
      bitmap.close();
    }
    refreshPendingRef.current.clear();
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
        rev: tileRevRef.current,
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

    const dpr = devicePixelRatio();
    const bw = Math.round(w * dpr);
    const bh = Math.round(h * dpr);
    canvas.style.width = `${w}px`;
    canvas.style.height = `${h}px`;

    if (canvas.width !== bw || canvas.height !== bh) {
      // Drop any pan translate before the backing store changes — a stale
      // transform + new CSS box reads as the image jumping sideways.
      canvas.style.transform = '';
      canvas.width = bw;
      canvas.height = bh;
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
