/** Apply replaces the selected document palette; otherwise it adds a new one. */
export function shouldReplaceSelectedPalette(
  selectedPaletteId: number | null,
  palettes: ReadonlyArray<{ id: number }>
): boolean {
  return selectedPaletteId !== null && palettes.some((p) => p.id === selectedPaletteId);
}

export function draftSignature(
  paletteId: number,
  name: string,
  colors: ReadonlyArray<{ hex: string }>
): string {
  return `${paletteId}|${name}|${colors.map((c) => c.hex).join(',')}`;
}

function colorsMatchPalette(
  paletteColors: ReadonlyArray<readonly [number, number, number]>,
  draft: ReadonlyArray<{ valid: boolean; r: number; g: number; b: number }>
): boolean {
  if (paletteColors.length !== draft.length) return false;
  return draft.every(
    (c, i) =>
      c.valid &&
      c.r === paletteColors[i][0] &&
      c.g === paletteColors[i][1] &&
      c.b === paletteColors[i][2]
  );
}

/** Push Color Lab draft into the bound document palette (live canvas preview). */
export function shouldLiveReplacePalette(
  selectedPaletteId: number | null,
  palettes: ReadonlyArray<{
    id: number;
    name: string;
    colors: ReadonlyArray<readonly [number, number, number]>;
  }>,
  name: string,
  colors: ReadonlyArray<{ valid: boolean; r: number; g: number; b: number }>
): boolean {
  if (selectedPaletteId == null) return false;
  if (colors.length === 0 || colors.some((c) => !c.valid)) return false;
  const trimmed = name.trim();
  if (!trimmed) return false;
  const palette = palettes.find((p) => p.id === selectedPaletteId);
  if (!palette) return false;
  if (palette.name === trimmed && colorsMatchPalette(palette.colors, colors)) return false;
  return true;
}
