import { invoke } from '@tauri-apps/api/core';

function withDocId(docId: number, body: Record<string, unknown>) {
  // Only snake_case: serde alias `docId` on the same field rejects duplicates.
  return { ...body, doc_id: docId };
}

export async function addFilter(
  docId: number,
  layerId: number,
  kind: string,
  params: Record<string, unknown>
): Promise<{ filter_id: string }> {
  return invoke<{ filter_id: string }>('add_filter', {
    req: withDocId(docId, { layer_id: layerId, kind, params }),
  });
}

export async function updateFilter(
  docId: number,
  layerId: number,
  filterId: string,
  params: Record<string, unknown>,
  extras?: { opacity?: number; blend_mode?: string; enabled?: boolean }
): Promise<void> {
  return invoke<void>('update_filter', {
    req: withDocId(docId, {
      layer_id: layerId,
      filter_id: filterId,
      params,
      opacity: extras?.opacity ?? null,
      blend_mode: extras?.blend_mode ?? null,
      enabled: extras?.enabled ?? null,
    }),
  });
}

export async function removeFilter(
  docId: number,
  layerId: number,
  filterId: string
): Promise<void> {
  return invoke<void>('remove_filter', {
    req: withDocId(docId, { layer_id: layerId, filter_id: filterId }),
  });
}

export async function reorderFilter(
  docId: number,
  layerId: number,
  filterId: string,
  newIndex: number
): Promise<void> {
  return invoke<void>('reorder_filter', {
    req: withDocId(docId, {
      layer_id: layerId,
      filter_id: filterId,
      new_index: newIndex,
    }),
  });
}
