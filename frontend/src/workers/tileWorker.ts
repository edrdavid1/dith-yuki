/**
 * Tile Web Worker — fetches tiles from the tile:// custom protocol,
 * decodes raw RGBA8 bytes into ImageBitmaps, and transfers them back
 * to the main thread for canvas rendering.
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
}

export interface FetchTileMessage {
  type: 'fetch-tile';
  level: number;
  x: number;
  y: number;
  docId: number;
}

export type WorkerInMessage = RequestTilesMessage | FetchTileMessage;

export interface TileDecodedMessage {
  type: 'tile-decoded';
  key: string;
  bitmap: ImageBitmap;
}

export interface TilePendingMessage {
  type: 'tile-pending';
  key: string;
}

export interface TileErrorMessage {
  type: 'tile-error';
  key: string;
  error: string;
}

export type WorkerOutMessage = TileDecodedMessage | TilePendingMessage | TileErrorMessage;

const TILE_SIZE = 256;
const TILE_BYTE_LENGTH = TILE_SIZE * TILE_SIZE * 4; // 262,144 bytes RGBA8

/**
 * Build a tile:// URL for the given document and tile coordinates.
 * In Tauri's webview, the tile:// protocol URLs may be normalized to
 * tile://localhost/... so we use the format that Tauri expects.
 */
function buildTileUrl(docId: number, tile: TileRequest): string {
  return `tile://localhost/doc/${docId}/layer/composite/stage/composite/l/${tile.level}/${tile.x}/${tile.y}`;
}

/**
 * Fetch a single tile from the tile:// protocol, decode the raw RGBA8
 * bytes into an ImageBitmap, and post it back to the main thread.
 */
async function fetchAndDecodeTile(docId: number, tile: TileRequest): Promise<void> {
  const key = `${tile.level}/${tile.x}/${tile.y}`;
  const url = buildTileUrl(docId, tile);

  try {
    const response = await fetch(url);

    if (response.status === 200) {
      const buffer = await response.arrayBuffer();

      if (buffer.byteLength !== TILE_BYTE_LENGTH) {
        postMessage({
          type: 'tile-error',
          key,
          error: `Unexpected tile data size: ${buffer.byteLength} bytes (expected ${TILE_BYTE_LENGTH})`,
        } satisfies TileErrorMessage);
        return;
      }

      const imageData = new ImageData(
        new Uint8ClampedArray(buffer),
        TILE_SIZE,
        TILE_SIZE,
      );
      const bitmap = await createImageBitmap(imageData);

      // Transfer the bitmap (zero-copy) to the main thread
      const msg: TileDecodedMessage = { type: 'tile-decoded', key, bitmap };
      postMessage(msg, [bitmap]);
    } else if (response.status === 202) {
      // Tile is pending computation — main thread will re-request on tile-ready event
      postMessage({ type: 'tile-pending', key } satisfies TilePendingMessage);
    } else {
      // Non-recoverable error (404, 400, etc.)
      const body = await response.text().catch(() => '');
      postMessage({
        type: 'tile-error',
        key,
        error: `Tile fetch failed with status ${response.status}: ${body}`,
      } satisfies TileErrorMessage);
    }
  } catch (err) {
    // Network error or decode failure
    const message = err instanceof Error ? err.message : String(err);
    postMessage({
      type: 'tile-error',
      key,
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
    // Batch fetch: process all requested tiles sequentially to avoid
    // overwhelming the protocol handler with too many concurrent requests.
    for (const tile of msg.tiles) {
      await fetchAndDecodeTile(msg.docId, tile);
    }
  } else if (msg.type === 'fetch-tile') {
    await fetchAndDecodeTile(msg.docId, {
      level: msg.level,
      x: msg.x,
      y: msg.y,
    });
  }
};
