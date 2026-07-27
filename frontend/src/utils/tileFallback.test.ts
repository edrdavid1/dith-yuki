import { describe, it, expect } from 'vitest';
import { findFallbackTile, drawPlaceholder, drawErrorIndicator } from './tileFallback';

describe('findFallbackTile', () => {
  it('returns null when tileMap is empty', () => {
    const tileMap = new Map<string, ImageBitmap>();
    const result = findFallbackTile(0, 3, 2, tileMap);
    expect(result).toBeNull();
  });

  it('finds a tile one level up (parent)', () => {
    // Tile at level 0, (4, 6) → parent at level 1, (2, 3)
    const tileMap = new Map<string, ImageBitmap>();
    tileMap.set('1/2/3', {} as ImageBitmap);

    const result = findFallbackTile(0, 4, 6, tileMap);
    expect(result).not.toBeNull();
    expect(result!.key).toBe('1/2/3');
    // Level diff = 1, scale = 2, srcSize = 128
    // offsetX = 4 - 2*2 = 0, offsetY = 6 - 3*2 = 0
    expect(result!.srcSize).toBe(128);
    expect(result!.srcX).toBe(0);
    expect(result!.srcY).toBe(0);
  });

  it('computes correct quadrant offset for odd tile positions', () => {
    // Tile at level 0, (5, 7) → parent at level 1, (2, 3)
    // offsetX = 5 - 2*2 = 1, offsetY = 7 - 3*2 = 1
    const tileMap = new Map<string, ImageBitmap>();
    tileMap.set('1/2/3', {} as ImageBitmap);

    const result = findFallbackTile(0, 5, 7, tileMap);
    expect(result).not.toBeNull();
    expect(result!.key).toBe('1/2/3');
    expect(result!.srcSize).toBe(128);
    expect(result!.srcX).toBe(128);
    expect(result!.srcY).toBe(128);
  });

  it('finds a tile two levels up', () => {
    // Tile at level 0, (4, 4) → level 1 (2, 2), level 2 (1, 1)
    // No tile at level 1 → finds at level 2
    const tileMap = new Map<string, ImageBitmap>();
    tileMap.set('2/1/1', {} as ImageBitmap);

    const result = findFallbackTile(0, 4, 4, tileMap);
    expect(result).not.toBeNull();
    expect(result!.key).toBe('2/1/1');
    // Level diff = 2, scale = 4, srcSize = 64
    // offsetX = 4 - 1*4 = 0, offsetY = 4 - 1*4 = 0
    expect(result!.srcSize).toBe(64);
    expect(result!.srcX).toBe(0);
    expect(result!.srcY).toBe(0);
  });

  it('finds a tile two levels up with offset', () => {
    // Tile at level 0, (7, 5) → level 2 (1, 1)
    // offsetX = 7 - 1*4 = 3, offsetY = 5 - 1*4 = 1
    const tileMap = new Map<string, ImageBitmap>();
    tileMap.set('2/1/1', {} as ImageBitmap);

    const result = findFallbackTile(0, 7, 5, tileMap);
    expect(result).not.toBeNull();
    expect(result!.key).toBe('2/1/1');
    expect(result!.srcSize).toBe(64);
    expect(result!.srcX).toBe(192); // 3 * 64
    expect(result!.srcY).toBe(64);  // 1 * 64
  });

  it('prefers the closest available level', () => {
    // Both level 1 and level 2 tiles available; should pick level 1
    const tileMap = new Map<string, ImageBitmap>();
    tileMap.set('1/2/3', {} as ImageBitmap);
    tileMap.set('2/1/1', {} as ImageBitmap);

    const result = findFallbackTile(0, 4, 6, tileMap);
    expect(result).not.toBeNull();
    expect(result!.key).toBe('1/2/3');
  });

  it('handles level > 0 as the starting level', () => {
    // Looking for fallback for tile at level 2, (1, 0) → parent at level 3, (0, 0)
    const tileMap = new Map<string, ImageBitmap>();
    tileMap.set('3/0/0', {} as ImageBitmap);

    const result = findFallbackTile(2, 1, 0, tileMap);
    expect(result).not.toBeNull();
    expect(result!.key).toBe('3/0/0');
    expect(result!.srcSize).toBe(128);
    expect(result!.srcX).toBe(128); // offsetX = 1 - 0*2 = 1
    expect(result!.srcY).toBe(0);
  });
});

describe('drawPlaceholder', () => {
  it('fills with gray at the specified position', () => {
    const calls: Array<{ method: string; args: unknown[] }> = [];
    const ctx = {
      fillStyle: '',
      fillRect: (...args: unknown[]) => calls.push({ method: 'fillRect', args }),
    } as unknown as CanvasRenderingContext2D;

    drawPlaceholder(ctx, 100, 200, 256);

    expect(ctx.fillStyle).toBe('#808080');
    expect(calls).toHaveLength(1);
    expect(calls[0].args).toEqual([100, 200, 256, 256]);
  });
});

describe('drawErrorIndicator', () => {
  it('draws background, outline, and X pattern', () => {
    const calls: Array<{ method: string; args: unknown[] }> = [];
    const ctx = {
      fillStyle: '',
      strokeStyle: '',
      lineWidth: 0,
      fillRect: (...args: unknown[]) => calls.push({ method: 'fillRect', args }),
      strokeRect: (...args: unknown[]) => calls.push({ method: 'strokeRect', args }),
      beginPath: () => calls.push({ method: 'beginPath', args: [] }),
      moveTo: (...args: unknown[]) => calls.push({ method: 'moveTo', args }),
      lineTo: (...args: unknown[]) => calls.push({ method: 'lineTo', args }),
      stroke: () => calls.push({ method: 'stroke', args: [] }),
    } as unknown as CanvasRenderingContext2D;

    drawErrorIndicator(ctx, 0, 0, 100);

    // Should have fillRect (background), strokeRect (outline), beginPath, moveTo/lineTo (X), stroke
    const fillRects = calls.filter(c => c.method === 'fillRect');
    const strokeRects = calls.filter(c => c.method === 'strokeRect');
    const moveToLines = calls.filter(c => c.method === 'moveTo');

    expect(fillRects).toHaveLength(1);
    expect(strokeRects).toHaveLength(1);
    expect(moveToLines).toHaveLength(2); // Two strokes for the X
  });
});
