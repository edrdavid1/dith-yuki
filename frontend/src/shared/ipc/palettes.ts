import { invoke } from '@tauri-apps/api/core';

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

export async function importBuiltinPalette(id: string): Promise<PaletteDto> {
  return invoke<PaletteDto>('import_builtin_palette', { id });
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

export async function importPalette(path: string): Promise<PaletteDto> {
  return invoke<PaletteDto>('import_palette', { path });
}

export async function addPalette(
  name: string,
  colors: [number, number, number][]
): Promise<PaletteDto> {
  return invoke<PaletteDto>('add_palette', { req: { name, colors } });
}

export async function generatePalette(
  layerId: number,
  targetCount: number,
  method: string,
  weights?: { chromaWeight?: number; contrastWeight?: number }
): Promise<PaletteDto> {
  return invoke<PaletteDto>('generate_palette', {
    req: {
      layer_id: layerId,
      target_count: targetCount,
      method,
      chroma_weight: weights?.chromaWeight ?? 0,
      contrast_weight: weights?.contrastWeight ?? 0,
    },
  });
}

export async function replacePalette(
  paletteId: number,
  name: string,
  colors: [number, number, number][]
): Promise<PaletteDto> {
  return invoke<PaletteDto>('replace_palette', {
    req: { palette_id: paletteId, name, colors },
  });
}

export async function removePalette(paletteId: number): Promise<void> {
  return invoke<void>('remove_palette', { paletteId });
}

export async function createPalette(name: string): Promise<PaletteDto> {
  return invoke<PaletteDto>('create_palette', { req: { name } });
}

export async function deletePalette(paletteId: number): Promise<DeletePaletteResponse> {
  return invoke<DeletePaletteResponse>('delete_palette', { paletteId });
}

export async function addColorToPalette(paletteId: number, hex: string): Promise<PaletteDto> {
  return invoke<PaletteDto>('add_color_to_palette', {
    req: { palette_id: paletteId, hex },
  });
}

export async function updatePaletteColor(
  paletteId: number,
  index: number,
  hex: string
): Promise<PaletteDto> {
  return invoke<PaletteDto>('update_palette_color', {
    req: { palette_id: paletteId, index, hex },
  });
}

export async function removePaletteColor(paletteId: number, index: number): Promise<PaletteDto> {
  return invoke<PaletteDto>('remove_palette_color', {
    req: { palette_id: paletteId, index },
  });
}

export async function reorderPaletteColor(
  paletteId: number,
  fromIndex: number,
  toIndex: number
): Promise<PaletteDto> {
  return invoke<PaletteDto>('reorder_palette_color', {
    req: { palette_id: paletteId, from_index: fromIndex, to_index: toIndex },
  });
}

export async function renamePalette(paletteId: number, name: string): Promise<PaletteDto> {
  return invoke<PaletteDto>('rename_palette', {
    req: { palette_id: paletteId, name },
  });
}

export async function exportPalette(
  paletteId: number,
  path: string,
  format: string
): Promise<void> {
  return invoke<void>('export_palette', {
    req: { palette_id: paletteId, path, format },
  });
}
