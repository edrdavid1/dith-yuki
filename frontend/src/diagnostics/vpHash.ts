/**
 * Viewport visible-set hash — must match Rust `tile_gen_trace::vp_hash`.
 * FNV-1a 64 over little-endian (level, x0, y0, x1, y1).
 */

export type TileRange = {
  level: number;
  x0: number;
  y0: number;
  x1: number;
  y1: number;
};

export function vpHash(level: number, x0: number, y0: number, x1: number, y1: number): string {
  let h = 0xcbf29ce484222325n;
  const parts = [BigInt(level >>> 0), BigInt(x0 >>> 0), BigInt(y0 >>> 0), BigInt(x1 >>> 0), BigInt(y1 >>> 0)];
  for (const v of parts) {
    for (let i = 0; i < 8; i++) {
      const b = (v >> BigInt(8 * i)) & 0xffn;
      h ^= b;
      h = BigInt.asUintN(64, h * 0x100000001b3n);
    }
  }
  return h.toString(16).padStart(16, '0');
}

export function tileRangeFromCoords(
  level: number,
  tiles: Array<{ level: number; x: number; y: number }>,
): TileRange & { vp_hash: string; count: number } {
  if (tiles.length === 0) {
    const h = vpHash(level, 0, 0, 0, 0);
    return { level, x0: 0, y0: 0, x1: 0, y1: 0, vp_hash: h, count: 0 };
  }
  let x0 = Infinity;
  let y0 = Infinity;
  let x1 = -Infinity;
  let y1 = -Infinity;
  for (const t of tiles) {
    x0 = Math.min(x0, t.x);
    y0 = Math.min(y0, t.y);
    x1 = Math.max(x1, t.x);
    y1 = Math.max(y1, t.y);
  }
  const xi0 = x0 >>> 0;
  const yi0 = y0 >>> 0;
  const xi1 = x1 >>> 0;
  const yi1 = y1 >>> 0;
  return {
    level,
    x0: xi0,
    y0: yi0,
    x1: xi1,
    y1: yi1,
    vp_hash: vpHash(level, xi0, yi0, xi1, yi1),
    count: tiles.length,
  };
}
