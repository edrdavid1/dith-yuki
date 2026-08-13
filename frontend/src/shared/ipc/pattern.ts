import { invoke } from '@tauri-apps/api/core';

export interface ExportPatternArgs {
  layerId: number;
  path: string;
  filterInstanceIds?: string[] | null;
  name?: string;
  description?: string;
}

export interface ImportPatternResponse {
  filter_ids: string[];
  palette_ids: number[];
}

export async function exportPattern(args: ExportPatternArgs): Promise<void> {
  return invoke<void>('export_pattern', {
    req: {
      layer_id: args.layerId,
      filter_instance_ids: args.filterInstanceIds ?? null,
      path: args.path,
      name: args.name ?? null,
      description: args.description ?? null,
    },
  });
}

export async function importPattern(
  path: string,
  targetLayerId: number
): Promise<ImportPatternResponse> {
  return invoke<ImportPatternResponse>('import_pattern', {
    req: { path, target_layer_id: targetLayerId },
  });
}
