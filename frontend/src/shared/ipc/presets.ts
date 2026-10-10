import { invoke } from '@tauri-apps/api/core';

export interface PresetLibraryEntry {
  id: string;
  name: string;
  created_at: string;
  path: string;
  filter_count: number;
  palette_ids: number[];
}

export interface PresetApplyResponse {
  layer_id: number;
  filter_count: number;
  warnings: string[];
  missing_palette_ids: number[];
}

export async function presetList(): Promise<PresetLibraryEntry[]> {
  return invoke<PresetLibraryEntry[]>('preset_list');
}

export async function presetSave(args: {
  docId: number;
  layerId?: number | null;
  name: string;
  presetId?: string | null;
}): Promise<PresetLibraryEntry> {
  return invoke<PresetLibraryEntry>('preset_save', {
    req: {
      doc_id: args.docId,
      layer_id: args.layerId ?? null,
      name: args.name,
      preset_id: args.presetId ?? null,
    },
  });
}

export async function presetLoad(presetId: string): Promise<unknown> {
  return invoke('preset_load', { presetId });
}

export async function presetDelete(presetId: string): Promise<void> {
  return invoke<void>('preset_delete', { presetId });
}

export async function presetRename(
  presetId: string,
  name: string
): Promise<PresetLibraryEntry> {
  return invoke<PresetLibraryEntry>('preset_rename', { presetId, name });
}

export async function presetApplyToDoc(
  docId: number,
  presetId: string,
  layerId?: number | null
): Promise<PresetApplyResponse> {
  return invoke<PresetApplyResponse>('preset_apply_to_doc', {
    docId,
    presetId,
    layerId: layerId ?? null,
  });
}
