import { isValidHex, parseHex } from '../../types/effects';

export interface PaletteData {
  name: string;
  colors: [number, number, number][];
}

export interface ColorEntry {
  hex: string;
  valid: boolean;
  r: number;
  g: number;
  b: number;
}

export type ExtractMethod = 'MedianCut' | 'KMeans';

export interface ColorLabDraftSnapshot {
  name: string;
  colors: ColorEntry[];
  extractMethod: ExtractMethod;
  extractCount: number;
  chromaWeight?: number;
  contrastWeight?: number;
}

export const MAX_COLORS = 256;

export function createColorEntry(hex: string): ColorEntry {
  const valid = isValidHex(hex);
  if (valid) {
    const [r, g, b] = parseHex(hex);
    return { hex, valid, r, g, b };
  }
  return { hex, valid, r: 0, g: 0, b: 0 };
}
