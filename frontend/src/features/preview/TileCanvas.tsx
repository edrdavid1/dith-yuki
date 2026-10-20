import { useRef, useEffect, useLayoutEffect, useCallback } from 'react';
import { listen } from '@tauri-apps/api/event';
import { onDocumentChanged } from '../../shared/ipc';
import { useAppSelector } from '../../app/hooks';
import styles from './TileCanvas.module.css';
import { bind } from '../../shared/ui/cn';
import { useShell } from '../../app/shell/ShellContext';
import {
  fillPreviewCanvasBackground,
  loadHalftoneImage,
} from './previewBackground';
import { snapCssPx } from './zoomSnap';
import { findFallbackTile } from '../../utils/tileFallback';
import { getPlatform } from '../../lib/platform';
import { tileProtocolBase } from '../../lib/tileProtocol';
import {
  noteCommitTimeout,
  noteCssPan,
  noteDisplayCurrent,
  noteDisplayPost,
  noteFramePainted,
  noteTileDecoded,
  noteUserInput,
  stampWorkerMessage,
} from '../../diagnostics/level1';
import {
  activeProfileRunId,
  ditherProfileEnabled,
  noteFeMark,
  probeDitherProfile,
} from '../../diagnostics/ditherProfile';
import {
  noteTraceEvent,
  probeTileGenTrace,
  registerTraceCanvasHooks,
  tileGenTraceEnabled,
} from '../../diagnostics/tileGenTrace';
import {
  recordDrawn,
  recordDropped,
  recordFetchPosted,
  recordHttpStatus,
  recordTileReady,
  takeInjectDrop,
} from '../../diagnostics/tileGenCanvasRegistry';
import { tileRangeFromCoords } from '../../diagnostics/vpHash';
import { decideEventDrivenWake } from './eventDrivenWake';
import {
  EVENT_DRIVEN_PENDING_REASONS,
  PENDING_SAFETY_RETRY_MS,
  setEdProgress,
} from '../../diagnostics/edProgress';

const cn = bind(styles);

/** Platform-aware tile origin for the worker (WebView2 vs WKWebView). */
function workerTileProtocolBase(): string {
  return tileProtocolBase(getPlatform());
}

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
  /** Draw the document repeated in a 3×3 grid to inspect tile seams. */
  tileRepeat3x3?: boolean;
}

interface TileReadyPayload {
  doc_id: number;
  layer_id: number;
  stage: string;
  level: number;
  x: number;
  y: number;
}

interface EdProgressPayload {
  level: number;
  done: number;
  total: number;
  doc_gen: number;
  source_id: string;
  doc_id: number;
  parallel_ed_passes: number;
}

type EventDrivenPending = {
  reason: string;
  rev: number;
  /** True while a wake fetch for this key is outstanding (≤1 in-flight). */
  inflight: boolean;
  /** performance.now() of last safety_timer wake for this key. */
  lastSafetyAt: number;
};

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

/** Visible tiles when previewing a 3×3 wrap (Phase 3.1). */
export function computeVisibleTilesRepeat3x3(
  viewport: ViewportState,
  docWidth: number,
  docHeight: number,
): TileCoord[] {
  const seen = new Set<string>();
  const out: TileCoord[] = [];
  for (let oy = -1; oy <= 1; oy++) {
    for (let ox = -1; ox <= 1; ox++) {
      const shifted: ViewportState = {
        ...viewport,
        panX: viewport.panX - ox * docWidth,
        panY: viewport.panY - oy * docHeight,
      };
      for (const t of computeVisibleTiles(shifted, docWidth, docHeight)) {
        const k = `${t.level}:${t.x}:${t.y}`;
        if (seen.has(k)) continue;
        seen.add(k);
        out.push(t);
      }
    }
  }
  return out;
}

