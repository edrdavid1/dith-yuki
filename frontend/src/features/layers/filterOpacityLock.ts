import type { FilterInfo, PaletteDitherMode } from '../../types';

/** Strict / Mixed with a bound palette — Fade would leave non-palette colors. */
export function ditherFilterLocksOpacity(filter: FilterInfo | null | undefined): boolean {
  if (!filter) return false;
  if (filter.kind !== 'DitherV2' && filter.kind !== 'Dither') return false;
  const params = filter.params as {
    type?: string;
    palette_id?: number | null;
    palette_dither_mode?: PaletteDitherMode;
  };
  if (params.palette_id == null) return false;
  const mode = params.palette_dither_mode ?? 'strict';
  if (mode === 'strict') return true;
  if (mode === 'simple') return false;
  if (typeof mode === 'object' && mode !== null && 'mixed' in mode) return true;
  return false;
}
