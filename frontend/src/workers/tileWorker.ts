/**
 * Tile Web Worker — fetches tiles from the Tauri `tile` custom protocol,
 * decodes raw RGBA8 bytes into ImageBitmaps, and transfers them back
 * to the main thread for canvas rendering.
 *
 * On Windows/Android the origin is `http://tile.localhost` (WebView2);
 * elsewhere it is `tile://localhost`.
 *
 * Message types received:
 *   - { type: 'request-tiles', tiles: TileRequest[], docId: number }
 *   - { type: 'fetch-tile', level: number, x: number, y: number, docId: number }
 *
 * Message types posted back:
 *   - { type: 'tile-decoded', key: string, bitmap: ImageBitmap }  (transferred)
 *   - { type: 'tile-pending', key: string }
 *   - { type: 'tile-error', key: string, error: string }
 */

import { buildTileUrl } from '../lib/tileProtocol';

// Worker-scoped postMessage with transferable support
declare function postMessage(message: unknown, transfer: Transferable[]): void;
declare function postMessage(message: unknown, options?: StructuredSerializeOptions): void;

export interface TileRequest {
  level: number;
  x: number;
  y: number;
}

export interface RequestTilesMessage {
  type: 'request-tiles';
  tiles: TileRequest[];
  docId: number;
  rev?: number;
  /** Override protocol origin; main thread should pass platform-aware base. */
  protocolBase?: string;
  /** Set only while preview diagnostics are enabled. */
  diag?: boolean;
  /** `DITHER_PROFILE=1` — attach protocol/decode epoch marks (no dither change). */
  profile?: boolean;
  profileRunId?: number;
  /** `DITHER_TRACE=1` — always read X-Tile-Generation for FE events. */
  trace?: boolean;
}

export interface FetchTileMessage {
  type: 'fetch-tile';
  level: number;
  x: number;
  y: number;
  docId: number;
  rev?: number;
  protocolBase?: string;
  /** Set only while preview diagnostics are enabled. */
  diag?: boolean;
  /** `DITHER_PROFILE=1` — attach protocol/decode epoch marks (no dither change). */
  profile?: boolean;
  profileRunId?: number;
  /** `DITHER_TRACE=1` — always read X-Tile-Generation for FE events. */
  trace?: boolean;
}

export type WorkerInMessage = RequestTilesMessage | FetchTileMessage;

export interface TileDecodedMessage {
  type: 'tile-decoded';
  key: string;
  bitmap: ImageBitmap;
  rev?: number;
  docId: number;
  /** Present when `diag` or `trace` requested generation from `X-Tile-Generation`. */
  gen?: number | null;
  httpStatus?: number;
  /** From `X-Tile-Quality` / legacy stale/fallback headers (always when present). */
  stale?: boolean;
  pyramid?: boolean;
  quality?: string | null;
  /** Present only when the request set `diag: true` (diagnostic.md §5). */
  diag?: TileFetchDiag;
  /** Present only when the request set `profile: true` (time-testing.md). */
  profile?: {
    runId?: number;
    protocol_epoch_ms: number;
    decoded_epoch_ms: number;
  };
}

export interface TileFetchDiag {
  retries: number;
  retry_wait_ms: number;
  fetch_ms: number;
  decode_ms: number;
  http_status: number;
  gen: number | null;
  stale: boolean;
  pyramid_fallback: boolean;
  bytes: number;
}

export type TilePendingReason =
  | 'ed_prefix'
  | 'waiting_children'
  | 'computing'
  | 'queued'
  | string;

export interface TilePendingMessage {
  type: 'tile-pending';
  key: string;
  docId: number;
  /** From `X-Tile-Pending-Reason` when present. */
  pendingReason?: TilePendingReason;
  rev?: number;
  /** Expected live gen from `X-Tile-Gen` / `X-Tile-Generation` on 202. */
  gen?: number | null;
}

/** No 5-retry ceiling — main thread re-fetches on tile-ready / ed-progress / safety. */
const EVENT_DRIVEN_PENDING = new Set([
  'ed_prefix',
  'waiting_children',
  'computing',
]);

export interface TileErrorMessage {
  type: 'tile-error';
  key: string;
  error: string;
  docId: number;
}

export type WorkerOutMessage = TileDecodedMessage | TilePendingMessage | TileErrorMessage;

const TILE_SIZE = 256;
const TILE_BYTE_LENGTH = TILE_SIZE * TILE_SIZE * 4; // 262,144 bytes RGBA8