function visibleTilesForPreview(
  viewport: ViewportState,
  docWidth: number,
  docHeight: number,
  repeat3x3: boolean,
): TileCoord[] {
  return repeat3x3
    ? computeVisibleTilesRepeat3x3(viewport, docWidth, docHeight)
    : computeVisibleTiles(viewport, docWidth, docHeight);
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

export function maxPyramidLevel(docWidth: number, docHeight: number): number {
  if (Math.max(docWidth, docHeight) <= TILE_SIZE) return 0;
  return computePyramidLevel(Number.MIN_VALUE, docWidth, docHeight);
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

/** Full document replace / restore / tab switch — drop stale bitmaps. */
export const DOCUMENT_SOURCE_REPLACE_KINDS = new Set([
  'image_loaded',
  'document_created',
  'project_opened',
  'document_undone',
  'document_redone',
  'document_activated',
  'document_closed',
  // soft_proof_changed: keep old bitmaps until new rev arrives (no flash).
]);

export function isDocumentSourceReplace(kind: string): boolean {
  return DOCUMENT_SOURCE_REPLACE_KINDS.has(kind);
}

/** On source replace, refetch every visible tile even if a bitmap is already cached. */
export function tilesToRequestAfterDocumentChange<T extends TileCoord>(
  visible: T[],
  displayedKeys: Iterable<string>,
  sourceReplace: boolean,
): T[] {
  if (sourceReplace) return visible;
  const displayed = displayedKeys instanceof Set ? displayedKeys : new Set(displayedKeys);
  return visible.filter((t) => !displayed.has(tileKey(t)));
}

/** True when two pyramid tiles cover overlapping document pixels. */
export function tilesCoverSameDocumentArea(a: TileCoord, b: TileCoord): boolean {
  if (a.level === b.level) return a.x === b.x && a.y === b.y;
  const lo = a.level < b.level ? a : b;
  const hi = a.level < b.level ? b : a;
  const shift = hi.level - lo.level;
  return (lo.x >> shift) === hi.x && (lo.y >> shift) === hi.y;
}

/**
 * Keep other-LOD bitmaps while the current level still has holes.
 * Dropping them on the zoom boundary is what flashed the preview gray.
 */
export function shouldKeepLodTile(
  tile: TileCoord,
  currentLevel: number,
  visibleCurrent: TileCoord[],
  displayedCurrentKeys: Iterable<string>,
): boolean {
  if (tile.level === currentLevel) return true;
  const displayed =
    displayedCurrentKeys instanceof Set
      ? displayedCurrentKeys
      : new Set(displayedCurrentKeys);
  const missing = visibleCurrent.filter((t) => !displayed.has(tileKey(t)));
  if (missing.length === 0) return false;
  return missing.some((v) => tilesCoverSameDocumentArea(tile, v));
}

/** Coarser tiles that cover holes — fetch so pyramid fallback can blit. */
export function coveringAncestorTiles(
  missing: TileCoord[],
  maxLevel: number,
): TileCoord[] {
  const extra: TileCoord[] = [];
  const seen = new Set<string>();
  for (const t of missing) {
    if (t.level >= maxLevel) continue;
    const parent = { level: t.level + 1, x: t.x >> 1, y: t.y >> 1 };
    const k = tileKey(parent);
    if (seen.has(k)) continue;
    seen.add(k);
    extra.push(parent);
  }
  return extra;
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
  tileRepeat3x3 = false,
}: TileCanvasProps) {
  const { previewBackground } = useShell();
  const documentEpoch = useAppSelector((s) => s.document.documentEpoch);
  const previewBackgroundRef = useRef(previewBackground);
  previewBackgroundRef.current = previewBackground;
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const workerRef = useRef<Worker | null>(null);
  const tileMapRef = useRef<Map<string, ImageBitmap>>(new Map());
  /** Trace-only: generation of the bitmap currently displayed / pending. */
  const tileGenMapRef = useRef<Map<string, number>>(new Map());
  /** Trace-only: how the displayed bitmap was obtained (fresh/stale/pyramid). */
  const tileSourceMapRef = useRef<Map<string, string>>(new Map());
  const refreshPendingRef = useRef<Map<string, ImageBitmap>>(new Map());
  const rafRef = useRef<number | null>(null);
  const viewportRef = useRef(viewport);
  const lastDrawnViewportRef = useRef(viewport);
  const docSizeRef = useRef({ docWidth, docHeight });
  const tileRevRef = useRef(0);
  /**
   * Tiles that returned 202 with ed_prefix / waiting_children / computing.
   * Re-fetched on tile-ready, ed-progress, and a 1–2s safety timer; cancelled
   * when leaving the visible set or when tileRev bumps.
   */
  const eventDrivenPendingRef = useRef<Map<string, EventDrivenPending>>(new Map());
  /** Soft-proof latency: click→first/last visible tile (p50/p95 via console samples). */
  const softProofMetricRef = useRef<{
    rev: number;
    t0: number;
    expected: Set<string>;
    firstMs: number | null;
    samples: number[];
  } | null>(null);
  const commitTimerRef = useRef<number | null>(null);
  const handleWorkerMessageRef = useRef<(e: MessageEvent) => void>(() => {});

  viewportRef.current = viewport;
  docSizeRef.current = { docWidth, docHeight };

  const clearEventDrivenPending = useCallback(() => {
    eventDrivenPendingRef.current.clear();
  }, []);

  const pruneEventDrivenPending = useCallback((visibleKeys: Set<string>) => {
    const map = eventDrivenPendingRef.current;
    for (const key of [...map.keys()]) {
      if (!visibleKeys.has(key)) map.delete(key);
    }
  }, []);

  const refetchEventDrivenPending = useCallback(
    (reason: string) => {
      const worker = workerRef.current;
      if (!worker) return;
      const rev = tileRevRef.current;
      const map = eventDrivenPendingRef.current;
      if (map.size === 0) return;
      const vp = viewportRef.current;
      const { docWidth: w, docHeight: h } = docSizeRef.current;
      const visible = new Set(
        visibleTilesForPreview(vp, w, h, tileRepeatRef.current).map(tileKey),
      );
      const now = performance.now();
      for (const [key, entry] of [...map.entries()]) {
        if (entry.rev !== rev) {
          map.delete(key);
          continue;
        }
        if (!visible.has(key)) {
          map.delete(key);
          continue;
        }
        // At most one in-flight wake fetch per tile.
        if (entry.inflight) {
          if (tileGenTraceEnabled()) {
            const [level, x, y] = key.split('/').map(Number);
            noteTraceEvent('wake_skipped_inflight', {
              level,
              x,
              y,
              details: {
                cause: reason,
                pending_reason: entry.reason,
                tileRev: rev,
              },
            });
          }
          continue;
        }
        // Safety timer: at most one fire per tile per PENDING_SAFETY_RETRY_MS window.
        if (reason === 'safety_timer') {
          if (now - entry.lastSafetyAt < PENDING_SAFETY_RETRY_MS) continue;
          entry.lastSafetyAt = now;
        }
        entry.inflight = true;
        map.set(key, entry);
        const [level, x, y] = key.split('/').map(Number);
        if (tileGenTraceEnabled()) {
          noteTraceEvent('fetch_tile_posted', {
            level,
            x,
            y,
            details: {
              tileRev: rev,
              set: 'visible',
              via: 'event_driven_pending',
              wake: reason,
              pending_reason: entry.reason,
              coalesced: true,
            },
          });
          noteTraceEvent('tile_requested', {
            level,
            x,
            y,
            details: {
              tileRev: rev,
              set: 'visible',
              via: 'event_driven_pending',
              wake: reason,
            },
          });
          recordFetchPosted(key);
        }
        worker.postMessage(
          stampWorkerMessage({
            type: 'fetch-tile',
            level,
            x,
            y,
            docId,
            rev,
            protocolBase: workerTileProtocolBase(),
            profile: ditherProfileEnabled() || undefined,
            profileRunId: activeProfileRunId() ?? undefined,
            trace: tileGenTraceEnabled() || undefined,
          }),
        );
      }
    },
    [docId],
  );

  // ─── Draw tiles to canvas ──────────────────────────────────────────────

  const tileRepeatRef = useRef(tileRepeat3x3);
  tileRepeatRef.current = tileRepeat3x3;

  const drawTiles = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const ctx = canvas.getContext('2d');
    if (!ctx) return;

    const vp = viewportRef.current;
    lastDrawnViewportRef.current = vp;
    const repeat = tileRepeatRef.current;

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
    const visible = visibleTilesForPreview(vp, docWidth, docHeight, repeat);
    const haveCurrent = new Set(
      [...tileMapRef.current.keys()].filter(
        (k) => parseTileKey(k).level === currentLevel,
      ),
    );

    const docDevice = vp.zoom * dpr;
    const dc = almostInt(docDevice);
    const stepX = (dc !== null ? docWidth * dc : docWidth * docDevice);
    const stepY = (dc !== null ? docHeight * dc : docHeight * docDevice);

    const blitOne = (x: number, y: number, level: number, bitmap: ImageBitmap) => {
      const blit = computeTileBlit(x, y, level, vp, docWidth, docHeight, dpr);
      if (!blit) return;
      const offsets = repeat
        ? ([-1, 0, 1] as const).flatMap((ox) =>
            ([-1, 0, 1] as const).map((oy) => [ox, oy] as const)
          )
        : ([[0, 0]] as const);
      for (const [ox, oy] of offsets) {
        const dx = blit.dx + ox * stepX;
        const dy = blit.dy + oy * stepY;
        if (
          dx + blit.dw < 0 ||
          dy + blit.dh < 0 ||
          dx > canvas.width ||
          dy > canvas.height
        ) {
          continue;
        }
        ctx.drawImage(
          bitmap,
          blit.sx,
          blit.sy,
          blit.sw,
          blit.sh,
          dx,
          dy,
          blit.dw,
          blit.dh,
        );
        if (tileGenTraceEnabled() && ox === 0 && oy === 0) {
          const k = `${level}/${x}/${y}`;
          const gen = tileGenMapRef.current.get(k) ?? null;
          const source = tileSourceMapRef.current.get(k) ?? 'unknown';
          recordDrawn(k, gen);
          noteTraceEvent('tile_drawn', {
            level,
            x,
            y,
            gen: gen ?? undefined,
            details: { source, tileRev: tileRevRef.current },
          });
        }
      }
    };

    // Other LOD first (holes), then the active level on top.
    for (const [key, bitmap] of tileMapRef.current) {
      const tile = parseTileKey(key);
      if (tile.level === currentLevel) continue;
      if (!shouldKeepLodTile(tile, currentLevel, visible, haveCurrent)) continue;
      blitOne(tile.x, tile.y, tile.level, bitmap);
    }
    for (const vis of visible) {
      if (haveCurrent.has(tileKey(vis))) continue;
      const fb = findFallbackTile(vis.level, vis.x, vis.y, tileMapRef.current);
      if (!fb) continue;
      const bitmap = tileMapRef.current.get(fb.key);
      if (!bitmap) continue;
      const blit = computeTileBlit(vis.x, vis.y, vis.level, vp, docWidth, docHeight, dpr);
      if (!blit) continue;
      ctx.imageSmoothingEnabled = true;
      const offsets = repeat
        ? ([-1, 0, 1] as const).flatMap((ox) =>
            ([-1, 0, 1] as const).map((oy) => [ox, oy] as const)
          )
        : ([[0, 0]] as const);
      for (const [ox, oy] of offsets) {
        const dx = blit.dx + ox * stepX;
        const dy = blit.dy + oy * stepY;
        if (
          dx + blit.dw < 0 ||
          dy + blit.dh < 0 ||
          dx > canvas.width ||
          dy > canvas.height
        ) {
          continue;
        }
        ctx.drawImage(
          bitmap,
          fb.srcX,
          fb.srcY,
          fb.srcSize,
          fb.srcSize,
          dx,
          dy,
          blit.dw,
          blit.dh,
        );
      }
      ctx.imageSmoothingEnabled = false;
      if (tileGenTraceEnabled()) {
        const gen = tileGenMapRef.current.get(fb.key) ?? null;
        noteTraceEvent('tile_drawn', {
          level: vis.level,
          x: vis.x,
          y: vis.y,
          gen: gen ?? undefined,
          details: {
            source: 'lod_fallback',
            from: fb.key,
            tileRev: tileRevRef.current,
          },
        });
      }
    }
    // Only blit the visible set at the current level. The map may still hold
    // offscreen same-level bitmaps for pan cache (`shouldKeepLodTile` keeps
    // all current-level keys); drawing them made `tile_drawn` fire outside
    // the quiet visible set → false DRAWN_NOT_VISIBLE in the stress checker.
    for (const vis of visible) {
      if (vis.level !== currentLevel) continue;
      const bitmap = tileMapRef.current.get(tileKey(vis));
      if (!bitmap) continue;
      blitOne(vis.x, vis.y, vis.level, bitmap);
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
      noteFramePainted();
      if (ditherProfileEnabled()) {
        noteFeMark('frame_drawn', activeProfileRunId());
      }
    });
  }, [drawTiles]);

  useEffect(() => {
    void probeDitherProfile();
    void probeTileGenTrace().then((on) => {
      if (!on) return;
      registerTraceCanvasHooks({
        getVisibleKeys: () => {
          const vp = viewportRef.current;
          const { docWidth: w, docHeight: h } = docSizeRef.current;
          return visibleTilesForPreview(vp, w, h, tileRepeatRef.current).map(tileKey);
        },
        getViewportLevel: () => {
          const vp = viewportRef.current;
          const { docWidth: w, docHeight: h } = docSizeRef.current;
          return computePyramidLevel(vp.zoom, w, h);
        },
        getTileRev: () => tileRevRef.current,
        sampleCenterPixelOpaque: (key) => {
          const canvas = canvasRef.current;
          if (!canvas) return null;
          const ctx = canvas.getContext('2d');
          if (!ctx) return null;
          const [level, x, y] = key.split('/').map(Number);
          const vp = viewportRef.current;
          const { docWidth: w, docHeight: h } = docSizeRef.current;
          const blit = computeTileBlit(x, y, level, vp, w, h, devicePixelRatio());
          if (!blit) return null;
          const px = Math.floor(blit.dx + blit.dw / 2);
          const py = Math.floor(blit.dy + blit.dh / 2);
          try {
            const data = ctx.getImageData(px, py, 1, 1).data;
            return data[3] > 0;
          } catch {
            return null;
          }
        },
      });
    });
  }, []);

  useEffect(() => {
    if (previewBackground === 'pattern') {
      void loadHalftoneImage().then(() => scheduleRedraw()).catch(() => {
        scheduleRedraw();
      });
      return;
    }
    scheduleRedraw();
  }, [previewBackground, scheduleRedraw]);

  useEffect(() => {
    scheduleRedraw();
  }, [tileRepeat3x3, scheduleRedraw]);

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
    if (ditherProfileEnabled()) {
      noteFeMark('commit_applied', activeProfileRunId());
    }
    if (tileGenTraceEnabled()) {
      const tiles = [...displayed.entries()].map(([k, _]) => {
        const [level, x, y] = k.split('/').map(Number);
        return { level, x, y, gen: tileGenMapRef.current.get(k) ?? null };
      });
      noteTraceEvent('commit_applied', { details: { tiles } });
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
      noteCommitTimeout();
      flushPendingCommit();
    }, COMMIT_WAIT_MS);
  }, [flushPendingCommit]);

  // ─── Handle worker messages ─────────────────────────────────────────────

  const handleWorkerMessage = useCallback(
    (e: MessageEvent) => {
      const msg = e.data;
      if (msg.type === 'tile-pending') {
        const key = msg.key as string;
        const pendingReason =
          typeof msg.pendingReason === 'string' ? msg.pendingReason : 'queued';
        const pendingRev =
          typeof msg.rev === 'number' ? msg.rev : tileRevRef.current;
        if (EVENT_DRIVEN_PENDING_REASONS.has(pendingReason)) {
          const vp = viewportRef.current;
          const { docWidth: w, docHeight: h } = docSizeRef.current;
          const visible = new Set(
            visibleTilesForPreview(vp, w, h, tileRepeatRef.current).map(tileKey),
          );
          if (
            pendingRev === tileRevRef.current &&
            visible.has(key)
          ) {
            eventDrivenPendingRef.current.set(key, {
              reason: pendingReason,
              rev: pendingRev,
              inflight: false,
              lastSafetyAt: 0,
            });
          }
        }
        if (tileGenTraceEnabled()) {
          recordHttpStatus(key, 202);
          const [lvl, xx, yy] = key.split('/').map(Number);
          const details = {
            status: 202,
            key,
            pending_reason: pendingReason,
            event_driven: EVENT_DRIVEN_PENDING_REASONS.has(pendingReason),
            willRetry: !EVENT_DRIVEN_PENDING_REASONS.has(pendingReason),
            level: lvl,
          };
          noteTraceEvent('fetch_tile_response', { level: lvl, x: xx, y: yy, details });
          noteTraceEvent('tile_response', { level: lvl, x: xx, y: yy, details });
        }
        return;
      }
      if (msg.type !== 'tile-decoded') return;

      const key = msg.key as string;
      const bitmap = msg.bitmap as ImageBitmap;
      const decodedRev = msg.rev as number | undefined;
      const msgDocId = msg.docId as number | undefined;
      // Drop late responses from a previous tab / open (keys are level/x/y only).
      if (typeof msgDocId === 'number' && msgDocId !== docId) {
        bitmap.close();
        return;
      }
      if (!shouldAcceptDecodedRev(decodedRev, tileRevRef.current)) {
        // Allow another wake fetch for this still-pending key.
        const pend = eventDrivenPendingRef.current.get(key);
        if (pend) {
          pend.inflight = false;
          eventDrivenPendingRef.current.set(key, pend);
        }
        if (tileGenTraceEnabled()) {
          const [lvl, xx, yy] = key.split('/').map(Number);
          recordDropped(key, 'tileRev_mismatch');
          noteTraceEvent('tile_dropped', {
            level: lvl,
            x: xx,
            y: yy,
            gen: typeof msg.gen === 'number' ? msg.gen : undefined,
            details: {
              reason: 'tileRev_mismatch',
              decodedRev,
              currentRev: tileRevRef.current,
            },
          });
        }
        bitmap.close();
        return;
      }

      eventDrivenPendingRef.current.delete(key);

      const fetchDiag = msg.diag as
        | { retries?: number; stale?: boolean; pyramid_fallback?: boolean; gen?: number | null }
        | undefined;
      const tileGen =
        typeof msg.gen === 'number'
          ? msg.gen
          : typeof fetchDiag?.gen === 'number'
            ? fetchDiag.gen
            : undefined;
      noteTileDecoded(
        key,
        fetchDiag
          ? {
              retries: fetchDiag.retries,
              stale: fetchDiag.stale,
              pyramidFallback: fetchDiag.pyramid_fallback,
            }
          : undefined,
      );

      if (tileGenTraceEnabled()) {
        const [lvl, xx, yy] = key.split('/').map(Number);
        const status =
          typeof msg.httpStatus === 'number'
            ? msg.httpStatus
            : 200;
        recordHttpStatus(key, status);
        const source = fetchDiag?.pyramid_fallback
          ? 'pyramid'
          : fetchDiag?.stale || msg.stale === true
            ? 'stale'
            : msg.pyramid === true
              ? 'pyramid'
              : 'fresh';
        if (tileGen != null) {
          tileGenMapRef.current.set(key, tileGen);
        }
        tileSourceMapRef.current.set(key, source);
        noteTraceEvent('tile_decoded', {
          level: lvl,
          x: xx,
          y: yy,
          gen: tileGen,
          details: {
            requestRev: decodedRev,
            currentRev: tileRevRef.current,
            status,
            source,
          },
        });
        noteTraceEvent('tile_response', {
          level: lvl,
          x: xx,
          y: yy,
          gen: tileGen,
          details: { status, level: lvl, source },
        });
      }

      if (ditherProfileEnabled()) {
        const profile = msg.profile as
          | { runId?: number; protocol_epoch_ms: number; decoded_epoch_ms: number }
          | undefined;
        const runId = profile?.runId ?? activeProfileRunId();
        if (profile) {
          noteFeMark('tile_protocol_response', runId);
        }
        noteFeMark('tile_decoded', runId);
      }

      const metric = softProofMetricRef.current;
      if (metric && decodedRev === metric.rev && metric.expected.has(key)) {
        const elapsed = performance.now() - metric.t0;
        if (metric.firstMs == null) {
          metric.firstMs = elapsed;
          performance.mark('soft_proof_first_tile');
          console.debug(`[soft_proof] click→first_tile ${elapsed.toFixed(1)}ms`);
        }
        metric.expected.delete(key);
        if (metric.expected.size === 0) {
          performance.mark('soft_proof_last_tile');
          metric.samples.push(elapsed);
          const sorted = [...metric.samples].sort((a, b) => a - b);
          const p50 = sorted[Math.floor(sorted.length * 0.5)] ?? elapsed;
          const p95 = sorted[Math.floor(sorted.length * 0.95)] ?? elapsed;
          console.debug(
            `[soft_proof] click→last_tile ${elapsed.toFixed(1)}ms (first=${metric.firstMs.toFixed(1)}ms) samples=${sorted.length} p50=${p50.toFixed(1)} p95=${p95.toFixed(1)}`,
          );
          softProofMetricRef.current = {
            ...metric,
            expected: new Set(),
            firstMs: null,
            t0: metric.t0,
            rev: -1,
          };
        }
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
      const visibleKeys = visibleTilesForPreview(
        viewportRef.current,
        w,
        h,
        tileRepeatRef.current,
      ).map(tileKey);
      if (!shouldCommitTileRefresh(displayed.keys(), pending.keys(), visibleKeys)) {
        return;
      }

      clearCommitTimer();
      flushPendingCommit();
    },
    [armCommitTimeout, clearCommitTimer, docId, flushPendingCommit, scheduleRedraw],
  );
  handleWorkerMessageRef.current = handleWorkerMessage;

  // ─── Initialize Web Worker (stable — do not recreate on docId / handler churn) ─

  useEffect(() => {
    const worker = new Worker(
      new URL('../../workers/tileWorker.ts', import.meta.url),
      { type: 'module' },
    );
    worker.onmessage = (e: MessageEvent) => {
      handleWorkerMessageRef.current(e);
    };
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
  }, []);

  const lastLoggedLevelRef = useRef<number | null>(null);

  // ─── Viewport change: CSS transform for instant pan, then async redraw ──

  useEffect(() => {
    if (viewport.canvasWidth === 0 || viewport.canvasHeight === 0) return;

    const canvas = canvasRef.current;
    const lastVp = lastDrawnViewportRef.current;

    const currentLevel = computePyramidLevel(viewport.zoom, docWidth, docHeight);
    const visibleNow = visibleTilesForPreview(
      viewport,
      docWidth,
      docHeight,
      tileRepeatRef.current,
    );

    if (tileGenTraceEnabled()) {
      const range = tileRangeFromCoords(currentLevel, visibleNow);
      noteTraceEvent('vp_computed', {
        details: {
          scale: viewport.zoom,
          level: currentLevel,
          offset: { x: viewport.panX, y: viewport.panY },
          canvas: { w: viewport.canvasWidth, h: viewport.canvasHeight },
          x0: range.x0,
          y0: range.y0,
          x1: range.x1,
          y1: range.y1,
          tile_count: range.count,
          vp_hash: range.vp_hash,
          tileRev: tileRevRef.current,
        },
      });
      if (
        lastLoggedLevelRef.current !== null &&
        lastLoggedLevelRef.current !== currentLevel
      ) {
        noteTraceEvent('vp_level_changed', {
          details: {
            old_level: lastLoggedLevelRef.current,
            new_level: currentLevel,
            scale: viewport.zoom,
            vp_hash: range.vp_hash,
          },
        });
      }
      lastLoggedLevelRef.current = currentLevel;
    }
    const haveCurrent = new Set(
      [...tileMapRef.current.keys()].filter(
        (k) => parseTileKey(k).level === currentLevel,
      ),
    );
    for (const [key, bitmap] of [...tileMapRef.current.entries()]) {
      if (!shouldKeepLodTile(parseTileKey(key), currentLevel, visibleNow, haveCurrent)) {
        bitmap.close();
        tileMapRef.current.delete(key);
      }
    }
    for (const [key, bitmap] of [...refreshPendingRef.current.entries()]) {
      if (!shouldKeepLodTile(parseTileKey(key), currentLevel, visibleNow, haveCurrent)) {
        bitmap.close();
        refreshPendingRef.current.delete(key);
      }
    }

    // Cancel event-driven 202 waits that left the visible set.
    pruneEventDrivenPending(new Set(visibleNow.map(tileKey)));

    // If only pan changed (same zoom), apply CSS transform for instant visual shift
    if (canvas && viewport.zoom === lastVp.zoom &&
        viewport.canvasWidth === lastVp.canvasWidth &&
        viewport.canvasHeight === lastVp.canvasHeight) {
      const dpr = devicePixelRatio();
      const dx = snapCssPx((lastVp.panX - viewport.panX) * viewport.zoom, dpr);
      const dy = snapCssPx((lastVp.panY - viewport.panY) * viewport.zoom, dpr);
      canvas.style.transform = `translate(${dx}px, ${dy}px)`;
      noteCssPan();
    }

    // Schedule actual redraw (async) — when it fires it resets the transform
    scheduleRedraw();

    // Request missing tiles
    if (workerRef.current) {
      const visible = visibleTilesForPreview(
        viewport,
        docWidth,
        docHeight,
        tileRepeatRef.current,
      );
      const needed = visible.filter(t => !tileMapRef.current.has(`${t.level}/${t.x}/${t.y}`));
      const ancestors = coveringAncestorTiles(
        needed,
        maxPyramidLevel(docWidth, docHeight),
      ).filter((t) => !tileMapRef.current.has(tileKey(t)));
      const tiles = [...needed, ...ancestors];
      if (tiles.length > 0) {
        const displayKeys = (needed.length > 0 ? needed : tiles).map(tileKey);
        noteDisplayPost(
          displayKeys,
          docId,
          viewport.zoom,
          viewport.canvasWidth,
          viewport.canvasHeight,
        );
        if (tileGenTraceEnabled()) {
          for (const t of tiles) {
            const set = needed.some((n) => n.level === t.level && n.x === t.x && n.y === t.y)
              ? 'visible'
              : 'prefetch';
            noteTraceEvent('tile_requested', {
              level: t.level,
              x: t.x,
              y: t.y,
              details: { tileRev: tileRevRef.current, set, via: 'request-tiles' },
            });
          }
        }
        workerRef.current.postMessage(stampWorkerMessage({
          type: 'request-tiles',
          tiles,
          docId,
          rev: tileRevRef.current,
          protocolBase: workerTileProtocolBase(),
          trace: tileGenTraceEnabled() || undefined,
        }));
      } else {
        noteDisplayCurrent(visible.length);
      }
    }
  }, [viewport, docId, docWidth, docHeight, scheduleRedraw, pruneEventDrivenPending]);

  // ─── ed-progress: wake event-driven pending + feed UI indicator ─────────

  useEffect(() => {
    const unlisten = listen<EdProgressPayload>('ed-progress', (event) => {
      const p = event.payload;
      if (p.doc_id !== docId) return;
      setEdProgress({
        level: p.level,
        done: p.done,
        total: p.total,
        docGen: p.doc_gen,
        sourceId: p.source_id,
        docId: p.doc_id,
        parallelEdPasses: p.parallel_ed_passes,
        updatedAt: performance.now(),
      });
      if (tileGenTraceEnabled()) {
        noteTraceEvent('ed_progress_received', {
          details: {
            done: p.done,
            total: p.total,
            doc_gen: p.doc_gen,
            parallel_ed_passes: p.parallel_ed_passes,
            pending_tracked: eventDrivenPendingRef.current.size,
          },
        });
      }
      refetchEventDrivenPending('ed_progress');
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [docId, refetchEventDrivenPending]);

  // One global safety interval (not per-tile). Per-tile coalescing lives in
  // refetchEventDrivenPending (inflight + lastSafetyAt).
  useEffect(() => {
    const id = window.setInterval(() => {
      if (eventDrivenPendingRef.current.size === 0) return;
      refetchEventDrivenPending('safety_timer');
    }, PENDING_SAFETY_RETRY_MS);
    return () => window.clearInterval(id);
  }, [refetchEventDrivenPending]);

  // ─── Listen for tile-ready events from Tauri backend ────────────────────

  useEffect(() => {
    const unlisten = listen<TileReadyPayload>('tile-ready', (event) => {
      const { level, x, y, stage, doc_id } = event.payload;
      const key = `${level}/${x}/${y}`;
      if (doc_id !== docId) {
        if (tileGenTraceEnabled()) {
          noteTraceEvent('tile_ready_received', {
            level,
            x,
            y,
            details: { accepted: false, reason: 'doc_mismatch', doc_id, expected: docId },
          });
        }
        return;
      }
      if (stage && stage !== 'composite') {
        if (tileGenTraceEnabled()) {
          noteTraceEvent('tile_ready_received', {
            level,
            x,
            y,
            details: { accepted: false, reason: 'stage_filter', stage },
          });
        }
        return;
      }
      const currentLevel = computePyramidLevel(
        viewportRef.current.zoom,
        docWidth,
        docHeight,
      );
      // Accept tile-ready for any level we still track as event-driven pending
      // (Step 1 registry); otherwise require the current viewport level.
      const pendingTracked = eventDrivenPendingRef.current.has(key);
      if (level !== currentLevel && !pendingTracked) {
        if (tileGenTraceEnabled()) {
          noteTraceEvent('tile_ready_received', {
            level,
            x,
            y,
            details: {
              accepted: false,
              reason: 'level_mismatch',
              viewportLevel: currentLevel,
            },
          });
        }
        return;
      }
      if (tileGenTraceEnabled()) {
        recordTileReady(key);
        if (takeInjectDrop(key)) {
          noteTraceEvent('tile_ready_received', {
            level,
            x,
            y,
            details: { accepted: false, reason: 'inject_drop', inject: true },
          });
          return;
        }
      }
      const vp = viewportRef.current;
      noteDisplayPost(
        [`${level}/${x}/${y}`],
        docId,
        vp.zoom,
        vp.canvasWidth,
        vp.canvasHeight,
      );
      if (tileGenTraceEnabled()) {
        noteTraceEvent('tile_ready_received', {
          level,
          x,
          y,
          details: { accepted: true, viewportLevel: currentLevel },
        });
      }
      if (ditherProfileEnabled()) {
        noteFeMark('tile_ready_received', activeProfileRunId());
      }
      const pend = eventDrivenPendingRef.current.get(key);
      const wake = decideEventDrivenWake(pend, 'tile_ready');
      if (wake.action === 'skip') {
        if (tileGenTraceEnabled()) {
          noteTraceEvent('wake_skipped_inflight', {
            level,
            x,
            y,
            details: {
              cause: wake.cause,
              pending_reason: pend?.reason ?? null,
              tileRev: tileRevRef.current,
            },
          });
        }
        return;
      }
      if (pend) {
        pend.inflight = true;
        eventDrivenPendingRef.current.set(key, pend);
      }
      if (tileGenTraceEnabled()) {
        recordFetchPosted(key);
        const reqDetails = {
          tileRev: tileRevRef.current,
          set: 'visible' as const,
          via: 'event_driven_pending' as const,
          wake: 'tile_ready' as const,
        };
        noteTraceEvent('fetch_tile_posted', { level, x, y, details: reqDetails });
        noteTraceEvent('tile_requested', { level, x, y, details: reqDetails });
      }
      if (ditherProfileEnabled()) {
        noteFeMark('fetch_tile_posted', activeProfileRunId());
      }
      workerRef.current?.postMessage(stampWorkerMessage({
        type: 'fetch-tile',
        level,
        x,
        y,
        docId,
        rev: tileRevRef.current,
        protocolBase: workerTileProtocolBase(),
        profile: ditherProfileEnabled() || undefined,
        profileRunId: activeProfileRunId() ?? undefined,
        trace: tileGenTraceEnabled() || undefined,
      }));
    });
    return () => { unlisten.then((fn) => fn()); };
  }, [docId, docWidth, docHeight]);

  useEffect(() => {
    const unlisten = onDocumentChanged((event) => {
      const eventDocId = event.payload.doc_id;
      // Stale event from another tab / previous open — do not paint its tiles.
      if (typeof eventDocId === 'number' && eventDocId !== docId) {
        return;
      }
      const sourceReplace = isDocumentSourceReplace(event.payload.kind);
      tileRevRef.current += 1;
      clearEventDrivenPending();
      setEdProgress(null);
      if (tileGenTraceEnabled()) {
        noteTraceEvent('doc_changed_received', {
          layerId: event.payload.layer_id ?? undefined,
          details: {
            kind: event.payload.kind,
            tileRev: tileRevRef.current,
            doc_id: event.payload.doc_id,
          },
        });
      }
      if (commitTimerRef.current != null) {
        window.clearTimeout(commitTimerRef.current);
        commitTimerRef.current = null;
      }
      for (const bitmap of refreshPendingRef.current.values()) {
        bitmap.close();
      }
      refreshPendingRef.current.clear();
      if (sourceReplace) {
        noteUserInput('open_doc');
        for (const bitmap of tileMapRef.current.values()) {
          bitmap.close();
        }
        tileMapRef.current.clear();
        tileGenMapRef.current.clear();
        tileSourceMapRef.current.clear();
      }
      if (!workerRef.current) return;
      const visible = visibleTilesForPreview(
        viewportRef.current,
        docWidth,
        docHeight,
        tileRepeatRef.current,
      );
      if (visible.length === 0) return;
      if (event.payload.kind === 'soft_proof_changed') {
        const prev = softProofMetricRef.current;
        softProofMetricRef.current = {
          rev: tileRevRef.current,
          t0: performance.now(),
          expected: new Set(visible.map(tileKey)),
          firstMs: null,
          samples: prev?.samples ?? [],
        };
      }
      const vpNow = viewportRef.current;
      noteDisplayPost(
        visible.map(tileKey),
        docId,
        vpNow.zoom,
        vpNow.canvasWidth,
        vpNow.canvasHeight,
      );
      workerRef.current.postMessage(stampWorkerMessage({
        type: 'request-tiles',
        tiles: visible,
        docId,
        rev: tileRevRef.current,
        protocolBase: workerTileProtocolBase(),
      }));
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, [docId, docWidth, docHeight, clearEventDrivenPending]);

  // Clear bitmaps before paint when identity changes — otherwise the viewport
  // effect still sees A's tiles under B's doc size ("same image, other canvas").
  useLayoutEffect(() => {
    tileRevRef.current += 1;
    clearEventDrivenPending();
    setEdProgress(null);
    for (const bitmap of tileMapRef.current.values()) {
      bitmap.close();
    }
    tileMapRef.current.clear();
    tileGenMapRef.current.clear();
    tileSourceMapRef.current.clear();
    for (const bitmap of refreshPendingRef.current.values()) {
      bitmap.close();
    }
    refreshPendingRef.current.clear();

    // Request immediately in the same layout pass so a worker recreate / event
    // race cannot leave the canvas cleared with no follow-up fetch.
    const vp = viewportRef.current;
    if (
      workerRef.current &&
      docId &&
      vp.canvasWidth > 0 &&
      vp.canvasHeight > 0 &&
      docWidth > 0 &&
      docHeight > 0
    ) {
      const visible = visibleTilesForPreview(
        vp,
        docWidth,
        docHeight,
        tileRepeatRef.current,
      );
      if (visible.length > 0) {
        noteUserInput('open_doc');
        noteDisplayPost(
          visible.map(tileKey),
          docId,
          vp.zoom,
          vp.canvasWidth,
          vp.canvasHeight,
        );
        workerRef.current.postMessage(stampWorkerMessage({
          type: 'request-tiles',
          tiles: visible,
          docId,
          rev: tileRevRef.current,
          protocolBase: workerTileProtocolBase(),
        }));
      }
    }
  }, [docId, documentEpoch, docWidth, docHeight, clearEventDrivenPending]);

  // ─── Force refetch all visible tiles after docId changes (initial load) ─

  useEffect(() => {
    if (!docId || viewport.canvasWidth === 0 || viewport.canvasHeight === 0) return;

    // After a short delay, force-refetch all visible tiles to catch any
    // that were computed by the backend after our initial request
    const timer = setTimeout(() => {
      if (!workerRef.current) return;
      const visible = visibleTilesForPreview(
        viewportRef.current,
        docWidth,
        docHeight,
        tileRepeatRef.current,
      );
      const vpNow = viewportRef.current;
      noteDisplayPost(
        visible.map(tileKey),
        docId,
        vpNow.zoom,
        vpNow.canvasWidth,
        vpNow.canvasHeight,
      );
      workerRef.current.postMessage(stampWorkerMessage({
        type: 'request-tiles',
        tiles: visible,
        docId,
        rev: tileRevRef.current,
        protocolBase: workerTileProtocolBase(),
      }));
    }, 300);

    return () => clearTimeout(timer);
  }, [docId, documentEpoch, docWidth, docHeight, viewport.canvasWidth, viewport.canvasHeight]);

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
