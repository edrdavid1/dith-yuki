import type { BuiltinPaletteDto, PaletteDto } from '../../shared/ipc';

export const PALETTE_NEW_VALUE = 'new';
export const PALETTE_TONE_STEPS_VALUE = 'tone-steps';
export const builtinKey = (id: string) => `builtin:${id}`;
export const savedKey = (id: number) => `saved:${id}`;

export type UnboundPicker = 'tone-steps' | 'new';

export type PaletteListEntry =
  | {
      kind: 'tone-steps';
      key: typeof PALETTE_TONE_STEPS_VALUE;
      name: string;
    }
  | { kind: 'new'; key: typeof PALETTE_NEW_VALUE; name: string }
  | {
      kind: 'builtin';
      key: string;
      id: string;
      name: string;
      colors: [number, number, number][];
    }
  | {
      kind: 'saved';
      key: string;
      id: number;
      name: string;
      colors: [number, number, number][];
    };

/** Full dropdown / manager list: Raw tone → New → builtins → saved. */
export function buildPaletteList(
  builtins: BuiltinPaletteDto[],
  saved: PaletteDto[]
): PaletteListEntry[] {
  const list: PaletteListEntry[] = [
    {
      kind: 'tone-steps',
      key: PALETTE_TONE_STEPS_VALUE,
      name: 'Raw tone',
    },
    { kind: 'new', key: PALETTE_NEW_VALUE, name: 'New palette' },
  ];
  for (const p of builtins) {
    list.push({
      kind: 'builtin',
      key: builtinKey(p.id),
      id: p.id,
      name: p.name,
      colors: p.colors,
    });
  }
  for (const p of saved) {
    list.push({
      kind: 'saved',
      key: savedKey(p.id),
      id: p.id,
      name: p.name,
      colors: p.colors,
    });
  }
  return list;
}

/** ◀▶ browse order — real bindings only (Raw tone + palettes; skips Draft / New). */
export function buildBrowsablePaletteList(
  builtins: BuiltinPaletteDto[],
  saved: PaletteDto[]
): PaletteListEntry[] {
  return buildPaletteList(builtins, saved).filter((e) => e.kind !== 'new');
}

/**
 * Dropdown / manager selection key.
 * Bound saved id wins; otherwise unbound picker is Raw tone (dither/glitch
 * uniform levels) or New (draft).
 */
export function currentPaletteKey(
  selectedPaletteId: number | null,
  unboundPicker: UnboundPicker = 'tone-steps',
  boundPaletteId: number | null = null
): string {
  if (selectedPaletteId !== null) return savedKey(selectedPaletteId);
  if (unboundPicker === 'new') return PALETTE_NEW_VALUE;
  if (boundPaletteId != null) return savedKey(boundPaletteId);
  return PALETTE_TONE_STEPS_VALUE;
}

/**
 * Index for ◀▶ browsing. Prefer an exact key match; if the active item is a
 * saved copy of a builtin (same name), stay on that builtin so stepping
 * continues through the built-in list instead of jumping to the saved tail.
 */
export function browseIndexForSelection(
  browseList: PaletteListEntry[],
  selectedPaletteId: number | null,
  saved: PaletteDto[],
  preferredIndex: number | null,
  unboundPicker: UnboundPicker = 'tone-steps',
  boundPaletteId: number | null = null
): number {
  if (browseList.length === 0) return 0;
  if (
    preferredIndex != null &&
    preferredIndex >= 0 &&
    preferredIndex < browseList.length
  ) {
    return preferredIndex;
  }
  const key = currentPaletteKey(selectedPaletteId, unboundPicker, boundPaletteId);
  const byKey = browseList.findIndex((e) => e.key === key);
  if (byKey >= 0) return byKey;
  if (selectedPaletteId == null && boundPaletteId == null) {
    const tone = browseList.findIndex((e) => e.kind === 'tone-steps');
    return tone >= 0 ? tone : 0;
  }
  const id = selectedPaletteId ?? boundPaletteId;
  if (id == null) return 0;
  const exact = browseList.findIndex((e) => e.kind === 'saved' && e.id === id);
  if (exact >= 0) return exact;
  const savedPal = saved.find((p) => p.id === id);
  if (savedPal) {
    const builtinMatch = browseList.findIndex(
      (e) =>
        e.kind === 'builtin' &&
        e.name.toLowerCase() === savedPal.name.toLowerCase()
    );
    if (builtinMatch >= 0) return builtinMatch;
  }
  return 0;
}

export function stepBrowseIndex(
  length: number,
  from: number,
  delta: -1 | 1
): number {
  if (length <= 0) return 0;
  return (from + delta + length) % length;
}
