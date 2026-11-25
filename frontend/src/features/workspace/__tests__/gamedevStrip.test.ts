/**
 * Release slice: Export reads cell + PNG8 from GamedevContext (Tile + Sprites).
 */

import { describe, expect, it } from 'vitest';
import {
  defaultGamedevContext,
  type GamedevContext,
} from '../../../shared/ipc/gamedev';

/** Pure prefill helper — mirrors ExportImageDialog open behavior. */
export function exportPrefillFromGamedev(ctx: GamedevContext): {
  formatHint: 'PNG' | 'PNG8';
  spriteCells: boolean;
  cellW: number;
  cellH: number;
} {
  return {
    formatHint: ctx.export.png8 ? 'PNG8' : 'PNG',
    spriteCells: ctx.cell != null,
    cellW: ctx.cell?.w ?? 32,
    cellH: ctx.cell?.h ?? 32,
  };
}

/** Release strip status order — Animation `stable` chip is deferred. */
export const RELEASE_CHIP_ORDER = ['seamless', 'cells', 'alpha'] as const;

/** Normalize deferred Animation fields (mirrors GamedevContext attach). */
export function withoutDeferredAnimation(ctx: GamedevContext): GamedevContext {
  const hasAnim =
    ctx.job === 'animation' || (ctx.frame_set != null && ctx.frame_set.length > 0);
  if (!hasAnim) return ctx;
  return {
    ...ctx,
    job: ctx.job === 'animation' ? 'none' : ctx.job,
    frame_set: [],
  };
}

describe('exportPrefillFromGamedev', () => {
  it('defaults when context empty', () => {
    const p = exportPrefillFromGamedev(defaultGamedevContext());
    expect(p.formatHint).toBe('PNG');
    expect(p.spriteCells).toBe(false);
    expect(p.cellW).toBe(32);
  });

  it('reads cell and png8 from context', () => {
    const ctx = defaultGamedevContext();
    ctx.export.png8 = true;
    ctx.cell = { w: 48, h: 24, padding: 0, independent: true };
    const p = exportPrefillFromGamedev(ctx);
    expect(p.formatHint).toBe('PNG8');
    expect(p.spriteCells).toBe(true);
    expect(p.cellW).toBe(48);
    expect(p.cellH).toBe(24);
  });
});

describe('release slice Tile + Sprites', () => {
  it('chip order excludes stable', () => {
    expect(RELEASE_CHIP_ORDER).toEqual(['seamless', 'cells', 'alpha']);
    expect(RELEASE_CHIP_ORDER).not.toContain('stable');
  });

  it('clears animation job and frame_set', () => {
    const ctx = defaultGamedevContext();
    ctx.job = 'animation';
    ctx.frame_set = ['/a.png', '/b.png'];
    const cleaned = withoutDeferredAnimation(ctx);
    expect(cleaned.job).toBe('none');
    expect(cleaned.frame_set).toEqual([]);
  });

  it('leaves tile/sprite context alone', () => {
    const ctx = defaultGamedevContext();
    ctx.job = 'tile';
    ctx.tile = { enabled: true };
    expect(withoutDeferredAnimation(ctx)).toBe(ctx);
  });
});
