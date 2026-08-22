import { invoke } from '@tauri-apps/api/core';

function withDocId(docId: number, body: Record<string, unknown>) {
  // Only snake_case: serde alias `docId` on the same field rejects duplicates.
  return { ...body, doc_id: docId };
}

export interface PaletteDto {
  id: number;
  name: string;
  colors: [number, number, number][];
  hex_colors: string[];
  color_count: number;
}

export interface DeletePaletteResponse {
  affected_filter_ids: string[];
}

export async function listPalettes(): Promise<PaletteDto[]> {
  return invoke<PaletteDto[]>('list_palettes');
}

export interface BuiltinPaletteDto {
  id: string;
  name: string;
  colors: [number, number, number][];
  color_count: number;
}

export async function listBuiltinPalettes(): Promise<BuiltinPaletteDto[]> {
  return invoke<BuiltinPaletteDto[]>('list_builtin_palettes');
}

export async function importBuiltinPalette(docId: number, id: string): Promise<PaletteDto> {
  return invoke<PaletteDto>('import_builtin_palette', { docId, id });
}

export interface GeneratedColorDto {
  hex: string;
  r: number;
  g: number;
  b: number;
}

export async function generateRampPalette(
  fromHex: string,
  toHex: string,
  steps: number
): Promise<GeneratedColorDto[]> {
  return invoke<GeneratedColorDto[]>('generate_ramp_palette', {
    fromHex,
    toHex,
    steps,
  });
}

export type HarmonyRuleName =
  | 'Monochromatic'
  | 'Analogous'
  | 'Complementary'
  | 'Triadic'
  | 'SplitComplementary';

export interface OklabPointDto {
  l: number;
  a: number;
  b: number;
  srgb_hex: string;
}

/** Draft hex list → Oklab (Rust `oklab.rs`). Do not convert sRGB→Oklab in JS. */
export async function colorsToOklab(colors: string[]): Promise<OklabPointDto[]> {
  return invoke<OklabPointDto[]>('colors_to_oklab', { colors });
}

/** Saved document palette → Oklab (same math as `colorsToOklab`). */
export async function getPaletteOklab(paletteId: number): Promise<OklabPointDto[]> {
  return invoke<OklabPointDto[]>('get_palette_oklab', { paletteId });
}

export async function generateHarmonyPalette(
  baseHex: string,
  rule: HarmonyRuleName,
  count: number,
  analogousSpread?: number
): Promise<GeneratedColorDto[]> {
  return invoke<GeneratedColorDto[]>('generate_harmony_palette', {
    baseHex,
    rule,
    count,
    analogousSpread: analogousSpread ?? null,
  });
}

export async function importPalette(docId: number, path: string): Promise<PaletteDto> {
  return invoke<PaletteDto>('import_palette', { docId, path });
}

export async function addPalette(
  docId: number,
  name: string,
  colors: [number, number, number][]
): Promise<PaletteDto> {
  return invoke<PaletteDto>('add_palette', { req: withDocId(docId, { name, colors }) });
}

export async function generatePalette(
  docId: number,
  layerId: number,
  targetCount: number,
  method: string,
  weights?: { chromaWeight?: number; contrastWeight?: number }
): Promise<PaletteDto> {
  return invoke<PaletteDto>('generate_palette', {
    req: withDocId(docId, {
      layer_id: layerId,
      target_count: targetCount,
      method,
      chroma_weight: weights?.chromaWeight ?? 0,
      contrast_weight: weights?.contrastWeight ?? 0,
    }),
  });
}

export async function replacePalette(
  docId: number,
  paletteId: number,
  name: string,
  colors: [number, number, number][]
): Promise<PaletteDto> {
  return invoke<PaletteDto>('replace_palette', {
    req: withDocId(docId, { palette_id: paletteId, name, colors }),
  });
}

export async function removePalette(docId: number, paletteId: number): Promise<void> {
  return invoke<void>('remove_palette', { docId, paletteId });
}

export async function createPalette(docId: number, name: string): Promise<PaletteDto> {
  return invoke<PaletteDto>('create_palette', { req: withDocId(docId, { name }) });
}

export async function deletePalette(docId: number, paletteId: number): Promise<DeletePaletteResponse> {
  return invoke<DeletePaletteResponse>('delete_palette', { docId, paletteId });
}

export async function addColorToPalette(docId: number, paletteId: number, hex: string): Promise<PaletteDto> {
  return invoke<PaletteDto>('add_color_to_palette', {
    req: withDocId(docId, { palette_id: paletteId, hex }),
  });
}

export async function updatePaletteColor(
  docId: number,
  paletteId: number,
  index: number,
  hex: string
): Promise<PaletteDto> {
  return invoke<PaletteDto>('update_palette_color', {
    req: withDocId(docId, { palette_id: paletteId, index, hex }),
  });
}

export async function removePaletteColor(docId: number, paletteId: number, index: number): Promise<PaletteDto> {
  return invoke<PaletteDto>('remove_palette_color', {
    req: withDocId(docId, { palette_id: paletteId, index }),
  });
}

export async function reorderPaletteColor(
  docId: number,
  paletteId: number,
  fromIndex: number,
  toIndex: number
): Promise<PaletteDto> {
  return invoke<PaletteDto>('reorder_palette_color', {
    req: withDocId(docId, {
      palette_id: paletteId,
      from_index: fromIndex,
      to_index: toIndex,
    }),
  });
}

export async function renamePalette(docId: number, paletteId: number, name: string): Promise<PaletteDto> {
  return invoke<PaletteDto>('rename_palette', {
    req: withDocId(docId, { palette_id: paletteId, name }),
  });
}

export async function exportPalette(
  docId: number,
  paletteId: number,
  path: string,
  format: string
): Promise<void> {
  return invoke<void>('export_palette', {
    req: withDocId(docId, { palette_id: paletteId, path, format }),
  });
}
