/**
 * Palette id bound on a filter for Color Lab, when the filter participates
 * in palette binding (`palette_id` in params).
 *
 * - `number` — document palette
 * - `null` — Tone Steps / unbound dither
 * - `undefined` — filter does not use a palette (Curves, Levels, …)
 */
export function boundPaletteIdFromFilter(
  filter: { params: unknown } | null | undefined
): number | null | undefined {
  if (!filter) return undefined;
  const params = filter.params as Record<string, unknown> | null | undefined;
  if (!params || typeof params !== 'object' || !('palette_id' in params)) {
    return undefined;
  }
  return typeof params.palette_id === 'number' ? params.palette_id : null;
}
