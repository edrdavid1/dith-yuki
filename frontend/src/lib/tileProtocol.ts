/**
 * Tauri custom-protocol URL origin for the `tile` scheme.
 *
 * WebView2 (Windows) and Android do not expose real custom schemes; Tauri
 * serves them as `http://{scheme}.localhost/...`. macOS/Linux use a real
 * `tile://localhost/...` scheme. See Tauri docs for
 * `register_uri_scheme_protocol`.
 */

export type TileProtocolPlatform = 'macos' | 'windows' | 'linux' | 'android' | 'unknown' | string;

/** Origin only — no trailing slash. */
export function tileProtocolBase(platform?: TileProtocolPlatform): string {
  if (platform === 'windows' || platform === 'android') {
    return 'http://tile.localhost';
  }
  if (platform == null || platform === 'unknown') {
    const ua =
      typeof globalThis.navigator !== 'undefined'
        ? globalThis.navigator.userAgent
        : '';
    if (/Windows|Android/i.test(ua)) {
      return 'http://tile.localhost';
    }
  }
  return 'tile://localhost';
}

export interface TileCoord {
  level: number;
  x: number;
  y: number;
}

/**
 * Build a fetchable tile URL for the composite stage.
 * Pass `platform` (or `base`) from the main thread when known; the worker
 * can fall back to userAgent detection.
 */
export function buildTileUrl(
  docId: number,
  tile: TileCoord,
  rev?: number,
  opts?: { platform?: TileProtocolPlatform; base?: string },
): string {
  const origin = opts?.base ?? tileProtocolBase(opts?.platform);
  const path = `doc/${docId}/layer/composite/stage/composite/l/${tile.level}/${tile.x}/${tile.y}`;
  const base = `${origin}/${path}`;
  return typeof rev === 'number' ? `${base}?g=${rev}` : base;
}
