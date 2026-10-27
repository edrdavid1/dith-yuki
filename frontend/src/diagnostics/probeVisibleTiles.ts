/**
 * DITHER_TRACE diagnose: compare FE displayed gen vs BE Composite/Processed.
 * Step A2 — if BE Composite is fresh at live doc_gen but FE still shows old
 * pixels, FE gen gates are blind to palette; if BE is dirty/old fingerprint,
 * BE lost schedule after invalidate / strip discard.
 */

import { invoke } from '@tauri-apps/api/core';
import { noteTraceEvent, tileGenTraceEnabled } from './tileGenTrace';
import { probeCacheTile } from './viewportTraceChecker';

export type VisibleTileProbeRow = {
  tileKey: string;
  fe_gen: number | null;
  fe_maxKnown: number | null;
  be: Record<string, unknown> | null;
  /** Heuristic: BE Composite fresh at doc_gen ⇒ FE should have refreshed. */
  blame: 'fe_likely' | 'be_likely' | 'unknown' | 'ok';
};

export async function probeVisibleTiles(args: {
  getVisibleKeys: () => string[];
  getTileGens?: () => Record<string, number>;
  getMaxKnownGen?: () => number;
  layerId?: number | null;
}): Promise<{ rows: VisibleTileProbeRow[]; summary: string }> {
  if (!tileGenTraceEnabled()) {
    return { rows: [], summary: 'Need DITHER_TRACE=1' };
  }
  const keys = args.getVisibleKeys();
  const gens = args.getTileGens?.() ?? {};
  const maxKnown = args.getMaxKnownGen?.() ?? null;
  const rows: VisibleTileProbeRow[] = [];
  let feLikely = 0;
  let beLikely = 0;
  let ok = 0;

  for (const key of keys) {
    const [ls, xs, ys] = key.split('/');
    const be = await probeCacheTile(
      Number(ls),
      Number(xs),
      Number(ys),
      args.layerId ?? undefined,
    );
    const fe_gen = gens[key] ?? null;
    const beFresh = be?.fresh === true;
    const beDirty = be?.dirty === true;
    const compositeGen = (be?.composite as { tile_gen?: number } | null | undefined)?.tile_gen;
    const beGen =
      typeof be?.tile_gen === 'number'
        ? be.tile_gen
        : typeof compositeGen === 'number'
          ? compositeGen
          : null;
    const docGen = typeof be?.doc_gen === 'number' ? be.doc_gen : null;

    let blame: VisibleTileProbeRow['blame'] = 'unknown';
    if (beFresh && docGen != null && beGen != null && beGen >= docGen) {
      // BE looks current by gen; sticky pixels ⇒ FE did not refetch (gen-blind).
      if (fe_gen != null && maxKnown != null && fe_gen >= maxKnown && fe_gen === beGen) {
        blame = 'fe_likely';
        feLikely += 1;
      } else if (fe_gen != null && docGen != null && fe_gen < docGen) {
        blame = 'fe_likely';
        feLikely += 1;
      } else {
        blame = 'ok';
        ok += 1;
      }
    } else if (beDirty || be == null || be.present === false) {
      blame = 'be_likely';
      beLikely += 1;
    } else {
      blame = 'unknown';
    }

    const row: VisibleTileProbeRow = {
      tileKey: key,
      fe_gen,
      fe_maxKnown: maxKnown,
      be,
      blame,
    };
    rows.push(row);
    noteTraceEvent('visible_tile_probe', {
      level: Number(ls),
      x: Number(xs),
      y: Number(ys),
      gen: fe_gen ?? undefined,
      details: {
        fe_gen,
        fe_maxKnown: maxKnown,
        blame,
        be,
      },
    });
  }

  noteTraceEvent('visible_tile_probe_summary', {
    details: {
      visible: keys.length,
      fe_likely: feLikely,
      be_likely: beLikely,
      ok,
      // Palette / soft_proof never raise FE maxKnownGen.
      note: 'If stuck tile has be.fresh && fe_gen==doc_gen → FE gen-blind (palette path)',
    },
  });

  const summary =
    `probed ${keys.length}: ok=${ok} fe_likely=${feLikely} be_likely=${beLikely}` +
    (maxKnown != null ? ` maxKnown=${maxKnown}` : '');
  return { rows, summary };
}

/** Optional: ask BE for current palette revisions (active session). */
export async function fetchPaletteRevisions(): Promise<
  Array<{ id: number; revision: number }> | null
> {
  if (!tileGenTraceEnabled()) return null;
  try {
    // Reuse first visible probe payload if needed — no dedicated command.
    const v = await invoke<{ palette_revisions?: Array<{ id: number; revision: number }> }>(
      'tile_gen_trace_cache_probe',
      { req: { level: 0, x: 0, y: 0 } },
    );
    return v?.palette_revisions ?? null;
  } catch {
    return null;
  }
}