function responseMeta(response: Response, _readHeaders: boolean): {
  gen: number | null;
  stale: boolean;
  pyramid: boolean;
  quality: string | null;
} {
  // Gen/quality always read (H4 Step B) — not gated on diag/trace.
  if (response.headers == null || typeof response.headers.get !== 'function') {
    return { gen: null, stale: false, pyramid: false, quality: null };
  }
  const tileGen = response.headers.get('X-Tile-Gen');
  const legacy = response.headers.get('X-Tile-Generation');
  // `doc.layer.content` → prefer content (last); legacy `doc.layer` → doc (first).
  let fromComposite: number | null = null;
  if (tileGen != null && tileGen !== '') {
    const parts = tileGen.split('.');
    const raw = parts.length >= 3 ? parts[parts.length - 1] : parts[0];
    const n = Number(raw);
    fromComposite = Number.isFinite(n) ? n : null;
  }
  const fromLegacy = legacy != null && legacy !== '' ? Number(legacy) : null;
  const gen =
    fromComposite != null && Number.isFinite(fromComposite)
      ? fromComposite
      : fromLegacy != null && Number.isFinite(fromLegacy)
        ? fromLegacy
        : null;
  const quality = response.headers.get('X-Tile-Quality');
  const stale =
    quality === 'stale' || response.headers.get('X-Dither-Tile-Stale') != null;
  const pyramid =
    quality === 'upsample' || response.headers.get('X-Dither-Tile-Fallback') != null;
  return { gen, stale, pyramid, quality };
}

function pendingReasonOf(response: Response): string | undefined {
  if (response.headers == null || typeof response.headers.get !== 'function') {
    return undefined;
  }
  const raw = response.headers.get('X-Tile-Pending-Reason');
  return raw == null || raw === '' ? undefined : raw;
}

/**
 * Fetch a single tile from the tile protocol, decode the raw RGBA8
 * bytes into an ImageBitmap, and post it back to the main thread.
 *
 * `diag` is set only while preview diagnostics are enabled. When it is
 * false this function does not call `performance.now` and the posted
 * message keys match the historical shape (diagnostic.md §2.3, §5).
 */
