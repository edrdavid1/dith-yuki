/** Externally-tagged serde keys used by `FilterParams`. */
const TAGGED_KEYS = [
  'DitherV2',
  'Glow',
  'Crt',
  'Adjust',
  'Curves',
  'Levels',
  'Glitch',
  'Dither',
  'PaletteQuantize',
] as const;

/**
 * Snapshot IPC serializes `FilterParams` as `{ Curves: { curve, channel } }`
 * (externally tagged). Editors read flat fields (`params.curve`).
 */
export function unwrapFilterParams(params: Record<string, unknown>): Record<string, unknown> {
  for (const key of TAGGED_KEYS) {
    const inner = params[key];
    if (inner && typeof inner === 'object' && !Array.isArray(inner)) {
      return inner as Record<string, unknown>;
    }
  }
  return params;
}
