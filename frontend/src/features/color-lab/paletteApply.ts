/** Apply replaces the selected document palette; otherwise it adds a new one. */
export function shouldReplaceSelectedPalette(
  selectedPaletteId: number | null,
  palettes: ReadonlyArray<{ id: number }>
): boolean {
  return selectedPaletteId !== null && palettes.some((p) => p.id === selectedPaletteId);
}