async function fetchAndDecodeTile(
  docId: number,
  tile: TileRequest,
  rev?: number,
  protocolBase?: string,
  diag = false,
  profile = false,
  profileRunId?: number,
  trace = false,
): Promise<void> {
  const key = `${tile.level}/${tile.x}/${tile.y}`;
  const url = buildTileUrl(docId, tile, rev, { base: protocolBase });
  const epoch = () => performance.timeOrigin + performance.now();
  const readHeaders = diag || trace;

  const postDecoded = (
    bitmap: ImageBitmap,
    fetchMs: number,
    decodeMs: number,
    retries: number,
    retryWait: number,
    status: number,
    meta: { gen: number | null; stale: boolean; pyramid: boolean; quality: string | null },
    bytes: number,
    protocolEpochMs?: number,
  ) => {
    const msg: TileDecodedMessage = { type: 'tile-decoded', key, bitmap, rev, docId };
    // Always attach gen/quality so main thread can gate H4 (not trace-only).
    msg.gen = meta.gen;
    msg.httpStatus = status;
    msg.stale = meta.stale;
    msg.pyramid = meta.pyramid;
    msg.quality = meta.quality;
    if (trace) {
      /* headers already attached */
    }
    if (diag) {
      msg.diag = {
        retries,
        retry_wait_ms: retryWait,
        fetch_ms: Math.round(fetchMs * 10) / 10,
        decode_ms: Math.round(decodeMs * 10) / 10,
        http_status: status,
        gen: meta.gen,
        stale: meta.stale,
        pyramid_fallback: meta.pyramid,
        bytes,
      };
    }
    if (profile && protocolEpochMs != null) {
      msg.profile = {
        runId: profileRunId,
        protocol_epoch_ms: protocolEpochMs,
        decoded_epoch_ms: epoch(),
      };
    }
    postMessage(msg, [bitmap]);
  };

  const decodeBitmap = async (buffer: ArrayBuffer, withOptions: boolean) => {
    const t0 = diag || profile ? performance.now() : 0;
    const imageData = new ImageData(new Uint8ClampedArray(buffer), TILE_SIZE, TILE_SIZE);
    const bitmap = withOptions
      ? await createImageBitmap(imageData, {
          premultiplyAlpha: 'none',
          colorSpaceConversion: 'none',
        })
      : await createImageBitmap(imageData);
    return { bitmap, decodeMs: diag || profile ? performance.now() - t0 : 0 };
  };

  try {
    const tFetch = diag || profile ? performance.now() : 0;
    const response = await fetch(url);
    const protocolEpochMs = profile ? epoch() : undefined;

    if (response.status === 200) {
      const buffer = await response.arrayBuffer();
      const fetchMs = diag || profile ? performance.now() - tFetch : 0;

      if (buffer.byteLength !== TILE_BYTE_LENGTH) {
        postMessage({
          type: 'tile-error',
          key,
          docId,
          error: `Unexpected tile data size: ${buffer.byteLength} bytes (expected ${TILE_BYTE_LENGTH})`,
        } satisfies TileErrorMessage);
        return;
      }

      const { bitmap, decodeMs } = await decodeBitmap(buffer, true);
      postDecoded(
        bitmap,
        fetchMs,
        decodeMs,
        0,
        0,
        200,
        responseMeta(response, readHeaders),
        buffer.byteLength,
        protocolEpochMs,
      );
    } else if (response.status === 202) {
      // Always read pending reason (protocol header); not gated on diag/trace.
      const reason = pendingReasonOf(response) ?? 'queued';
      const meta202 = responseMeta(response, true);
      postMessage({
        type: 'tile-pending',
        key,
        docId,
        pendingReason: reason,
        rev,
        gen: meta202.gen,
      } satisfies TilePendingMessage);

      // ed_prefix / waiting_children / computing: no 5-retry ceiling.
      // Main thread re-queries on tile-ready, ed-progress, and a 1–2s safety timer.
      if (EVENT_DRIVEN_PENDING.has(reason)) {
        return;
      }

      // queued (and unknown): keep bounded exponential backoff.
      let retryWait = 0;
      let retries = 0;
      for (let attempt = 0; attempt < 5; attempt++) {
        const delay = 50 * Math.pow(2, attempt);
        if (diag) {
          retries += 1;
          retryWait += delay;
        }
        await new Promise(resolve => setTimeout(resolve, delay));
        try {
          const tRetry = diag || profile ? performance.now() : 0;
          const retryResponse = await fetch(url);
          const retryProtocolEpoch = profile ? epoch() : undefined;
          if (retryResponse.status === 200) {
            const retryBuffer = await retryResponse.arrayBuffer();
            const fetchMs = diag || profile ? performance.now() - tRetry : 0;
            if (retryBuffer.byteLength === TILE_BYTE_LENGTH) {
              const { bitmap, decodeMs } = await decodeBitmap(retryBuffer, false);
              postDecoded(
                bitmap,
                fetchMs,
                decodeMs,
                retries,
                retryWait,
                200,
                responseMeta(retryResponse, readHeaders),
                retryBuffer.byteLength,
                retryProtocolEpoch,
              );
            }
            break;
          }
        } catch {
          break;
        }
      }
    } else {
      // Non-recoverable error (404, 400, etc.)
      const body = await response.text().catch(() => '');
      postMessage({
        type: 'tile-error',
        key,
        docId,
        error: `Tile fetch failed with status ${response.status}: ${body}`,
      } satisfies TileErrorMessage);
    }
  } catch (err) {
    // Network error or decode failure
    const message = err instanceof Error ? err.message : String(err);
    postMessage({
      type: 'tile-error',
      key,
      docId,
      error: `Tile fetch/decode error: ${message}`,
    } satisfies TileErrorMessage);
  }
}

/**
 * Main message handler — dispatches to the appropriate fetch logic.
 */
self.onmessage = async (e: MessageEvent<WorkerInMessage>) => {
  const msg = e.data;

  if (msg.type === 'request-tiles') {
    // Batch fetch: process all requested tiles in parallel. Each
    // fetchAndDecodeTile handles its own error and posts results independently.
    await Promise.all(
      msg.tiles.map(tile =>
        fetchAndDecodeTile(
          msg.docId,
          tile,
          msg.rev,
          msg.protocolBase,
          msg.diag === true,
          msg.profile === true,
          msg.profileRunId,
          msg.trace === true,
        ),
      ),
    );
  } else if (msg.type === 'fetch-tile') {
    await fetchAndDecodeTile(
      msg.docId,
      {
        level: msg.level,
        x: msg.x,
        y: msg.y,
      },
      msg.rev,
      msg.protocolBase,
      msg.diag === true,
      msg.profile === true,
      msg.profileRunId,
      msg.trace === true,
    );
  }
};
