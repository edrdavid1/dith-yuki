import { invoke } from '@tauri-apps/api/core';

export type AsciiExportFormat = 'txt' | 'ansi' | 'html' | 'svg' | 'png' | 'json';

export interface ExportAsciiRequest {
  doc_id: number;
  path: string;
  format: AsciiExportFormat;
  layer_id?: number | null;
}

export interface AsciiClipboardRequest {
  doc_id: number;
  format: 'txt' | 'ansi';
  layer_id?: number | null;
}

export async function exportAscii(req: ExportAsciiRequest): Promise<void> {
  return invoke<void>('export_ascii', { req });
}

export async function asciiClipboardText(req: AsciiClipboardRequest): Promise<string> {
  return invoke<string>('ascii_clipboard_text', { req });
}

/** Preview shows ASCII raster when true (default); Image when false. */
export async function setAsciiPreview(enabled: boolean): Promise<boolean> {
  return invoke<boolean>('set_ascii_preview', { enabled });
}

export async function getAsciiPreview(): Promise<boolean> {
  return invoke<boolean>('get_ascii_preview');
}
