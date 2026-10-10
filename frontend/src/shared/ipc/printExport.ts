import { invoke } from '@tauri-apps/api/core';
import type { SoftProofIntent } from './proof';

export type PrintExportFormat = 'tiff';
export type TiffCompression = 'none' | 'lzw';

export interface PrintExportConfig {
  format: PrintExportFormat;
  profile_id: string;
  intent: SoftProofIntent;
  bpc: boolean;
  ppi: number;
  scale: number;
  pure_black_k: boolean;
  compression: TiffCompression;
}

export interface PrintExportEstimate {
  width: number;
  height: number;
  out_width: number;
  out_height: number;
  ppi: number;
  width_mm: number;
  height_mm: number;
  unique_colors: number;
  uses_palette_path: boolean;
  uncompressed_bytes: number;
}

export interface GamutShiftedColor {
  rgb: [number, number, number];
  cmyk: [number, number, number, number];
  delta_e2000: number;
}

export interface GamutReport {
  unique_colors: number;
  out_of_gamut_fraction: number;
  shifted: GamutShiftedColor[];
  palette_collisions: number;
  max_ink_coverage: number;
  warning_palette_collapse: boolean;
}

export interface ExportSummary {
  out_width: number;
  out_height: number;
  unique_colors: number;
  used_palette_path: boolean;
  path: string;
}

export async function printExportEstimate(
  docId: number,
  config: PrintExportConfig
): Promise<PrintExportEstimate> {
  return invoke<PrintExportEstimate>('print_export_estimate', { docId, config });
}

export async function printExportGamutReport(
  docId: number,
  config: PrintExportConfig
): Promise<GamutReport> {
  return invoke<GamutReport>('print_export_gamut_report', { docId, config });
}

export async function printExportRun(
  docId: number,
  config: PrintExportConfig,
  path: string
): Promise<ExportSummary> {
  return invoke<ExportSummary>('print_export_run', { docId, config, path });
}

export async function printExportCancel(): Promise<void> {
  return invoke<void>('print_export_cancel');
}
