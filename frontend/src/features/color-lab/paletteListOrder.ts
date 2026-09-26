import type { BuiltinPaletteDto, PaletteDto } from '../../shared/ipc';

export const PALETTE_NEW_VALUE = 'new';
export const builtinKey = (id: string) => `builtin:${id}`;
export const savedKey = (id: number) => `saved:${id}`;

export type PaletteListEntry =
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

/** Full dropdown / manager list: New → builtins → saved. */
export function buildPaletteList(
  builtins: BuiltinPaletteDto[],
  saved: PaletteDto[]
): PaletteListEntry[] {
  const list: PaletteListEntry[] = [
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

/** ◀▶ browse order — real palettes only (skips Draft / New). */
export function buildBrowsablePaletteList(
  builtins: BuiltinPaletteDto[],
  saved: PaletteDto[]
): PaletteListEntry[] {
  return buildPaletteList(builtins, saved).filter((e) => e.kind !== 'new');
}

export function currentPaletteKey(selectedPaletteId: number | null): string {
  return selectedPaletteId !== null ? savedKey(selectedPaletteId) : PALETTE_NEW_VALUE;
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
  preferredIndex: number | null
): number {
  if (browseList.length === 0) return 0;
  if (
    preferredIndex != null &&
    preferredIndex >= 0 &&
    preferredIndex < browseList.length
  ) {
    return preferredIndex;
  }
  if (selectedPaletteId == null) return 0;
  const exact = browseList.findIndex(
    (e) => e.kind === 'saved' && e.id === selectedPaletteId
  );
  if (exact >= 0) return exact;
  const savedPal = saved.find((p) => p.id === selectedPaletteId);
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
