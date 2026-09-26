import { invoke } from '@tauri-apps/api/core';
import type { ImportPatternResponse } from './pattern';

export interface PatternLibraryEntry {
  id: string;
  name: string;
  created_at: string;
  path: string;
}

export async function listPatternLibrary(): Promise<PatternLibraryEntry[]> {
  return invoke<PatternLibraryEntry[]>('list_pattern_library');
}

export async function savePatternToLibrary(args: {
  docId: number;
  layerId: number;
  name: string;
  description?: string;
}): Promise<PatternLibraryEntry> {
  return invoke<PatternLibraryEntry>('save_pattern_to_library', {
    req: {
      doc_id: args.docId,
      layer_id: args.layerId,
      filter_instance_ids: null,
      path: '', // filled by backend into app_data/patterns/
      name: args.name,
      description: args.description ?? null,
    },
  });
}

export async function applyPatternFromLibrary(
  docId: number,
  patternId: string,
  targetLayerId: number
): Promise<ImportPatternResponse> {
  return invoke<ImportPatternResponse>('apply_pattern_from_library', {
    docId,
    patternId,
    targetLayerId,
  });
}

export async function deletePatternFromLibrary(patternId: string): Promise<void> {
  return invoke<void>('delete_pattern_from_library', { patternId });
}

export async function renamePatternInLibrary(
  patternId: string,
  name: string
): Promise<PatternLibraryEntry> {
  return invoke<PatternLibraryEntry>('rename_pattern_in_library', {
    patternId,
    name,
  });
}

export async function importPatternToLibrary(path: string): Promise<PatternLibraryEntry> {
  return invoke<PatternLibraryEntry>('import_pattern_to_library', { path });
}

export async function exportPatternFromLibrary(
  patternId: string,
  path: string
): Promise<void> {
  return invoke<void>('export_pattern_from_library', {
    patternId,
    path,
  });
}
