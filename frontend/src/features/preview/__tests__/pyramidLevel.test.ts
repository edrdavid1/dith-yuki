import { describe, expect, it } from 'vitest';
import {
  computePyramidLevel,
  computeTileBlit,
  computeVisibleTiles,
  shouldCommitTileRefresh,
  shouldAcceptDecodedRev,
  isDocumentSourceReplace,
  tilesToRequestAfterDocumentChange,
  COMMIT_WAIT_MS,
  type ViewportState,
} from '../TileCanvas';

function vp(partial: Partial<ViewportState> & Pick<ViewportState, 'zoom'>): ViewportState {
  return {
    panX: 0,
    panY: 0,
    canvasWidth: 1200,
    canvasHeight: 900,
    ...partial,
  };
}

describe('computePyramidLevel', () => {
  it('uses level 0 at 100% and above', () => {
    expect(computePyramidLevel(1, 3000, 3000)).toBe(0);
    expect(computePyramidLevel(2, 3000, 3000)).toBe(0);
  });

  it('matches backend floor(log2(1/zoom)) for a 3000 canvas', () => {
    expect(computePyramidLevel(0.5, 3000, 3000)).toBe(1);
    expect(computePyramidLevel(0.25, 3000, 3000)).toBe(2);
    expect(computePyramidLevel(0.125, 3000, 3000)).toBe(3);
  });

  it('clamps to max level for the document', () => {
    expect(computePyramidLevel(0.01, 512, 512)).toBe(1);
    expect(computePyramidLevel(0.5, 256, 256)).toBe(0);
  });
});

describe('computeVisibleTiles LOD', () => {
  it('requests 9 tiles for a 3000×3000 fit at 25% (level 2)', () => {
    const tiles = computeVisibleTiles(vp({ zoom: 0.25 }), 3000, 3000);
    expect(tiles).toHaveLength(9);
    expect(tiles.every((t) => t.level === 2)).toBe(true);
  });

  it('requests the full 12×12 grid only at level 0', () => {
    const tiles = computeVisibleTiles(vp({ zoom: 1, canvasWidth: 3000, canvasHeight: 3000 }), 3000, 3000);
    expect(tiles).toHaveLength(144);
    expect(tiles.every((t) => t.level === 0)).toBe(true);
  });
});

describe('computeTileBlit', () => {
  it('clips the last L0 tile of a 3000 canvas to 184 source pixels', () => {
    const blit = computeTileBlit(11, 11, 0, vp({ zoom: 2, canvasWidth: 800, canvasHeight: 800 }), 3000, 3000, 1);
    expect(blit).not.toBeNull();
    expect(blit!.sw).toBe(184);
    expect(blit!.sh).toBe(184);
    expect(blit!.dw).toBe(368);
    expect(blit!.dh).toBe(368);
  });

  it('draws a full interior L0 tile as 256² source', () => {
    const blit = computeTileBlit(0, 0, 0, vp({ zoom: 2 }), 3000, 3000, 1);
    expect(blit).not.toBeNull();
    expect(blit!.sw).toBe(256);
    expect(blit!.sh).toBe(256);
    expect(blit!.dw).toBe(512);
    expect(blit!.dh).toBe(512);
  });

  it('does not emit a quad past the document', () => {
    expect(computeTileBlit(20, 0, 0, vp({ zoom: 1 }), 3000, 3000, 1)).toBeNull();
  });

  it('abuts adjacent tiles with no dest gap or overlap', () => {
    const a = computeTileBlit(0, 0, 0, vp({ zoom: 1, panX: 0.3, panY: 0.7 }), 3000, 3000, 1);
    const b = computeTileBlit(1, 0, 0, vp({ zoom: 1, panX: 0.3, panY: 0.7 }), 3000, 3000, 1);
    const c = computeTileBlit(0, 1, 0, vp({ zoom: 1, panX: 0.3, panY: 0.7 }), 3000, 3000, 1);
    expect(a).not.toBeNull();
    expect(b).not.toBeNull();
    expect(c).not.toBeNull();
    expect(a!.dx + a!.dw).toBe(b!.dx);
    expect(a!.dy + a!.dh).toBe(c!.dy);
  });

  it('keeps a uniform device-pixel cell size across tiles at integer zoom×dpr', () => {
    const dpr = 2;
    const view = vp({ zoom: 1, panX: 0.4, panY: 0.6 });
    const a = computeTileBlit(0, 0, 0, view, 3000, 3000, dpr);
    const b = computeTileBlit(1, 0, 0, view, 3000, 3000, dpr);
    expect(a!.dw).toBe(256 * dpr);
    expect(b!.dw).toBe(256 * dpr);
    expect(a!.dx + a!.dw).toBe(b!.dx);
  });
});

describe('COMMIT_WAIT_MS', () => {
  it('is a finite safety valve, not an open wait', () => {
    expect(COMMIT_WAIT_MS).toBe(100);
  });
});

describe('shouldAcceptDecodedRev', () => {
  it('drops bitmaps from an older filter generation', () => {
    expect(shouldAcceptDecodedRev(3, 4)).toBe(false);
    expect(shouldAcceptDecodedRev(undefined, 1)).toBe(false);
  });

  it('accepts current and newer rev', () => {
    expect(shouldAcceptDecodedRev(4, 4)).toBe(true);
    expect(shouldAcceptDecodedRev(5, 4)).toBe(true);
    expect(shouldAcceptDecodedRev(0, 0)).toBe(true);
  });
});

describe('shouldCommitTileRefresh', () => {
  it('waits until every on-screen tile has a replacement', () => {
    const visible = ['2/0/0', '2/1/0', '2/0/1'];
    expect(shouldCommitTileRefresh(visible, ['2/0/0', '2/1/0'], visible)).toBe(false);
    expect(shouldCommitTileRefresh(visible, visible, visible)).toBe(true);
  });

  it('ignores tiles that are not on screen yet (first paint stays progressive)', () => {
    expect(shouldCommitTileRefresh([], ['2/0/0'], ['2/0/0'])).toBe(false);
  });

  it('does not wait for off-screen tiles', () => {
    const displayed = ['2/0/0', '2/1/0', '2/2/0'];
    const visible = ['2/0/0', '2/1/0'];
    const pending = ['2/0/0', '2/1/0'];
    expect(shouldCommitTileRefresh(displayed, pending, visible)).toBe(true);
  });
});

describe('document source replace refetch', () => {
  it('treats open/create/project/undo as source replace', () => {
    expect(isDocumentSourceReplace('image_loaded')).toBe(true);
    expect(isDocumentSourceReplace('document_created')).toBe(true);
    expect(isDocumentSourceReplace('project_opened')).toBe(true);
    expect(isDocumentSourceReplace('document_undone')).toBe(true);
    expect(isDocumentSourceReplace('filter_updated')).toBe(false);
  });

  it('requests already-displayed keys when the source is replaced at the same docId', () => {
    const visible = [
      { level: 0, x: 0, y: 0 },
      { level: 0, x: 1, y: 0 },
    ];
    const displayed = ['0/0/0'];
    expect(tilesToRequestAfterDocumentChange(visible, displayed, false)).toEqual([
      { level: 0, x: 1, y: 0 },
    ]);
    expect(tilesToRequestAfterDocumentChange(visible, displayed, true)).toEqual(visible);
  });
});
