/**
 * Tile fallback display utilities.
 *
 * Handles:
 * 1. Finding the nearest lower-resolution pyramid tile to use as a fallback
 *    while a higher-resolution tile is still loading.
 * 2. Drawing a neutral gray placeholder when no fallback tile is available.
 * 3. Drawing an error indicator for permanently failed tiles.
 *
 * Requirements: 5.5, 5.6
 */

export interface FallbackTileInfo {
  /** Tile map key of the fallback tile (e.g. "2/1/0") */
  key: string;
  /** X pixel offset within the fallback tile to start sampling from */
  srcX: number;
  /** Y pixel offset within the fallback tile to start sampling from */
  srcY: number;
  /** Size of the region to sample from the fallback tile (in pixels) */
  srcSize: number;
}

/**
 * Find the nearest available lower-resolution pyramid tile in the tile map.
 *
 * At pyramid level L, the tile at (x, y) corresponds to tile (floor(x/2), floor(y/2))
 * at level L+1 (which is lower resolution). We walk up the pyramid levels until we
 * find a cached tile or exhaust available levels.
 *
 * @param level - Current pyramid level of the pending tile
 * @param x - Tile column index at the current level
 * @param y - Tile row index at the current level
 * @param tileMap - Map of tile keys ("level/x/y") to ImageBitmap
 * @returns FallbackTileInfo describing which portion of the fallback tile to draw, or null
 */
export function findFallbackTile(
  level: number,
  x: number,
  y: number,
  tileMap: Map<string, ImageBitmap>
): FallbackTileInfo | null {
  // Walk up the pyramid: each level up halves the resolution
  // (tile at level L covers same area as 2×2 region at level L-1)
  let currentX = x;
  let currentY = y;
  let stepsUp = 0;

  for (let parentLevel = level + 1; parentLevel <= level + 8; parentLevel++) {
    // The parent tile that covers this tile's area
    const parentX = Math.floor(currentX / 2);
    const parentY = Math.floor(currentY / 2);

    // Which quadrant of the parent tile does our current position fall in?
    const quadX = currentX - parentX * 2; // 0 or 1
    const quadY = currentY - parentY * 2; // 0 or 1

    stepsUp++;

    const key = `${parentLevel}/${parentX}/${parentY}`;
    if (tileMap.has(key)) {
      // Found a cached lower-res tile. Compute the source region.
      // Each step up halves the visible portion: at 1 step up, we need
      // a 128px region; at 2 steps, a 64px region, etc.
      const srcSize = Math.floor(256 / (1 << stepsUp));
      const srcX = quadX * srcSize;
      const srcY = quadY * srcSize;

      // But we need to account for intermediate steps.
      // If we went up multiple levels, we need to compute the cumulative offset.
      return computeFallbackRegion(level, x, y, parentLevel, parentX, parentY);
    }

    // Move up for next iteration
    currentX = parentX;
    currentY = parentY;
  }

  return null;
}

/**
 * Compute the exact source region within a fallback tile.
 * The fallback tile at a higher level covers a larger document area;
 * we need the sub-region that corresponds to our target tile.
 */
function computeFallbackRegion(
  targetLevel: number,
  targetX: number,
  targetY: number,
  fallbackLevel: number,
  fallbackX: number,
  fallbackY: number
): FallbackTileInfo {
  const levelDiff = fallbackLevel - targetLevel;
  const scale = 1 << levelDiff; // 2^levelDiff

  // The size of the source region in the fallback tile
  // (256 pixels in the fallback tile covers `scale` tiles at the target level)
  const srcSize = Math.floor(256 / scale);

  // The offset within the fallback tile:
  // target tile's position relative to the fallback tile's coverage
  const offsetX = targetX - fallbackX * scale;
  const offsetY = targetY - fallbackY * scale;

  const srcX = offsetX * srcSize;
  const srcY = offsetY * srcSize;

  return {
    key: `${fallbackLevel}/${fallbackX}/${fallbackY}`,
    srcX,
    srcY,
    srcSize,
  };
}

/** Neutral gray color used for placeholders (#808080) */
const PLACEHOLDER_COLOR = '#808080';

/**
 * Draw a neutral gray placeholder rectangle at the given tile position.
 * Used when no pyramid tile is available as a fallback.
 *
 * @param ctx - Canvas 2D rendering context
 * @param screenX - Screen X position to draw at
 * @param screenY - Screen Y position to draw at
 * @param size - Size of the placeholder square in screen pixels
 */
export function drawPlaceholder(
  ctx: CanvasRenderingContext2D,
  screenX: number,
  screenY: number,
  size: number
): void {
  ctx.fillStyle = PLACEHOLDER_COLOR;
  ctx.fillRect(screenX, screenY, size, size);
}

/** Error indicator outline color */
const ERROR_OUTLINE_COLOR = '#cc3333';
/** Error indicator X-pattern color */
const ERROR_X_COLOR = '#cc3333';

/**
 * Draw an error indicator at the given tile position.
 * Renders a red outline with an X pattern to clearly indicate failure.
 *
 * @param ctx - Canvas 2D rendering context
 * @param screenX - Screen X position to draw at
 * @param screenY - Screen Y position to draw at
 * @param size - Size of the error indicator square in screen pixels
 */
export function drawErrorIndicator(
  ctx: CanvasRenderingContext2D,
  screenX: number,
  screenY: number,
  size: number
): void {
  // Draw gray background first
  ctx.fillStyle = PLACEHOLDER_COLOR;
  ctx.fillRect(screenX, screenY, size, size);

  // Draw red outline
  ctx.strokeStyle = ERROR_OUTLINE_COLOR;
  ctx.lineWidth = 2;
  ctx.strokeRect(screenX + 1, screenY + 1, size - 2, size - 2);

  // Draw X pattern
  ctx.beginPath();
  ctx.strokeStyle = ERROR_X_COLOR;
  ctx.lineWidth = 1.5;
  const margin = Math.max(4, size * 0.15);
  ctx.moveTo(screenX + margin, screenY + margin);
  ctx.lineTo(screenX + size - margin, screenY + size - margin);
  ctx.moveTo(screenX + size - margin, screenY + margin);
  ctx.lineTo(screenX + margin, screenY + size - margin);
  ctx.stroke();
}
