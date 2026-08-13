import { invoke } from '@tauri-apps/api/core';

export async function addFilter(
  layerId: number,
  kind: string,
  params: Record<string, unknown>
): Promise<{ filter_id: string }> {
  return invoke<{ filter_id: string }>('add_filter', {
    req: { layer_id: layerId, kind, params },
  });
}

export async function updateFilter(
  layerId: number,
  filterId: string,
  params: Record<string, unknown>,
  extras?: { opacity?: number; blend_mode?: string }
): Promise<void> {
  return invoke<void>('update_filter', {
    req: {
      layer_id: layerId,
      filter_id: filterId,
      params,
      opacity: extras?.opacity ?? null,
      blend_mode: extras?.blend_mode ?? null,
    },
  });
}

export async function removeFilter(layerId: number, filterId: string): Promise<void> {
  return invoke<void>('remove_filter', {
    req: { layer_id: layerId, filter_id: filterId },
  });
}

export async function reorderFilter(
  layerId: number,
  filterId: string,
  newIndex: number
): Promise<void> {
  return invoke<void>('reorder_filter', {
    req: { layer_id: layerId, filter_id: filterId, new_index: newIndex },
  });
}
